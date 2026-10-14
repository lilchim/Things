//! Research: steps that look a thing up somewhere else and suggest
//! components for it, one confirmation at a time.
//!
//! Each [`Step`] needs some facts about a thing before it can run (a title,
//! a confirmed Steam app) and fills in one component file. A step whose
//! file is already there is done; one whose needs aren't met waits. So
//! confirming a step's suggestion is what unlocks the next ones: a title
//! finds the Steam app, and the Steam app finds the release date, credits
//! and tags.
//!
//! Nothing here touches the network or the files. A step says which URL to
//! fetch ([`Step::request`]) and turns the response into a [`Finding`]; the
//! app does the fetching and only writes a file once it's confirmed.

pub mod steam;

use crate::Entity;
use crate::components::{Steam, from_ron, to_ron};

/// One thing research can find out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Step {
    /// Title → which Steam app it is.
    FindOnSteam,
    /// Steam app → release date and stage.
    SteamRelease,
    /// Steam app → developers and publishers.
    SteamCredits,
    /// Steam app → the store's user tags.
    SteamTags,
}

/// A fact a step needs before it can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Title,
    SteamApp,
}

impl Need {
    /// Reads as "needs …".
    pub fn label(self) -> &'static str {
        match self {
            Self::Title => "a title",
            Self::SteamApp => "a Steam link",
        }
    }
}

/// Where a step stands for one thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// Its file is there already.
    Recorded,
    /// Everything it needs is known; it can run.
    Ready,
    /// Missing these first.
    Waiting(Vec<Need>),
}

/// What a step's run turned up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// Several matches: pick the right one.
    Choose(Vec<steam::Candidate>),
    /// The file to add, with a one-line summary of what's in it.
    File { summary: String, text: String },
    /// Looked and found nothing worth suggesting.
    Nothing(String),
}

/// A URL to fetch for a step, and how to ask for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    /// Cookies to send, such as Steam's age check.
    pub cookie: Option<&'static str>,
}

/// The facts steps can use, read from a thing's files.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub title: String,
    pub steam: Option<Steam>,
}

impl Facts {
    pub fn of(entity: &Entity) -> Self {
        let steam = entity
            .components
            .iter()
            .find(|c| c.path == Step::FindOnSteam.file())
            .and_then(|c| from_ron::<Steam>(&c.text).ok())
            .filter(|s| s.appid != 0);
        Self {
            title: entity.id.title(),
            steam,
        }
    }

    fn has(&self, need: Need) -> bool {
        match need {
            Need::Title => !self.title.trim().is_empty(),
            Need::SteamApp => self.steam.is_some(),
        }
    }
}

impl Step {
    pub const ALL: [Step; 4] = [
        Self::FindOnSteam,
        Self::SteamRelease,
        Self::SteamCredits,
        Self::SteamTags,
    ];

    /// A stable name, for remembering skipped steps.
    pub fn key(self) -> &'static str {
        match self {
            Self::FindOnSteam => "steam",
            Self::SteamRelease => "steam-release",
            Self::SteamCredits => "steam-credits",
            Self::SteamTags => "steam-tags",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::FindOnSteam => "Steam store page",
            Self::SteamRelease => "Release date",
            Self::SteamCredits => "Developer and publisher",
            Self::SteamTags => "Steam tags",
        }
    }

    /// The button that runs it.
    pub fn verb(self) -> &'static str {
        match self {
            Self::FindOnSteam => "Find on Steam",
            Self::SteamRelease | Self::SteamCredits | Self::SteamTags => "Look up on Steam",
        }
    }

    /// Where it looks.
    pub fn source(self) -> &'static str {
        "Steam"
    }

    /// The component file it fills in.
    pub fn file(self) -> &'static str {
        match self {
            Self::FindOnSteam => "steam.ron",
            Self::SteamRelease => "release.ron",
            Self::SteamCredits => "credits.ron",
            Self::SteamTags => "steam-tags.ron",
        }
    }

    pub fn needs(self) -> &'static [Need] {
        match self {
            Self::FindOnSteam => &[Need::Title],
            Self::SteamRelease | Self::SteamCredits | Self::SteamTags => &[Need::SteamApp],
        }
    }

    pub fn standing(self, entity: &Entity, facts: &Facts) -> Standing {
        if entity.components.iter().any(|c| c.path == self.file()) {
            return Standing::Recorded;
        }
        let missing: Vec<Need> = self
            .needs()
            .iter()
            .copied()
            .filter(|n| !facts.has(*n))
            .collect();
        if missing.is_empty() {
            Standing::Ready
        } else {
            Standing::Waiting(missing)
        }
    }

    /// What to fetch, or `None` if the facts it needs aren't there.
    pub fn request(self, facts: &Facts) -> Option<Request> {
        let appid = facts.steam.as_ref().map(|s| s.appid);
        match self {
            Self::FindOnSteam => (facts.has(Need::Title)).then(|| Request {
                url: steam::search_url(&facts.title),
                cookie: None,
            }),
            Self::SteamRelease | Self::SteamCredits => appid.map(|id| Request {
                url: steam::details_url(id),
                cookie: None,
            }),
            Self::SteamTags => appid.map(|id| Request {
                url: steam::page_url(id),
                cookie: Some(steam::AGE_COOKIE),
            }),
        }
    }

    /// Reads a fetched response into what to suggest.
    pub fn finding(self, body: &str, facts: &Facts) -> Result<Finding, String> {
        let appid = facts.steam.as_ref().map_or(0, |s| s.appid);
        let file = |summary: String, text: Result<String, ron::Error>| Finding::File {
            summary,
            text: text.expect("components always serialise"),
        };
        Ok(match self {
            Self::FindOnSteam => {
                let found = steam::parse_search(body)?;
                if found.is_empty() {
                    Finding::Nothing(format!("Steam has nothing called “{}”.", facts.title))
                } else {
                    Finding::Choose(found)
                }
            }
            Self::SteamRelease => match steam::parse_details(body, appid)?.release {
                Some(release) => {
                    let date = if release.date.is_empty() {
                        "no date yet".to_owned()
                    } else {
                        release.date.clone()
                    };
                    file(
                        format!("{}, {date}", release.stage.label()),
                        to_ron(&release),
                    )
                }
                None => Finding::Nothing("Steam doesn't give a release date.".into()),
            },
            Self::SteamCredits => {
                let credits = steam::parse_details(body, appid)?.credits;
                if credits.developers.is_empty() && credits.publishers.is_empty() {
                    Finding::Nothing("Steam doesn't list who made it.".into())
                } else {
                    let summary = format!(
                        "Developer: {}. Publisher: {}.",
                        or_unknown(&credits.developers),
                        or_unknown(&credits.publishers)
                    );
                    file(summary, to_ron(&credits))
                }
            }
            Self::SteamTags => {
                let tags = steam::parse_tags(body);
                if tags.tags.is_empty() {
                    Finding::Nothing("The store page had no tags on it.".into())
                } else {
                    file(tags.tags.join(", "), to_ron(&tags))
                }
            }
        })
    }
}

fn or_unknown(names: &[String]) -> String {
    if names.is_empty() {
        "unknown".into()
    } else {
        names.join(", ")
    }
}

/// Every step and where it stands for a thing, in step order.
pub fn plan(entity: &Entity) -> Vec<(Step, Standing)> {
    let facts = Facts::of(entity);
    Step::ALL
        .into_iter()
        .map(|s| (s, s.standing(entity, &facts)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ComponentFile, EntityId};

    fn entity(files: &[(&str, &str)]) -> Entity {
        Entity {
            id: EntityId::parse("mass-effect-7f3a").unwrap(),
            collection: "games".into(),
            components: files
                .iter()
                .map(|(path, text)| ComponentFile {
                    path: (*path).into(),
                    text: (*text).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_title_alone_can_only_look_for_the_steam_app() {
        let plan = plan(&entity(&[("status.ron", "(state: Backlog)")]));
        assert_eq!(plan[0], (Step::FindOnSteam, Standing::Ready));
        for (step, standing) in &plan[1..] {
            assert_eq!(
                *standing,
                Standing::Waiting(vec![Need::SteamApp]),
                "{step:?}"
            );
        }
    }

    #[test]
    fn a_confirmed_steam_app_unlocks_the_rest() {
        let steam = "Steam(appid: 1328670, name: \"Mass Effect Legendary Edition\")";
        let plan = plan(&entity(&[("steam.ron", steam), ("release.ron", "()")]));
        assert_eq!(
            plan,
            [
                (Step::FindOnSteam, Standing::Recorded),
                (Step::SteamRelease, Standing::Recorded),
                (Step::SteamCredits, Standing::Ready),
                (Step::SteamTags, Standing::Ready),
            ]
        );
        let facts = Facts::of(&entity(&[("steam.ron", steam)]));
        let request = Step::SteamTags.request(&facts).unwrap();
        assert!(request.url.contains("/app/1328670"), "{}", request.url);
    }

    #[test]
    fn a_steam_file_without_an_app_unlocks_nothing() {
        let plan = plan(&entity(&[("steam.ron", "Steam(appid: 0)")]));
        assert_eq!(plan[0].1, Standing::Recorded);
        assert_eq!(plan[1].1, Standing::Waiting(vec![Need::SteamApp]));
    }

    #[test]
    fn keys_round_trip() {
        for step in Step::ALL {
            assert_eq!(Step::from_key(step.key()), Some(step));
        }
    }
}
