//! Acceptance tests for writing collections as LightCraft albums and album
//! folders, module `lc_writer::apply`, against the stateful
//! fake LightCraft (which has the real selection gates of `album.addPhotos`
//! and `album.removePhotos`, trims names, allows equal names and refuses
//! `addSelected` and a parent that is not an existing folder).
//!
//! API under test (see also `plan_albums.rs`):
//! - `WriteInput.collections`;
//! - `ApplySummary { folders_created, albums_created, collections_known,
//!   members_added, members_removed, collections_renamed, collections_moved,
//!   collections_removed: usize; collections_skipped, collections_incomplete,
//!   members_skipped: Vec<Skipped> }`;
//! - the mapping file entry `collections.<Lightroom id>` =
//!   `{lightcraft_id, kind: "album" | "folder", name, parent, members}` with
//!   the name and parent at creation time and `members` = sorted global ids
//!   that lr2lc added (and did not remove).
//!
//! Rules:
//! - plan order: photos, then folders (parents first), then albums, then
//!   members; `album.create` params are `name`, `folder`, `parent` only;
//! - members: `library.select {ids: all LightCraft ids of the call, active:
//!   first}` directly before `album.addPhotos {id, ids}` /
//!   `album.removePhotos {id, ids}`, ids in Lightroom order;
//! - no change, no album command and no select; rename, move and deletion in
//!   Lightroom only count; never `album.delete`, `.rename`, `.move`;
//! - a member whose photo has no LightCraft id is not sent and counted in
//!   `members_skipped` (reason names the Lightroom id of the photo);
//! - `album.create` rejected: the collection and its descendants are in
//!   `collections_skipped`, nothing is recorded for them; `addPhotos` or
//!   `select` rejected: `collections_incomplete`, the run goes on, the
//!   members are not recorded; a lost connection aborts;
//! - every successful create or add/remove is recorded at once;
//! - balance: `input.collections.len() == folders_created + albums_created +
//!   collections_known + collections_skipped.len()`, in every run;
//! - reasons never contain a collection name.

use lc_writer::apply::{ApplySummary, apply, apply_partial};
use lc_writer::control::{ControlClient, WriteError};
use lc_writer::plan::WriteInput;
use lc_writer::state::State;
use model::{Collection, CollectionKind, Photo, Rating};
use serde_json::{Value, json};
use std::path::PathBuf;

use fake_lightcraft::{AlbumFailure, AlbumView, Config, Failure, FakeLightCraft};
use lc_test_support::fake_lightcraft;

const NAMES: [&str; 5] = ["Set A", "Set B", "Deep", "Top Album", "Empty"];

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

/// Photos 101..=104 (global ids g101..g104, no rating, so imports need no
/// updates and no selects), imported as 1001..1004.
fn photos() -> Vec<Photo> {
    (101..=104)
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

/// Set A (1) > Set B (2) > Deep (3: 103, 101); Top Album (4: 102) on top;
/// Empty (5) below Set A.
fn tree() -> Vec<Collection> {
    vec![
        folder(1, "Set A", None),
        folder(2, "Set B", Some(1)),
        album(3, "Deep", Some(2), &[103, 101]),
        album(4, "Top Album", None, &[102]),
        album(5, "Empty", Some(1), &[]),
    ]
}

fn input_with(collections: Vec<Collection>) -> WriteInput {
    WriteInput {
        photos: photos(),
        collections,
        ..WriteInput::default()
    }
}

struct Env {
    _dir: tempfile::TempDir,
    library: PathBuf,
    state_file: PathBuf,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Test.lightcraft");
    let state_file = lc_writer::state::state_path(&library);
    Env {
        _dir: dir,
        library,
        state_file,
    }
}

type Commands = Vec<(String, Value)>;

fn run(
    fake: &FakeLightCraft,
    env: &Env,
    input: &WriteInput,
) -> (Result<ApplySummary, WriteError>, Commands) {
    let before = fake.commands().len();
    let mut state = State::load(&env.state_file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let result = apply(&mut client, input, &mut state, &env.state_file);
    (result, fake.commands()[before..].to_vec())
}

fn ok(fake: &FakeLightCraft, env: &Env, input: &WriteInput) -> (ApplySummary, Commands) {
    let (result, commands) = run(fake, env, input);
    let summary = result.unwrap();
    assert_balance(input, &summary);
    (summary, commands)
}

fn assert_balance(input: &WriteInput, s: &ApplySummary) {
    assert_eq!(
        input.collections.len(),
        s.folders_created + s.albums_created + s.collections_known + s.collections_skipped.len(),
        "{s:?}"
    );
}

fn state(env: &Env) -> Value {
    serde_json::from_str(&std::fs::read_to_string(&env.state_file).unwrap()).unwrap()
}

fn album_named(fake: &FakeLightCraft, name: &str) -> AlbumView {
    let found: Vec<AlbumView> = fake
        .albums()
        .into_iter()
        .filter(|a| a.name == name)
        .collect();
    assert_eq!(found.len(), 1, "{name}: {found:?}");
    found.into_iter().next().unwrap()
}

fn of<'a>(commands: &'a Commands, name: &str) -> Vec<&'a Value> {
    commands
        .iter()
        .filter(|(c, _)| c == name)
        .map(|(_, p)| p)
        .collect()
}

fn album_commands(commands: &Commands) -> usize {
    commands
        .iter()
        .filter(|(c, _)| c.starts_with("album."))
        .count()
}

/// Every `album.addPhotos` and `album.removePhotos` directly follows a
/// `library.select` with exactly its ids and the first one active.
fn assert_selected_before_members(commands: &Commands) {
    for (i, (command, params)) in commands.iter().enumerate() {
        if command == "album.addPhotos" || command == "album.removePhotos" {
            let (prev, select) = &commands[i - 1];
            assert_eq!(prev, "library.select", "before {command}");
            assert_eq!(select["ids"], params["ids"], "{select} {params}");
            assert_eq!(select["active"], params["ids"][0]);
            assert!(!params["ids"].as_array().unwrap().is_empty());
        }
    }
}

fn first_run() -> (FakeLightCraft, Env) {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    ok(&fake, &env, &input_with(tree()));
    (fake, env)
}

#[test]
fn tree_is_created_with_folder_flags_and_parent_ids() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let (s, commands) = ok(&fake, &env, &input_with(tree()));
    assert_eq!((s.folders_created, s.albums_created), (2, 3));
    assert_eq!(s.collections_known, 0);
    assert!(s.collections_skipped.is_empty() && s.collections_incomplete.is_empty());

    let a = album_named(&fake, "Set A");
    let b = album_named(&fake, "Set B");
    let deep = album_named(&fake, "Deep");
    let top = album_named(&fake, "Top Album");
    let empty = album_named(&fake, "Empty");
    assert_eq!(fake.albums().len(), 5);
    assert!(a.folder && b.folder && !deep.folder && !top.folder && !empty.folder);
    assert_eq!(a.parent, None);
    assert_eq!(b.parent, Some(a.id));
    assert_eq!(deep.parent, Some(b.id));
    assert_eq!(top.parent, None);
    assert_eq!(empty.parent, Some(a.id));

    // Folders are created before albums, parents before children; only the
    // documented parameters are sent.
    let creates = of(&commands, "album.create");
    assert_eq!(creates.len(), 5);
    let kinds: Vec<bool> = creates.iter().map(|p| p["folder"] == true).collect();
    assert_eq!(kinds, [true, true, false, false, false]);
    assert_eq!(creates[0]["name"], "Set A");
    assert_eq!(creates[1]["name"], "Set B");
    for p in creates {
        let keys: Vec<&String> = p.as_object().unwrap().keys().collect();
        assert!(
            keys.iter()
                .all(|k| ["name", "folder", "parent"].contains(&k.as_str())),
            "{p}"
        );
    }
    assert!(
        !fake
            .received()
            .iter()
            .any(|r| r.line.contains("addSelected"))
    );
    // The albums come after every photo command.
    let first_create = commands
        .iter()
        .position(|(c, _)| c == "album.create")
        .unwrap();
    assert!(
        commands[first_create..]
            .iter()
            .all(|(c, _)| c != "library.import")
    );
}

#[test]
fn mapping_file_records_collections() {
    let (fake, env) = first_run();
    let st = state(&env);
    assert_eq!(st["version"], 1);
    let c = &st["collections"];
    assert_eq!(c.as_object().unwrap().len(), 5);
    let deep = album_named(&fake, "Deep");
    let b = album_named(&fake, "Set B");
    assert_eq!(
        c["3"],
        json!({
            "lightcraft_id": deep.id,
            "kind": "album",
            "name": "Deep",
            "parent": 2,
            "members": ["g101", "g103"],
        })
    );
    assert_eq!(
        c["2"],
        json!({
            "lightcraft_id": b.id,
            "kind": "folder",
            "name": "Set B",
            "parent": 1,
            "members": [],
        })
    );
    assert_eq!(c["1"]["parent"], Value::Null);
    assert_eq!(c["5"]["members"], json!([]));
}

#[test]
fn members_are_added_in_lightroom_order_after_a_select() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let (s, commands) = ok(&fake, &env, &input_with(tree()));
    assert_eq!(s.members_added, 3);
    assert_eq!(s.members_removed, 0);
    assert!(s.members_skipped.is_empty());
    // g101 = 1001 ... g104 = 1004
    assert_eq!(
        fake.album_members(album_named(&fake, "Deep").id),
        [1003, 1001]
    );
    assert_eq!(
        fake.album_members(album_named(&fake, "Top Album").id),
        [1002]
    );
    assert!(
        fake.album_members(album_named(&fake, "Empty").id)
            .is_empty()
    );
    assert_selected_before_members(&commands);
    // One call per album with members, none for the empty album.
    assert_eq!(of(&commands, "album.addPhotos").len(), 2);
    assert!(of(&commands, "album.removePhotos").is_empty());
}

#[test]
fn second_run_sends_no_album_command_and_no_select() {
    let (fake, env) = first_run();
    let albums = fake.albums();
    let (s, commands) = ok(&fake, &env, &input_with(tree()));
    assert_eq!(album_commands(&commands), 0, "{commands:?}");
    assert!(of(&commands, "library.select").is_empty());
    assert_eq!(
        (s.folders_created, s.albums_created, s.collections_known),
        (0, 0, 5)
    );
    assert_eq!((s.members_added, s.members_removed), (0, 0));
    assert_eq!(fake.albums(), albums);
}

#[test]
fn new_member_is_one_select_and_one_add_with_one_id() {
    let (fake, env) = first_run();
    // A new app session: nothing is selected.
    fake.clear_selection();
    let mut col = tree();
    col[2].members.push(104);
    let (s, commands) = ok(&fake, &env, &input_with(col));
    assert_eq!(s.members_added, 1);
    assert_eq!(album_commands(&commands), 1);
    let adds = of(&commands, "album.addPhotos");
    assert_eq!(adds.len(), 1);
    let deep = album_named(&fake, "Deep");
    assert_eq!(adds[0]["id"], deep.id);
    assert_eq!(adds[0]["ids"], json!([1004]));
    assert_eq!(
        of(&commands, "library.select"),
        [&json!({"ids": [1004], "active": 1004})]
    );
    assert_selected_before_members(&commands);
    assert_eq!(fake.album_members(deep.id), [1003, 1001, 1004]);
    assert_eq!(
        state(&env)["collections"]["3"]["members"],
        json!(["g101", "g103", "g104"])
    );
    // And now it is stable again.
    let mut col = tree();
    col[2].members.push(104);
    let (_, again) = ok(&fake, &env, &input_with(col));
    assert_eq!(album_commands(&again), 0);
}

#[test]
fn removed_member_is_one_select_and_one_remove() {
    let (fake, env) = first_run();
    fake.clear_selection();
    let mut col = tree();
    col[2].members = vec![103];
    let (s, commands) = ok(&fake, &env, &input_with(col));
    assert_eq!((s.members_added, s.members_removed), (0, 1));
    assert_eq!(album_commands(&commands), 1);
    let removes = of(&commands, "album.removePhotos");
    assert_eq!(removes.len(), 1);
    let deep = album_named(&fake, "Deep");
    assert_eq!(removes[0]["id"], deep.id);
    assert_eq!(removes[0]["ids"], json!([1001]));
    assert_selected_before_members(&commands);
    assert_eq!(fake.album_members(deep.id), [1003]);
    assert_eq!(state(&env)["collections"]["3"]["members"], json!(["g103"]));
}

#[test]
fn renamed_and_moved_collections_are_only_counted_every_run() {
    let (fake, env) = first_run();
    let albums = fake.albums();
    let mut col = tree();
    col[2].name = "Deep Renamed".to_string();
    col[3].parent = Some(1);
    for _ in 0..2 {
        let (s, commands) = ok(&fake, &env, &input_with(col.clone()));
        assert_eq!(album_commands(&commands), 0);
        assert_eq!((s.collections_renamed, s.collections_moved), (1, 1));
        assert_eq!(s.collections_known, 5);
    }
    assert_eq!(fake.albums(), albums);
    assert_eq!(state(&env)["collections"]["3"]["name"], "Deep");
    assert_eq!(state(&env)["collections"]["4"]["parent"], Value::Null);
}

#[test]
fn collection_deleted_in_lightroom_is_only_counted() {
    let (fake, env) = first_run();
    let albums = fake.albums();
    let mut col = tree();
    col.retain(|c| c.lightroom_id != 5);
    let (s, commands) = ok(&fake, &env, &input_with(col));
    assert_eq!(s.collections_removed, 1);
    assert_eq!(s.collections_known, 4);
    assert_eq!(album_commands(&commands), 0);
    assert_eq!(fake.albums(), albums);
    for word in ["album.delete", "album.rename", "album.move"] {
        assert!(!fake.received().iter().any(|r| r.line.contains(word)));
    }
}

#[test]
fn member_whose_photo_was_not_imported_is_skipped_with_its_id() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.import_failed_for = Some("102.jpg".to_string());
    let fake = FakeLightCraft::start(config);
    let input = input_with(tree());
    let (s, commands) = ok(&fake, &env, &input);
    // Top Album is created, but its only member has no LightCraft id.
    assert_eq!(s.albums_created, 3);
    assert_eq!(s.members_added, 2);
    assert_eq!(s.members_skipped.len(), 1, "{:?}", s.members_skipped);
    let reason = &s.members_skipped[0].reason;
    assert!(reason.contains("102"), "{reason}");
    for name in NAMES {
        assert!(!reason.contains(name), "{reason}");
    }
    assert!(
        fake.album_members(album_named(&fake, "Top Album").id)
            .is_empty()
    );
    assert!(
        of(&commands, "album.addPhotos")
            .iter()
            .all(|p| p["ids"].as_array().unwrap().len() == 2)
    );
    assert_selected_before_members(&commands);
}

#[test]
fn rejected_create_skips_the_collection_and_its_descendants() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.album_failures.push(AlbumFailure {
        command: "album.create".into(),
        album_name: Some("Set B".into()),
        message: "boom".into(),
    });
    let fake = FakeLightCraft::start(config);
    let input = input_with(tree());
    let (s, commands) = ok(&fake, &env, &input);
    // Set B and its child Deep are skipped; the rest is written.
    assert_eq!((s.folders_created, s.albums_created), (1, 2));
    assert_eq!(
        s.collections_skipped.len(),
        2,
        "{:?}",
        s.collections_skipped
    );
    let text: String = s
        .collections_skipped
        .iter()
        .map(|x| x.reason.clone())
        .collect();
    assert!(text.contains('2') && text.contains('3'), "{text}");
    for name in NAMES {
        assert!(!text.contains(name), "{text}");
    }
    // No create for the child, no members for it, nothing recorded for either.
    let creates = of(&commands, "album.create");
    assert_eq!(creates.len(), 4);
    assert!(creates.iter().all(|p| p["name"] != "Deep"));
    assert!(
        fake.albums()
            .iter()
            .all(|a| a.name != "Deep" && a.name != "Set B")
    );
    let st = state(&env);
    let recorded = st["collections"].as_object().unwrap();
    assert!(!recorded.contains_key("2") && !recorded.contains_key("3"));
    assert_eq!(recorded.len(), 3);
    assert_eq!(s.members_added, 1);
    // The next run tries only the skipped ones again.
    let (s2, c2) = ok(&fake, &env, &input);
    assert_eq!(of(&c2, "album.create").len(), 1);
    assert_eq!(s2.collections_known, 3);
    assert_eq!(s2.collections_skipped.len(), 2);
    assert_eq!(fake.albums().len(), 3);
}

#[test]
fn rejected_add_makes_the_collection_incomplete_and_the_run_goes_on() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.album_failures.push(AlbumFailure {
        command: "album.addPhotos".into(),
        album_name: Some("Deep".into()),
        message: "boom".into(),
    });
    let fake = FakeLightCraft::start(config);
    let input = input_with(tree());
    let (s, _) = ok(&fake, &env, &input);
    assert_eq!((s.folders_created, s.albums_created), (2, 3));
    assert_eq!(s.collections_incomplete.len(), 1);
    let reason = &s.collections_incomplete[0].reason;
    assert!(reason.contains('3'), "{reason}");
    for name in NAMES {
        assert!(!reason.contains(name), "{reason}");
    }
    assert_eq!(s.members_added, 1, "only Top Album's member");
    assert_eq!(
        fake.album_members(album_named(&fake, "Top Album").id),
        [1002]
    );
    // Deep exists and is recorded, without members; it is not created again.
    let st = state(&env);
    assert_eq!(st["collections"]["3"]["members"], json!([]));
    assert!(st["collections"]["3"]["lightcraft_id"].is_number());
    let (s2, c2) = ok(&fake, &env, &input);
    assert!(of(&c2, "album.create").is_empty());
    assert_eq!(of(&c2, "album.addPhotos").len(), 1);
    assert_eq!(s2.collections_incomplete.len(), 1);
    assert_eq!(fake.albums().len(), 5);
}

#[test]
fn rejected_select_makes_the_collection_incomplete() {
    let env = env();
    let mut config = Config::new(&env.library);
    // Deep is selected as [1003, 1001]; photos have no updates, so no other
    // select starts with 1003.
    config.failures.push(Failure {
        command: "library.select".into(),
        photo_id: Some(1003),
        message: "boom".into(),
    });
    let fake = FakeLightCraft::start(config);
    let input = input_with(tree());
    let (s, commands) = ok(&fake, &env, &input);
    assert_eq!(s.collections_incomplete.len(), 1);
    assert_eq!(s.members_added, 1);
    // No add is sent after a rejected select of the same collection.
    assert_eq!(of(&commands, "album.addPhotos").len(), 1);
    assert!(fake.album_members(album_named(&fake, "Deep").id).is_empty());
    assert_eq!(state(&env)["collections"]["3"]["members"], json!([]));
}

#[test]
fn lost_connection_during_creates_keeps_what_was_created() {
    let env = env();
    let mut config = Config::new(&env.library);
    // 1 keyword.list, 2-5 imports, 6 albums.list, 7-8 folders, 9 the first album.
    config.close_at = Some(9);
    let fake = FakeLightCraft::start(config);
    let input = input_with(tree());
    let before = fake.commands().len();
    let mut state_in = State::load(&env.state_file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let aborted = apply_partial(&mut client, &input, &mut state_in, &env.state_file).unwrap_err();
    assert_eq!(before, 0);
    assert!(
        matches!(&aborted.error, WriteError::ConnectionLost { command } if command == "album.create"),
        "{:?}",
        aborted.error
    );
    assert_eq!(aborted.summary.folders_created, 2);
    assert_eq!(aborted.summary.albums_created, 0);
    let st = state(&env);
    assert_eq!(st["collections"].as_object().unwrap().len(), 2);
    assert_eq!(fake.albums().len(), 2);

    // The follow-up run creates only the missing albums.
    let (s, commands) = ok(&fake, &env, &input);
    assert_eq!(of(&commands, "album.create").len(), 3);
    assert_eq!(
        (s.folders_created, s.albums_created, s.collections_known),
        (0, 3, 2)
    );
    assert_eq!(fake.albums().len(), 5);
    assert_eq!(
        fake.album_members(album_named(&fake, "Deep").id),
        [1003, 1001]
    );
    for n in NAMES {
        album_named(&fake, n);
    }
}

#[test]
fn lost_connection_during_members_loses_nothing_and_adds_nothing_twice() {
    let env = env();
    let mut config = Config::new(&env.library);
    // 1 keyword.list, 2-5 imports, 6 albums.list, 7-11 creates, 12 select, 13 first add.
    config.close_at = Some(13);
    let fake = FakeLightCraft::start(config);
    let input = input_with(tree());
    let (result, _) = run(&fake, &env, &input);
    assert!(matches!(
        result,
        Err(WriteError::ConnectionLost { ref command }) if command == "album.addPhotos"
    ));
    assert_eq!(fake.albums().len(), 5);
    let st = state(&env);
    assert_eq!(st["collections"].as_object().unwrap().len(), 5);
    let (s, commands) = ok(&fake, &env, &input);
    assert!(of(&commands, "album.create").is_empty());
    assert_eq!(s.collections_known, 5);
    assert_eq!(s.members_added, 3);
    assert_eq!(fake.albums().len(), 5);
    assert_eq!(
        fake.album_members(album_named(&fake, "Deep").id),
        [1003, 1001]
    );
    assert_eq!(
        fake.album_members(album_named(&fake, "Top Album").id),
        [1002]
    );
}

#[test]
fn state_file_without_collections_from_an_older_run_still_works() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    ok(&fake, &env, &input_with(Vec::new()));
    let mut st = state(&env);
    st.as_object_mut().unwrap().remove("collections");
    std::fs::write(&env.state_file, st.to_string()).unwrap();
    let (s, _) = ok(&fake, &env, &input_with(tree()));
    assert_eq!((s.folders_created, s.albums_created), (2, 3));
    assert_eq!(state(&env)["version"], 1);
}

#[test]
fn equal_names_below_one_parent_become_two_albums() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let col = vec![
        folder(1, "Set A", None),
        album(2, "Twin", Some(1), &[101]),
        album(3, "Twin", Some(1), &[102]),
    ];
    let (s, _) = ok(&fake, &env, &input_with(col.clone()));
    assert_eq!(s.albums_created, 2);
    assert_eq!(fake.albums().iter().filter(|a| a.name == "Twin").count(), 2);
    let (_, again) = ok(&fake, &env, &input_with(col));
    assert_eq!(album_commands(&again), 0);
    assert_eq!(fake.albums().len(), 3);
}

#[test]
fn awkward_names_are_sent_and_not_seen_as_renamed_later() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let col = vec![
        album(1, "\u{1f305} S\u{fc}\u{df}e \u{c4}pfel", None, &[]),
        album(2, "Gru\u{308}n ", None, &[]),
    ];
    ok(&fake, &env, &input_with(col.clone()));
    let names: Vec<String> = fake.albums().into_iter().map(|a| a.name).collect();
    assert!(names.contains(&"\u{1f305} S\u{fc}\u{df}e \u{c4}pfel".to_string()));
    // The app trims; the mapping file keeps what lr2lc knows from Lightroom.
    let (s, commands) = ok(&fake, &env, &input_with(col));
    assert_eq!(album_commands(&commands), 0);
    assert_eq!(s.collections_renamed, 0);
}

#[test]
fn balance_holds_with_a_mix_of_known_new_and_skipped() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.album_failures.push(AlbumFailure {
        command: "album.create".into(),
        album_name: Some("Late".into()),
        message: "boom".into(),
    });
    let fake = FakeLightCraft::start(config);
    let first = input_with(tree());
    ok(&fake, &env, &first);
    let mut col = tree();
    col.push(album(6, "Late", Some(2), &[104]));
    col.push(album(7, "Fresh", None, &[104]));
    let (s, _) = ok(&fake, &env, &input_with(col));
    assert_eq!(s.collections_known, 5);
    assert_eq!(s.albums_created, 1);
    assert_eq!(s.collections_skipped.len(), 1);
}

#[test]
fn photo_commands_are_unchanged_by_collections() {
    // Same photos, with and without collections: the photo commands match.
    let a = env();
    let fa = FakeLightCraft::start(Config::new(&a.library));
    let (_, ca) = ok(&fa, &a, &input_with(Vec::new()));
    let b = env();
    let fb = FakeLightCraft::start(Config::new(&b.library));
    let (_, cb) = ok(&fb, &b, &input_with(tree()));
    let photo_part = |c: &Commands| -> Commands {
        c.iter()
            .filter(|(n, _)| {
                !n.starts_with("album.") && n != "library.select" && n != "albums.list"
            })
            .cloned()
            .collect()
    };
    assert_eq!(photo_part(&ca), photo_part(&cb));
}
