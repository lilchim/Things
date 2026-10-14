use eframe::egui;
use things_core::components::{Steam, from_ron, to_ron};

use super::{Editor, Kind, single_file};

const FILE: &str = "steam.ron";

pub struct SteamKind;

impl Kind for SteamKind {
    fn label(&self, _path: &str) -> String {
        "Steam".into()
    }

    fn matches(&self, path: &str) -> bool {
        path == FILE
    }

    fn open(&self, text: &str) -> Result<Box<dyn Editor>, String> {
        let steam: Steam = from_ron(text).map_err(|e| e.to_string())?;
        Ok(Box::new(steam))
    }

    fn create(&self, existing: &[&str]) -> Option<(String, String)> {
        single_file(FILE, existing, Steam::default().text())
    }
}

impl Editor for Steam {
    fn ui(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("steam").num_columns(2).show(ui, |ui| {
            ui.label("Store name");
            ui.add(
                egui::TextEdit::singleline(&mut self.name)
                    .hint_text("as Steam lists it")
                    .desired_width(f32::INFINITY),
            );
            ui.end_row();

            ui.label("App ID");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut self.appid).speed(0.0));
                if self.appid != 0 {
                    ui.hyperlink_to("Store page", self.store_url());
                } else {
                    ui.weak("the number in the store page's address");
                }
            });
            ui.end_row();
        });
    }

    fn text(&self) -> String {
        to_ron(self).expect("steam always serialises")
    }
}
