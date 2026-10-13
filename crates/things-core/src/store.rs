use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::collection::{self, Collection, Manifest};
use crate::components::{from_ron, to_ron};
use crate::{EntityId, ParseIdError};

/// The data folder. Files are the source of truth; nothing is cached here.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

/// An entity and the raw text of every component file in its folder.
#[derive(Debug, Clone)]
pub struct Entity {
    pub id: EntityId,
    /// Folder name of the collection it lives in, such as `games`.
    pub collection: String,
    /// Sorted by path.
    pub components: Vec<ComponentFile>,
}

/// One component file, with its path relative to the entity folder using
/// `/` separators (e.g. `status.ron`, `journal/2026-10-13-1930.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentFile {
    pub path: String,
    pub text: String,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error(transparent)]
    BadId(#[from] ParseIdError),
    #[error("`{0}` is not a valid component path")]
    BadPath(String),
    #[error("{path}: {message}")]
    BadManifest { path: PathBuf, message: String },
    #[error("no collection has `{0}`")]
    NotFound(EntityId),
    #[error("there is no collection `{0}`")]
    NoCollection(String),
    #[error("a folder called `{0}` is already there")]
    Taken(String),
}

/// The folder that held every entity before collections existed.
const OLD_ENTITIES: &str = "entities";
/// Where the old folder's things go.
const FIRST_COLLECTION: &str = "games";

impl Store {
    /// Opens (and creates if needed) a data folder. A folder from before
    /// collections has its `entities/` renamed to `games/`, and a data
    /// folder with no collections gets a Games collection.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let store = Self { root: root.into() };
        fs::create_dir_all(&store.root).map_err(io_err(&store.root))?;
        let old = store.root.join(OLD_ENTITIES);
        let first = store.root.join(FIRST_COLLECTION);
        if old.is_dir() && !first.exists() {
            fs::rename(&old, &first).map_err(io_err(&old))?;
            store.write_manifest(FIRST_COLLECTION, &Manifest::games())?;
        }
        if store.collections()?.is_empty() {
            store.create_collection_in(FIRST_COLLECTION, &Manifest::games())?;
        }
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every top-level folder with a `collection.ron`, sorted by folder.
    pub fn collections(&self) -> Result<Vec<Collection>, StoreError> {
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(io_err(&self.root))? {
            let entry = entry.map_err(io_err(&self.root))?;
            let path = entry.path().join(collection::MANIFEST);
            let Some(folder) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if folder.starts_with('.') || !path.is_file() {
                continue;
            }
            let text = fs::read_to_string(&path).map_err(io_err(&path))?;
            let manifest = from_ron(&text).map_err(|e| StoreError::BadManifest {
                path: path.clone(),
                message: e.to_string(),
            })?;
            out.push(Collection { folder, manifest });
        }
        out.sort_by(|a, b| a.folder.cmp(&b.folder));
        Ok(out)
    }

    /// Makes a new collection folder named after `name`.
    pub fn create_collection(&self, manifest: &Manifest) -> Result<Collection, StoreError> {
        let folder = crate::id::slugify(&manifest.name);
        self.create_collection_in(&folder, manifest)?;
        Ok(Collection {
            folder,
            manifest: manifest.clone(),
        })
    }

    fn create_collection_in(&self, folder: &str, manifest: &Manifest) -> Result<(), StoreError> {
        let dir = self.root.join(folder);
        match fs::create_dir(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                return Err(StoreError::Taken(folder.to_owned()));
            }
            Err(e) => return Err(io_err(&dir)(e)),
        }
        self.write_manifest(folder, manifest)
    }

    fn write_manifest(&self, folder: &str, manifest: &Manifest) -> Result<(), StoreError> {
        let text = to_ron(manifest).expect("manifests always serialise");
        write_atomic(&self.root.join(folder).join(collection::MANIFEST), &text)
    }

    fn collection_dir(&self, folder: &str) -> Result<PathBuf, StoreError> {
        let dir = self.root.join(folder);
        let plain = Path::new(folder).components().count() == 1
            && matches!(
                Path::new(folder).components().next(),
                Some(Component::Normal(_))
            );
        if plain && dir.join(collection::MANIFEST).is_file() {
            Ok(dir)
        } else {
            Err(StoreError::NoCollection(folder.to_owned()))
        }
    }

    /// The collection holding an entity, by folder name.
    pub fn collection_of(&self, id: &EntityId) -> Result<String, StoreError> {
        // Runs on every read and write, so it only looks for folders and
        // doesn't parse manifests.
        for entry in fs::read_dir(&self.root).map_err(io_err(&self.root))? {
            let entry = entry.map_err(io_err(&self.root))?;
            let dir = entry.path();
            if dir.join(collection::MANIFEST).is_file()
                && dir.join(id.to_string()).is_dir()
                && let Some(folder) = entry.file_name().to_str()
                && !folder.starts_with('.')
            {
                return Ok(folder.to_owned());
            }
        }
        Err(StoreError::NotFound(id.clone()))
    }

    fn entity_dir(&self, id: &EntityId) -> Result<PathBuf, StoreError> {
        let folder = self.collection_of(id)?;
        Ok(self.root.join(folder).join(id.to_string()))
    }

    /// Every entity in every collection, with its collection's folder,
    /// sorted by ID.
    pub fn list(&self) -> Result<Vec<(EntityId, String)>, StoreError> {
        let mut all = Vec::new();
        for c in self.collections()? {
            for id in self.list_in(&c.folder)? {
                all.push((id, c.folder.clone()));
            }
        }
        all.sort();
        Ok(all)
    }

    /// Every entity folder in one collection, sorted. Folders that aren't
    /// `slug-xxxx` are skipped.
    pub fn list_in(&self, collection: &str) -> Result<Vec<EntityId>, StoreError> {
        let dir = self.collection_dir(collection)?;
        let mut ids = Vec::new();
        for entry in fs::read_dir(&dir).map_err(io_err(&dir))? {
            let entry = entry.map_err(io_err(&dir))?;
            if !entry.path().is_dir() {
                continue;
            }
            if let Some(id) = entry
                .file_name()
                .to_str()
                .and_then(|n| EntityId::parse(n).ok())
            {
                ids.push(id);
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// Makes a new, empty entity folder in a collection. The ID is unique
    /// across all collections, so moving the entity never clashes.
    pub fn create(&self, collection: &str, name: &str) -> Result<EntityId, StoreError> {
        let dir = self.collection_dir(collection)?;
        loop {
            let id = EntityId::generate(name);
            if self.collection_of(&id).is_ok() {
                continue;
            }
            let path = dir.join(id.to_string());
            match fs::create_dir(&path) {
                Ok(()) => return Ok(id),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io_err(&path)(e)),
            }
        }
    }

    /// Moves an entity's folder into another collection.
    pub fn move_to(&self, id: &EntityId, collection: &str) -> Result<(), StoreError> {
        let from = self.entity_dir(id)?;
        let to = self.collection_dir(collection)?.join(id.to_string());
        if from == to {
            return Ok(());
        }
        if to.exists() {
            return Err(StoreError::Taken(format!("{collection}/{id}")));
        }
        fs::rename(&from, &to).map_err(io_err(&from))
    }

    /// Reads every component file of an entity, including subfolders.
    pub fn load(&self, id: &EntityId) -> Result<Entity, StoreError> {
        let collection = self.collection_of(id)?;
        let mut components = Vec::new();
        read_tree(
            &self.root.join(&collection).join(id.to_string()),
            "",
            &mut components,
        )?;
        components.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Entity {
            id: id.clone(),
            collection,
            components,
        })
    }

    /// Writes one component file, creating subfolders as needed. The write
    /// goes to a temporary file first so a crash never leaves half a file.
    pub fn write(&self, id: &EntityId, path: &str, text: &str) -> Result<(), StoreError> {
        let target = self.component_path(id, path)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        write_atomic(&target, text)
    }

    /// Deletes one component file.
    pub fn remove(&self, id: &EntityId, path: &str) -> Result<(), StoreError> {
        let target = self.component_path(id, path)?;
        fs::remove_file(&target).map_err(io_err(&target))
    }

    /// Resolves a component path, refusing anything that escapes the entity folder.
    fn component_path(&self, id: &EntityId, path: &str) -> Result<PathBuf, StoreError> {
        let rel = Path::new(path);
        let plain = !path.is_empty() && rel.components().all(|c| matches!(c, Component::Normal(_)));
        if !plain {
            return Err(StoreError::BadPath(path.to_owned()));
        }
        Ok(self.entity_dir(id)?.join(rel))
    }
}

fn write_atomic(target: &Path, text: &str) -> Result<(), StoreError> {
    let tmp = target.with_extension("things-tmp");
    fs::write(&tmp, text).map_err(io_err(&tmp))?;
    fs::rename(&tmp, target).map_err(io_err(target))
}

fn read_tree(dir: &Path, prefix: &str, out: &mut Vec<ComponentFile>) -> Result<(), StoreError> {
    for entry in fs::read_dir(dir).map_err(io_err(dir))? {
        let entry = entry.map_err(io_err(dir))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        // Hidden files and our own temp files are not components.
        if name.starts_with('.') || name.ends_with(".things-tmp") {
            continue;
        }
        let path = entry.path();
        let rel = format!("{prefix}{name}");
        if path.is_dir() {
            read_tree(&path, &format!("{rel}/"), out)?;
        } else {
            let text = fs::read_to_string(&path).map_err(io_err(&path))?;
            out.push(ComponentFile { path: rel, text });
        }
    }
    Ok(())
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> StoreError + '_ {
    move |source| StoreError::Io {
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn games(store: &Store) -> Vec<EntityId> {
        store.list_in("games").unwrap()
    }

    #[test]
    fn create_write_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let id = store.create("games", "Mass Effect").unwrap();

        store
            .write(&id, "status.ron", "Status(state: Playing)")
            .unwrap();
        store
            .write(&id, "journal/2026-10-13-1930.md", "Saved Kaidan.")
            .unwrap();

        assert_eq!(games(&store), vec![id.clone()]);
        let entity = store.load(&id).unwrap();
        assert_eq!(entity.collection, "games");
        let paths: Vec<_> = entity.components.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, ["journal/2026-10-13-1930.md", "status.ron"]);
        assert_eq!(entity.components[1].text, "Status(state: Playing)");

        store.remove(&id, "status.ron").unwrap();
        assert_eq!(store.load(&id).unwrap().components.len(), 1);
    }

    #[test]
    fn a_new_data_folder_starts_with_games() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let collections = store.collections().unwrap();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].folder, "games");
        assert_eq!(collections[0].manifest, Manifest::games());
        // Opening again doesn't add another.
        Store::open(dir.path()).unwrap();
        assert_eq!(store.collections().unwrap().len(), 1);
    }

    #[test]
    fn old_entities_folder_becomes_games() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("entities/hades-2-b41c")).unwrap();
        fs::write(dir.path().join("entities/hades-2-b41c/notes.md"), "Run 12").unwrap();

        let store = Store::open(dir.path()).unwrap();
        assert!(!dir.path().join("entities").exists());
        let id = EntityId::parse("hades-2-b41c").unwrap();
        assert_eq!(store.list().unwrap(), [(id.clone(), "games".into())]);
        assert_eq!(store.load(&id).unwrap().components[0].text, "Run 12");
    }

    #[test]
    fn collections_keep_things_apart_and_things_can_move() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let tasks = store.create_collection(&Manifest::named("Tasks")).unwrap();
        assert_eq!(tasks.folder, "tasks");
        assert!(matches!(
            store.create_collection(&Manifest::named("tasks")),
            Err(StoreError::Taken(_))
        ));

        let game = store.create("games", "Hades II").unwrap();
        let chore = store.create("tasks", "Mow the lawn").unwrap();
        assert_eq!(games(&store), vec![game.clone()]);
        assert_eq!(store.list_in("tasks").unwrap(), vec![chore.clone()]);
        assert_eq!(store.list().unwrap().len(), 2);

        store.write(&chore, "notes.md", "Saturday").unwrap();
        store.move_to(&chore, "games").unwrap();
        assert!(store.list_in("tasks").unwrap().is_empty());
        let moved = store.load(&chore).unwrap();
        assert_eq!(moved.collection, "games");
        assert_eq!(moved.components[0].text, "Saturday");

        assert!(matches!(
            store.create("nope", "x"),
            Err(StoreError::NoCollection(_))
        ));
        assert!(matches!(
            store.create("..", "x"),
            Err(StoreError::NoCollection(_))
        ));
    }

    #[test]
    fn list_skips_folders_that_are_not_entities_or_collections() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        fs::create_dir(dir.path().join("games/.git")).unwrap();
        fs::create_dir(dir.path().join("games/scratch")).unwrap();
        fs::create_dir_all(dir.path().join("docs/notes-1234")).unwrap();
        assert!(store.list().unwrap().is_empty());
        assert_eq!(store.collections().unwrap().len(), 1);
    }

    #[test]
    fn write_refuses_paths_outside_the_entity() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let id = store.create("games", "x").unwrap();
        for bad in ["", "../escape.ron", "/etc/passwd", "a/../../b"] {
            assert!(
                matches!(store.write(&id, bad, ""), Err(StoreError::BadPath(_))),
                "{bad}"
            );
        }
    }
}
