//! Acceptance tests for the mapping file (task P5-4b), module `lc_writer::state`.
//!
//! API under test:
//! - `State::default()` is empty; `State` derives Debug, Clone, PartialEq;
//! - `State::load(path) -> Result<State, StateError>`: a missing file gives the
//!   empty state; broken JSON or an unknown `version` is an error and the file
//!   is left untouched;
//! - `State::save(&self, path) -> Result<(), StateError>`: atomic (temporary
//!   file in the same folder, then rename; no temporary file is left behind);
//! - `state_path(library) -> PathBuf`: next to the library folder, named
//!   `<folder name>.lr2lc.json`;
//! - `State::record(&mut self, global_id, lightroom_id, lightcraft_id: Value,
//!   fields: PhotoFields)`, `State::record_keyword(&mut self, &Keyword)`,
//!   `snapshot(&Photo, &[Keyword]) -> PhotoFields`.
//!
//! File content: `version` is 1; per photo an object with `lightroom_id`,
//! `lightcraft_id` and `fields` (`rating`, ..., `keywords` as a sorted list of
//! `a|b|c` path texts).
//!

use lc_writer::state::{State, snapshot, state_path};
use model::{ColorLabel, Flag, Keyword, Photo, Rating};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn kw(parts: &[&str]) -> Keyword {
    Keyword::new(parts.iter().map(|s| s.to_string()).collect()).unwrap()
}

fn sample_photo() -> Photo {
    Photo::new("/Users/example/Pictures/a.jpg", 7, Rating::new(4).unwrap())
        .with_global_id("gid-a")
        .with_flag(Flag::Pick)
        .with_label(ColorLabel::Red)
        .with_title("Grüße 🎉")
        .with_caption("Caption")
        .with_copyright("(c) example")
        .with_creator("example")
}

fn sample_state() -> State {
    let keywords = vec![kw(&["Zebra"]), kw(&["Reisen", "Köln 🌅"])];
    let mut state = State::default();
    state.record("gid-a", 7, json!(42), snapshot(&sample_photo(), &keywords));
    state.record_keyword(&kw(&["Reisen"]).with_global_id("kg-1"));
    state
}

/// Finds the first object that has the given key, anywhere in the value.
fn find_object_with<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(map) => {
            if map.contains_key(key) {
                return Some(v);
            }
            map.values().find_map(|c| find_object_with(c, key))
        }
        Value::Array(items) => items.iter().find_map(|c| find_object_with(c, key)),
        _ => None,
    }
}

#[test]
fn state_path_is_next_to_the_library_folder() {
    let p = state_path(Path::new("/x/My.lightcraft"));
    assert_eq!(p, PathBuf::from("/x/My.lightcraft.lr2lc.json"));
    assert_eq!(p.parent(), Some(Path::new("/x")));
}

#[test]
fn state_path_ignores_a_trailing_separator() {
    let p = state_path(Path::new("/x/My.lightcraft/"));
    assert_eq!(p, PathBuf::from("/x/My.lightcraft.lr2lc.json"));
}

#[test]
fn load_of_a_missing_file_gives_the_empty_state() {
    let dir = tempfile::tempdir().unwrap();
    let state = State::load(&dir.path().join("none.lr2lc.json")).unwrap();
    assert_eq!(state, State::default());
}

#[test]
fn save_then_load_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("My.lightcraft.lr2lc.json");
    let state = sample_state();
    state.save(&path).unwrap();
    assert_eq!(State::load(&path).unwrap(), state);
}

#[test]
fn empty_state_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("e.lr2lc.json");
    State::default().save(&path).unwrap();
    assert_eq!(State::load(&path).unwrap(), State::default());
}

#[test]
fn saved_file_has_version_1_and_the_documented_photo_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.lr2lc.json");
    sample_state().save(&path).unwrap();
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(v["version"], json!(1));
    let photo = find_object_with(&v, "lightroom_id").expect("photo entry");
    assert_eq!(photo["lightroom_id"], json!(7));
    assert_eq!(photo["lightcraft_id"], json!(42));
    let fields = &photo["fields"];
    assert_eq!(fields["rating"], json!(4));
    assert_eq!(fields["keywords"], json!(["Reisen|Köln 🌅", "Zebra"]));
}

#[test]
fn snapshot_keywords_are_sorted_regardless_of_input_order() {
    let a = snapshot(&sample_photo(), &[kw(&["B"]), kw(&["A", "x"])]);
    let b = snapshot(&sample_photo(), &[kw(&["A", "x"]), kw(&["B"])]);
    assert_eq!(a, b);
}

#[test]
fn save_leaves_no_temporary_file_behind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.lr2lc.json");
    sample_state().save(&path).unwrap();
    sample_state().save(&path).unwrap();
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 1, "only the state file: {names:?}");
}

#[test]
fn save_replaces_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.lr2lc.json");
    State::default().save(&path).unwrap();
    let state = sample_state();
    state.save(&path).unwrap();
    assert_eq!(State::load(&path).unwrap(), state);
}

#[test]
fn broken_json_is_an_error_and_the_file_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("b.lr2lc.json");
    std::fs::write(&path, "{ not json").unwrap();
    assert!(State::load(&path).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
}

#[test]
fn empty_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("z.lr2lc.json");
    std::fs::write(&path, "").unwrap();
    assert!(State::load(&path).is_err());
}

#[test]
fn unknown_version_is_an_error_and_the_file_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v.lr2lc.json");
    let text = r#"{"version":2,"photos":{},"keywords":{}}"#;
    std::fs::write(&path, text).unwrap();
    assert!(State::load(&path).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
}

#[test]
fn missing_version_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nv.lr2lc.json");
    std::fs::write(&path, r#"{"photos":{},"keywords":{}}"#).unwrap();
    assert!(State::load(&path).is_err());
}

#[test]
fn umlaut_and_emoji_survive_the_file_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("u.lr2lc.json");
    let state = sample_state();
    state.save(&path).unwrap();
    let loaded = State::load(&path).unwrap();
    assert_eq!(loaded, state);
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let fields = &find_object_with(&v, "lightroom_id").unwrap()["fields"];
    assert_eq!(fields["title"], json!("Grüße 🎉"));
}
