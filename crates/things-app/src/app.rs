use std::collections::HashMap;

use eframe::egui;
use things_core::search::{Doc, Query, Suggestion, Vocabulary};
use things_core::{EntityId, Store};

use crate::components::{self, Editor, Kind, Raw};

pub struct ThingsApp {
    store: Store,
    kinds: Vec<Box<dyn Kind>>,
    ids: Vec<EntityId>,
    /// What search sees of each entity, kept in step with every write.
    docs: HashMap<EntityId, Doc>,
    search: String,
    /// The last search and what it found, so typing doesn't rescan per frame.
    hits: Option<(String, Vec<EntityId>)>,
    /// Keys and values for completion, rebuilt when the index changes.
    vocab: Option<Vocabulary>,
    completion: Completion,
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

/// Suggestions for the token being typed in the search box.
#[derive(Default)]
struct Completion {
    /// The search line these suggestions were made for.
    line: String,
    items: Vec<Suggestion>,
    selected: usize,
    /// Arrow keys moved the selection, so Enter picks it.
    navigated: bool,
    /// Escape hides the list until the line changes.
    dismissed: bool,
    /// The pointer was over the list last frame; keeps it open for a click.
    hovered: bool,
}

const MAX_SUGGESTIONS: usize = 8;

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
            docs: HashMap::new(),
            search: String::new(),
            hits: None,
            vocab: None,
            completion: Completion::default(),
            new_name: String::new(),
            open: None,
            error: None,
        };
        app.refresh_list();
        app
    }

    /// Re-reads the entity list and every entity for the search index.
    fn refresh_list(&mut self) {
        match self.store.list() {
            Ok(ids) => self.ids = ids,
            Err(e) => self.error = Some(e.to_string()),
        }
        self.docs.clear();
        for id in self.ids.clone() {
            self.reindex(&id);
        }
    }

    fn reindex(&mut self, id: &EntityId) {
        self.hits = None;
        self.vocab = None;
        self.completion.line.clear();
        self.completion.items.clear();
        match self.store.load(id) {
            Ok(entity) => {
                self.docs.insert(id.clone(), Doc::new(&entity));
            }
            Err(e) => {
                self.docs.remove(id);
                self.error = Some(e.to_string());
            }
        }
    }

    /// Entities matching the search line, in library order.
    fn hits(&mut self) -> &[EntityId] {
        if self.hits.as_ref().is_none_or(|(q, _)| *q != self.search) {
            let query = Query::parse(&self.search);
            let hits = self
                .ids
                .iter()
                .filter(|id| {
                    query.is_empty() || self.docs.get(id).is_some_and(|d| query.matches(d))
                })
                .cloned()
                .collect();
            self.hits = Some((self.search.clone(), hits));
        }
        &self.hits.as_ref().expect("just filled").1
    }

    /// Recomputes suggestions when the search line changed.
    fn refresh_completion(&mut self) {
        let vocab = self
            .vocab
            .get_or_insert_with(|| Vocabulary::new(self.docs.values()));
        if self.completion.line == self.search {
            return;
        }
        let items = vocab.suggest(&self.search, MAX_SUGGESTIONS);
        self.completion = Completion {
            line: self.search.clone(),
            items,
            hovered: self.completion.hovered,
            ..Completion::default()
        };
    }

    /// The search box, with suggestions for the token being typed: arrows
    /// pick one, Tab (or Enter after arrows) takes it, Escape hides them.
    fn search_ui(&mut self, ui: &mut egui::Ui) {
        let id = ui.make_persistent_id("search");
        self.refresh_completion();
        let c = &mut self.completion;
        let focused = ui.memory(|m| m.has_focus(id));
        let open = (focused || c.hovered) && !c.dismissed && !c.items.is_empty();

        let mut accept = None;
        if open && focused {
            let n = c.items.len();
            ui.input_mut(|i| {
                use egui::{Key, Modifiers};
                if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                    c.selected = (c.selected + 1) % n;
                    c.navigated = true;
                }
                if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                    c.selected = (c.selected + n - 1) % n;
                    c.navigated = true;
                }
                if i.consume_key(Modifiers::NONE, Key::Tab)
                    || (c.navigated && i.consume_key(Modifiers::NONE, Key::Enter))
                {
                    accept = Some(c.selected);
                }
                if i.consume_key(Modifiers::NONE, Key::Escape) {
                    c.dismissed = true;
                }
            });
        }

        // While the list is up, Tab and Escape belong to it, not to focus.
        let filter = egui::EventFilter {
            tab: open,
            escape: open,
            vertical_arrows: true,
            horizontal_arrows: true,
        };
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .id(id)
                .hint_text("Search: mood:chill -status:finished")
                .event_filter(filter)
                .desired_width(f32::INFINITY),
        )
        .on_hover_ui(|ui| search_help(ui, self.vocab.as_ref()));

        if open {
            let c = &self.completion;
            let list = egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for (i, item) in c.items.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.selectable_label(i == c.selected, &item.label).clicked() {
                            accept = Some(i);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.weak(item.count.to_string());
                        });
                    });
                }
            });
            self.completion.hovered = list.response.contains_pointer();
        } else {
            self.completion.hovered = false;
        }

        if let Some(i) = accept {
            self.search = self.completion.items[i].line.clone();
            self.completion.hovered = false;
            // Keep typing where the suggestion ended.
            let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
            let end = egui::text::CCursor::new(self.search.chars().count());
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(end)));
            state.store(ui.ctx(), id);
            ui.memory_mut(|m| m.request_focus(id));
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
            Err(e) => return self.error = Some(e.to_string()),
        }
        let id = open.id.clone();
        self.reindex(&id);
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
                        if let Err(e) = self.store.list().map(|ids| self.ids = ids) {
                            self.error = Some(e.to_string());
                        }
                        self.reindex(&id);
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
                    Ok(()) => {
                        self.reindex(&id);
                        self.open_entity(id);
                    }
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
                        let id = open.id.clone();
                        self.reindex(&id);
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        }
    }

    fn library_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.heading("Library");
        self.search_ui(ui);
        if !self.search.trim().is_empty() {
            let shown = self.hits().len();
            ui.weak(format!("{shown} of {}", self.ids.len()));
        }
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

        self.hits();
        let hits = &self.hits.as_ref().expect("just filled").1;
        let selected = self.open.as_ref().map(|o| &o.id);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for id in hits {
                let title = id.title();
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

fn search_help(ui: &mut egui::Ui, vocab: Option<&Vocabulary>) {
    ui.label("key:value keeps things whose field contains the value.");
    ui.label("key:a,b matches either; key: alone means it has that field.");
    ui.label("Plain words search titles and everything written.");
    ui.label("Put - in front to exclude, quotes to keep spaces.");
    ui.label("Tab takes a suggestion; arrows pick one, Escape hides them.");
    let keys: Vec<&str> = vocab.into_iter().flat_map(Vocabulary::keys).collect();
    if !keys.is_empty() {
        ui.separator();
        ui.weak(format!("Keys in your library: {}", keys.join(", ")));
    }
}
