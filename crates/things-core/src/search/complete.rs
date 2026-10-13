//! Completion for the search line: keys and values that exist in the library.
//!
//! Only the last token of the line is completed, the one being typed:
//!
//! - `mo` suggests the key `moods:` and values starting with `mo` under any
//!   key (`mood:moody`).
//! - `moods:` or `moods:gl` suggests the values of that key, most used first.
//! - `moods:chill,` suggests the remaining values for another alternative.

use std::collections::BTreeMap;

use super::{Doc, normalise};

/// Every key and short value in the library, with how many entities use it.
#[derive(Debug, Clone, Default)]
pub struct Vocabulary {
    /// Normalised key -> entry. The first spelling seen is the one suggested.
    keys: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, Default)]
struct Entry {
    raw: String,
    count: usize,
    /// Names a whole component file (`moods`) rather than a field inside one.
    file: bool,
    /// Normalised value -> (value as written, entity count).
    values: BTreeMap<String, (String, usize)>,
}

/// One way to finish the token being typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// What to show: `moods:`, `gloomy`, `moods:gloomy`.
    pub label: String,
    /// How many entities have it.
    pub count: usize,
    /// The whole search line with the suggestion applied.
    pub line: String,
}

impl Vocabulary {
    pub fn new<'a>(docs: impl IntoIterator<Item = &'a Doc>) -> Self {
        let mut vocab = Self::default();
        for doc in docs {
            for key in &doc.keys {
                let entry = vocab.entry(key);
                entry.count += 1;
                entry.file |= doc.files.contains(key);
            }
            for (key, value) in &doc.words {
                let entry = vocab.entry(key);
                let slot = entry
                    .values
                    .entry(normalise(value))
                    .or_insert_with(|| (value.clone(), 0));
                slot.1 += 1;
            }
        }
        vocab
    }

    fn entry(&mut self, key: &str) -> &mut Entry {
        self.keys.entry(normalise(key)).or_insert_with(|| Entry {
            raw: key.to_owned(),
            ..Entry::default()
        })
    }

    /// Every key as written, sorted.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.keys.values().map(|e| e.raw.as_str())
    }

    /// Up to `limit` ways to finish the last token of `line`.
    pub fn suggest(&self, line: &str, limit: usize) -> Vec<Suggestion> {
        let mut start = last_token_start(line);
        if line[start..].starts_with('-') {
            start += 1;
        }
        let token = &line[start..];
        let mut out = match split_token(token) {
            Some((key, chosen, partial_at)) => {
                self.values_for(line, key, chosen, start + partial_at)
            }
            None => self.keys_and_words(line, start),
        };
        out.truncate(limit);
        out
    }

    /// `key:` is typed: values under every key it names.
    fn values_for(&self, line: &str, key: &str, chosen: Vec<String>, at: usize) -> Vec<Suggestion> {
        let key = normalise(&unquote(key));
        let partial = normalise(&unquote(&line[at..]));
        let mut values: BTreeMap<&str, (&str, usize)> = BTreeMap::new();
        for (_, entry) in self
            .keys
            .range(key.clone()..)
            .take_while(|(k, _)| k.starts_with(&key))
        {
            for (norm, (raw, count)) in &entry.values {
                values.entry(norm).or_insert((raw, 0)).1 += count;
            }
        }
        let mut hits: Vec<_> = values
            .into_iter()
            .filter(|(norm, _)| !chosen.iter().any(|c| c == norm))
            .filter_map(|(norm, (raw, count))| {
                let rank = rank(norm, &partial)?;
                Some((rank, count, raw))
            })
            .collect();
        hits.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        hits.into_iter()
            .map(|(_, count, raw)| Suggestion {
                label: raw.to_owned(),
                count,
                line: format!("{}{} ", &line[..at], quote(raw)),
            })
            .collect()
    }

    /// A bare word is typed: keys it starts, then `key:value` pairs whose
    /// value it starts. Pairs use the file's key (`moods:gloomy`), not the
    /// field inside it (`tags:gloomy`), so each value shows up once.
    fn keys_and_words(&self, line: &str, at: usize) -> Vec<Suggestion> {
        let partial = normalise(&unquote(&line[at..]));
        if partial.is_empty() {
            return Vec::new();
        }
        let before = &line[..at];
        let keys = self
            .keys
            .range(partial.clone()..)
            .take_while(|(k, _)| k.starts_with(&partial))
            .map(|(_, e)| Suggestion {
                label: format!("{}:", e.raw),
                count: e.count,
                line: format!("{before}{}:", quote(&e.raw)),
            });
        let mut words: Vec<_> = self
            .keys
            .values()
            .filter(|e| e.file)
            .flat_map(|e| e.values.iter().map(move |(norm, value)| (e, norm, value)))
            .filter(|(_, norm, _)| norm.starts_with(&partial))
            .map(|(e, _, (raw, count))| Suggestion {
                label: format!("{}:{raw}", e.raw),
                count: *count,
                line: format!("{before}{}:{} ", quote(&e.raw), quote(raw)),
            })
            .collect();
        words.sort_by_key(|w| std::cmp::Reverse(w.count));
        keys.chain(words).collect()
    }
}

/// Values starting with what's typed come first, then ones containing it.
fn rank(value: &str, partial: &str) -> Option<u8> {
    if value.starts_with(partial) {
        Some(0)
    } else if value.contains(partial) {
        Some(1)
    } else {
        None
    }
}

/// Byte index where the last whitespace-separated token starts.
fn last_token_start(line: &str) -> usize {
    let mut quoted = false;
    let mut start = 0;
    for (i, c) in line.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if c.is_whitespace() && !quoted {
            start = i + c.len_utf8();
        }
    }
    start
}

/// For a `key:a,b,partial` token: the key, the values already chosen
/// (normalised), and where the partial value starts. `None` without a key.
fn split_token(token: &str) -> Option<(&str, Vec<String>, usize)> {
    let mut quoted = false;
    let mut colon = None;
    let mut part = 0;
    for (i, c) in token.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ':' if !quoted && colon.is_none() => {
                colon = Some(i);
                part = i + 1;
            }
            ',' if !quoted && colon.is_some() => part = i + 1,
            _ => {}
        }
    }
    let colon = colon.filter(|&c| c > 0)?;
    let chosen = token[colon + 1..part]
        .split(',')
        .map(|v| normalise(&unquote(v)))
        .filter(|v| !v.is_empty())
        .collect();
    Some((&token[..colon], chosen, part))
}

fn unquote(text: &str) -> String {
    text.replace('"', "")
}

/// Quotes a key or value if the query parser would otherwise split it.
fn quote(text: &str) -> String {
    if text
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, ':' | ',' | '"'))
    {
        format!("\"{}\"", unquote(text))
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ComponentFile, Entity, EntityId};

    fn doc(folder: &str, files: &[(&str, &str)]) -> Doc {
        Doc::new(&Entity {
            id: EntityId::parse(folder).unwrap(),
            components: files
                .iter()
                .map(|(path, text)| ComponentFile {
                    path: (*path).into(),
                    text: (*text).into(),
                })
                .collect(),
        })
    }

    fn vocab() -> Vocabulary {
        let docs = [
            doc(
                "diablo-2-a1b2",
                &[
                    ("moods.ron", r#"Tags(tags: ["gloomy", "tense"])"#),
                    ("steam.ron", r#"(developer: "Blizzard North")"#),
                    (
                        "notes.md",
                        "Gloomy cathedral, long notes that are never suggested.",
                    ),
                ],
            ),
            doc(
                "hades-2-b41c",
                &[
                    ("moods.ron", r#"Tags(tags: ["tense", "fast"])"#),
                    ("status.ron", "Status(state: Backlog)"),
                ],
            ),
        ];
        Vocabulary::new(&docs)
    }

    fn labels(line: &str) -> Vec<String> {
        vocab()
            .suggest(line, 10)
            .into_iter()
            .map(|s| s.label)
            .collect()
    }

    #[test]
    fn values_for_a_key_most_used_first() {
        assert_eq!(labels("mood:"), ["tense", "fast", "gloomy"]);
        assert_eq!(labels("mood:g"), ["gloomy"]);
        // Containing comes after starting with.
        assert_eq!(labels("moods:s"), ["tense", "fast"]);
        assert_eq!(labels("moods:tense,"), ["fast", "gloomy"]);
        assert_eq!(labels("-mood:fa"), ["fast"]);
    }

    #[test]
    fn bare_words_suggest_keys_then_values() {
        assert_eq!(labels("st"), ["state:", "status:", "steam:"]);
        assert_eq!(labels("glo"), ["moods:gloomy"]);
        assert_eq!(labels("blizz"), ["steam:Blizzard North"]);
        assert!(labels("").is_empty());
        assert!(labels("mood:tense ").is_empty());
        // Prose is searchable but never suggested.
        assert!(labels("cathedral").is_empty());
    }

    #[test]
    fn applying_a_suggestion_rewrites_only_the_last_token() {
        let v = vocab();
        let first = |line: &str| v.suggest(line, 1).remove(0).line;
        assert_eq!(
            first("status:backlog mood:te"),
            "status:backlog mood:tense "
        );
        assert_eq!(first("-mood:tense,f"), "-mood:tense,fast ");
        assert_eq!(first("dev"), "developer:");
        assert_eq!(first("developer:bl"), r#"developer:"Blizzard North" "#);
        assert_eq!(
            first("developer:\"blizzard n"),
            r#"developer:"Blizzard North" "#
        );
    }

    #[test]
    fn counts_are_per_entity() {
        let v = vocab();
        let tense = v.suggest("moods:tense", 1).remove(0);
        assert_eq!(tense.count, 2);
        let moods = v.suggest("moo", 1).remove(0);
        assert_eq!((moods.label.as_str(), moods.count), ("moods:", 2));
    }
}
