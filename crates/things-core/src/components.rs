//! Data formats for the components shipped so far. Each one owns its file
//! format; RON components round-trip through serde.

use serde::{Deserialize, Serialize};

/// Parses a RON component.
pub fn from_ron<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, ron::error::SpannedError> {
    ron::from_str(text)
}

/// Writes a RON component in a readable, diff-friendly layout.
pub fn to_ron<T: Serialize>(value: &T) -> Result<String, ron::Error> {
    let config = ron::ser::PrettyConfig::new().struct_names(true);
    let mut text = ron::ser::to_string_pretty(value, config)?;
    text.push('\n');
    Ok(text)
}

/// `status.ron`: where a thing stands in the backlog.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub state: PlayState,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayState {
    #[default]
    Backlog,
    Playing,
    Finished,
    Shelved,
}

impl PlayState {
    pub const ALL: [PlayState; 4] = [Self::Backlog, Self::Playing, Self::Finished, Self::Shelved];

    pub fn label(self) -> &'static str {
        match self {
            Self::Backlog => "Backlog",
            Self::Playing => "Playing",
            Self::Finished => "Finished",
            Self::Shelved => "Shelved",
        }
    }
}

/// `ownership.ron`: whether I have a thing or only want it, and where I
/// came across it. Kept apart from [`Status`], which is about playing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ownership {
    pub state: OwnState,
    /// When it went on my radar, `YYYY-MM-DD`.
    pub added: String,
    /// Where I saw it: "Sam's Steam link", "tweet from @someone".
    pub via: String,
    /// A store page, post or anything to go back to.
    pub link: String,
    /// Off the radar until this day (`YYYY-MM-DD`), like a card in Anki.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub hidden_until: String,
    /// How many times it was put off; each one hides it for longer.
    #[serde(skip_serializing_if = "is_zero")]
    pub snoozes: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// The first "later" hides a thing for a week, each next one twice as long,
/// up to about half a year.
const SNOOZE_DAYS: [i64; 6] = [7, 14, 30, 60, 120, 180];

impl Ownership {
    /// How long the next "later" hides it for.
    pub fn next_snooze_days(&self) -> i64 {
        SNOOZE_DAYS[(self.snoozes as usize).min(SNOOZE_DAYS.len() - 1)]
    }

    /// Puts it off for longer than last time. `today` is `YYYY-MM-DD`.
    /// Returns the day it comes back.
    pub fn snooze(&mut self, today: &str) -> Option<&str> {
        let today = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d").ok()?;
        let days = self.next_snooze_days();
        self.snoozes += 1;
        self.hidden_until = (today + chrono::Days::new(days as u64))
            .format("%Y-%m-%d")
            .to_string();
        Some(&self.hidden_until)
    }

    /// Snoozed and not due back yet.
    pub fn is_hidden(&self, today: &str) -> bool {
        let until = self.hidden_until.trim();
        !until.is_empty() && until > today
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnState {
    /// Don't have it, want to try it.
    #[default]
    Interested,
    Owned,
    /// Looked again and decided against it. Kept so it isn't re-added.
    NotInterested,
}

impl OwnState {
    pub const ALL: [OwnState; 3] = [Self::Interested, Self::Owned, Self::NotInterested];

    pub fn label(self) -> &'static str {
        match self {
            Self::Interested => "Interested",
            Self::Owned => "Owned",
            Self::NotInterested => "Not interested",
        }
    }
}

/// `release.ron`: when a thing comes out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Release {
    /// `YYYY-MM-DD`, `YYYY-MM` or `YYYY`; empty when unknown.
    pub date: String,
    pub stage: Stage,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    #[default]
    Announced,
    /// Playable but unfinished. Counts as not out yet.
    EarlyAccess,
    Released,
}

impl Stage {
    pub const ALL: [Stage; 3] = [Self::Announced, Self::EarlyAccess, Self::Released];

    pub fn label(self) -> &'static str {
        match self {
            Self::Announced => "Announced",
            Self::EarlyAccess => "Early access",
            Self::Released => "Released",
        }
    }
}

impl Release {
    /// Whether the date is a well-formed `YYYY`, `YYYY-MM` or `YYYY-MM-DD`.
    /// An empty date is fine too: it means unknown.
    pub fn date_is_valid(&self) -> bool {
        let d = self.date.trim();
        let digits = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_digit());
        let parts: Vec<&str> = d.split('-').collect();
        d.is_empty()
            || match parts[..] {
                [y] => digits(y, 4),
                [y, m] => digits(y, 4) && digits(m, 2) && ("01"..="12").contains(&m),
                [y, m, day] => {
                    digits(y, 4)
                        && digits(m, 2)
                        && digits(day, 2)
                        && ("01"..="12").contains(&m)
                        && ("01"..="31").contains(&day)
                }
                _ => false,
            }
    }

    /// Whether the date has arrived by `today` (`YYYY-MM-DD`). A month or a
    /// year has arrived once it is over, since the day could be any in it.
    pub fn date_passed(&self, today: &str) -> bool {
        let d = self.date.trim();
        if d.is_empty() || !self.date_is_valid() {
            return false;
        }
        match today.get(..d.len()) {
            Some(now) if d.len() == 10 => d <= now,
            Some(now) => d < now,
            None => false,
        }
    }

    /// Out and playable, from my point of view: released, or the date came
    /// and went for something only announced. Early access never counts.
    pub fn is_out(&self, today: &str) -> bool {
        match self.stage {
            Stage::Released => true,
            Stage::Announced => self.date_passed(today),
            Stage::EarlyAccess => false,
        }
    }
}

/// A tag set such as `moods.ron` or `gameplay.ron`. Every set uses this one
/// format; the file name says which concept the tags describe.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tags {
    pub tags: Vec<String>,
}

impl Tags {
    /// Adds a tag (trimmed, lowercased) unless it's empty or already there.
    pub fn add(&mut self, tag: &str) -> bool {
        let tag = tag.trim().to_lowercase();
        if tag.is_empty() || self.tags.contains(&tag) {
            return false;
        }
        self.tags.push(tag);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_round_trips_through_ron() {
        let status = Status {
            state: PlayState::Shelved,
        };
        let text = to_ron(&status).unwrap();
        assert!(text.starts_with("Status("), "{text}");
        assert_eq!(from_ron::<Status>(&text).unwrap(), status);
        // Hand-written files can leave the struct name off.
        assert_eq!(from_ron::<Status>("(state: Shelved)").unwrap(), status);
    }

    #[test]
    fn tags_are_normalised_and_unique() {
        let mut tags = Tags::default();
        assert!(tags.add(" Chill "));
        assert!(!tags.add("chill"));
        assert!(!tags.add("  "));
        assert_eq!(tags.tags, ["chill"]);
        assert_eq!(from_ron::<Tags>(&to_ron(&tags).unwrap()).unwrap(), tags);
    }
    #[test]
    fn ownership_and_release_read_partial_files() {
        let own: Ownership = from_ron("(state: Owned)").unwrap();
        assert_eq!(own.state, OwnState::Owned);
        assert!(own.via.is_empty());
        let full = Ownership {
            state: OwnState::Interested,
            added: "2026-10-13".into(),
            via: "Sam".into(),
            link: "https://example.com".into(),
            ..Ownership::default()
        };
        assert_eq!(
            from_ron::<Ownership>(&to_ron(&full).unwrap()).unwrap(),
            full
        );
        let release: Release = from_ron("(stage: EarlyAccess)").unwrap();
        assert_eq!(release.stage, Stage::EarlyAccess);
        assert!(release.date.is_empty());
    }

    #[test]
    fn release_dates_accept_day_month_or_year() {
        let r = |date: &str| Release {
            date: date.into(),
            stage: Stage::Announced,
        };
        for ok in ["", "2026", "2026-10", "2026-10-19"] {
            assert!(r(ok).date_is_valid(), "{ok}");
        }
        for bad in [
            "26",
            "2026-13",
            "2026-1-19",
            "Q4 2026",
            "2026-10-32",
            "soon",
        ] {
            assert!(!r(bad).date_is_valid(), "{bad}");
        }
    }

    #[test]
    fn release_is_out_once_its_date_has_passed() {
        let today = "2026-10-13";
        let r = |date: &str, stage| Release {
            date: date.into(),
            stage,
        };
        assert!(r("2026-10-13", Stage::Announced).is_out(today));
        assert!(r("2026-09", Stage::Announced).is_out(today));
        assert!(r("2025", Stage::Announced).is_out(today));
        assert!(!r("2026-10-19", Stage::Announced).is_out(today));
        // This month or this year could still be ahead.
        assert!(!r("2026-10", Stage::Announced).is_out(today));
        assert!(!r("2026", Stage::Announced).is_out(today));
        assert!(!r("", Stage::Announced).is_out(today));
        assert!(!r("2020", Stage::EarlyAccess).is_out(today));
        assert!(r("", Stage::Released).is_out(today));
    }

    #[test]
    fn snoozing_hides_for_longer_each_time() {
        let mut own = Ownership::default();
        let text = to_ron(&own).unwrap();
        assert!(
            !text.contains("hidden_until") && !text.contains("snoozes"),
            "{text}"
        );

        assert_eq!(own.snooze("2026-10-13"), Some("2026-10-20"));
        assert!(own.is_hidden("2026-10-19"));
        assert!(!own.is_hidden("2026-10-20"));
        assert_eq!(own.snooze("2026-10-20"), Some("2026-11-03"));
        assert_eq!(own.snoozes, 2);
        for _ in 0..10 {
            own.snooze("2026-10-20");
        }
        assert_eq!(own.hidden_until, "2027-04-18", "capped at 180 days");
        assert_eq!(from_ron::<Ownership>(&to_ron(&own).unwrap()).unwrap(), own);
        assert_eq!(own.snooze("soon"), None);
    }
}
