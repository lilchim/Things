use std::collections::HashMap;

use chrono::{NaiveDateTime, Timelike};
use eframe::egui;
use things_core::components::{OwnState, PlayState, Release, Stage, Status, to_ron};
use things_core::radar::{Radar, Watch};
use things_core::research::{self as steps, Facts, Finding, Standing, Step};
use things_core::search::{Doc, Query, Suggestion, Vocabulary};
use things_core::tasks::{self, Agenda, Entry, Tasks, When};
use things_core::{Collection, ComponentFile, Entity, EntityId, Manifest, Store};

use crate::components::{self, Editor, Kind, Raw};
use crate::research::{self, Job, Research};

pub struct ThingsApp {
    store: Store,
    kinds: Vec<Box<dyn Kind>>,
    collections: Vec<Collection>,
    /// The collection the library shows, by folder; `None` shows all.
    scope: Option<String>,
    new_collection: String,
    /// Every entity in every collection.
    ids: Vec<EntityId>,
    /// Which collection (folder) each entity lives in.
    homes: HashMap<EntityId, String>,
    /// What search sees of each entity, kept in step with every write.
    docs: HashMap<EntityId, Doc>,
    /// Ownership and release of each entity that has them, for the radar.
    watches: HashMap<EntityId, Watch>,
    /// The radar and the day it was sorted for, rebuilt on change.
    radar: Option<(String, Radar)>,
    /// Each entity's task list, for the tasks view.
    task_lists: HashMap<EntityId, Tasks>,
    /// Every task sorted for the minute it was made, rebuilt on change.
    agenda: Option<(NaiveDateTime, Agenda)>,
    /// What the home screen shows when no thing is open.
    home: Home,
    spot: Spot,
    /// The last radar triage, so a misclick can be taken back.
    undo: Option<Undo>,
    search: String,
    /// The last search, its scope and what it found, so typing doesn't
    /// rescan per frame.
    hits: Option<(String, Option<String>, Vec<EntityId>)>,
    /// Keys and values in scope for completion, rebuilt when the index or
    /// the scope changes.
    vocab: Option<Vocabulary>,
    completion: Completion,
    new_name: String,
    open: Option<OpenEntity>,
    /// Which side of a thing is on screen.
    view: View,
    research: Research,
    error: Option<String>,
}

/// A thing's two views.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum View {
    /// Its components, with research hints if the collection shows them.
    #[default]
    Page,
    /// Everything research could fill in, ready steps first.
    Research,
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

/// The two home screens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Home {
    #[default]
    Radar,
    Tasks,
}

/// What to do with a thing on the radar, like answering an Anki card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Triage {
    /// Not now: one point up, so it sinks below the rest.
    Later,
    /// Bought it and starting now: Owned, and play status Playing.
    PlayNow,
    /// Off the radar for good.
    NotInterested,
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
    /// Show one collection (by folder), or all with `None`.
    Scope(Option<String>),
    NewCollection,
    /// Move the open entity into another collection.
    Move(String),
    Reload,
    Add(String, String),
    Save(usize),
    SaveAll,
    Delete(usize),
    /// Close the open entity and show a home screen.
    Home(Home),
    /// Create an entity from the quick-capture form.
    Spot,
    Triage(EntityId, Triage),
    Undo,
    /// Tick a task (by its place in `tasks.ron`) done, or take it back.
    Check(EntityId, usize, bool),
    View(View),
    /// Run a research step for the open thing; `true` skips the cache.
    Run(Step, bool),
    /// Run every step that's ready and hasn't been run.
    RunAll,
    /// Write a step's suggested file.
    Accept(Step, String),
    /// Add every suggested file that doesn't need a choice.
    AcceptAll,
    /// Stop asking about a step for the open thing, or start again.
    Skip(Step, bool),
    /// Turn research hints on or off for the open thing's collection.
    Hints(bool),
}

impl ThingsApp {
    pub fn new(store: Store) -> Self {
        let mut app = Self {
            store,
            kinds: components::registry(),
            collections: Vec::new(),
            scope: None,
            new_collection: String::new(),
            ids: Vec::new(),
            homes: HashMap::new(),
            docs: HashMap::new(),
            watches: HashMap::new(),
            radar: None,
            task_lists: HashMap::new(),
            agenda: None,
            home: Home::default(),
            spot: Spot::default(),
            undo: None,
            search: String::new(),
            hits: None,
            vocab: None,
            completion: Completion::default(),
            new_name: String::new(),
            open: None,
            view: View::default(),
            research: Research::new(research::cache_dir()),
            error: None,
        };
        app.refresh_list();
        app.scope = app.collections.first().map(|c| c.folder.clone());
        app
    }

    /// Re-reads the collections, the entity list and every entity for the
    /// search index.
    fn refresh_list(&mut self) {
        self.refresh_ids();
        self.docs.clear();
        self.watches.clear();
        self.task_lists.clear();
        for id in self.ids.clone() {
            self.reindex(&id);
        }
    }

    /// Re-reads the collections and which entities live where.
    fn refresh_ids(&mut self) {
        match self.store.collections() {
            Ok(collections) => self.collections = collections,
            Err(e) => self.error = Some(e.to_string()),
        }
        match self.store.list() {
            Ok(list) => {
                self.ids = list.iter().map(|(id, _)| id.clone()).collect();
                self.homes = list.into_iter().collect();
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        if let Some(scope) = &self.scope
            && !self.collections.iter().any(|c| c.folder == *scope)
        {
            self.scope = None;
        }
        self.hits = None;
        self.vocab = None;
        self.radar = None;
        self.agenda = None;
    }

    fn collection(&self, folder: &str) -> Option<&Collection> {
        self.collections.iter().find(|c| c.folder == folder)
    }

    /// The collection new things go into: the one on screen, else the first.
    fn target(&self) -> Option<&Collection> {
        self.scope
            .as_deref()
            .and_then(|f| self.collection(f))
            .or(self.collections.first())
    }

    fn in_scope(&self, id: &EntityId) -> bool {
        self.scope
            .as_ref()
            .is_none_or(|s| self.homes.get(id) == Some(s))
    }

    /// Whether an entity lives in a collection that feeds the radar.
    fn on_radar(&self, id: &EntityId) -> bool {
        self.homes
            .get(id)
            .and_then(|f| self.collection(f))
            .is_some_and(|c| c.manifest.radar)
    }

    fn reindex(&mut self, id: &EntityId) {
        self.hits = None;
        self.vocab = None;
        self.radar = None;
        self.agenda = None;
        self.completion.line.clear();
        self.completion.items.clear();
        match self.store.load(id) {
            Ok(entity) => {
                self.docs.insert(id.clone(), Doc::new(&entity));
                match Watch::new(&entity) {
                    Some(watch) => self.watches.insert(id.clone(), watch),
                    None => self.watches.remove(id),
                };
                match tasks::of(&entity) {
                    Some(list) => self.task_lists.insert(id.clone(), list),
                    None => self.task_lists.remove(id),
                };
            }
            Err(e) => {
                self.docs.remove(id);
                self.watches.remove(id);
                self.task_lists.remove(id);
                self.error = Some(e.to_string());
            }
        }
    }

    /// Entities matching the search line, in library order.
    fn hits(&mut self) -> &[EntityId] {
        let stale = self
            .hits
            .as_ref()
            .is_none_or(|(q, scope, _)| *q != self.search || *scope != self.scope);
        if stale {
            let query = Query::parse(&self.search);
            let hits = self
                .ids
                .iter()
                .filter(|id| self.in_scope(id))
                .filter(|id| {
                    query.is_empty() || self.docs.get(id).is_some_and(|d| query.matches(d))
                })
                .cloned()
                .collect();
            self.hits = Some((self.search.clone(), self.scope.clone(), hits));
        }
        &self.hits.as_ref().expect("just filled").2
    }

    /// Recomputes suggestions when the search line changed.
    fn refresh_completion(&mut self) {
        if self.vocab.is_none() {
            let docs = self
                .docs
                .iter()
                .filter(|(id, _)| self.in_scope(id))
                .map(|(_, doc)| doc);
            self.vocab = Some(Vocabulary::new(docs));
        }
        let vocab = self.vocab.as_ref().expect("just filled");
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

    /// The open thing as research sees it: its files as last saved.
    fn open_files(&self) -> Option<Entity> {
        let open = self.open.as_ref()?;
        Some(Entity {
            id: open.id.clone(),
            collection: self.homes.get(&open.id).cloned().unwrap_or_default(),
            components: open
                .components
                .iter()
                .map(|c| ComponentFile {
                    path: c.path.clone(),
                    text: c.saved.clone(),
                })
                .collect(),
        })
    }

    fn apply(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Select(id) => {
                // Leaving an entity keeps its edits rather than dropping them.
                self.save_all();
                self.open_entity(id);
            }
            Action::Create => {
                let name = self.new_name.trim().to_owned();
                let Some(target) = self.target().cloned() else {
                    return;
                };
                if name.is_empty() {
                    return;
                }
                self.save_all();
                match self.store.create(&target.folder, &name) {
                    Ok(id) => {
                        self.new_name.clear();
                        self.start_components(&id, &target.manifest);
                        self.refresh_ids();
                        self.reindex(&id);
                        self.open_entity(id);
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            Action::Scope(scope) => {
                self.scope = scope;
                self.vocab = None;
            }
            Action::NewCollection => {
                let name = self.new_collection.trim().to_owned();
                if name.is_empty() {
                    return;
                }
                match self.store.create_collection(&Manifest::named(&name)) {
                    Ok(c) => {
                        self.new_collection.clear();
                        self.refresh_ids();
                        self.scope = Some(c.folder);
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            Action::Move(folder) => {
                let Some(id) = self.open.as_ref().map(|o| o.id.clone()) else {
                    return;
                };
                self.save_all();
                match self.store.move_to(&id, &folder) {
                    Ok(()) => {
                        self.refresh_ids();
                        self.reindex(&id);
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
            Action::Home(home) => {
                self.save_all();
                self.open = None;
                self.home = home;
            }
            Action::Spot => self.spot(),
            Action::Triage(id, triage) => self.triage(id, triage),
            Action::Undo => self.undo(),
            Action::Check(id, index, done) => self.check(id, index, done),
            Action::View(view) => self.view = view,
            Action::Run(step, fresh) => {
                if let Some(entity) = self.open_files() {
                    let facts = Facts::of(&entity);
                    self.research.run(ctx, &entity.id, step, facts, fresh);
                }
            }
            Action::RunAll => {
                let Some(entity) = self.open_files() else {
                    return;
                };
                for (step, standing) in steps::plan(&entity) {
                    let idle = self.research.job(&entity.id, step).is_none();
                    if standing == Standing::Ready
                        && idle
                        && !self.research.is_skipped(&entity.id, step)
                    {
                        self.research
                            .run(ctx, &entity.id, step, Facts::of(&entity), false);
                    }
                }
            }
            Action::Accept(step, text) => {
                let Some(id) = self.open.as_ref().map(|o| o.id.clone()) else {
                    return;
                };
                self.research.clear(&id, step);
                self.apply(ctx, Action::Add(step.file().to_owned(), text));
            }
            Action::AcceptAll => {
                let Some(entity) = self.open_files() else {
                    return;
                };
                for (step, standing) in steps::plan(&entity) {
                    if let (Standing::Ready, Some(Job::Found(Finding::File { text, .. }))) =
                        (standing, self.research.job(&entity.id, step))
                    {
                        let text = text.clone();
                        self.apply(ctx, Action::Accept(step, text));
                    }
                }
            }
            Action::Skip(step, skipped) => {
                let Some(id) = self.open.as_ref().map(|o| o.id.clone()) else {
                    return;
                };
                if let Err(e) = self.research.set_skipped(&id, step, skipped) {
                    self.error = Some(e);
                }
            }
            Action::Hints(on) => {
                let Some(id) = self.open.as_ref().map(|o| &o.id) else {
                    return;
                };
                let Some(c) = self.homes.get(id).and_then(|f| self.collection(f)) else {
                    return;
                };
                let (folder, mut manifest) = (c.folder.clone(), c.manifest.clone());
                manifest.hints = on;
                match self.store.set_manifest(&folder, &manifest) {
                    Ok(()) => self.refresh_ids(),
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

    /// Gives a new entity the components its collection starts things
    /// with, made the way "Add component" makes them. A file no kind
    /// knows starts out empty.
    fn start_components(&mut self, id: &EntityId, manifest: &Manifest) {
        for (path, text) in components::starting_files(&self.kinds, &manifest.starts_with) {
            if let Err(e) = self.store.write(id, &path, &text) {
                self.error = Some(e.to_string());
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
        let Some(folder) = self
            .collections
            .iter()
            .find(|c| c.manifest.radar)
            .map(|c| c.folder.clone())
        else {
            return self.error = Some("No collection feeds the radar.".into());
        };
        let id = match self.store.create(&folder, &name) {
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
        self.refresh_ids();
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
        let mut ownership = self
            .watches
            .get(&id)
            .and_then(|w| w.ownership.clone())
            .unwrap_or_else(|| components::interested("", ""));
        let title = id.title();
        let mut files = vec![("ownership.ron", before("ownership.ron"))];
        let message = match triage {
            Triage::Later => {
                ownership.score += 1;
                format!("{title} moved down the radar (score {}).", ownership.score)
            }
            Triage::PlayNow => {
                ownership.state = OwnState::Owned;
                files.push(("status.ron", before("status.ron")));
                format!("{title} is owned and playing.")
            }
            Triage::NotInterested => {
                ownership.state = OwnState::NotInterested;
                format!("{title} is off the radar for good.")
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

    /// Ticks a task done today, or takes the latest tick back, in its
    /// thing's `tasks.ron`.
    fn check(&mut self, id: EntityId, index: usize, done: bool) {
        let Some(mut list) = self.task_lists.get(&id).cloned() else {
            return;
        };
        let Some(task) = list.tasks.get_mut(index) else {
            return;
        };
        if done {
            task.check(components::now());
        } else {
            task.uncheck();
        }
        let text = to_ron(&list).expect("tasks always serialise");
        if let Err(e) = self.store.write(&id, tasks::FILE, &text) {
            self.error = Some(e.to_string());
        }
        self.reindex(&id);
    }

    /// Every task, sorted for this minute.
    fn agenda(&mut self) -> &Agenda {
        let now = components::now();
        let minute = now
            .with_second(0)
            .and_then(|t| t.with_nanosecond(0))
            .unwrap_or(now);
        if self.agenda.as_ref().is_none_or(|(at, _)| *at != minute) {
            self.agenda = Some((minute, Agenda::new(&self.task_lists, now)));
        }
        &self.agenda.as_ref().expect("just filled").1
    }

    fn library_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let to_do = {
            let agenda = self.agenda();
            agenda.overdue.len() + agenda.now.len()
        };
        ui.horizontal(|ui| {
            ui.heading("Library");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let home = self.open.is_none().then_some(self.home);
                let tasks = if to_do > 0 {
                    format!("Tasks ({to_do})")
                } else {
                    "Tasks".to_owned()
                };
                for (screen, label, hint) in [
                    (Home::Tasks, tasks, "What to do now and what's coming up"),
                    (
                        Home::Radar,
                        "Radar".to_owned(),
                        "Games coming out and games you want to try",
                    ),
                ] {
                    if ui
                        .selectable_label(home == Some(screen), label)
                        .on_hover_text(hint)
                        .clicked()
                        && home != Some(screen)
                    {
                        actions.push(Action::Home(screen));
                    }
                }
            });
        });
        self.collections_ui(ui, actions);
        self.search_ui(ui);
        if !self.search.trim().is_empty() {
            let shown = self.hits().len();
            let total = self.ids.iter().filter(|id| self.in_scope(id)).count();
            ui.weak(format!("{shown} of {total}"));
        }
        ui.separator();

        let hint = match (&self.scope, self.target()) {
            (Some(_), _) | (None, None) => "New thing".to_owned(),
            (None, Some(c)) => format!("New thing in {}", c.name()),
        };
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.new_name)
                    .hint_text(hint)
                    .desired_width(ui.available_width() - 50.0),
            );
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Add").clicked() || enter {
                actions.push(Action::Create);
            }
        });
        ui.separator();

        self.hits();
        let hits = &self.hits.as_ref().expect("just filled").2;
        let selected = self.open.as_ref().map(|o| &o.id);
        let all = self.scope.is_none();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for id in hits {
                let response = ui
                    .horizontal(|ui| {
                        let r = ui.selectable_label(selected == Some(id), id.title());
                        // Searching everything says where each thing lives.
                        if all && let Some(c) = self.homes.get(id).and_then(|f| self.collection(f))
                        {
                            ui.weak(c.name());
                        }
                        r
                    })
                    .inner
                    .on_hover_text(id.to_string());
                if response.clicked() && selected != Some(id) {
                    actions.push(Action::Select(id.clone()));
                }
            }
            if !self.ids.iter().any(|id| self.in_scope(id)) {
                ui.weak("Nothing here yet. Add a thing above.");
            }
        });
    }

    /// Picks the collection the library shows, or all of them, and makes
    /// new collections.
    fn collections_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let count = |folder: &str| self.homes.values().filter(|f| *f == folder).count();
        let shown = match self.scope.as_deref().and_then(|f| self.collection(f)) {
            Some(c) => c.name(),
            None => "Everything".to_owned(),
        };
        egui::ComboBox::from_id_salt("collection")
            .selected_text(shown)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for c in &self.collections {
                    let label = format!("{}  ({})", c.name(), count(&c.folder));
                    let on = self.scope.as_deref() == Some(c.folder.as_str());
                    if ui.selectable_label(on, label).clicked() && !on {
                        actions.push(Action::Scope(Some(c.folder.clone())));
                    }
                }
                let label = format!("Everything  ({})", self.ids.len());
                if ui.selectable_label(self.scope.is_none(), label).clicked()
                    && self.scope.is_some()
                {
                    actions.push(Action::Scope(None));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut self.new_collection)
                            .hint_text("New collection")
                            .desired_width(140.0),
                    );
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let ready = !self.new_collection.trim().is_empty();
                    if ui.add_enabled(ready, egui::Button::new("Add")).clicked() || (enter && ready)
                    {
                        actions.push(Action::NewCollection);
                    }
                });
            });
    }

    fn entity_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let plan = self
            .open_files()
            .map(|e| steps::plan(&e))
            .unwrap_or_default();
        let hints = self
            .open
            .as_ref()
            .and_then(|o| self.homes.get(&o.id))
            .and_then(|f| self.collection(f))
            .is_some_and(|c| c.manifest.hints);
        let Some(open) = &mut self.open else {
            return match self.home {
                Home::Radar => self.radar_ui(ui, actions),
                Home::Tasks => self.tasks_ui(ui, actions),
            };
        };

        let home = self.homes.get(&open.id).cloned().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.heading(open.id.title());
            ui.weak(open.id.to_string());
            let here = self.collections.iter().find(|c| c.folder == home);
            let label = here.map_or_else(|| "Move to".to_owned(), |c| format!("in {}", c.name()));
            ui.menu_button(label, |ui| {
                ui.weak("Move to");
                for c in self.collections.iter().filter(|c| c.folder != home) {
                    if ui.button(c.name()).clicked() {
                        actions.push(Action::Move(c.folder.clone()));
                    }
                }
            })
            .response
            .on_hover_text("The collection this thing lives in. Click to move it.");
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
        let id = open.id.clone();
        let research = &self.research;
        let ready = plan
            .iter()
            .filter(|(s, st)| *st == Standing::Ready && !research.is_skipped(&id, *s))
            .count();
        ui.horizontal(|ui| {
            let label = match ready {
                0 => "Research".to_owned(),
                n => format!("Research ({n} ready)"),
            };
            for (view, label) in [(View::Page, "Page".to_owned()), (View::Research, label)] {
                if ui.selectable_label(self.view == view, label).clicked() && self.view != view {
                    actions.push(Action::View(view));
                }
            }
            if self.view == View::Page {
                let mut on = hints;
                let hint = "Show what research could fill in as faded placeholders. \
                            Applies to every thing in this collection.";
                if ui
                    .checkbox(&mut on, "Research hints")
                    .on_hover_text(hint)
                    .changed()
                {
                    actions.push(Action::Hints(on));
                }
            }
        });
        ui.separator();
        if self.view == View::Research {
            return research_ui(ui, research, &id, &plan, actions);
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            if open.components.is_empty() {
                ui.weak("No components yet. Use “Add component” to give this thing some.");
            }
            // Hints sit where their component would, in registry order.
            let kinds = &self.kinds;
            let kind_of = |path: &str| {
                kinds
                    .iter()
                    .position(|k| k.matches(path))
                    .unwrap_or(usize::MAX)
            };
            let mut hinted: Vec<_> = plan
                .iter()
                .filter(|(s, st)| {
                    hints && *st != Standing::Recorded && !research.is_skipped(&id, *s)
                })
                .map(|(s, st)| (kind_of(s.file()), *s, st))
                .collect();
            hinted.sort_by_key(|(k, _, _)| *k);
            let mut next = 0;
            for (i, component) in open.components.iter_mut().enumerate() {
                let here = kind_of(&component.path);
                while let Some((_, step, standing)) = hinted.get(next).filter(|h| h.0 < here) {
                    step_ui(ui, research, &id, *step, standing, true, actions);
                    ui.add_space(6.0);
                    next += 1;
                }
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
            for (_, step, standing) in &hinted[next..] {
                step_ui(ui, research, &id, *step, standing, true, actions);
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
            let watches = self.watches.values().filter(|w| self.on_radar(&w.id));
            self.radar = Some((today.clone(), Radar::new(watches, &today)));
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
                    if ownership.score > 0 {
                        ui.weak(format!("put off {}×", ownership.score));
                    }
                    let buttons = [Triage::NotInterested, Triage::PlayNow, Triage::Later];
                    triage_buttons(ui, w, &buttons, actions);
                });
            }
        });
    }
}

impl ThingsApp {
    /// The other home screen: every task, by what needs doing.
    fn tasks_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        // Times on screen count down, and tasks reset on their own.
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(30));
        let now = components::now();
        let agenda = self.agenda().clone();

        ui.heading("Tasks");
        ui.weak("Tasks from every thing. Give a thing a Tasks component to add some.");
        ui.add_space(6.0);

        if agenda.is_empty() {
            ui.weak("No tasks yet. Open a game, add the Tasks component, and list its dailies.");
            return;
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            let sections: [(&str, &[Entry], &str); 5] = [
                ("Overdue", &agenda.overdue, ""),
                ("To do now", &agenda.now, "All caught up."),
                ("Coming up", &agenda.coming, ""),
                ("Anytime", &agenda.anytime, ""),
                ("Done for now", &agenda.done, ""),
            ];
            for (title, entries, empty) in sections {
                if entries.is_empty() && empty.is_empty() {
                    continue;
                }
                ui.strong(format!("{title} ({})", entries.len()));
                if entries.is_empty() {
                    ui.weak(empty);
                }
                for e in entries {
                    task_row(ui, e, now, actions);
                }
                ui.add_space(10.0);
            }
        });
    }
}

fn task_row(ui: &mut egui::Ui, e: &Entry, now: NaiveDateTime, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let mut done = matches!(e.when, When::Done);
        if ui.checkbox(&mut done, "").changed() {
            actions.push(Action::Check(e.id.clone(), e.index, done));
        }
        let name = e.task.name.trim();
        ui.label(if name.is_empty() {
            "(unnamed task)"
        } else {
            name
        });
        if ui
            .link(e.id.title())
            .on_hover_text(e.id.to_string())
            .clicked()
        {
            actions.push(Action::Select(e.id.clone()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let text = components::describe(&e.task, e.when, now);
            if matches!(e.when, When::Overdue { .. }) {
                ui.colored_label(ui.visuals().warn_fg_color, text);
            } else {
                ui.weak(text);
            }
            if e.task.repeat != tasks::Repeat::Once {
                ui.weak(e.task.repeat.label());
            }
        });
    });
}

/// Everything research could fill in for one thing: ready steps (with
/// their results) first, then what's waiting on something, what's recorded
/// and what was skipped.
fn research_ui(
    ui: &mut egui::Ui,
    research: &Research,
    id: &EntityId,
    plan: &[(Step, Standing)],
    actions: &mut Vec<Action>,
) {
    let skipped = |s: Step| research.is_skipped(id, s);
    let ready: Vec<_> = plan
        .iter()
        .filter(|(s, st)| *st == Standing::Ready && !skipped(*s))
        .collect();
    let idle = ready
        .iter()
        .filter(|(s, _)| research.job(id, *s).is_none())
        .count();
    let found = ready
        .iter()
        .filter(|(s, _)| matches!(research.job(id, *s), Some(Job::Found(Finding::File { .. }))))
        .count();

    ui.weak(
        "What research could fill in. Nothing is written until you add it, \
         and each thing you add can unlock more.",
    );
    ui.horizontal(|ui| {
        if idle > 0 && ui.button(format!("Run all ready ({idle})")).clicked() {
            actions.push(Action::RunAll);
        }
        if found > 0 && ui.button(format!("Add all found ({found})")).clicked() {
            actions.push(Action::AcceptAll);
        }
    });
    ui.add_space(6.0);

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.strong(format!("Ready ({})", ready.len()));
        if ready.is_empty() {
            ui.weak("Nothing to run right now.");
        }
        for (step, standing) in &ready {
            step_ui(ui, research, id, *step, standing, false, actions);
            ui.add_space(6.0);
        }
        ui.add_space(6.0);

        let waiting: Vec<_> = plan
            .iter()
            .filter(|(s, st)| matches!(st, Standing::Waiting(_)) && !skipped(*s))
            .collect();
        if !waiting.is_empty() {
            ui.strong(format!("Waiting ({})", waiting.len()));
            for (step, standing) in waiting {
                ui.horizontal(|ui| {
                    ui.label(step.label());
                    step_body(ui, research.job(id, *step), *step, standing, actions);
                });
            }
            ui.add_space(10.0);
        }

        let recorded: Vec<_> = plan
            .iter()
            .filter(|(_, st)| *st == Standing::Recorded)
            .collect();
        if !recorded.is_empty() {
            ui.strong(format!("Recorded ({})", recorded.len()));
            for (step, _) in recorded {
                ui.horizontal(|ui| {
                    ui.label(step.label());
                    ui.weak(step.file());
                });
            }
            ui.add_space(10.0);
        }

        let skips: Vec<_> = plan
            .iter()
            .filter(|(s, st)| *st != Standing::Recorded && skipped(*s))
            .collect();
        if !skips.is_empty() {
            ui.strong(format!("Skipped ({})", skips.len()));
            for (step, _) in skips {
                ui.horizontal(|ui| {
                    ui.label(step.label());
                    if ui.small_button("Ask again").clicked() {
                        actions.push(Action::Skip(*step, false));
                    }
                });
            }
        }
    });
}

/// One step in a frame: its name, where it looks, and what can be done
/// with it now. `faded` is for the hints on a thing's page, until there's
/// a result to look at.
fn step_ui(
    ui: &mut egui::Ui,
    research: &Research,
    id: &EntityId,
    step: Step,
    standing: &Standing,
    faded: bool,
    actions: &mut Vec<Action>,
) {
    let job = research.job(id, step);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if faded && !matches!(job, Some(Job::Found(_) | Job::Failed(_))) {
            ui.multiply_opacity(0.55);
        }
        ui.horizontal(|ui| {
            ui.strong(step.label());
            ui.weak(format!("from {}", step.source()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("Skip")
                    .on_hover_text("Stop suggesting this for this thing")
                    .clicked()
                {
                    actions.push(Action::Skip(step, true));
                }
                ui.weak(step.file());
            });
        });
        step_body(ui, job, step, standing, actions);
    });
}

/// What a step shows: why it's waiting, a button to run it, that it's
/// running, or what it found with the buttons to add or turn it down.
fn step_body(
    ui: &mut egui::Ui,
    job: Option<&Job>,
    step: Step,
    standing: &Standing,
    actions: &mut Vec<Action>,
) {
    let retry = |ui: &mut egui::Ui, actions: &mut Vec<Action>| {
        if ui.small_button("Try again").clicked() {
            actions.push(Action::Run(step, true));
        }
    };
    match (standing, job) {
        (Standing::Recorded, _) => {
            ui.weak(format!("Recorded in {}.", step.file()));
        }
        (Standing::Waiting(needs), _) => {
            let needs: Vec<&str> = needs.iter().map(|n| n.label()).collect();
            ui.weak(format!("Needs {} first.", needs.join(" and ")));
        }
        (Standing::Ready, None) => {
            if ui.button(step.verb()).clicked() {
                actions.push(Action::Run(step, false));
            }
        }
        (Standing::Ready, Some(Job::Running)) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(format!("Looking on {}…", step.source()));
            });
        }
        (Standing::Ready, Some(Job::Failed(e))) => {
            ui.horizontal(|ui| {
                ui.colored_label(ui.visuals().warn_fg_color, e);
                retry(ui, actions);
            });
        }
        (Standing::Ready, Some(Job::Found(Finding::Nothing(why)))) => {
            ui.horizontal(|ui| {
                ui.weak(why);
                retry(ui, actions);
            });
        }
        (Standing::Ready, Some(Job::Found(Finding::File { summary, text }))) => {
            ui.label(summary);
            ui.horizontal(|ui| {
                if ui.button("Add").clicked() {
                    actions.push(Action::Accept(step, text.clone()));
                }
                if ui
                    .button("Not right")
                    .on_hover_text("Skip it for this thing")
                    .clicked()
                {
                    actions.push(Action::Skip(step, true));
                }
            });
        }
        (Standing::Ready, Some(Job::Found(Finding::Choose(matches)))) => {
            ui.label("Which one is it?");
            for m in matches {
                ui.horizontal(|ui| {
                    if ui.button("This one").clicked() {
                        actions.push(Action::Accept(step, m.text()));
                    }
                    ui.label(&m.name);
                    ui.hyperlink_to("store page", m.steam().store_url());
                });
            }
            ui.horizontal(|ui| {
                if ui.small_button("None of these").clicked() {
                    actions.push(Action::Skip(step, true));
                }
                retry(ui, actions);
            });
        }
    }
}

/// Small buttons at the right end of a radar row.
fn triage_buttons(ui: &mut egui::Ui, w: &Watch, which: &[Triage], actions: &mut Vec<Action>) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        for &triage in which {
            let (label, hint) = match triage {
                Triage::Later => ("Later", "Move it down the list, below the rest"),
                Triage::PlayNow => ("Play now", "Mark it owned and playing"),
                Triage::NotInterested => ("Not interested", "Take it off the radar for good"),
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
        self.research.poll();

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
            self.apply(ui.ctx(), action);
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
