use std::collections::HashMap;

use eframe::egui;
use things_core::components::{OwnState, PlayState, Release, Stage, Status, to_ron};
use things_core::radar::{Radar, Watch};
use things_core::search::{Doc, Query, Suggestion, Vocabulary};
use things_core::{EntityId, Store};

use crate::components::{self, Editor, Kind, Raw};

pub struct ThingsApp {
    store: Store,
    kinds: Vec<Box<dyn Kind>>,
    ids: Vec<EntityId>,
    /// What search sees of each entity, kept in step with every write.
    docs: HashMap<EntityId, Doc>,
    /// Ownership and release of each entity that has them, for the radar.
    watches: HashMap<EntityId, Watch>,
    /// The radar and the day it was sorted for, rebuilt on change.
    radar: Option<(String, Radar)>,
    spot: Spot,
    /// The last radar triage, so a misclick can be taken back.
    undo: Option<Undo>,
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

/// What to do with a thing on the radar, like answering an Anki card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Triage {
    /// Hide it for a while, longer each time.
    Later,
    /// Bought it and starting now: Owned, and play status Playing.
    PlayNow,
    /// Off the radar for good.
    NotInterested,
    /// Back from a snooze early.
    ShowNow,
}

/// Files as they were before a triage. `None` means the file didn't exist.
struct Undo {
    id: EntityId,
    message: String,
    files: Vec<(&'static str, Option<String>)>,
}

/// The quick-capture form on the radar: something seen in passing.
#[derive(Default)]
struct Spot {
    name: String,
    via: String,
    link: String,
    /// Optional release date; filled in means it isn't out yet.
    date: String,
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
    /// Close the open entity and show the radar.
    Home,
    /// Create an entity from the quick-capture form.
    Spot,
    Triage(EntityId, Triage),
    Undo,
}

impl ThingsApp {
    pub fn new(store: Store) -> Self {
        let mut app = Self {
            store,
            kinds: components::registry(),
            ids: Vec::new(),
            docs: HashMap::new(),
            watches: HashMap::new(),
            radar: None,
            spot: Spot::default(),
            undo: None,
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
        self.watches.clear();
        for id in self.ids.clone() {
            self.reindex(&id);
        }
    }

    fn reindex(&mut self, id: &EntityId) {
        self.hits = None;
        self.vocab = None;
        self.radar = None;
        self.completion.line.clear();
        self.completion.items.clear();
        match self.store.load(id) {
            Ok(entity) => {
                self.docs.insert(id.clone(), Doc::new(&entity));
                match Watch::new(&entity) {
                    Some(watch) => self.watches.insert(id.clone(), watch),
                    None => self.watches.remove(id),
                };
            }
            Err(e) => {
                self.docs.remove(id);
                self.watches.remove(id);
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
            Action::Home => {
                self.save_all();
                self.open = None;
            }
            Action::Spot => self.spot(),
            Action::Triage(id, triage) => self.triage(id, triage),
            Action::Undo => self.undo(),
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

    /// Creates an entity marked Interested from the quick-capture form, with
    /// a release if a date was given, and stays on the radar for the next.
    fn spot(&mut self) {
        let name = self.spot.name.trim().to_owned();
        if name.is_empty() {
            return;
        }
        let release = Release {
            date: self.spot.date.trim().to_owned(),
            stage: Stage::Announced,
        };
        if !release.date_is_valid() {
            return self.error = Some("Release date must be YYYY-MM-DD, YYYY-MM or YYYY.".into());
        }
        let id = match self.store.create(&name) {
            Ok(id) => id,
            Err(e) => return self.error = Some(e.to_string()),
        };
        let ownership = components::interested(&self.spot.via, &self.spot.link);
        let mut files = vec![("ownership.ron", to_ron(&ownership))];
        if !release.date.is_empty() {
            files.push(("release.ron", to_ron(&release)));
        }
        for (path, text) in files {
            let text = text.expect("components always serialise");
            if let Err(e) = self.store.write(&id, path, &text) {
                self.error = Some(e.to_string());
            }
        }
        match self.store.list() {
            Ok(ids) => self.ids = ids,
            Err(e) => self.error = Some(e.to_string()),
        }
        self.reindex(&id);
        self.spot = Spot::default();
    }

    /// Applies a radar decision by rewriting the entity's files.
    fn triage(&mut self, id: EntityId, triage: Triage) {
        let entity = match self.store.load(&id) {
            Ok(entity) => entity,
            Err(e) => return self.error = Some(e.to_string()),
        };
        let before = |path: &str| {
            entity
                .components
                .iter()
                .find(|c| c.path == path)
                .map(|c| c.text.clone())
        };
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let mut ownership = self
            .watches
            .get(&id)
            .and_then(|w| w.ownership.clone())
            .unwrap_or_else(|| components::interested("", ""));
        let title = id.title();
        let mut files = vec![("ownership.ron", before("ownership.ron"))];
        let message = match triage {
            Triage::Later => {
                let back = ownership.snooze(&today).unwrap_or_default().to_owned();
                format!("{title} is off the radar until {back}.")
            }
            Triage::PlayNow => {
                ownership.state = OwnState::Owned;
                ownership.hidden_until.clear();
                files.push(("status.ron", before("status.ron")));
                format!("{title} is owned and playing.")
            }
            Triage::NotInterested => {
                ownership.state = OwnState::NotInterested;
                format!("{title} is off the radar for good.")
            }
            Triage::ShowNow => {
                ownership.hidden_until.clear();
                format!("{title} is back on the radar.")
            }
        };
        let mut writes = vec![("ownership.ron", to_ron(&ownership))];
        if triage == Triage::PlayNow {
            let status = Status {
                state: PlayState::Playing,
            };
            writes.push(("status.ron", to_ron(&status)));
        }
        for (path, text) in writes {
            let text = text.expect("components always serialise");
            if let Err(e) = self.store.write(&id, path, &text) {
                self.error = Some(e.to_string());
            }
        }
        self.reindex(&id);
        self.undo = Some(Undo { id, message, files });
    }

    fn undo(&mut self) {
        let Some(undo) = self.undo.take() else { return };
        for (path, text) in undo.files {
            let result = match text {
                Some(text) => self.store.write(&undo.id, path, &text),
                None => self.store.remove(&undo.id, path),
            };
            if let Err(e) = result {
                self.error = Some(e.to_string());
            }
        }
        self.reindex(&undo.id);
    }

    fn library_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.horizontal(|ui| {
            ui.heading("Library");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let home = self.open.is_none();
                if ui
                    .add_enabled(!home, egui::Button::new("Radar"))
                    .on_hover_text("Games coming out and games you want to try")
                    .clicked()
                {
                    actions.push(Action::Home);
                }
            });
        });
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
            return self.radar_ui(ui, actions);
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

impl ThingsApp {
    /// The home screen: what's coming out, what I meant to try, and a quick
    /// way to add something seen in passing.
    fn radar_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        if self.radar.as_ref().is_none_or(|(day, _)| *day != today) {
            self.radar = Some((today.clone(), Radar::new(self.watches.values(), &today)));
        }
        let radar = &self.radar.as_ref().expect("just filled").1;

        ui.heading("Radar");
        ui.weak("Things you want to get to. Pick one from the library to see all of it.");
        if let Some(undo) = &self.undo {
            ui.horizontal(|ui| {
                ui.label(&undo.message);
                if ui.small_button("Undo").clicked() {
                    actions.push(Action::Undo);
                }
            });
        }
        ui.add_space(6.0);

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.strong("Spotted something?");
            let spot = &mut self.spot;
            let mut submit = false;
            egui::Grid::new("spot").num_columns(2).show(ui, |ui| {
                let mut field = |ui: &mut egui::Ui, label: &str, text: &mut String, hint: &str| {
                    ui.label(label);
                    let r = ui.add(
                        egui::TextEdit::singleline(text)
                            .hint_text(hint)
                            .desired_width(f32::INFINITY),
                    );
                    submit |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.end_row();
                };
                field(ui, "Name", &mut spot.name, "the game");
                field(
                    ui,
                    "Seen via",
                    &mut spot.via,
                    "a friend's link, a tweet, a trailer…",
                );
                field(ui, "Link", &mut spot.link, "https://…");
                field(
                    ui,
                    "Out on",
                    &mut spot.date,
                    "leave empty if it's out (2026-10-19, 2026-10, 2027)",
                );
            });
            let ready = !spot.name.trim().is_empty();
            if ui
                .add_enabled(ready, egui::Button::new("Add as interested"))
                .clicked()
                || (submit && ready)
            {
                actions.push(Action::Spot);
            }
        });
        ui.add_space(6.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.strong(format!("Coming out ({})", radar.coming.len()));
            if radar.coming.is_empty() {
                ui.weak("Nothing announced. Give a thing a Release to see it here.");
            }
            for w in &radar.coming {
                let release = w.release.as_ref().expect("coming things have a release");
                ui.horizontal(|ui| {
                    let date = release.date.trim();
                    let date = if date.is_empty() { "unknown" } else { date };
                    ui.monospace(format!("{date:<10}"));
                    title_link(ui, w, actions);
                    ui.weak(release.stage.label());
                    if w.date_passed(&today) {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            "1.0 date passed: still in early access?",
                        );
                    }
                    triage_buttons(ui, w, &[Triage::NotInterested], actions);
                });
            }
            ui.add_space(10.0);

            ui.strong(format!("Interested, out now ({})", radar.out_now.len()));
            if radar.out_now.is_empty() {
                ui.weak("Nothing waiting. Spotted games you don't own show up here.");
            }
            for w in &radar.out_now {
                let ownership = w
                    .ownership
                    .as_ref()
                    .expect("interested things have ownership");
                ui.horizontal(|ui| {
                    let added = ownership.added.trim();
                    ui.monospace(format!("{added:<10}"));
                    title_link(ui, w, actions);
                    if w.date_passed(&today) {
                        ui.colored_label(ui.visuals().warn_fg_color, "just came out");
                    }
                    if !ownership.via.trim().is_empty() {
                        ui.weak(format!("via {}", ownership.via.trim()));
                    }
                    let link = ownership.link.trim();
                    if !link.is_empty() {
                        ui.hyperlink_to("link", link);
                    }
                    let later = format!("Later ({})", snooze_label(ownership.next_snooze_days()));
                    let buttons = [Triage::NotInterested, Triage::PlayNow, Triage::Later];
                    triage_buttons_labelled(ui, w, &buttons, Some(&later), actions);
                });
            }

            if !radar.snoozed.is_empty() {
                ui.add_space(10.0);
                egui::CollapsingHeader::new(format!("Snoozed ({})", radar.snoozed.len()))
                    .id_salt("snoozed")
                    .show(ui, |ui| {
                        for w in &radar.snoozed {
                            let o = w.ownership.as_ref().expect("snoozed things have ownership");
                            ui.horizontal(|ui| {
                                ui.monospace(format!("{:<10}", o.hidden_until.trim()));
                                title_link(ui, w, actions);
                                ui.weak(format!("put off {}×", o.snoozes));
                                triage_buttons(
                                    ui,
                                    w,
                                    &[Triage::NotInterested, Triage::ShowNow],
                                    actions,
                                );
                            });
                        }
                    });
            }
        });
    }
}

/// `7` -> `1 week`, `30` -> `1 month`.
fn snooze_label(days: i64) -> String {
    match days {
        d if d < 30 && d % 7 == 0 => match d / 7 {
            1 => "1 week".to_owned(),
            w => format!("{w} weeks"),
        },
        d if d >= 30 => match d / 30 {
            1 => "1 month".to_owned(),
            m => format!("{m} months"),
        },
        d => format!("{d} days"),
    }
}

/// Small buttons at the right end of a radar row.
fn triage_buttons(ui: &mut egui::Ui, w: &Watch, which: &[Triage], actions: &mut Vec<Action>) {
    triage_buttons_labelled(ui, w, which, None, actions);
}

fn triage_buttons_labelled(
    ui: &mut egui::Ui,
    w: &Watch,
    which: &[Triage],
    later: Option<&str>,
    actions: &mut Vec<Action>,
) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        for &triage in which {
            let (label, hint) = match triage {
                Triage::Later => (
                    later.unwrap_or("Later"),
                    "Hide it for a while, longer each time",
                ),
                Triage::PlayNow => ("Play now", "Mark it owned and playing"),
                Triage::NotInterested => ("Not interested", "Take it off the radar for good"),
                Triage::ShowNow => ("Show now", "Bring it back on the radar"),
            };
            if ui.small_button(label).on_hover_text(hint).clicked() {
                actions.push(Action::Triage(w.id.clone(), triage));
            }
        }
    });
}

fn title_link(ui: &mut egui::Ui, w: &Watch, actions: &mut Vec<Action>) {
    if ui
        .link(w.id.title())
        .on_hover_text(w.id.to_string())
        .clicked()
    {
        actions.push(Action::Select(w.id.clone()));
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
