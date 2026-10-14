//! Component kinds the app knows how to show and edit.
//!
//! A kind recognises its files by path, opens them into an [`Editor`] and
//! knows how to start a new one. Adding a feature means adding a kind here;
//! files no kind claims still open in the raw text editor, so nothing on
//! disk is ever hidden or lost.

mod credits;
mod markdown;
mod ownership;
mod raw;
mod release;
mod status;
mod steam;
mod tags;
mod tasks;

use eframe::egui;

pub use ownership::interested;
pub use raw::Raw;
pub use tasks::{describe, now};

/// The in-memory, editable form of one component file.
pub trait Editor {
    /// Draws the component's view and form.
    fn ui(&mut self, ui: &mut egui::Ui);
    /// The file contents for the current state.
    fn text(&self) -> String;
}

pub trait Kind {
    /// Heading shown above the component.
    fn label(&self, path: &str) -> String;
    fn matches(&self, path: &str) -> bool;
    fn open(&self, text: &str) -> Result<Box<dyn Editor>, String>;
    /// Path and contents for a new component of this kind, or `None` when
    /// the entity can't take another one.
    fn create(&self, existing: &[&str]) -> Option<(String, String)>;
    /// Several files of this kind are shown newest first (journal entries).
    fn newest_first(&self) -> bool {
        false
    }
}

/// Every known kind, in the order they appear on an entity page.
pub fn registry() -> Vec<Box<dyn Kind>> {
    vec![
        Box::new(status::StatusKind),
        Box::new(ownership::OwnershipKind),
        Box::new(steam::SteamKind),
        Box::new(release::ReleaseKind),
        Box::new(credits::CreditsKind),
        Box::new(tasks::TasksKind),
        Box::new(tags::TagSetKind {
            file: "moods.ron",
            label: "Mood",
        }),
        Box::new(tags::TagSetKind {
            file: "gameplay.ron",
            label: "Gameplay",
        }),
        Box::new(tags::TagSetKind {
            file: "steam-tags.ron",
            label: "Steam",
        }),
        Box::new(markdown::NotesKind),
        Box::new(markdown::JournalKind),
    ]
}

/// The files a new thing starts with, for a collection's `starts_with`:
/// each made the way "Add component" makes it. A path no kind makes starts
/// out as an empty file, so a collection can name components the app
/// doesn't know yet.
pub fn starting_files(kinds: &[Box<dyn Kind>], paths: &[String]) -> Vec<(String, String)> {
    paths
        .iter()
        .map(|path| {
            let made = kinds
                .iter()
                .find(|k| k.matches(path))
                .and_then(|k| k.create(&[]));
            let text = match made {
                Some((made, text)) if made == *path => text,
                _ => String::new(),
            };
            (path.clone(), text)
        })
        .collect()
}

/// Shared by kinds that live in exactly one file.
fn single_file(file: &str, existing: &[&str], text: String) -> Option<(String, String)> {
    (!existing.contains(&file)).then(|| (file.to_owned(), text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_files_use_each_kind_or_start_empty() {
        let paths = ["status.ron", "notes.md", "due.ron"].map(String::from);
        let files = starting_files(&registry(), &paths);
        assert_eq!(
            files[0],
            (
                "status.ron".into(),
                "Status(\n    state: Backlog,\n)\n".into()
            )
        );
        assert_eq!(files[1], ("notes.md".into(), String::new()));
        assert_eq!(files[2], ("due.ron".into(), String::new()));
    }
}
