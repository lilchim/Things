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
}
