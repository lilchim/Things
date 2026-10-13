use eframe::egui;
use things_core::components::{Tags, from_ron, to_ron};

use super::{Editor, Kind, single_file};

/// One tag set (mood, gameplay, ...). All sets share the [`Tags`] format.
pub struct TagSetKind {
    pub file: &'static str,
    pub label: &'static str,
}

impl Kind for TagSetKind {
    fn label(&self, _path: &str) -> String {
        format!("{} tags", self.label)
    }

    fn matches(&self, path: &str) -> bool {
        path == self.file
    }

    fn open(&self, text: &str) -> Result<Box<dyn Editor>, String> {
        let tags: Tags = from_ron(text).map_err(|e| e.to_string())?;
        Ok(Box::new(TagsEditor {
            tags,
            input: String::new(),
        }))
    }

    fn create(&self, existing: &[&str]) -> Option<(String, String)> {
        let text = to_ron(&Tags::default()).expect("tags always serialise");
        single_file(self.file, existing, text)
    }
}

struct TagsEditor {
    tags: Tags,
    input: String,
}

impl Editor for TagsEditor {
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let mut removed = None;
            for (i, tag) in self.tags.tags.iter().enumerate() {
                if ui
                    .small_button(format!("{tag}  ×"))
                    .on_hover_text("Remove")
                    .clicked()
                {
                    removed = Some(i);
                }
            }
            if let Some(i) = removed {
                self.tags.tags.remove(i);
            }

            let field = ui.add(
                egui::TextEdit::singleline(&mut self.input)
                    .hint_text("add tag")
                    .desired_width(120.0),
            );
            if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.tags.add(&self.input);
                self.input.clear();
                field.request_focus();
            }
        });
    }

    fn text(&self) -> String {
        to_ron(&self.tags).expect("tags always serialise")
    }
}
