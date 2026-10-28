//! Apply-path tests for the skip codes `no_id_returned`,
//! `applied_not_saved` and `ambiguous`, module
//! `lc_writer::apply`, against the stateful fake LightCraft.
//!
//! The implementation exists, so these tests are green from the start. The
//! fake got three switches for them (`Config`): `albums_no_id`,
//! `import_no_id` and `not_saved_creates`.
//!
//! Reachability:
//! - `no_id_returned`: `album.create`, `album.createSmart` and
//!   `library.import` applied but answered without an id;
//! - `applied_not_saved`: the same three commands answered `saved in memory
//!   but not written to disk`. This is LightCraft's reply, not a failure to
//!   write lr2lc's own mapping file; a failing mapping file is a hard error
//!   (`WriteError`), not a skip, and is not a skip code;
//! - `ambiguous`: two seeded albums (or smart albums) that match one
//!   collection after the mapping file was lost.
//!
//! Rules checked for each: the entry has the code and the reason names no
//! collection or photo name; the item is not recorded in the mapping file, so
//! a later healthy run creates it;
//! the balance holds.

use lc_writer::apply::{ApplySummary, apply};
use lc_writer::control::{ControlClient, WriteError};
use lc_writer::plan::WriteInput;
use lc_writer::smart::translate;
use lc_writer::state::State;
use model::{
    Collection, CollectionKind, Photo, Rating, SkipCode, SmartCollection, SmartCombine, SmartRule,
    SmartRules, SmartValue,
};
use serde_json::Value;
use std::path::PathBuf;

#[path = "support/fake_lightcraft.rs"]
mod fake_lightcraft;
use fake_lightcraft::{Config, FakeLightCraft, SeedAlbum};

const NAME: &str = "ZyxName";

fn rules() -> SmartRules {
    SmartRules {
        combine: SmartCombine::Intersect,
        rules: vec![SmartRule {
            criterion: "rating".into(),
            operation: ">=".into(),
            value: Some(SmartValue::Number(3.0)),
            value2: None,
            units: None,
        }],
    }
}

fn album_input() -> WriteInput {
    WriteInput {
        collections: vec![Collection {
            lightroom_id: 1,
            name: NAME.into(),
            kind: CollectionKind::Album,
            parent: None,
            members: Vec::new(),
        }],
        ..WriteInput::default()
    }
}

fn smart_input() -> WriteInput {
    WriteInput {
        smart: vec![SmartCollection {
            lightroom_id: 2,
            name: NAME.into(),
            parent: None,
            rules: rules(),
        }],
        ..WriteInput::default()
    }
}

fn photo_input() -> WriteInput {
    WriteInput {
        photos: vec![
            Photo::new(
                format!("/Users/example/Pictures/{NAME}.jpg"),
                7,
                Rating::default(),
            )
            .with_global_id("g7"),
        ],
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

fn run(fake: &FakeLightCraft, env: &Env, input: &WriteInput) -> Result<ApplySummary, WriteError> {
    let mut state = State::load(&env.state_file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    apply(&mut client, input, &mut state, &env.state_file)
}

fn state(env: &Env) -> Value {
    std::fs::read_to_string(&env.state_file)
        .ok()
        .map(|t| serde_json::from_str(&t).unwrap())
        .unwrap_or(Value::Null)
}

fn only(list: &[model::Skipped]) -> &model::Skipped {
    assert_eq!(list.len(), 1, "{list:?}");
    &list[0]
}

fn clean(reason: &str) {
    assert!(!reason.contains(NAME), "{reason}");
}

// ---- no_id_returned ----------------------------------------------------

#[test]
fn album_create_without_id_is_no_id_returned() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.albums_no_id = true;
    let fake = FakeLightCraft::start(config);
    let s = run(&fake, &env, &album_input()).unwrap();
    let e = only(&s.collections_skipped);
    assert_eq!(e.code, SkipCode::NoIdReturned);
    assert!(e.reason.contains("album.create"), "{}", e.reason);
    clean(&e.reason);
    assert_eq!(s.albums_created + s.folders_created, 0);
    assert_eq!(
        1,
        s.folders_created + s.albums_created + s.collections_known + s.collections_skipped.len()
    );
    assert!(state(&env)["collections"].get("1").is_none());
}

#[test]
fn smart_create_without_id_is_no_id_returned() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.albums_no_id = true;
    let fake = FakeLightCraft::start(config);
    let s = run(&fake, &env, &smart_input()).unwrap();
    let e = only(&s.smart_skipped);
    assert_eq!(e.code, SkipCode::NoIdReturned);
    assert!(e.reason.contains("album.createSmart"), "{}", e.reason);
    clean(&e.reason);
    assert_eq!(s.smart_created + s.smart_known, 0);
    assert!(state(&env)["collections"].get("2").is_none());
}

#[test]
fn import_without_id_is_no_id_returned() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.import_no_id = true;
    let fake = FakeLightCraft::start(config);
    let s = run(&fake, &env, &photo_input()).unwrap();
    let e = only(&s.photos_skipped);
    assert_eq!(e.code, SkipCode::NoIdReturned);
    assert!(e.reason.contains("library.import"), "{}", e.reason);
    clean(&e.reason);
    assert_eq!(s.photos_imported + s.photos_existing, 0);
    assert!(
        state(&env)["photos"]
            .as_object()
            .is_none_or(|m| m.is_empty())
    );
}

// ---- applied_not_saved -------------------------------------------------

#[test]
fn album_create_not_saved_is_applied_not_saved() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.not_saved_creates = true;
    let fake = FakeLightCraft::start(config);
    let s = run(&fake, &env, &album_input()).unwrap();
    let e = only(&s.collections_skipped);
    assert_eq!(e.code, SkipCode::AppliedNotSaved);
    clean(&e.reason);
    assert_eq!(s.not_saved, 1);
    assert!(state(&env)["collections"].get("1").is_none());
}

#[test]
fn smart_create_not_saved_is_applied_not_saved() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.not_saved_creates = true;
    let fake = FakeLightCraft::start(config);
    let s = run(&fake, &env, &smart_input()).unwrap();
    let e = only(&s.smart_skipped);
    assert_eq!(e.code, SkipCode::AppliedNotSaved);
    clean(&e.reason);
    assert_eq!(s.not_saved, 1);
    assert!(state(&env)["collections"].get("2").is_none());
}

#[test]
fn import_not_saved_is_applied_not_saved() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.not_saved_creates = true;
    let fake = FakeLightCraft::start(config);
    let s = run(&fake, &env, &photo_input()).unwrap();
    let e = only(&s.photos_skipped);
    assert_eq!(e.code, SkipCode::AppliedNotSaved);
    clean(&e.reason);
    assert_eq!(s.not_saved, 1);
    assert!(
        state(&env)["photos"]
            .as_object()
            .is_none_or(|m| m.is_empty())
    );
}

// ---- ambiguous ---------------------------------------------------------

#[test]
fn two_matching_albums_are_ambiguous_and_nothing_is_created() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.albums = vec![SeedAlbum::album(NAME, None), SeedAlbum::album(NAME, None)];
    let fake = FakeLightCraft::start(config);
    let s = run(&fake, &env, &album_input()).unwrap();
    let e = only(&s.collections_skipped);
    assert_eq!(e.code, SkipCode::Ambiguous);
    assert!(e.reason.contains("ambiguous: 2 albums"), "{}", e.reason);
    clean(&e.reason);
    assert_eq!(fake.albums().len(), 2, "nothing created");
    assert!(state(&env)["collections"].get("1").is_none());
}

#[test]
fn two_matching_smart_albums_are_ambiguous_and_nothing_is_created() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.albums = vec![
        SeedAlbum::smart(NAME, None, translate(&rules()).unwrap()),
        SeedAlbum::smart(NAME, None, translate(&rules()).unwrap()),
    ];
    let fake = FakeLightCraft::start(config);
    let s = run(&fake, &env, &smart_input()).unwrap();
    let e = only(&s.smart_skipped);
    assert_eq!(e.code, SkipCode::Ambiguous);
    assert!(
        e.reason.contains("ambiguous: 2 smart albums"),
        "{}",
        e.reason
    );
    clean(&e.reason);
    assert_eq!(fake.albums().len(), 2, "nothing created");
}
