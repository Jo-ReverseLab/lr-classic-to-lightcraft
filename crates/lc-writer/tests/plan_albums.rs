//! Acceptance tests for the pure planning rules of collections (task
//! P5b-3b), module `lc_writer::plan`.
//!
//! STATUS: waiting for P5b-3b. The whole file is disabled by the crate-level
//! `cfg(any())` on the first line. The developer may ONLY remove that line.
//! Any other change to this file needs the test author.
//!
//! API under test (all pub):
//! - `WriteInput.collections: Vec<model::Collection>`;
//! - `Step::CreateAlbum { collection_id: i64, params: Value }`,
//!   `Step::AddMembers { collection_id: i64, gids: Vec<String> }`,
//!   `Step::RemoveMembers { collection_id: i64, gids: Vec<String> }`;
//! - `Plan::collections_known()`, `collections_renamed()`,
//!   `collections_moved()`, `collections_removed()` -> `usize`;
//! - `Plan::member_skips() -> &[Skipped]`: members whose photo is not in the
//!   input or has no global id (reason names the photo's Lightroom id, never
//!   a collection name).
//!
//! The mapping file keeps `collections`, keyed by the Lightroom collection id
//! as text: `{lightcraft_id, kind: "album" | "folder", name, parent:
//! <Lightroom id> | null, members: [sorted global ids]}`; `#[serde(default)]`,
//! so a file without the key still loads; `version` stays 1.
//!
//! Rules:
//! - plan order: all photo steps, then `CreateAlbum` (every folder before
//!   every album, a parent before its children), then `AddMembers` and
//!   `RemoveMembers`;
//! - `CreateAlbum.params` has only `name`, `folder` (bool) and `parent`;
//!   `parent` is absent on top level, the LightCraft id (number) of a
//!   parent that is known, or the text `lr-collection:<Lightroom id>` for a
//!   parent that this plan creates. Never `addSelected`;
//! - `AddMembers.gids`: members (Lightroom order) that have a global id in
//!   the input and are not in the recorded `members`; `RemoveMembers.gids`:
//!   recorded members that are no longer members;
//! - a collection that is known and has changed name or parent gets no
//!   command, only a count; a recorded collection missing from the input is
//!   only counted as removed.

use lc_writer::plan::{Plan, Step, WriteInput, plan};
use lc_writer::state::{PhotoFields, State};
use model::{Collection, CollectionKind, Photo, Rating};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn folder(id: i64, name: &str, parent: Option<i64>) -> Collection {
    Collection {
        lightroom_id: id,
        name: name.to_string(),
        kind: CollectionKind::Folder,
        parent,
        members: Vec::new(),
    }
}

fn album(id: i64, name: &str, parent: Option<i64>, members: &[i64]) -> Collection {
    Collection {
        lightroom_id: id,
        name: name.to_string(),
        kind: CollectionKind::Album,
        parent,
        members: members.to_vec(),
    }
}

/// Photos with Lightroom ids 101.. and global ids `g101`..
fn photos(n: i64) -> Vec<Photo> {
    (101..101 + n)
        .map(|i| {
            Photo::new(
                format!("/Users/example/Pictures/{i}.jpg"),
                i,
                Rating::default(),
            )
            .with_global_id(format!("g{i}"))
        })
        .collect()
}

fn input(collections: Vec<Collection>, photo_count: i64) -> WriteInput {
    WriteInput {
        photos: photos(photo_count),
        collections,
        ..WriteInput::default()
    }
}

/// The state from a mapping file text with the given `collections` and with
/// the photos `g101..` recorded as LightCraft ids 1001...
fn state_with(collections: Value, photo_count: i64) -> State {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("x.lr2lc.json");
    let text = json!({"version": 1, "photos": {}, "keywords": {}, "collections": collections});
    std::fs::write(&file, text.to_string()).unwrap();
    let mut state = State::load(&file).unwrap();
    for i in 0..photo_count {
        state.record(
            format!("g{}", 101 + i),
            101 + i,
            json!(1001 + i),
            PhotoFields::imported_default(),
        );
    }
    state
}

fn entry(id: u64, kind: &str, name: &str, parent: Option<i64>, members: &[&str]) -> Value {
    json!({
        "lightcraft_id": id,
        "kind": kind,
        "name": name,
        "parent": parent,
        "members": members,
    })
}

fn run(input: &WriteInput, state: &State) -> Plan {
    plan(input, state, &BTreeSet::new())
}

fn creates(p: &Plan) -> Vec<(i64, &Value)> {
    p.steps
        .iter()
        .filter_map(|s| match s {
            Step::CreateAlbum {
                collection_id,
                params,
            } => Some((*collection_id, params)),
            _ => None,
        })
        .collect()
}

fn adds(p: &Plan) -> Vec<(i64, Vec<&str>)> {
    p.steps
        .iter()
        .filter_map(|s| match s {
            Step::AddMembers {
                collection_id,
                gids,
            } => Some((*collection_id, gids.iter().map(String::as_str).collect())),
            _ => None,
        })
        .collect()
}

fn removes(p: &Plan) -> Vec<(i64, Vec<&str>)> {
    p.steps
        .iter()
        .filter_map(|s| match s {
            Step::RemoveMembers {
                collection_id,
                gids,
            } => Some((*collection_id, gids.iter().map(String::as_str).collect())),
            _ => None,
        })
        .collect()
}

fn position(p: &Plan, collection: i64) -> usize {
    creates(p)
        .iter()
        .position(|(id, _)| *id == collection)
        .unwrap_or_else(|| panic!("no create for {collection}"))
}

/// Set 1 > set 2 > album 3, album 4 on top, empty album 5 below set 1.
fn tree() -> Vec<Collection> {
    vec![
        album(3, "Deep", Some(2), &[102, 101]),
        album(4, "Top Album", None, &[103]),
        folder(2, "Mid", Some(1)),
        album(5, "Empty", Some(1), &[]),
        folder(1, "Top", None),
    ]
}

#[test]
fn new_tree_is_created_folders_first_and_parents_before_children() {
    let p = run(&input(tree(), 3), &State::default());
    let c = creates(&p);
    assert_eq!(c.len(), 5);
    // Every folder before every album, a parent before its children.
    assert!(position(&p, 1) < position(&p, 2));
    for folder_id in [1, 2] {
        for album_id in [3, 4, 5] {
            assert!(position(&p, folder_id) < position(&p, album_id));
        }
    }
}

#[test]
fn create_params_have_name_folder_flag_and_parent_only() {
    let p = run(&input(tree(), 3), &State::default());
    let by_id = |id: i64| creates(&p).into_iter().find(|(c, _)| *c == id).unwrap().1;
    let by_id = |id: i64| by_id(id).clone();
    assert_eq!(by_id(1), json!({"name": "Top", "folder": true}));
    assert_eq!(
        by_id(2),
        json!({"name": "Mid", "folder": true, "parent": "lr-collection:1"})
    );
    assert_eq!(
        by_id(3),
        json!({"name": "Deep", "folder": false, "parent": "lr-collection:2"})
    );
    assert_eq!(by_id(4), json!({"name": "Top Album", "folder": false}));
    assert_eq!(
        by_id(5),
        json!({"name": "Empty", "folder": false, "parent": "lr-collection:1"})
    );
    for (_, params) in creates(&p) {
        assert!(params.get("addSelected").is_none(), "{params}");
    }
}

#[test]
fn parent_that_is_known_is_sent_as_its_lightcraft_id() {
    let state = state_with(json!({"1": entry(7, "folder", "Top", None, &[])}), 0);
    let col = vec![folder(1, "Top", None), album(4, "New", Some(1), &[])];
    let p = run(&input(col, 0), &state);
    let c = creates(&p);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].0, 4);
    assert_eq!(
        c[0].1,
        &json!({"name": "New", "folder": false, "parent": 7})
    );
    assert_eq!(p.collections_known(), 1);
}

#[test]
fn names_are_sent_unchanged() {
    let name = "\u{1f305} Gru\u{308}n \u{fc}";
    let p = run(
        &input(vec![album(4, name, None, &[])], 0),
        &State::default(),
    );
    assert_eq!(creates(&p)[0].1["name"], json!(name));
}

#[test]
fn member_steps_come_after_all_photo_steps_and_all_creates() {
    let p = run(&input(tree(), 3), &State::default());
    let last_photo_step = p
        .steps
        .iter()
        .rposition(|s| matches!(s, Step::Import { .. } | Step::Update { .. }));
    let first_member_step = p
        .steps
        .iter()
        .position(|s| matches!(s, Step::AddMembers { .. } | Step::RemoveMembers { .. }));
    let last_create = p
        .steps
        .iter()
        .rposition(|s| matches!(s, Step::CreateAlbum { .. }));
    assert!(last_photo_step.unwrap() < p.steps.len());
    assert!(last_photo_step < last_create);
    assert!(last_create < first_member_step);
    // The three new photos are imported in the same plan.
    assert_eq!(
        p.steps
            .iter()
            .filter(|s| matches!(s, Step::Import { .. }))
            .count(),
        3
    );
}

#[test]
fn members_of_new_albums_keep_lightroom_order_even_for_new_photos() {
    let p = run(&input(tree(), 3), &State::default());
    let mut a = adds(&p);
    a.sort();
    assert_eq!(a, [(3, vec!["g102", "g101"]), (4, vec!["g103"])]);
    assert!(removes(&p).is_empty());
}

#[test]
fn empty_album_gets_a_create_but_no_member_step() {
    let p = run(
        &input(vec![album(5, "Empty", None, &[])], 0),
        &State::default(),
    );
    assert_eq!(creates(&p).len(), 1);
    assert!(adds(&p).is_empty());
    assert!(removes(&p).is_empty());
}

#[test]
fn nothing_changed_gives_no_collection_step() {
    let state = state_with(
        json!({
            "1": entry(7, "folder", "Top", None, &[]),
            "3": entry(8, "album", "Deep", Some(1), &["g101", "g102"]),
        }),
        2,
    );
    let col = vec![
        folder(1, "Top", None),
        album(3, "Deep", Some(1), &[102, 101]),
    ];
    let p = run(&input(col, 2), &state);
    assert!(p.steps.is_empty(), "{:?}", p.steps);
    assert_eq!(p.collections_known(), 2);
    assert_eq!(p.collections_renamed(), 0);
    assert_eq!(p.collections_moved(), 0);
    assert_eq!(p.collections_removed(), 0);
    assert!(p.member_skips().is_empty());
}

#[test]
fn only_new_members_are_added() {
    let state = state_with(json!({"3": entry(8, "album", "Deep", None, &["g101"])}), 3);
    let col = vec![album(3, "Deep", None, &[101, 103, 102])];
    let p = run(&input(col, 3), &state);
    assert_eq!(adds(&p), [(3, vec!["g103", "g102"])]);
    assert!(removes(&p).is_empty());
    assert!(creates(&p).is_empty());
}

#[test]
fn members_that_left_the_collection_are_removed() {
    let state = state_with(
        json!({"3": entry(8, "album", "Deep", None, &["g101", "g102", "g103"])}),
        3,
    );
    let col = vec![album(3, "Deep", None, &[102])];
    let p = run(&input(col, 3), &state);
    assert_eq!(removes(&p), [(3, vec!["g101", "g103"])]);
    assert!(adds(&p).is_empty());
}

#[test]
fn a_photo_removed_from_lightroom_altogether_is_removed_from_the_album() {
    // g102 is no longer in the catalog at all, so it is no member any more.
    let state = state_with(
        json!({"3": entry(8, "album", "Deep", None, &["g101", "g102"])}),
        2,
    );
    let p = run(&input(vec![album(3, "Deep", None, &[101])], 1), &state);
    assert_eq!(removes(&p), [(3, vec!["g102"])]);
}

#[test]
fn renamed_and_moved_collections_get_no_command_only_a_count() {
    let state = state_with(
        json!({
            "1": entry(7, "folder", "Old Top", None, &[]),
            "2": entry(9, "folder", "Other", None, &[]),
            "3": entry(8, "album", "Deep", Some(1), &[]),
            "4": entry(10, "album", "Both", Some(1), &[]),
        }),
        0,
    );
    let col = vec![
        folder(1, "New Top", None),
        folder(2, "Other", None),
        album(3, "Deep", Some(2), &[]),
        album(4, "Both Renamed", Some(2), &[]),
    ];
    let p = run(&input(col, 0), &state);
    assert!(p.steps.is_empty(), "{:?}", p.steps);
    assert_eq!(p.collections_known(), 4);
    assert_eq!(p.collections_renamed(), 2);
    assert_eq!(p.collections_moved(), 2);
}

#[test]
fn moved_to_top_level_counts_as_moved() {
    let state = state_with(json!({"3": entry(8, "album", "Deep", Some(1), &[])}), 0);
    let p = run(&input(vec![album(3, "Deep", None, &[])], 0), &state);
    assert_eq!(p.collections_moved(), 1);
    assert!(p.steps.is_empty());
}

#[test]
fn a_name_with_trailing_space_is_compared_as_it_was_recorded() {
    // The app trims names on creation; the mapping file keeps the name that
    // was sent, so the same input is not a rename.
    let name = "Gru\u{308}n ";
    let state = state_with(json!({"3": entry(8, "album", name, None, &[])}), 0);
    let p = run(&input(vec![album(3, name, None, &[])], 0), &state);
    assert_eq!(p.collections_renamed(), 0);
}

#[test]
fn recorded_collection_missing_from_the_input_is_only_counted() {
    let state = state_with(
        json!({
            "1": entry(7, "folder", "Top", None, &[]),
            "3": entry(8, "album", "Deep", Some(1), &["g101"]),
        }),
        1,
    );
    let p = run(&input(vec![folder(1, "Top", None)], 1), &state);
    assert_eq!(p.collections_removed(), 1);
    assert_eq!(p.collections_known(), 1);
    assert!(p.steps.is_empty(), "{:?}", p.steps);
}

#[test]
fn members_without_a_usable_photo_are_skipped_with_their_id() {
    let mut ph = photos(2);
    ph.push(Photo::new(
        "/Users/example/Pictures/x.jpg",
        777,
        Rating::default(),
    ));
    let input = WriteInput {
        photos: ph,
        collections: vec![album(3, "Deep", None, &[101, 777, 888, 102])],
        ..WriteInput::default()
    };
    let p = run(&input, &State::default());
    assert_eq!(adds(&p), [(3, vec!["g101", "g102"])]);
    let reasons: Vec<&str> = p.member_skips().iter().map(|s| s.reason.as_str()).collect();
    assert_eq!(reasons.len(), 2, "{reasons:?}");
    assert!(reasons.iter().any(|r| r.contains("777")));
    assert!(reasons.iter().any(|r| r.contains("888")));
    assert!(reasons.iter().all(|r| !r.contains("Deep")));
}

#[test]
fn a_photo_listed_twice_is_added_once() {
    let col = vec![album(3, "Deep", None, &[101, 102, 101])];
    let p = run(&input(col, 2), &State::default());
    assert_eq!(adds(&p), [(3, vec!["g101", "g102"])]);
}

#[test]
fn a_state_file_without_collections_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("x.lr2lc.json");
    std::fs::write(
        &file,
        json!({"version": 1, "photos": {}, "keywords": {}}).to_string(),
    )
    .unwrap();
    let state = State::load(&file).unwrap();
    let p = run(&input(tree(), 3), &state);
    assert_eq!(creates(&p).len(), 5);
    assert_eq!(p.collections_known(), 0);
}

#[test]
fn no_collections_means_no_collection_steps() {
    let p = run(&input(Vec::new(), 3), &State::default());
    assert!(creates(&p).is_empty() && adds(&p).is_empty() && removes(&p).is_empty());
    assert_eq!(p.collections_known(), 0);
    assert_eq!(p.collections_removed(), 0);
}

fn script_lines(p: &Plan, state: &State) -> Vec<(String, Value)> {
    p.to_script(state)
        .lines()
        .map(|l| {
            let v: Value = serde_json::from_str(l).unwrap();
            assert_eq!(v["method"], "engine.execute");
            (
                v["params"]["command"].as_str().unwrap().to_string(),
                v["params"]["params"].clone(),
            )
        })
        .collect()
}

#[test]
fn script_of_a_new_tree_uses_placeholders() {
    let col = vec![
        folder(1, "Top", None),
        album(3, "Deep", Some(1), &[102, 101]),
    ];
    let state = State::default();
    let p = run(&input(col, 2), &state);
    let lines = script_lines(&p, &state);
    let names: Vec<&str> = lines.iter().map(|(c, _)| c.as_str()).collect();
    assert_eq!(
        names,
        [
            "library.import",
            "library.import",
            "album.create",
            "album.create",
            "library.select",
            "album.addPhotos",
        ]
    );
    assert_eq!(lines[2].1, json!({"name": "Top", "folder": true}));
    assert_eq!(
        lines[3].1,
        json!({"name": "Deep", "folder": false, "parent": "lr-collection:1"})
    );
    assert_eq!(
        lines[4].1,
        json!({"ids": ["lr:g102", "lr:g101"], "active": "lr:g102"})
    );
    assert_eq!(
        lines[5].1,
        json!({"id": "lr-collection:3", "ids": ["lr:g102", "lr:g101"]})
    );
}

#[test]
fn script_of_known_albums_and_photos_uses_real_ids() {
    let state = state_with(
        json!({"3": entry(8, "album", "Deep", None, &["g101", "g103"])}),
        3,
    );
    let col = vec![album(3, "Deep", None, &[101, 102])];
    let p = run(&input(col, 3), &state);
    let lines = script_lines(&p, &state);
    let names: Vec<&str> = lines.iter().map(|(c, _)| c.as_str()).collect();
    assert_eq!(
        names,
        [
            "library.select",
            "album.addPhotos",
            "library.select",
            "album.removePhotos"
        ]
    );
    assert_eq!(lines[0].1, json!({"ids": [1002], "active": 1002}));
    assert_eq!(lines[1].1, json!({"id": 8, "ids": [1002]}));
    assert_eq!(lines[2].1, json!({"ids": [1003], "active": 1003}));
    assert_eq!(lines[3].1, json!({"id": 8, "ids": [1003]}));
}

#[test]
fn script_never_contains_add_selected_or_delete_rename_move() {
    let state = state_with(
        json!({
            "1": entry(7, "folder", "Old", None, &[]),
            "9": entry(11, "album", "Gone", None, &[]),
        }),
        0,
    );
    let col = vec![folder(1, "New", None), album(3, "Deep", Some(1), &[])];
    let p = run(&input(col, 0), &state);
    let text = p.to_script(&state);
    for word in ["addSelected", "album.delete", "album.rename", "album.move"] {
        assert!(!text.contains(word), "{word} in the script");
    }
}
