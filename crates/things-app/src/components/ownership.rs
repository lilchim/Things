use eframe::egui;
use things_core::components::{OwnState, Ownership, from_ron, to_ron};

use super::{Editor, Kind, single_file};

const FILE: &str = "ownership.ron";

pub struct OwnershipKind;

impl Kind for OwnershipKind {
    fn label(&self, _path: &str) -> String {
        "Ownership".into()
    }

    fn matches(&self, path: &str) -> bool {
        path == FILE
    }

    fn open(&self, text: &str) -> Result<Box<dyn Editor>, String> {
        let ownership: Ownership = from_ron(text).map_err(|e| e.to_string())?;
        Ok(Box::new(ownership))
    }

    fn create(&self, existing: &[&str]) -> Option<(String, String)> {
        single_file(FILE, existing, interested("", "").text())
    }
}

/// A new "want to try this" record, dated today.
pub fn interested(via: &str, link: &str) -> Ownership {
    Ownership {
        state: OwnState::Interested,
        added: chrono::Local::now().format("%Y-%m-%d").to_string(),
        via: via.trim().to_owned(),
        link: link.trim().to_owned(),
    }
}

impl Editor for Ownership {
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for state in OwnState::ALL {
                ui.selectable_value(&mut self.state, state, state.label());
            }
        });
        egui::Grid::new("ownership").num_columns(2).show(ui, |ui| {
            ui.label("Seen via");
            ui.add(
                egui::TextEdit::singleline(&mut self.via)
                    .hint_text("a friend's link, a tweet, a trailer…")
                    .desired_width(f32::INFINITY),
            );
            ui.end_row();

            ui.label("Link");
            ui.horizontal(|ui| {
                let link = self.link.trim().to_owned();
                let open = !link.is_empty();
                ui.add(
                    egui::TextEdit::singleline(&mut self.link)
                        .hint_text("https://…")
                        .desired_width(ui.available_width() - if open { 50.0 } else { 0.0 }),
                );
                if open {
                    ui.hyperlink_to("Open", link);
                }
            });
            ui.end_row();

            ui.label("Added");
            ui.add(
                egui::TextEdit::singleline(&mut self.added)
                    .hint_text("YYYY-MM-DD")
                    .desired_width(100.0),
            );
            ui.end_row();
        });
    }

    fn text(&self) -> String {
        to_ron(self).expect("ownership always serialises")
    }
}
