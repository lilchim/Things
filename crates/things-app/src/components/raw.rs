use eframe::egui;

use super::Editor;

/// Plain text editing for files no kind claims, or that failed to parse.
pub struct Raw {
    text: String,
    problem: Option<String>,
}

impl Raw {
    pub fn new(text: &str, problem: Option<String>) -> Self {
        Self {
            text: text.to_owned(),
            problem,
        }
    }
}

impl Editor for Raw {
    fn ui(&mut self, ui: &mut egui::Ui) {
        if let Some(problem) = &self.problem {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!("Couldn't read this file: {problem}"),
            );
        }
        ui.add(
            egui::TextEdit::multiline(&mut self.text)
                .code_editor()
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
    }

    fn text(&self) -> String {
        self.text.clone()
    }
}
