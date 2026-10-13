use eframe::egui;
use things_core::{EntityId, Store};

use crate::components::{self, Editor, Kind, Raw};

pub struct ThingsApp {
    store: Store,
    kinds: Vec<Box<dyn Kind>>,
    ids: Vec<EntityId>,
    filter: String,
    new_name: String,
    open: Option<OpenEntity>,
    error: Option<String>,
}

/// The entity on screen. Editors hold the working state; the files are only
/// touched on save.
struct OpenEntity {
    id: EntityId,
    components: Vec<OpenComponent>,
}

struct OpenComponent {
    path: String,
    label: String,
    /// What's on disk, to tell whether there are unsaved changes.
    saved: String,
    editor: Box<dyn Editor>,
    confirm_delete: bool,
}

impl OpenComponent {
    fn dirty(&self) -> bool {
        self.editor.text() != self.saved
    }
}

enum Action {
    Select(EntityId),
    Create,
    Reload,
    Add(String, String),
    Save(usize),
    SaveAll,
    Delete(usize),
}

impl ThingsApp {
    pub fn new(store: Store) -> Self {
        let mut app = Self {
            store,
            kinds: components::registry(),
            ids: Vec::new(),
            filter: String::new(),
            new_name: String::new(),
            open: None,
            error: None,
        };
        app.refresh_list();
        app
    }

    fn refresh_list(&mut self) {
        match self.store.list() {
            Ok(ids) => self.ids = ids,
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Reads an entity's files and opens each with the kind that claims it.
    fn open_entity(&mut self, id: EntityId) {
        let entity = match self.store.load(&id) {
            Ok(entity) => entity,
            Err(e) => return self.error = Some(e.to_string()),
        };
        let mut components: Vec<(usize, OpenComponent)> = entity
            .components
            .into_iter()
            .map(|file| {
                let kind = self.kinds.iter().position(|k| k.matches(&file.path));
                let (label, editor) = match kind.map(|i| &self.kinds[i]) {
                    Some(kind) => (
                        kind.label(&file.path),
                        kind.open(&file.text)
                            .unwrap_or_else(|e| Box::new(Raw::new(&file.text, Some(e)))),
                    ),
                    None => (file.path.clone(), Box::new(Raw::new(&file.text, None)) as _),
                };
                let open = OpenComponent {
                    path: file.path,
                    label,
                    saved: file.text,
                    editor,
                    confirm_delete: false,
                };
                (kind.unwrap_or(usize::MAX), open)
            })
            .collect();
        // Registry order, then by path (newest first where the kind asks).
        components.sort_by(|(ka, a), (kb, b)| {
            let newest_first = self.kinds.get(*ka).is_some_and(|k| k.newest_first());
            let by_path = if newest_first {
                b.path.cmp(&a.path)
            } else {
                a.path.cmp(&b.path)
            };
            ka.cmp(kb).then(by_path)
        });
        self.open = Some(OpenEntity {
            id,
            components: components.into_iter().map(|(_, c)| c).collect(),
        });
    }

    fn save(&mut self, index: usize) {
        let Some(open) = &mut self.open else { return };
        let component = &mut open.components[index];
        let text = component.editor.text();
        match self.store.write(&open.id, &component.path, &text) {
            Ok(()) => component.saved = text,
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn save_all(&mut self) {
        let count = self.open.as_ref().map_or(0, |o| o.components.len());
        for i in 0..count {
            if self.open.as_ref().is_some_and(|o| o.components[i].dirty()) {
                self.save(i);
            }
        }
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::Select(id) => {
                // Leaving an entity keeps its edits rather than dropping them.
                self.save_all();
                self.open_entity(id);
            }
            Action::Create => {
                let name = self.new_name.trim().to_owned();
                if name.is_empty() {
                    return;
                }
                self.save_all();
                match self.store.create(&name) {
                    Ok(id) => {
                        self.new_name.clear();
                        self.refresh_list();
                        self.open_entity(id);
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            Action::Reload => {
                self.refresh_list();
                if let Some(id) = self.open.as_ref().map(|o| o.id.clone()) {
                    self.open_entity(id);
                }
            }
            Action::Add(path, text) => {
                let Some(id) = self.open.as_ref().map(|o| o.id.clone()) else {
                    return;
                };
                self.save_all();
                match self.store.write(&id, &path, &text) {
                    Ok(()) => self.open_entity(id),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            Action::Save(i) => self.save(i),
            Action::SaveAll => self.save_all(),
            Action::Delete(i) => {
                let Some(open) = &mut self.open else { return };
                let path = open.components[i].path.clone();
                match self.store.remove(&open.id, &path) {
                    Ok(()) => {
                        open.components.remove(i);
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        }
    }

    fn library_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.heading("Library");
        ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .hint_text("Filter")
                .desired_width(f32::INFINITY),
        );
        ui.separator();

        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.new_name)
                    .hint_text("New thing")
                    .desired_width(ui.available_width() - 50.0),
            );
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Add").clicked() || enter {
                actions.push(Action::Create);
            }
        });
        ui.separator();

        let filter = self.filter.to_lowercase();
        let selected = self.open.as_ref().map(|o| &o.id);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for id in &self.ids {
                let title = id.title();
                if !filter.is_empty() && !title.to_lowercase().contains(&filter) {
                    continue;
                }
                let response = ui
                    .selectable_label(selected == Some(id), title)
                    .on_hover_text(id.to_string());
                if response.clicked() && selected != Some(id) {
                    actions.push(Action::Select(id.clone()));
                }
            }
            if self.ids.is_empty() {
                ui.weak("Nothing here yet. Add a thing above.");
            }
        });
    }

    fn entity_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let Some(open) = &mut self.open else {
            ui.centered_and_justified(|ui| ui.weak("Pick a thing from the library."));
            return;
        };

        ui.horizontal(|ui| {
            ui.heading(open.id.title());
            ui.weak(open.id.to_string());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let existing: Vec<&str> = open.components.iter().map(|c| c.path.as_str()).collect();
                ui.menu_button("Add component", |ui| {
                    for kind in &self.kinds {
                        if let Some((path, text)) = kind.create(&existing)
                            && ui.button(kind.label(&path)).clicked()
                        {
                            actions.push(Action::Add(path, text));
                        }
                    }
                });
                if ui
                    .button("Reload")
                    .on_hover_text("Re-read files from disk")
                    .clicked()
                {
                    actions.push(Action::Reload);
                }
                let any_dirty = open.components.iter().any(OpenComponent::dirty);
                if ui
                    .add_enabled(any_dirty, egui::Button::new("Save all"))
                    .clicked()
                {
                    actions.push(Action::SaveAll);
                }
            });
        });
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            if open.components.is_empty() {
                ui.weak("No components yet. Use “Add component” to give this thing some.");
            }
            for (i, component) in open.components.iter_mut().enumerate() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.strong(&component.label);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if component.confirm_delete {
                                if ui.small_button("Cancel").clicked() {
                                    component.confirm_delete = false;
                                }
                                if ui.small_button("Delete file").clicked() {
                                    actions.push(Action::Delete(i));
                                }
                            } else if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                                component.confirm_delete = true;
                            }
                            if component.dirty() && ui.small_button("Save").clicked() {
                                actions.push(Action::Save(i));
                            }
                            ui.weak(&component.path);
                        });
                    });
                    component.editor.ui(ui);
                });
                ui.add_space(6.0);
            }
        });
    }
}

impl eframe::App for ThingsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut actions = Vec::new();

        let save_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);
        if ui.input_mut(|i| i.consume_shortcut(&save_shortcut)) {
            actions.push(Action::SaveAll);
        }

        if let Some(error) = self.error.clone() {
            egui::Panel::top("error").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                    if ui.small_button("Dismiss").clicked() {
                        self.error = None;
                    }
                });
            });
        }

        egui::Panel::left("library")
            .resizable(true)
            .default_size(240.0)
            .show(ui, |ui| self.library_ui(ui, &mut actions));

        egui::CentralPanel::default_margins().show(ui, |ui| self.entity_ui(ui, &mut actions));

        for action in actions {
            self.apply(action);
        }
    }

    fn on_exit(&mut self) {
        self.save_all();
    }
}
