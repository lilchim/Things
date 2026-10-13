//! Component kinds the app knows how to show and edit.
//!
//! A kind recognises its files by path, opens them into an [`Editor`] and
//! knows how to start a new one. Adding a feature means adding a kind here;
//! files no kind claims still open in the raw text editor, so nothing on
//! disk is ever hidden or lost.

mod markdown;
mod raw;
mod status;
mod tags;

use eframe::egui;

pub use raw::Raw;

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
        Box::new(tags::TagSetKind {
            file: "moods.ron",
            label: "Mood",
        }),
        Box::new(tags::TagSetKind {
            file: "gameplay.ron",
            label: "Gameplay",
        }),
        Box::new(markdown::NotesKind),
        Box::new(markdown::JournalKind),
    ]
}

/// Shared by kinds that live in exactly one file.
fn single_file(file: &str, existing: &[&str], text: String) -> Option<(String, String)> {
    (!existing.contains(&file)).then(|| (file.to_owned(), text))
}
