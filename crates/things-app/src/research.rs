//! Runs research steps for the app: fetches off the UI thread, keeps
//! responses in a cache folder outside the data, and remembers steps you
//! skipped so they stop asking.
//!
//! What a step needs, what it fetches and what it suggests all live in
//! `things_core::research`; this only moves bytes and holds results until
//! they're added or skipped.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, SystemTime};

use eframe::egui;
use things_core::EntityId;
use things_core::research::{Facts, Finding, Request, Step};

/// Cached responses older than this are fetched again.
const FRESH_FOR: Duration = Duration::from_secs(24 * 60 * 60);
const SKIPPED: &str = "skipped.ron";

/// A step that's been run for a thing.
pub enum Job {
    Running,
    Found(Finding),
    Failed(String),
}

struct Done {
    id: EntityId,
    step: Step,
    result: Result<Finding, String>,
}

pub struct Research {
    /// `None` when there's nowhere to cache; everything then lives in memory.
    cache: Option<PathBuf>,
    agent: ureq::Agent,
    jobs: HashMap<(EntityId, Step), Job>,
    /// Step keys skipped per thing, by ID.
    skipped: BTreeMap<String, BTreeSet<String>>,
    tx: Sender<Done>,
    rx: Receiver<Done>,
}

impl Research {
    pub fn new(cache: Option<PathBuf>) -> Self {
        let skipped = cache
            .as_ref()
            .and_then(|dir| fs::read_to_string(dir.join(SKIPPED)).ok())
            .and_then(|text| ron::from_str(&text).ok())
            .unwrap_or_default();
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .user_agent("Things/0.1 (a personal game journal)")
            .build()
            .into();
        let (tx, rx) = channel();
        Self {
            cache,
            agent,
            jobs: HashMap::new(),
            skipped,
            tx,
            rx,
        }
    }

    pub fn job(&self, id: &EntityId, step: Step) -> Option<&Job> {
        self.jobs.get(&(id.clone(), step))
    }

    /// Starts a step in the background. `fresh` skips the cache, for "Try
    /// again".
    pub fn run(
        &mut self,
        ctx: &egui::Context,
        id: &EntityId,
        step: Step,
        facts: Facts,
        fresh: bool,
    ) {
        let Some(request) = step.request(&facts) else {
            return;
        };
        if matches!(self.job(id, step), Some(Job::Running)) {
            return;
        }
        self.jobs.insert((id.clone(), step), Job::Running);
        let (tx, ctx, agent) = (self.tx.clone(), ctx.clone(), self.agent.clone());
        let cache = self.cache.clone();
        let id = id.clone();
        std::thread::spawn(move || {
            let result = fetch(&agent, cache.as_deref(), &request, fresh, step.source())
                .and_then(|body| step.finding(&body, &facts));
            let _ = tx.send(Done { id, step, result });
            ctx.request_repaint();
        });
    }

    /// Takes in whatever finished since the last frame.
    pub fn poll(&mut self) {
        while let Ok(done) = self.rx.try_recv() {
            let job = match done.result {
                Ok(finding) => Job::Found(finding),
                Err(e) => Job::Failed(e),
            };
            self.jobs.insert((done.id, done.step), job);
        }
    }

    /// Drops a result once it's been added or turned down.
    pub fn clear(&mut self, id: &EntityId, step: Step) {
        self.jobs.remove(&(id.clone(), step));
    }

    pub fn is_skipped(&self, id: &EntityId, step: Step) -> bool {
        self.skipped
            .get(&id.to_string())
            .is_some_and(|s| s.contains(step.key()))
    }

    /// Stops asking about a step for this thing, or starts again.
    pub fn set_skipped(&mut self, id: &EntityId, step: Step, skipped: bool) -> Result<(), String> {
        self.clear(id, step);
        let keys = self.skipped.entry(id.to_string()).or_default();
        if skipped {
            keys.insert(step.key().to_owned());
        } else {
            keys.remove(step.key());
        }
        self.skipped.retain(|_, keys| !keys.is_empty());
        let Some(dir) = &self.cache else {
            return Ok(());
        };
        let text = ron::ser::to_string_pretty(&self.skipped, Default::default())
            .expect("skipped steps always serialise");
        fs::create_dir_all(dir)
            .and_then(|()| fs::write(dir.join(SKIPPED), text))
            .map_err(|e| format!("Couldn't remember the skip in {}: {e}", dir.display()))
    }
}

/// Gets a response, from the cache when it's recent enough.
fn fetch(
    agent: &ureq::Agent,
    cache: Option<&Path>,
    request: &Request,
    fresh: bool,
    source: &str,
) -> Result<String, String> {
    let file = cache.map(|dir| {
        dir.join("http")
            .join(format!("{:016x}", fnv1a(&request.url)))
    });
    if !fresh
        && let Some(file) = &file
        && let Ok(age) = fs::metadata(file)
            .and_then(|m| m.modified())
            .map(|t| SystemTime::now().duration_since(t).unwrap_or_default())
        && age < FRESH_FOR
        && let Ok(body) = fs::read_to_string(file)
    {
        return Ok(body);
    }
    let mut call = agent.get(&request.url);
    if let Some(cookie) = request.cookie {
        call = call.header("Cookie", cookie);
    }
    let body = call
        .call()
        .and_then(|mut r| r.body_mut().read_to_string())
        .map_err(|e| format!("Couldn't reach {source}: {e}"))?;
    if let Some(file) = &file
        && let Some(dir) = file.parent()
    {
        // A cache that can't be written only costs a refetch next time.
        let _ = fs::create_dir_all(dir).and_then(|()| fs::write(file, &body));
    }
    Ok(body)
}

/// A stable hash for cache file names.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Where research keeps its cache: `$THINGS_CACHE`, else the system's cache
/// folder. Never inside the data folder, so it stays out of Git and can be
/// deleted at any time.
pub fn cache_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("THINGS_CACHE") {
        return Some(dir.into());
    }
    let var = |name| std::env::var_os(name).map(PathBuf::from);
    let base = if cfg!(windows) {
        var("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library/Caches"))
    } else {
        var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|h| h.join(".cache")))
    };
    base.map(|b| b.join("things"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_responses_are_used_without_the_network() {
        let dir = tempfile::tempdir().unwrap();
        let request = Request {
            // Nothing listens here, so a network call would fail.
            url: "http://127.0.0.1:9/api".into(),
            cookie: None,
        };
        let agent = ureq::Agent::new_with_defaults();
        assert!(fetch(&agent, Some(dir.path()), &request, false, "test").is_err());
        let cached = dir
            .path()
            .join("http")
            .join(format!("{:016x}", fnv1a(&request.url)));
        fs::create_dir_all(cached.parent().unwrap()).unwrap();
        fs::write(&cached, "{}").unwrap();
        assert_eq!(
            fetch(&agent, Some(dir.path()), &request, false, "test").unwrap(),
            "{}"
        );
        // Trying again goes past the cache.
        assert!(fetch(&agent, Some(dir.path()), &request, true, "test").is_err());
    }

    #[test]
    fn skips_are_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let id = EntityId::parse("mass-effect-7f3a").unwrap();
        let mut research = Research::new(Some(dir.path().into()));
        research.set_skipped(&id, Step::SteamTags, true).unwrap();
        let again = Research::new(Some(dir.path().into()));
        assert!(again.is_skipped(&id, Step::SteamTags));
        assert!(!again.is_skipped(&id, Step::SteamCredits));
        research.set_skipped(&id, Step::SteamTags, false).unwrap();
        assert!(!Research::new(Some(dir.path().into())).is_skipped(&id, Step::SteamTags));
    }
}
