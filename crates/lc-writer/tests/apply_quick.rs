//! Acceptance tests for writing the quick collection, module
//! `lc_writer::apply`, against the stateful fake LightCraft (which has the
//! real rules of `album.toggleTarget`: all photos already in the target ->
//! they are removed; target album set by `album.setTarget` wins over the
//! Quick Collection; Quick Collection is created when missing).
//!
//! API under test:
//! - `WriteInput.quick: Option<model::QuickCollection>`;
//! - `ApplySummary { quick_added, quick_already, quick_removed: usize,
//!   quick_skipped, quick_incomplete: Vec<Skipped> }`;
//! - the mapping file entry `quick = {members: [global ids lr2lc added]}`.
//!
//! Rules (assumptions in brackets):
//! - the quick step comes last (after albums); it is only run when some
//!   member is not in `state.quick`;
//! - run: `album.setTarget {id: null}`, `albums.list`, then members of the
//!   one top-level album named "Quick Collection" via `catalog.query
//!   {filter:{album}, offset, limit: 200}` pages, then (only when some are
//!   missing) `library.select {ids: missing, active: first}` and
//!   `album.toggleTarget {ids: missing}`; ids in Lightroom order;
//! - photos that LightCraft already has in it are never sent; no
//!   `album.clearQuick`; a second run sends no quick command at all;
//! - two albums of that name: nothing sent, `quick_skipped` has a reason
//!   with "ambiguous"; a failed command goes to `quick_incomplete`; members
//!   removed in Lightroom are only counted (`quick_removed`);
//! - [the mapping file lists the recorded global ids sorted].

use lc_writer::apply::{ApplySummary, apply};
use lc_writer::control::{ControlClient, WriteError};
use lc_writer::plan::WriteInput;
use lc_writer::state::State;
use model::{Collection, CollectionKind, Photo, QuickCollection, Rating};
use serde_json::{Value, json};
use std::path::PathBuf;

#[path = "support/fake_lightcraft.rs"]
mod fake_lightcraft;
use fake_lightcraft::{Config, Failure, FakeLightCraft, SeedAlbum};

type Commands = Vec<(String, Value)>;

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

fn input(n: i64, members: &[i64]) -> WriteInput {
    WriteInput {
        photos: photos(n),
        quick: Some(QuickCollection {
            lightroom_id: 50,
            members: members.to_vec(),
        }),
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
    (result.unwrap(), commands)
}

fn of<'a>(commands: &'a Commands, name: &str) -> Vec<&'a Value> {
    commands
        .iter()
        .filter(|(c, _)| c == name)
        .map(|(_, p)| p)
        .collect()
}

fn pos(commands: &Commands, name: &str) -> usize {
    commands
        .iter()
        .position(|(c, _)| c == name)
        .unwrap_or_else(|| panic!("no {name}: {commands:?}"))
}

fn quick_commands(commands: &Commands) -> usize {
    commands
        .iter()
        .filter(|(c, _)| {
            ["album.setTarget", "album.toggleTarget", "catalog.query"].contains(&c.as_str())
        })
        .count()
}

fn state(env: &Env) -> Value {
    serde_json::from_str(&std::fs::read_to_string(&env.state_file).unwrap()).unwrap()
}

fn quick_members(fake: &FakeLightCraft) -> Vec<i64> {
    let mut m = fake
        .quick_collection()
        .map(|a| a.photos)
        .unwrap_or_default();
    m.sort_unstable();
    m
}

#[test]
fn first_run_adds_the_members_with_select_and_toggle() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let (s, commands) = ok(&fake, &env, &input(4, &[103, 101]));
    assert_eq!(s.quick_added, 2);
    assert_eq!((s.quick_already, s.quick_removed), (0, 0));
    assert!(s.quick_skipped.is_empty() && s.quick_incomplete.is_empty());
    assert_eq!(quick_members(&fake), vec![1001, 1003]);
    assert_eq!(fake.target_album(), None);

    let set = of(&commands, "album.setTarget");
    assert_eq!(set.len(), 1);
    assert!(set[0]["id"].is_null());
    // No Quick Collection yet: nothing to read members from.
    assert!(of(&commands, "catalog.query").is_empty());
    let toggles = of(&commands, "album.toggleTarget");
    assert_eq!(toggles.len(), 1);
    assert_eq!(toggles[0]["ids"], json!([1003, 1001]));
    let select = of(&commands, "library.select");
    let last = select.last().unwrap();
    assert_eq!(last["ids"], json!([1003, 1001]));
    assert_eq!(last["active"], 1003);
    assert!(pos(&commands, "album.setTarget") < pos(&commands, "album.toggleTarget"));
    assert!(of(&commands, "album.clearQuick").is_empty());
    // Recorded at once.
    assert_eq!(state(&env)["quick"]["members"], json!(["g101", "g103"]));
}

#[test]
fn the_quick_step_comes_after_the_albums() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let mut i = input(4, &[103]);
    i.collections = vec![Collection {
        lightroom_id: 4,
        name: "Top Album".into(),
        kind: CollectionKind::Album,
        parent: None,
        members: vec![102],
    }];
    let (_, commands) = ok(&fake, &env, &i);
    let last_album = commands
        .iter()
        .rposition(|(c, _)| c == "album.addPhotos" || c == "album.create")
        .unwrap();
    assert!(last_album < pos(&commands, "album.setTarget"));
}

#[test]
fn second_run_sends_no_quick_command() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let i = input(4, &[103, 101]);
    ok(&fake, &env, &i);
    let (s, commands) = ok(&fake, &env, &i);
    assert_eq!(quick_commands(&commands), 0, "{commands:?}");
    assert_eq!((s.quick_added, s.quick_removed), (0, 0));
    assert_eq!(s.quick_already, 2);
    assert_eq!(quick_members(&fake), vec![1001, 1003]);
}

#[test]
fn a_new_member_is_the_only_one_sent() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    ok(&fake, &env, &input(4, &[103, 101]));
    let (s, commands) = ok(&fake, &env, &input(4, &[103, 101, 104]));
    assert_eq!((s.quick_added, s.quick_already), (1, 2));
    let toggles = of(&commands, "album.toggleTarget");
    assert_eq!(toggles.len(), 1);
    assert_eq!(toggles[0]["ids"], json!([1004]));
    assert_eq!(quick_members(&fake), vec![1001, 1003, 1004]);
    // The members are read from LightCraft before anything is sent.
    assert!(pos(&commands, "catalog.query") < pos(&commands, "album.toggleTarget"));
}

#[test]
fn members_that_lightcraft_already_has_are_never_sent() {
    // The mapping file is lost, LightCraft still has the members: the
    // writer reads them, sends nothing (a toggle with all in would remove).
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let i = input(4, &[103, 101]);
    ok(&fake, &env, &i);
    std::fs::remove_file(&env.state_file).unwrap();
    let (s, commands) = ok(&fake, &env, &i);
    assert_eq!(of(&commands, "album.setTarget").len(), 1);
    assert_eq!(of(&commands, "catalog.query").len(), 1);
    assert!(
        of(&commands, "album.toggleTarget").is_empty(),
        "{commands:?}"
    );
    assert_eq!((s.quick_added, s.quick_already), (0, 2));
    assert_eq!(quick_members(&fake), vec![1001, 1003]);
    // Mixed: one of three is new.
    std::fs::remove_file(&env.state_file).unwrap();
    let (s, commands) = ok(&fake, &env, &input(4, &[103, 101, 102]));
    assert_eq!((s.quick_added, s.quick_already), (1, 2));
    assert_eq!(of(&commands, "album.toggleTarget")[0]["ids"], json!([1002]));
    assert_eq!(quick_members(&fake), vec![1001, 1002, 1003]);
}

#[test]
fn members_are_read_page_by_page() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let all: Vec<i64> = (101..101 + 250).collect();
    let i = input(250, &all);
    ok(&fake, &env, &i);
    assert_eq!(quick_members(&fake).len(), 250);
    std::fs::remove_file(&env.state_file).unwrap();
    let (s, commands) = ok(&fake, &env, &i);
    let queries = of(&commands, "catalog.query");
    let offsets: Vec<u64> = queries
        .iter()
        .map(|q| q["offset"].as_u64().unwrap_or(0))
        .collect();
    assert_eq!(offsets, vec![0, 200], "{queries:?}");
    for q in &queries {
        assert!(q["filter"]["album"].is_u64());
        assert_eq!(q["limit"], 200);
    }
    assert!(of(&commands, "album.toggleTarget").is_empty());
    assert_eq!((s.quick_added, s.quick_already), (0, 250));
}

#[test]
fn a_target_album_set_by_the_user_is_reset_and_left_unchanged() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.albums = vec![SeedAlbum::album("Mine", None)];
    let fake = FakeLightCraft::start(config);
    fake.set_target_album(Some(5001));
    let (s, commands) = ok(&fake, &env, &input(4, &[101, 102]));
    assert_eq!(s.quick_added, 2);
    assert!(
        pos(&commands, "album.setTarget") < pos(&commands, "album.toggleTarget"),
        "{commands:?}"
    );
    assert!(of(&commands, "album.setTarget")[0]["id"].is_null());
    assert_eq!(fake.album_members(5001), Vec::<i64>::new());
    assert_eq!(quick_members(&fake), vec![1001, 1002]);
}

#[test]
fn two_albums_named_quick_collection_are_ambiguous() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.albums = vec![
        SeedAlbum::album("Quick Collection", None),
        SeedAlbum::album("Quick Collection", None),
    ];
    let fake = FakeLightCraft::start(config);
    let (s, commands) = ok(&fake, &env, &input(4, &[101, 102]));
    assert_eq!(s.quick_added, 0);
    assert_eq!(s.quick_skipped.len(), 1, "{:?}", s.quick_skipped);
    assert!(
        s.quick_skipped[0]
            .reason
            .to_lowercase()
            .contains("ambiguous")
    );
    assert!(of(&commands, "album.toggleTarget").is_empty());
    assert!(of(&commands, "library.select").iter().all(|s| {
        // only photo-command selects (imports need none: no ratings)
        s["ids"] != json!([1001, 1002])
    }));
    assert!(fake.albums().iter().all(|a| a.photos.is_empty()));
    assert!(
        state(&env)["quick"]["members"]
            .as_array()
            .is_none_or(Vec::is_empty)
    );
    // The photos themselves were written.
    assert_eq!(fake.photos().len(), 4);
}

#[test]
fn a_member_removed_in_lightroom_is_only_counted() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    ok(&fake, &env, &input(4, &[103, 101]));
    let (s, commands) = ok(&fake, &env, &input(4, &[103]));
    assert_eq!(s.quick_removed, 1);
    assert_eq!((s.quick_added, s.quick_already), (0, 1));
    assert!(
        of(&commands, "album.toggleTarget").is_empty(),
        "{commands:?}"
    );
    assert!(of(&commands, "album.clearQuick").is_empty());
    assert_eq!(quick_members(&fake), vec![1001, 1003]);
    // Still recorded: nothing is forgotten silently.
    assert_eq!(state(&env)["quick"]["members"], json!(["g101", "g103"]));
}

#[test]
fn a_rejected_toggle_is_incomplete_and_records_nothing() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.failures = vec![Failure {
        command: "album.toggleTarget".into(),
        photo_id: None,
        message: "no".into(),
    }];
    let fake = FakeLightCraft::start(config);
    let (s, _) = ok(&fake, &env, &input(4, &[101, 102]));
    assert_eq!(s.quick_added, 0);
    assert_eq!(s.quick_incomplete.len(), 1);
    assert!(
        state(&env)["quick"]["members"]
            .as_array()
            .is_none_or(Vec::is_empty)
    );
    assert_eq!(fake.photos().len(), 4);
}

#[test]
fn no_quick_collection_means_no_quick_command() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let mut i = input(4, &[]);
    i.quick = None;
    let (s, commands) = ok(&fake, &env, &i);
    assert_eq!(quick_commands(&commands), 0);
    assert_eq!((s.quick_added, s.quick_already, s.quick_removed), (0, 0, 0));
    // An empty quick collection sends nothing either.
    i.quick = Some(QuickCollection {
        lightroom_id: 50,
        members: Vec::new(),
    });
    let (_, commands) = ok(&fake, &env, &i);
    assert_eq!(quick_commands(&commands), 0);
    assert!(fake.quick_collection().is_none());
}

#[test]
fn a_member_without_a_lightcraft_id_is_skipped_with_a_reason() {
    // Photo 999 is not in the input photos: it has no LightCraft id.
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let (s, commands) = ok(&fake, &env, &input(2, &[101, 999]));
    assert_eq!(s.quick_added, 1);
    assert_eq!(s.quick_skipped.len(), 1);
    assert!(s.quick_skipped[0].reason.contains("999"));
    assert_eq!(of(&commands, "album.toggleTarget")[0]["ids"], json!([1001]));
}
