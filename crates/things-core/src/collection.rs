//! Collections keep different kinds of things apart: games in one, tasks in
//! another. A collection is a top-level folder of the data folder holding a
//! `collection.ron` and the entity folders that belong to it:
//!
//! ```text
//! things/
//!   games/
//!     collection.ron        Collection(name: "Games", starts_with: [...], radar: true)
//!     mass-effect-7f3a/
//!   tasks/
//!     collection.ron
//!     mow-the-lawn-1a2b/
//! ```
//!
//! A collection gives its things no schema. It names them, says which
//! components a new one starts with, and says where they show up (the
//! radar). Entity IDs stay unique across collections, so a link to
//! `mass-effect-7f3a` keeps working when the game moves.

use serde::{Deserialize, Serialize};

/// The manifest file that makes a folder a collection.
pub const MANIFEST: &str = "collection.ron";

/// `collection.ron`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename = "Collection")]
pub struct Manifest {
    /// Shown in the app: "Games".
    pub name: String,
    /// Component files a new thing in this collection gets, such as
    /// `status.ron`. Each starts out empty the way "Add component" makes it.
    pub starts_with: Vec<String>,
    /// Whether things here show up on the radar.
    pub radar: bool,
}

impl Manifest {
    /// What the first launch makes, and what an old `entities/` folder becomes.
    pub fn games() -> Self {
        Self {
            name: "Games".into(),
            starts_with: vec!["status.ron".into(), "moods.ron".into()],
            radar: true,
        }
    }

    pub fn named(name: &str) -> Self {
        Self {
            name: name.trim().to_owned(),
            ..Self::default()
        }
    }
}

/// A collection on disk: its folder name and its manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    /// The folder name, such as `games`. This is what search sees as
    /// `collection:games`.
    pub folder: String,
    pub manifest: Manifest,
}

impl Collection {
    /// The manifest's name, else one made from the folder.
    pub fn name(&self) -> String {
        let name = self.manifest.name.trim();
        if name.is_empty() {
            crate::id::title_case(&self.folder)
        } else {
            name.to_owned()
        }
    }
}
