//! Acceptance tests for virtual copies (task T03).
//!
//! A virtual copy is a photo that shares its master's file but has its own
//! stars, colour label, pick/reject flag and keywords. LightCraft's
//! `photo.virtualCopy {ids: [master id]}` is not idempotent, so the writer
//! creates each copy at most once and records it in the mapping file with the
//! global id of its master.
//!
//! Checked:
//! - first run creates one copy from the master's LightCraft id;
//! - second run creates none;
//! - a copy whose master has no LightCraft id (its import failed, or the
//!   mapping file is missing) is skipped with the `virtual_copy` code and not
//!   created blindly;
//! - the copy carries its own stars, label and keywords, the master none of
//!   them;
//! - a master with two copies creates both;
//! - a rejected `photo.virtualCopy` is not retried in the same run.

#![allow(clippy::needless_update)]

use lc_writer::apply::{ApplySummary, apply};
use lc_writer::control::{ControlClient, WriteError};
use lc_writer::plan::WriteInput;
use lc_writer::state::State;
use model::{ColorLabel, Keyword, Photo, Rating};
use std::collections::BTreeMap;
use std::path::PathBuf;

use fake_lightcraft::{Config, Failure, FakeLightCraft};
use lc_test_support::fake_lightcraft;

const PHOTO_DIR: &str = "/Users/example/Pictures";

fn kw(parts: &[&str]) -> Keyword {
    Keyword::new(parts.iter().map(|s| s.to_string()).collect()).unwrap()
}

fn path_of(gid: &str) -> String {
    format!("{PHOTO_DIR}/{gid}.jpg")
}

/// A master and one copy that shares the master's file. The copy has its own
/// rating, label and keyword; the master has none of them.
fn master_and_copy() -> WriteInput {
    let master = Photo::new(path_of("gm"), 1, Rating::new(0).unwrap()).with_global_id("gm");
    let copy = Photo::new(path_of("gm"), 2, Rating::new(4).unwrap())
        .with_global_id("gc")
        .with_master(1)
        .with_label(ColorLabel::Blue);
    let mut photo_keywords = BTreeMap::new();
    photo_keywords.insert(2, vec![kw(&["Travel"])]);
    WriteInput {
        photos: vec![master, copy],
        keywords: vec![kw(&["Travel"])],
        photo_keywords,
        ..WriteInput::default()
    }
}

struct Env {
    _dir: tempfile::TempDir,
    state_file: PathBuf,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Test.lightcraft");
    let state_file = lc_writer::state::state_path(&library);
    Env {
        _dir: dir,
        state_file,
    }
}

fn run(fake: &FakeLightCraft, env: &Env, input: &WriteInput) -> Result<ApplySummary, WriteError> {
    let mut state = State::load(&env.state_file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    apply(&mut client, input, &mut state, &env.state_file)
}

fn virtual_copy_calls(fake: &FakeLightCraft) -> Vec<serde_json::Value> {
    fake.commands()
        .into_iter()
        .filter(|(c, _)| c == "photo.virtualCopy")
        .map(|(_, p)| p)
        .collect()
}

#[test]
fn first_run_creates_one_copy() {
    let fake = FakeLightCraft::start(Config::new(&PathBuf::from("Test.lightcraft")));
    let env = env();
    let summary = run(&fake, &env, &master_and_copy()).unwrap();
    // Master imported, copy created.
    assert_eq!(summary.photos_imported, 2);
    assert!(
        summary.photos_skipped.is_empty(),
        "{:?}",
        summary.photos_skipped
    );
    // Exactly one photo.virtualCopy, naming the master's LightCraft id.
    let state = State::load(&env.state_file).unwrap();
    let master_id = state.photo("gm").unwrap().lightcraft_id.clone();
    let calls = virtual_copy_calls(&fake);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["ids"][0], master_id);
    // The copy is recorded with its own id and the master's global id.
    let copy = state.photo("gc").unwrap();
    assert_eq!(copy.copy_of.as_deref(), Some("gm"));
    assert_ne!(copy.lightcraft_id, master_id);
}

#[test]
fn second_run_creates_none() {
    let fake = FakeLightCraft::start(Config::new(&PathBuf::from("Test.lightcraft")));
    let env = env();
    let input = master_and_copy();
    run(&fake, &env, &input).unwrap();
    fake.clear_selection();
    // Second run with the state from the first.
    let summary = run(&fake, &env, &input).unwrap();
    assert_eq!(summary.photos_imported, 0);
    assert!(virtual_copy_calls(&fake).is_empty());
    assert!(
        fake.commands().iter().all(|(c, _)| c != "library.import"),
        "second run imported a photo"
    );
}

#[test]
fn copy_carries_own_stars_label_and_keywords() {
    let fake = FakeLightCraft::start(Config::new(&PathBuf::from("Test.lightcraft")));
    let env = env();
    let summary = run(&fake, &env, &master_and_copy()).unwrap();
    assert!(summary.photos_skipped.is_empty());
    let state = State::load(&env.state_file).unwrap();
    let master_id = state.photo("gm").unwrap().lightcraft_id.clone();
    let copy_id = state.photo("gc").unwrap().lightcraft_id.clone();
    // The copy was rated; the master was not (rating 0 = imported default).
    assert_eq!(fake.ratings().get(&copy_id.as_i64().unwrap()), Some(&4));
    assert!(!fake.ratings().contains_key(&master_id.as_i64().unwrap()));
    // The copy got its label; the master did not.
    let labelled: Vec<i64> = fake
        .commands()
        .into_iter()
        .filter(|(c, _)| c == "photo.label")
        .map(|(_, p)| p["ids"][0].as_i64().unwrap())
        .collect();
    assert_eq!(labelled, vec![copy_id.as_i64().unwrap()]);
    // The copy got its keyword; the master did not.
    let set_meta: Vec<i64> = fake
        .commands()
        .into_iter()
        .filter(|(c, _)| c == "photo.setMeta")
        .map(|(_, p)| p["ids"][0].as_i64().unwrap())
        .collect();
    assert_eq!(set_meta, vec![copy_id.as_i64().unwrap()]);
}

#[test]
fn master_with_two_copies_creates_both() {
    let master = Photo::new(path_of("gm"), 1, Rating::new(0).unwrap()).with_global_id("gm");
    let copy_a = Photo::new(path_of("gm"), 2, Rating::new(3).unwrap())
        .with_global_id("ga")
        .with_master(1);
    let copy_b = Photo::new(path_of("gm"), 3, Rating::new(5).unwrap())
        .with_global_id("gb")
        .with_master(1);
    let input = WriteInput {
        photos: vec![master, copy_a, copy_b],
        ..WriteInput::default()
    };
    let fake = FakeLightCraft::start(Config::new(&PathBuf::from("Test.lightcraft")));
    let env = env();
    let summary = run(&fake, &env, &input).unwrap();
    assert_eq!(summary.photos_imported, 3);
    assert!(summary.photos_skipped.is_empty());
    assert_eq!(virtual_copy_calls(&fake).len(), 2);
    let state = State::load(&env.state_file).unwrap();
    assert_eq!(state.photo("ga").unwrap().copy_of.as_deref(), Some("gm"));
    assert_eq!(state.photo("gb").unwrap().copy_of.as_deref(), Some("gm"));
}

#[test]
fn failed_command_is_not_retried() {
    let mut config = Config::new(&PathBuf::from("Test.lightcraft"));
    config.failures.push(Failure {
        command: "photo.virtualCopy".into(),
        photo_id: None,
        message: "virtual copy refused".into(),
    });
    let fake = FakeLightCraft::start(config);
    let env = env();
    let summary = run(&fake, &env, &master_and_copy()).unwrap();
    // The copy was skipped, the master still imported.
    assert_eq!(summary.photos_imported, 1);
    let codes: Vec<&str> = summary
        .photos_skipped
        .iter()
        .map(|s| s.code.as_str())
        .collect();
    assert!(codes.contains(&"rejected"), "{:?}", summary.photos_skipped);
    // Exactly one attempt: not retried.
    assert_eq!(virtual_copy_calls(&fake).len(), 1);
    // The copy was not recorded, so a re-run still has no entry for it.
    let state = State::load(&env.state_file).unwrap();
    assert!(state.photo("gc").is_none());
}

#[test]
fn copy_whose_master_has_no_lightcraft_id_is_skipped() {
    // The master's import fails, so it never gets a LightCraft id. The copy
    // cannot be created from an unknown master and is skipped with the
    // `virtual_copy` code rather than created blindly.
    let mut config = Config::new(&PathBuf::from("Test.lightcraft"));
    config.import_failed_for = Some("gm.jpg".into());
    let fake = FakeLightCraft::start(config);
    let env = env();
    let summary = run(&fake, &env, &master_and_copy()).unwrap();
    assert_eq!(summary.photos_imported, 0);
    let copy_skip = summary
        .photos_skipped
        .iter()
        .find(|s| s.code.as_str() == "virtual_copy")
        .expect("copy skipped with virtual_copy");
    assert!(copy_skip.reason.contains("virtual copy of"));
    // No photo.virtualCopy was ever sent.
    assert!(virtual_copy_calls(&fake).is_empty());
    let state = State::load(&env.state_file).unwrap();
    assert!(state.photo("gc").is_none());
}
