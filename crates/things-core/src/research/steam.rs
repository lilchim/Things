//! Reading what Steam's store says about a game. None of these calls need
//! an API key: the store search and app details are the JSON endpoints the
//! store itself uses, and tags come from the app's store page.

use serde::Deserialize;

use crate::components::{Credits, Release, Stage, Steam, Tags, to_ron};

/// Gets past the store's age check, so mature games show their page.
pub const AGE_COOKIE: &str = "birthtime=0; lastagecheckage=1-0-1990; wants_mature_content=1";

/// How many matches a search offers to pick from.
const MAX_MATCHES: usize = 6;
/// How many store tags to keep; the first are the most voted.
const MAX_TAGS: usize = 15;

pub fn search_url(term: &str) -> String {
    format!(
        "https://store.steampowered.com/api/storesearch/?term={}&l=english&cc=US",
        encode(term)
    )
}

pub fn details_url(appid: u32) -> String {
    format!("https://store.steampowered.com/api/appdetails?appids={appid}&l=english&cc=US")
}

pub fn page_url(appid: u32) -> String {
    format!("https://store.steampowered.com/app/{appid}/?l=english")
}

/// One search match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub appid: u32,
    pub name: String,
}

impl Candidate {
    pub fn steam(&self) -> Steam {
        Steam {
            appid: self.appid,
            name: self.name.clone(),
        }
    }

    /// `steam.ron` for this match.
    pub fn text(&self) -> String {
        to_ron(&self.steam()).expect("steam always serialises")
    }
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    items: Vec<SearchItem>,
}

#[derive(Deserialize)]
struct SearchItem {
    #[serde(default, rename = "type")]
    kind: String,
    id: u32,
    name: String,
}

/// Matches from the store search, best first. Only games and other apps,
/// not bundles or packages.
pub fn parse_search(body: &str) -> Result<Vec<Candidate>, String> {
    let response: SearchResponse =
        serde_json::from_str(body).map_err(|e| format!("Steam's search answered oddly: {e}"))?;
    Ok(response
        .items
        .into_iter()
        .filter(|i| i.kind.is_empty() || i.kind == "app")
        .take(MAX_MATCHES)
        .map(|i| Candidate {
            appid: i.id,
            name: i.name,
        })
        .collect())
}

/// What the app details tell us.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
    pub name: String,
    pub credits: Credits,
    pub release: Option<Release>,
}

#[derive(Deserialize)]
struct DetailsEntry {
    success: bool,
    data: Option<DetailsData>,
}

#[derive(Deserialize)]
struct DetailsData {
    #[serde(default)]
    name: String,
    #[serde(default)]
    developers: Vec<String>,
    #[serde(default)]
    publishers: Vec<String>,
    #[serde(default)]
    genres: Vec<Named>,
    release_date: Option<ReleaseDate>,
}

#[derive(Deserialize)]
struct Named {
    #[serde(default)]
    description: String,
}

#[derive(Deserialize)]
struct ReleaseDate {
    #[serde(default)]
    coming_soon: bool,
    #[serde(default)]
    date: String,
}

/// Reads `appdetails` for one app. The response is keyed by app ID.
pub fn parse_details(body: &str, appid: u32) -> Result<Details, String> {
    let mut all: std::collections::HashMap<String, DetailsEntry> =
        serde_json::from_str(body).map_err(|e| format!("Steam's details answered oddly: {e}"))?;
    let entry = all
        .remove(&appid.to_string())
        .ok_or_else(|| format!("Steam sent no details for app {appid}."))?;
    let data = entry.data.filter(|_| entry.success).ok_or_else(|| {
        format!("Steam has no details for app {appid} (region-locked or removed?).")
    })?;
    let early = data
        .genres
        .iter()
        .any(|g| g.description.eq_ignore_ascii_case("early access"));
    let release = data.release_date.map(|r| {
        let stage = if early {
            Stage::EarlyAccess
        } else if r.coming_soon {
            Stage::Announced
        } else {
            Stage::Released
        };
        Release {
            date: steam_date(&r.date),
            stage,
        }
    });
    let clean = |names: Vec<String>| -> Vec<String> {
        names
            .into_iter()
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty())
            .collect()
    };
    Ok(Details {
        name: data.name,
        credits: Credits {
            developers: clean(data.developers),
            publishers: clean(data.publishers),
        },
        release,
    })
}

/// Turns the store's English dates into `YYYY-MM-DD`, `YYYY-MM` or `YYYY`:
/// "14 May, 2021", "May 14, 2021", "Oct 2026", "Q4 2026", "2027". Anything
/// vaguer ("Coming soon", "To be announced") is an unknown date.
pub fn steam_date(text: &str) -> String {
    let words: Vec<String> = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let year = words
        .iter()
        .find(|w| w.len() == 4 && w.bytes().all(|b| b.is_ascii_digit()));
    let Some(year) = year else {
        return String::new();
    };
    let month = words.iter().find_map(|w| month_number(w));
    let day = words
        .iter()
        .filter(|w| w.len() <= 2)
        .find_map(|w| w.parse::<u32>().ok())
        .filter(|d| (1..=31).contains(d));
    match (month, day) {
        (Some(m), Some(d)) => format!("{year}-{m:02}-{d:02}"),
        (Some(m), None) => format!("{year}-{m:02}"),
        _ => year.clone(),
    }
}

fn month_number(word: &str) -> Option<u32> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let word = word.trim_end_matches('.');
    if word.len() < 3 {
        return None;
    }
    MONTHS
        .iter()
        .position(|m| word.starts_with(m))
        .map(|i| i as u32 + 1)
}

/// The user tags on a store page, most voted first. They're links with
/// class `app_tag`; the "+" button shares the class name with more after it.
pub fn parse_tags(html: &str) -> Tags {
    let mut tags = Tags::default();
    let marker = "class=\"app_tag\"";
    let mut rest = html;
    while let Some(at) = rest.find(marker) {
        rest = &rest[at + marker.len()..];
        let Some(open) = rest.find('>') else { break };
        let after = &rest[open + 1..];
        let Some(close) = after.find('<') else { break };
        let name = decode(after[..close].trim());
        if name != "+" && tags.tags.len() < MAX_TAGS {
            tags.add(&name);
        }
        rest = &after[close..];
    }
    tags
}

/// The few HTML entities that turn up in tag names.
fn decode(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// Percent-encodes a search term for a query string.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.trim().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH: &str = include_str!("../../tests/fixtures/steam-search.json");
    const DETAILS: &str = include_str!("../../tests/fixtures/steam-details.json");
    const PAGE: &str = include_str!("../../tests/fixtures/steam-page.html");

    #[test]
    fn search_offers_apps_best_first() {
        let found = parse_search(SEARCH).unwrap();
        assert_eq!(found[0].appid, 1328670);
        assert_eq!(found[0].name, "Mass Effect™ Legendary Edition");
        // The bundle in the response is skipped.
        assert!(found.iter().all(|c| c.appid != 99999));
        assert_eq!(found.len(), 3);
        assert!(
            parse_search("{\"total\":0,\"items\":[]}")
                .unwrap()
                .is_empty()
        );
        assert!(parse_search("<html>").is_err());
    }

    #[test]
    fn details_give_credits_and_release() {
        let d = parse_details(DETAILS, 1328670).unwrap();
        assert_eq!(d.credits.developers, ["BioWare"]);
        assert_eq!(d.credits.publishers, ["Electronic Arts"]);
        assert_eq!(
            d.release,
            Some(Release {
                date: "2021-05-14".into(),
                stage: Stage::Released
            })
        );
        assert!(parse_details(DETAILS, 1).is_err());
        let gone = r#"{"5": {"success": false}}"#;
        assert!(parse_details(gone, 5).is_err());
    }

    #[test]
    fn early_access_and_upcoming_games() {
        let early = r#"{"7": {"success": true, "data": {
            "genres": [{"id": "70", "description": "Early Access"}],
            "release_date": {"coming_soon": false, "date": "Jan 3, 2024"}}}}"#;
        let release = parse_details(early, 7).unwrap().release.unwrap();
        assert_eq!(release.stage, Stage::EarlyAccess);
        assert_eq!(release.date, "2024-01-03");
        let soon = r#"{"8": {"success": true, "data": {
            "release_date": {"coming_soon": true, "date": "Coming soon"}}}}"#;
        let release = parse_details(soon, 8).unwrap().release.unwrap();
        assert_eq!(release.stage, Stage::Announced);
        assert_eq!(release.date, "");
    }

    #[test]
    fn steam_dates_become_ours() {
        for (steam, ours) in [
            ("14 May, 2021", "2021-05-14"),
            ("May 14, 2021", "2021-05-14"),
            ("3 Sep, 2025", "2025-09-03"),
            ("Oct 2026", "2026-10"),
            ("October 2026", "2026-10"),
            ("Q4 2026", "2026"),
            ("2027", "2027"),
            ("Coming soon", ""),
            ("To be announced", ""),
            ("", ""),
        ] {
            assert_eq!(steam_date(steam), ours, "{steam}");
            assert!(
                Release {
                    date: steam_date(steam),
                    stage: Stage::Announced
                }
                .date_is_valid()
            );
        }
    }

    #[test]
    fn tags_come_from_the_store_page() {
        let tags = parse_tags(PAGE);
        assert_eq!(
            tags.tags,
            [
                "rpg",
                "story rich",
                "sci-fi",
                "choices matter",
                "action & adventure"
            ]
        );
        assert!(parse_tags("<html>no tags</html>").tags.is_empty());
    }

    #[test]
    fn search_terms_are_encoded() {
        assert_eq!(
            search_url("Mass Effect: 2"),
            "https://store.steampowered.com/api/storesearch/?term=Mass+Effect%3A+2&l=english&cc=US"
        );
    }
}
