use eframe::egui;
use things_core::components::{PlayState, Status, from_ron, to_ron};

use super::{Editor, Kind, single_file};

const FILE: &str = "status.ron";

pub struct StatusKind;

impl Kind for StatusKind {
    fn label(&self, _path: &str) -> String {
        "Play status".into()
    }

    fn matches(&self, path: &str) -> bool {
        path == FILE
    }

    fn open(&self, text: &str) -> Result<Box<dyn Editor>, String> {
        let status: Status = from_ron(text).map_err(|e| e.to_string())?;
        Ok(Box::new(status))
    }

    fn create(&self, existing: &[&str]) -> Option<(String, String)> {
        single_file(FILE, existing, Status::default().text())
    }
}

impl Editor for Status {
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for state in PlayState::ALL {
                ui.selectable_value(&mut self.state, state, state.label());
            }
        });
    }

    fn text(&self) -> String {
        to_ron(self).expect("status always serialises")
    }
}
