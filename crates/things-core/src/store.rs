use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

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
}

impl Store {
    /// Opens (and creates if needed) a data folder.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let store = Self { root: root.into() };
        let entities = store.entities_dir();
        fs::create_dir_all(&entities).map_err(io_err(&entities))?;
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn entities_dir(&self) -> PathBuf {
        self.root.join("entities")
    }

    fn entity_dir(&self, id: &EntityId) -> PathBuf {
        self.entities_dir().join(id.to_string())
    }

    /// Every entity folder, sorted. Folders that aren't `slug-xxxx` are skipped.
    pub fn list(&self) -> Result<Vec<EntityId>, StoreError> {
        let dir = self.entities_dir();
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

    /// Makes a new, empty entity folder.
    pub fn create(&self, name: &str) -> Result<EntityId, StoreError> {
        loop {
            let id = EntityId::generate(name);
            let dir = self.entity_dir(&id);
            match fs::create_dir(&dir) {
                Ok(()) => return Ok(id),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(StoreError::Io {
                        path: dir,
                        source: e,
                    });
                }
            }
        }
    }

    /// Reads every component file of an entity, including subfolders.
    pub fn load(&self, id: &EntityId) -> Result<Entity, StoreError> {
        let mut components = Vec::new();
        read_tree(&self.entity_dir(id), "", &mut components)?;
        components.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Entity {
            id: id.clone(),
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
        let tmp = target.with_extension("things-tmp");
        fs::write(&tmp, text).map_err(io_err(&tmp))?;
        fs::rename(&tmp, &target).map_err(io_err(&target))
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
        Ok(self.entity_dir(id).join(rel))
    }
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

    #[test]
    fn create_write_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let id = store.create("Mass Effect").unwrap();

        store
            .write(&id, "status.ron", "Status(state: Playing)")
            .unwrap();
        store
            .write(&id, "journal/2026-10-13-1930.md", "Saved Kaidan.")
            .unwrap();

        assert_eq!(store.list().unwrap(), vec![id.clone()]);
        let entity = store.load(&id).unwrap();
        let paths: Vec<_> = entity.components.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, ["journal/2026-10-13-1930.md", "status.ron"]);
        assert_eq!(entity.components[1].text, "Status(state: Playing)");

        store.remove(&id, "status.ron").unwrap();
        assert_eq!(store.load(&id).unwrap().components.len(), 1);
    }

    #[test]
    fn list_skips_folders_that_are_not_entities() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        fs::create_dir(dir.path().join("entities/.git")).unwrap();
        fs::create_dir(dir.path().join("entities/scratch")).unwrap();
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn write_refuses_paths_outside_the_entity() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let id = store.create("x").unwrap();
        for bad in ["", "../escape.ron", "/etc/passwd", "a/../../b"] {
            assert!(
                matches!(store.write(&id, bad, ""), Err(StoreError::BadPath(_))),
                "{bad}"
            );
        }
    }
}
