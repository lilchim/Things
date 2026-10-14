use eframe::egui;
use things_core::components::{Credits, from_ron, to_ron};

use super::{Editor, Kind, single_file};

const FILE: &str = "credits.ron";

pub struct CreditsKind;

impl Kind for CreditsKind {
    fn label(&self, _path: &str) -> String {
        "Credits".into()
    }

    fn matches(&self, path: &str) -> bool {
        path == FILE
    }

    fn open(&self, text: &str) -> Result<Box<dyn Editor>, String> {
        let credits: Credits = from_ron(text).map_err(|e| e.to_string())?;
        Ok(Box::new(CreditsEditor {
            developers: credits.developers.join(", "),
            publishers: credits.publishers.join(", "),
        }))
    }

    fn create(&self, existing: &[&str]) -> Option<(String, String)> {
        let text = to_ron(&Credits::default()).expect("credits always serialise");
        single_file(FILE, existing, text)
    }
}

/// Names are edited as comma-separated lines.
struct CreditsEditor {
    developers: String,
    publishers: String,
}

impl Editor for CreditsEditor {
    fn ui(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("credits").num_columns(2).show(ui, |ui| {
            for (label, names) in [
                ("Developer", &mut self.developers),
                ("Publisher", &mut self.publishers),
            ] {
                ui.label(label);
                ui.add(
                    egui::TextEdit::singleline(names)
                        .hint_text("separate several with commas")
                        .desired_width(f32::INFINITY),
                );
                ui.end_row();
            }
        });
    }

    fn text(&self) -> String {
        let split = |s: &str| -> Vec<String> {
            s.split(',')
                .map(|n| n.trim().to_owned())
                .filter(|n| !n.is_empty())
                .collect()
        };
        let credits = Credits {
            developers: split(&self.developers),
            publishers: split(&self.publishers),
        };
        to_ron(&credits).expect("credits always serialise")
    }
}
