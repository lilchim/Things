//! Search over the example data folder, the way the app runs it.

use std::path::Path;

use things_core::Store;
use things_core::search::{Doc, Query};

fn search(query: &str) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/data");
    let store = Store::open(root).unwrap();
    let query = Query::parse(query);
    store
        .list()
        .unwrap()
        .into_iter()
        .filter(|id| query.matches(&Doc::new(&store.load(id).unwrap())))
        .map(|id| id.slug().to_owned())
        .collect()
}

#[test]
fn example_library() {
    assert_eq!(search(""), ["hades-2", "mass-effect", "stardew-valley"]);
    assert_eq!(search("mood:chill"), ["stardew-valley"]);
    assert_eq!(search("mood:tense,epic -status:playing"), ["hades-2"]);
    assert_eq!(search("gameplay:rpg noveria"), ["mass-effect"]);
    assert_eq!(search("notes:"), ["mass-effect", "stardew-valley"]);
    assert_eq!(search("co-op"), ["stardew-valley"]);
    assert!(search("developer:blizzard").is_empty());
}
