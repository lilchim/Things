use eframe::egui;

use super::{Editor, Kind, single_file};

const NOTES: &str = "notes.md";
const JOURNAL_DIR: &str = "journal/";

/// `notes.md`: a free Markdown document about the thing.
pub struct NotesKind;

impl Kind for NotesKind {
    fn label(&self, _path: &str) -> String {
        "Notes".into()
    }

    fn matches(&self, path: &str) -> bool {
        path == NOTES
    }

    fn open(&self, text: &str) -> Result<Box<dyn Editor>, String> {
        Ok(Box::new(Markdown(text.to_owned())))
    }

    fn create(&self, existing: &[&str]) -> Option<(String, String)> {
        single_file(NOTES, existing, String::new())
    }
}

/// `journal/<date>-<time>.md`: one Markdown file per entry.
pub struct JournalKind;

impl Kind for JournalKind {
    fn label(&self, path: &str) -> String {
        let stem = path
            .strip_prefix(JOURNAL_DIR)
            .and_then(|p| p.strip_suffix(".md"))
            .unwrap_or(path);
        format!(
            "Journal · {}",
            entry_date(stem).unwrap_or_else(|| stem.to_owned())
        )
    }

    fn matches(&self, path: &str) -> bool {
        path.strip_prefix(JOURNAL_DIR)
            .is_some_and(|rest| rest.ends_with(".md") && !rest.contains('/'))
    }

    fn open(&self, text: &str) -> Result<Box<dyn Editor>, String> {
        Ok(Box::new(Markdown(text.to_owned())))
    }

    fn create(&self, existing: &[&str]) -> Option<(String, String)> {
        let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M").to_string();
        let path = (1..)
            .map(|n| match n {
                1 => format!("{JOURNAL_DIR}{stamp}.md"),
                n => format!("{JOURNAL_DIR}{stamp}-{n}.md"),
            })
            .find(|p| !existing.contains(&p.as_str()))?;
        Some((path, String::new()))
    }

    fn newest_first(&self) -> bool {
        true
    }
}

/// `2026-10-13-1930` -> `2026-10-13 19:30`.
fn entry_date(stem: &str) -> Option<String> {
    let (date, time) = stem.get(..10).zip(stem.get(11..15))?;
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let time = chrono::NaiveTime::parse_from_str(time, "%H%M").ok()?;
    Some(format!("{date} {}", time.format("%H:%M")))
}

struct Markdown(String);

impl Editor for Markdown {
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.add(
            egui::TextEdit::multiline(&mut self.0)
                .hint_text("Write in Markdown…")
                .desired_rows(4)
                .desired_width(f32::INFINITY),
        );
    }

    fn text(&self) -> String {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_labels_show_the_entry_time() {
        assert_eq!(
            JournalKind.label("journal/2026-10-13-1930.md"),
            "Journal · 2026-10-13 19:30"
        );
        assert_eq!(
            JournalKind.label("journal/2026-10-13-1930-2.md"),
            "Journal · 2026-10-13 19:30"
        );
        assert_eq!(JournalKind.label("journal/trip.md"), "Journal · trip");
    }

    #[test]
    fn journal_matches_only_entries() {
        assert!(JournalKind.matches("journal/2026-10-13-1930.md"));
        assert!(!JournalKind.matches("journal/old/2026-10-13-1930.md"));
        assert!(!JournalKind.matches("notes.md"));
    }

    #[test]
    fn new_journal_entries_never_overwrite() {
        let (first, _) = JournalKind.create(&[]).unwrap();
        let (second, _) = JournalKind.create(&[first.as_str()]).unwrap();
        assert_ne!(first, second);
        assert!(second.ends_with("-2.md"));
    }
}
