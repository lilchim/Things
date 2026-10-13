//! The radar: things I want to get to, read from their `ownership.ron` and
//! `release.ron`, so they don't get lost the way bookmarks do.
//!
//! - **Coming out**: anything with a release that isn't out yet (announced
//!   for later, or still in early access), soonest first.
//! - **Out now**: things I'm interested in but don't own that I could play
//!   today, oldest first so forgotten ones come back up.

use crate::components::{OwnState, Ownership, Release, Stage, from_ron};
use crate::{Entity, EntityId};

/// What the radar knows about one entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watch {
    pub id: EntityId,
    pub ownership: Option<Ownership>,
    pub release: Option<Release>,
}

impl Watch {
    /// `None` when the entity has neither component. A file that doesn't
    /// parse counts as missing; its page shows the error.
    pub fn new(entity: &Entity) -> Option<Self> {
        let read = |path: &str| {
            entity
                .components
                .iter()
                .find(|c| c.path == path)
                .map(|c| c.text.as_str())
        };
        let ownership = read("ownership.ron").and_then(|t| from_ron(t).ok());
        let release = read("release.ron").and_then(|t| from_ron(t).ok());
        (ownership.is_some() || release.is_some()).then(|| Self {
            id: entity.id.clone(),
            ownership,
            release,
        })
    }

    pub fn interested(&self) -> bool {
        self.ownership
            .as_ref()
            .is_some_and(|o| o.state == OwnState::Interested)
    }

    /// The release date has come but the stage still says it isn't out:
    /// worth a look to update it.
    pub fn date_passed(&self, today: &str) -> bool {
        self.release
            .as_ref()
            .is_some_and(|r| r.stage != Stage::Released && r.date_passed(today))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Radar {
    pub coming: Vec<Watch>,
    pub out_now: Vec<Watch>,
}

impl Radar {
    /// Sorts watched things into the two lists. `today` is `YYYY-MM-DD`.
    pub fn new<'a>(watches: impl IntoIterator<Item = &'a Watch>, today: &str) -> Self {
        let mut radar = Self::default();
        for w in watches {
            match &w.release {
                Some(r) if !r.is_out(today) => radar.coming.push(w.clone()),
                _ if w.interested() => radar.out_now.push(w.clone()),
                _ => {}
            }
        }
        // Unknown dates go last in both lists.
        let key = |s: &str| (s.trim().is_empty(), s.trim().to_owned());
        radar.coming.sort_by_cached_key(|w| {
            let date = w.release.as_ref().map_or("", |r| r.date.as_str());
            (key(date), w.id.title())
        });
        radar.out_now.sort_by_cached_key(|w| {
            let added = w.ownership.as_ref().map_or("", |o| o.added.as_str());
            (key(added), w.id.title())
        });
        radar
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ComponentFile;

    fn watch(folder: &str, files: &[(&str, &str)]) -> Option<Watch> {
        Watch::new(&Entity {
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

    fn slugs(list: &[Watch]) -> Vec<&str> {
        list.iter().map(|w| w.id.slug()).collect()
    }

    #[test]
    fn sorts_things_into_coming_and_out_now() {
        let today = "2026-10-13";
        let interested = |added: &str| format!("(state: Interested, added: \"{added}\")");
        let watches: Vec<Watch> = [
            watch(
                "next-week-0001",
                &[("release.ron", "(date: \"2026-10-19\")")],
            ),
            watch("someday-0002", &[("release.ron", "(stage: Announced)")]),
            watch(
                "early-0003",
                &[
                    ("release.ron", "(date: \"2026-03\", stage: EarlyAccess)"),
                    ("ownership.ron", "(state: Owned)"),
                ],
            ),
            watch(
                "came-out-0004",
                &[
                    ("release.ron", "(date: \"2026-10-01\")"),
                    ("ownership.ron", &interested("2026-09-01")),
                ],
            ),
            watch(
                "old-find-0005",
                &[("ownership.ron", &interested("2025-01-02"))],
            ),
            watch("no-date-0006", &[("ownership.ron", "(state: Interested)")]),
            watch("owned-0007", &[("ownership.ron", "(state: Owned)")]),
            watch("plain-0008", &[("status.ron", "(state: Playing)")]),
            watch("broken-0009", &[("ownership.ron", "(state: Maybe)")]),
        ]
        .into_iter()
        .flatten()
        .collect();
        assert_eq!(watches.len(), 7, "plain and broken files are not watched");

        let radar = Radar::new(&watches, today);
        assert_eq!(slugs(&radar.coming), ["early", "next-week", "someday"]);
        assert_eq!(slugs(&radar.out_now), ["old-find", "came-out", "no-date"]);
        assert!(radar.coming[0].date_passed(today));
        assert!(!radar.coming[1].date_passed(today));
        assert!(radar.out_now[1].date_passed(today));
    }
}
