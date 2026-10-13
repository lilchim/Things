//! Search over the example data folder, the way the app runs it.

use std::path::Path;

use chrono::NaiveDateTime;
use things_core::Store;
use things_core::search::{Doc, Query};
use things_core::tasks::{self, Agenda, Entry};

fn search(query: &str) -> Vec<String> {
    search_on(query, "2026-10-13")
}

/// Searches as if it were `today`, so date comparisons stay put.
fn search_on(query: &str, today: &str) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/data");
    let store = Store::open(root).unwrap();
    let query = Query::parse_on(query, today);
    store
        .list()
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .filter(|id| query.matches(&Doc::new(&store.load(id).unwrap())))
        .map(|id| id.slug().to_owned())
        .collect()
}

#[test]
fn example_library() {
    assert_eq!(
        search(""),
        [
            "hades-2",
            "lumen-drift",
            "mass-effect",
            "mow-the-lawn",
            "renew-passport",
            "stardew-valley",
            "tiny-orchard",
            "wuthering-waves"
        ]
    );
    assert_eq!(search("mood:chill"), ["stardew-valley", "tiny-orchard"]);
    assert_eq!(search("mood:tense,epic -status:playing"), ["hades-2"]);
    assert_eq!(search("gameplay:rpg noveria"), ["mass-effect"]);
    assert_eq!(
        search("notes: collection:games"),
        ["mass-effect", "stardew-valley"]
    );
    assert_eq!(search("co-op"), ["stardew-valley"]);
    assert!(search("developer:blizzard").is_empty());
}

#[test]
fn interest_and_release() {
    assert_eq!(
        search("ownership:interested"),
        ["lumen-drift", "tiny-orchard"]
    );
    assert_eq!(search("ownership:interested -release:"), ["tiny-orchard"]);
    assert_eq!(search("via:sam"), ["tiny-orchard"]);
    assert_eq!(search("date:>today"), ["lumen-drift"]);
    assert!(search_on("date:>today", "2026-10-19").is_empty());
    assert_eq!(search("added:<2026-09"), ["tiny-orchard"]);
}

#[test]
fn collections() {
    assert_eq!(
        search("collection:tasks"),
        ["mow-the-lawn", "renew-passport"]
    );
    assert_eq!(search("saturday"), ["mow-the-lawn"]);
    assert_eq!(
        search("-collection:games notes:"),
        search("collection:tasks")
    );
}

#[test]
fn task_fields() {
    assert_eq!(
        search("tasks:"),
        ["mow-the-lawn", "renew-passport", "wuthering-waves"]
    );
    assert_eq!(search("repeat:daily"), ["wuthering-waves"]);
    assert_eq!(search("due:<2026-11"), ["wuthering-waves"]);
}

#[test]
fn example_agenda() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/data");
    let store = Store::open(root).unwrap();
    let lists: Vec<_> = store
        .list()
        .unwrap()
        .into_iter()
        .filter_map(|(id, _)| Some((id.clone(), tasks::of(&store.load(&id).ok()?)?)))
        .collect();
    let now = NaiveDateTime::parse_from_str("2026-10-13 20:00", "%Y-%m-%d %H:%M").unwrap();
    let agenda = Agenda::new(lists.iter().map(|(id, t)| (id, t)), now);
    fn names(list: &[Entry]) -> Vec<&str> {
        list.iter().map(|e| e.task.name.as_str()).collect()
    }
    assert_eq!(names(&agenda.overdue), ["Mow the lawn"]);
    assert_eq!(
        names(&agenda.now),
        ["Daily activity and Waveplates", "Weekly bosses"]
    );
    assert_eq!(
        names(&agenda.coming),
        [
            "Tower of Adversity",
            "Spend the event shop currency",
            "Book passport photos"
        ]
    );
    assert_eq!(names(&agenda.anytime), ["Send the online form"]);
}
