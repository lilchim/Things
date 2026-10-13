use eframe::egui;
use things_core::components::{Release, Stage, from_ron, to_ron};

use super::{Editor, Kind, single_file};

const FILE: &str = "release.ron";

pub struct ReleaseKind;

impl Kind for ReleaseKind {
    fn label(&self, _path: &str) -> String {
        "Release".into()
    }

    fn matches(&self, path: &str) -> bool {
        path == FILE
    }

    fn open(&self, text: &str) -> Result<Box<dyn Editor>, String> {
        let release: Release = from_ron(text).map_err(|e| e.to_string())?;
        Ok(Box::new(release))
    }

    fn create(&self, existing: &[&str]) -> Option<(String, String)> {
        single_file(FILE, existing, Release::default().text())
    }
}

impl Editor for Release {
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for stage in Stage::ALL {
                ui.selectable_value(&mut self.stage, stage, stage.label());
            }
        });
        ui.horizontal(|ui| {
            ui.label("Date");
            ui.add(
                egui::TextEdit::singleline(&mut self.date)
                    .hint_text("2026-10-19, 2026-10 or 2027")
                    .desired_width(140.0),
            );
            if !self.date_is_valid() {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "Use YYYY-MM-DD, YYYY-MM or YYYY",
                );
            } else if self.date.trim().is_empty() {
                ui.weak("unknown");
            }
        });
    }

    fn text(&self) -> String {
        to_ron(self).expect("release always serialises")
    }
}
