//! Token search over entities, typed as one line:
//!
//! ```text
//! mood:chill,cozy -status:finished developer:"blizzard north" noveria
//! ```
//!
//! - `key:value` keeps entities with a field whose name starts with `key`
//!   and whose value contains `value`. `mood` finds `moods.ron`, `dev` finds
//!   a `developer` field.
//! - `key:a,b` matches either value. `key:` alone keeps entities that have
//!   the field at all (`journal:`).
//! - A bare word must appear in the title or anywhere in the components.
//! - `-` in front of any term excludes instead. Quotes keep spaces together.
//!
//! Matching ignores case and treats `-` and `_` as spaces, so
//! `story heavy` finds `story-heavy`.
//!
//! Fields come from the files themselves, not from a schema: each component
//! is a field named after its file (`moods.ron` -> `moods`, anything under
//! `journal/` -> `journal`), and every named field inside a RON file is one
//! too. A hand-written `steam.ron` with `developer: "Blizzard"` is
//! searchable as `developer:blizzard` and `steam:blizzard` with no new code.

mod complete;

use std::collections::BTreeSet;

use crate::Entity;

pub use complete::{Suggestion, Vocabulary};

/// A parsed search line. Every term must hold for an entity to match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    terms: Vec<Term>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Term {
    negated: bool,
    /// `None` for free text.
    key: Option<String>,
    /// Any of these; empty means "has the field".
    values: Vec<String>,
}

/// What search sees of one entity: its title and every field value.
#[derive(Debug, Clone, Default)]
pub struct Doc {
    title: String,
    /// (field name, value), both normalised.
    fields: Vec<(String, String)>,
    /// Field names as written, for listing and completion.
    keys: BTreeSet<String>,
    /// The subset of `keys` that name a whole component file.
    files: BTreeSet<String>,
    /// (field name, value) as written, for short values only: tags, enum
    /// variants, names. Notes and journal text are searched, never suggested.
    words: BTreeSet<(String, String)>,
}

/// Values longer than this are prose, not something to suggest.
const MAX_WORD_LEN: usize = 40;

impl Query {
    pub fn parse(input: &str) -> Self {
        let mut terms = Vec::new();
        let mut chars = input.chars().peekable();
        loop {
            while chars.next_if(|c| c.is_whitespace()).is_some() {}
            if chars.peek().is_none() {
                break;
            }
            let negated = chars.next_if_eq(&'-').is_some();
            if let Some(term) = read_term(&mut chars, negated) {
                terms.push(term);
            }
        }
        Self { terms }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn matches(&self, doc: &Doc) -> bool {
        self.terms.iter().all(|t| t.matches(doc) != t.negated)
    }
}

/// Reads one whitespace-separated term. Quoted parts may hold spaces, `:`
/// and `,`; the first bare `:` splits key from value, and bare `,` after it
/// separates alternatives.
fn read_term(chars: &mut std::iter::Peekable<std::str::Chars>, negated: bool) -> Option<Term> {
    let mut key = None;
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    while let Some(c) = chars.next_if(|c| quoted || !c.is_whitespace()) {
        match c {
            '"' => quoted = !quoted,
            ':' if !quoted && key.is_none() => key = Some(std::mem::take(&mut current)),
            ',' if !quoted && key.is_some() => parts.push(std::mem::take(&mut current)),
            c => current.push(c),
        }
    }
    parts.push(current);

    let values: Vec<String> = parts
        .iter()
        .map(|p| normalise(p))
        .filter(|p| !p.is_empty())
        .collect();
    match key.map(|k| normalise(&k)) {
        // `:foo` has no key to speak of; read it as plain text.
        Some(k) if k.is_empty() => (!values.is_empty()).then_some(Term {
            negated,
            key: None,
            values,
        }),
        Some(k) => Some(Term {
            negated,
            key: Some(k),
            values,
        }),
        None => (!values.is_empty()).then_some(Term {
            negated,
            key: None,
            values,
        }),
    }
}

impl Term {
    fn matches(&self, doc: &Doc) -> bool {
        match &self.key {
            None => self.values.iter().all(|word| {
                doc.title.contains(word.as_str())
                    || doc.fields.iter().any(|(_, v)| v.contains(word.as_str()))
            }),
            Some(key) => doc.fields.iter().any(|(name, value)| {
                name.starts_with(key.as_str())
                    && (self.values.is_empty()
                        || self.values.iter().any(|v| value.contains(v.as_str())))
            }),
        }
    }
}

impl Doc {
    pub fn new(entity: &Entity) -> Self {
        let mut doc = Doc {
            title: normalise(&entity.id.title()),
            ..Doc::default()
        };
        for component in &entity.components {
            let first = component.path.split('/').next().unwrap_or_default();
            let stem = first.split_once('.').map_or(first, |(stem, _)| stem);
            doc.keys.insert(stem.to_owned());
            doc.files.insert(stem.to_owned());
            if component.path.ends_with(".ron") {
                scan_ron(&component.text, |inner, value| {
                    let normalised = normalise(&value);
                    for name in inner.iter().map(String::as_str).chain([stem]) {
                        doc.fields.push((normalise(name), normalised.clone()));
                        doc.keys.insert(name.to_owned());
                        if value.chars().count() <= MAX_WORD_LEN {
                            doc.words.insert((name.to_owned(), value.clone()));
                        }
                    }
                });
            } else {
                doc.fields
                    .push((normalise(stem), normalise(&component.text)));
            }
        }
        doc
    }

    /// Every field name as written, to show what can be searched.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.keys.iter().map(String::as_str)
    }
}

/// Lowercase, with `-` and `_` read as spaces.
fn normalise(text: &str) -> String {
    text.chars()
        .map(|c| if c == '-' || c == '_' { ' ' } else { c })
        .flat_map(char::to_lowercase)
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Open,
    Close,
    Comma,
    Colon,
    Ident(String),
    Value(String),
}

/// Walks a RON file and reports each value (string, number, enum variant,
/// bool) with the names of the fields it sits inside, outermost first.
///
/// `ron::Value` would drop enum variants (`state: Playing` becomes a unit),
/// and those are exactly what people search for, so this reads tokens
/// instead. It never fails: text it doesn't understand is skipped.
fn scan_ron(text: &str, mut emit: impl FnMut(&[String], String)) {
    let toks = tokenize_ron(text);
    let mut depth = 0usize;
    // (depth the field was named at, name)
    let mut fields: Vec<(usize, String)> = Vec::new();
    let mut names = Vec::new();
    for (i, tok) in toks.iter().enumerate() {
        let next = toks.get(i + 1);
        match tok {
            Tok::Open => depth += 1,
            Tok::Close => {
                fields.retain(|(d, _)| *d < depth);
                depth = depth.saturating_sub(1);
            }
            Tok::Comma => fields.retain(|(d, _)| *d < depth),
            Tok::Colon => {}
            Tok::Ident(name) | Tok::Value(name) if next == Some(&Tok::Colon) => {
                fields.retain(|(d, _)| *d < depth);
                fields.push((depth, name.clone()));
            }
            // Struct and variant names: `Status(`, `Some(`.
            Tok::Ident(_) if next == Some(&Tok::Open) => {}
            Tok::Ident(name) if name == "None" => {}
            Tok::Ident(value) | Tok::Value(value) => {
                names.clear();
                names.extend(fields.iter().map(|(_, n)| n.clone()));
                emit(&names, value.clone());
            }
        }
    }
}

fn tokenize_ron(text: &str) -> Vec<Tok> {
    let mut toks = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '(' | '[' | '{' => toks.push(Tok::Open),
            ')' | ']' | '}' => toks.push(Tok::Close),
            ',' => toks.push(Tok::Comma),
            ':' => toks.push(Tok::Colon),
            '/' if chars.next_if_eq(&'/').is_some() => {
                while chars.next_if(|&c| c != '\n').is_some() {}
            }
            '/' if chars.next_if_eq(&'*').is_some() => {
                let mut prev = ' ';
                for c in chars.by_ref() {
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
            }
            '"' => toks.push(Tok::Value(read_string(&mut chars))),
            'r' if matches!(chars.peek(), Some('"' | '#')) => {
                let mut hashes = 0;
                while chars.next_if_eq(&'#').is_some() {
                    hashes += 1;
                }
                chars.next_if_eq(&'"');
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    if c == '"' {
                        let mut seen = 0;
                        while seen < hashes && chars.next_if_eq(&'#').is_some() {
                            seen += 1;
                        }
                        if seen == hashes {
                            break;
                        }
                        s.push('"');
                        s.extend(std::iter::repeat_n('#', seen));
                    } else {
                        s.push(c);
                    }
                }
                toks.push(Tok::Value(s));
            }
            '\'' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '\'' => break,
                        '\\' => s.extend(chars.next()),
                        c => s.push(c),
                    }
                }
                toks.push(Tok::Value(s));
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut s = String::from(c);
                while let Some(c) = chars.next_if(|c| c.is_alphanumeric() || *c == '_') {
                    s.push(c);
                }
                toks.push(Tok::Ident(s));
            }
            c if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' => {
                let mut s = String::from(c);
                while let Some(c) = chars.next_if(|c| c.is_alphanumeric() || matches!(c, '.' | '_'))
                {
                    s.push(c);
                }
                toks.push(Tok::Value(s));
            }
            _ => {}
        }
    }
    toks
}

fn read_string(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut s = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => match chars.next() {
                Some('n') => s.push('\n'),
                Some('t') => s.push('\t'),
                Some(c) => s.push(c),
                None => break,
            },
            c => s.push(c),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ComponentFile, EntityId};

    fn entity(folder: &str, files: &[(&str, &str)]) -> Doc {
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

    fn diablo() -> Doc {
        entity(
            "diablo-2-a1b2",
            &[
                ("status.ron", "Status(state: Shelved)"),
                ("moods.ron", "Tags(tags: [\"gloomy\", \"story-heavy\"])"),
                (
                    "steam.ron",
                    "// imported\n(app_id: 2536520, developer: \"Blizzard North\", extra: Some((year: 2000)))",
                ),
                ("journal/2026-10-13-1930.md", "Killed Andariel."),
            ],
        )
    }

    fn find(query: &str, doc: &Doc) -> bool {
        Query::parse(query).matches(doc)
    }

    #[test]
    fn parses_keys_values_negation_and_quotes() {
        let q = Query::parse(
            r#"  mood:chill,Cozy -status:finished dev:"blizzard north" plain  -"two words" :x "#,
        );
        let t = |negated, key: Option<&str>, values: &[&str]| Term {
            negated,
            key: key.map(Into::into),
            values: values.iter().map(|v| (*v).into()).collect(),
        };
        assert_eq!(
            q.terms,
            [
                t(false, Some("mood"), &["chill", "cozy"]),
                t(true, Some("status"), &["finished"]),
                t(false, Some("dev"), &["blizzard north"]),
                t(false, None, &["plain"]),
                t(true, None, &["two words"]),
                t(false, None, &["x"]),
            ]
        );
        assert!(Query::parse("   ").is_empty());
        assert!(Query::parse(" - ").is_empty());
    }

    #[test]
    fn keyed_terms_match_fields_by_prefix() {
        let d = diablo();
        assert!(find("developer:blizzard mood:gloomy", &d));
        assert!(find("dev:blizzard", &d));
        assert!(find("steam:blizzard", &d));
        assert!(find("status:shelved", &d));
        assert!(find("state:shelv", &d));
        assert!(find("year:2000", &d));
        assert!(find("mood:chill,gloomy", &d));
        assert!(!find("mood:chill", &d));
        assert!(!find("developer:valve", &d));
        // A field that isn't there never matches.
        assert!(!find("gameplay:rpg", &d));
    }

    #[test]
    fn empty_value_means_has_field() {
        let d = diablo();
        assert!(find("journal:", &d));
        assert!(!find("notes:", &d));
        assert!(find("-notes:", &d));
    }

    #[test]
    fn free_text_searches_title_and_content() {
        let d = diablo();
        assert!(find("diablo", &d));
        assert!(find("andariel", &d));
        assert!(find("journal:andariel", &d));
        assert!(find("\"story heavy\"", &d));
        assert!(find("story-heavy", &d));
        assert!(!find("baal", &d));
        // Struct and field names are not content.
        assert!(!find("tags", &d));
        assert!(!find("imported", &d));
    }

    #[test]
    fn negation_excludes() {
        let d = diablo();
        assert!(!find("-mood:gloomy", &d));
        assert!(find("-status:finished", &d));
        assert!(!find("diablo -andariel", &d));
    }

    #[test]
    fn ron_scan_tracks_nested_fields() {
        let mut seen = Vec::new();
        scan_ron(
            r##"Game(name: "A, B", tags: ["x"], inner: (deep: On), raw: r#"q"uote"#, c: 'z', n: -1.5e3, none: None), tail: 1"##,
            |fields, v| seen.push((fields.join("."), v)),
        );
        let seen: Vec<(&str, &str)> = seen.iter().map(|(f, v)| (f.as_str(), v.as_str())).collect();
        assert_eq!(
            seen,
            [
                ("name", "A, B"),
                ("tags", "x"),
                ("inner.deep", "On"),
                ("raw", "q\"uote"),
                ("c", "z"),
                ("n", "-1.5e3"),
                ("tail", "1"),
            ]
        );
    }

    #[test]
    fn keys_list_what_is_searchable() {
        let doc = diablo();
        let keys: Vec<_> = doc.keys().collect();
        assert_eq!(
            keys,
            [
                "app_id",
                "developer",
                "extra",
                "journal",
                "moods",
                "state",
                "status",
                "steam",
                "tags",
                "year"
            ]
        );
    }
}
