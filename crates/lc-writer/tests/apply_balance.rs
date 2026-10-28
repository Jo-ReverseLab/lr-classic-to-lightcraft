//! Acceptance tests for the photo balance of `lc_writer::apply` in EVERY run
//!, against the stateful fake LightCraft.
//!
//! New fields of `ApplySummary` (usize):
//! - `photos_updated`: the photo is already known (mapping file) and at least
//!   one update was sent for it (also if it failed, see below);
//! - `photos_unchanged`: the photo is known and needed no command.
//!
//! Rules:
//! - every photo counts in exactly one of the four counters or in
//!   `photos_skipped`:
//!   `photos_imported + photos_existing + photos_updated + photos_unchanged
//!   + photos_skipped.len() == number of photos in the input`, in every run;
//! - a known photo whose update failed is in `photos_incomplete` and counts in
//!   `photos_updated`; `photos_incomplete` is a subset of the photos counted
//!   in imported + existing + updated;
//! - a photo imported (or found as a duplicate) in this run counts only as
//!   imported (existing), also when later updates succeed or fail.
//!

// `..WriteInput::default()` stays valid when the struct gains fields.
#![allow(clippy::needless_update)]

use lc_writer::apply::{ApplySummary, apply};
use lc_writer::control::{ControlClient, WriteError};
use lc_writer::plan::WriteInput;
use lc_writer::state::State;
use model::{ColorLabel, Keyword, Photo, Rating};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[path = "support/fake_lightcraft.rs"]
mod fake_lightcraft;
use fake_lightcraft::{Config, Failure, FakeLightCraft};

const PHOTOS: usize = 4;

fn kw(parts: &[&str]) -> Keyword {
    Keyword::new(parts.iter().map(|s| s.to_string()).collect()).unwrap()
}

fn photo(gid: &str, lr_id: i64, rating: u8) -> Photo {
    Photo::new(
        format!("/Users/example/Pictures/{gid}.jpg"),
        lr_id,
        Rating::new(rating).unwrap(),
    )
    .with_global_id(gid)
}

/// Four photos; g2 has nothing to set after its import.
fn input() -> WriteInput {
    let mut photo_keywords = BTreeMap::new();
    photo_keywords.insert(1, vec![kw(&["Travel", "Rome"])]);
    WriteInput {
        photos: vec![
            photo("g1", 1, 3),
            photo("g2", 2, 0),
            photo("g3", 3, 5),
            photo("g4", 4, 2).with_label(ColorLabel::Blue),
        ],
        keywords: vec![kw(&["Travel", "Rome"])],
        photo_keywords,
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

fn counts(s: &ApplySummary) -> [usize; 5] {
    [
        s.photos_imported,
        s.photos_existing,
        s.photos_updated,
        s.photos_unchanged,
        s.photos_skipped.len(),
    ]
}

/// The strict balance of a run.
fn assert_balance(s: &ApplySummary, total: usize) {
    assert_eq!(counts(s).iter().sum::<usize>(), total, "{s:?}");
    assert!(
        s.photos_incomplete.len() <= s.photos_imported + s.photos_existing + s.photos_updated,
        "{s:?}"
    );
}

#[test]
fn first_run_counts_every_photo_as_imported() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let s = run(&fake, &env, &input()).unwrap();
    assert_eq!(counts(&s), [PHOTOS, 0, 0, 0, 0], "{s:?}");
    assert_balance(&s, PHOTOS);
}

#[test]
fn second_run_counts_every_photo_as_unchanged() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    run(&fake, &env, &input()).unwrap();
    let s = run(&fake, &env, &input()).unwrap();
    assert_eq!(counts(&s), [0, 0, 0, PHOTOS, 0], "{s:?}");
    assert_eq!(s.updates, 0);
    assert_balance(&s, PHOTOS);
}

#[test]
fn a_changed_field_counts_one_photo_as_updated() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    run(&fake, &env, &input()).unwrap();
    let mut changed = input();
    changed.photos[2] = photo("g3", 3, 1);
    let s = run(&fake, &env, &changed).unwrap();
    assert_eq!(counts(&s), [0, 0, 1, PHOTOS - 1, 0], "{s:?}");
    assert_eq!(s.updates, 1);
    assert!(s.photos_incomplete.is_empty());
    assert_balance(&s, PHOTOS);
}

#[test]
fn two_updates_of_one_photo_count_the_photo_once() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    run(&fake, &env, &input()).unwrap();
    let mut changed = input();
    changed.photos[3] = photo("g4", 4, 5).with_label(ColorLabel::Red);
    let s = run(&fake, &env, &changed).unwrap();
    assert_eq!(s.updates, 2, "{s:?}");
    assert_eq!(counts(&s), [0, 0, 1, PHOTOS - 1, 0], "{s:?}");
    assert_balance(&s, PHOTOS);
}

#[test]
fn a_failed_update_counts_as_imported_first_and_as_updated_when_known() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.failures.push(Failure {
        command: "photo.rate".into(),
        photo_id: Some(1003), // g3 is imported third
        message: "refused".into(),
    });
    let fake = FakeLightCraft::start(config);
    let first = run(&fake, &env, &input()).unwrap();
    assert_eq!(counts(&first), [PHOTOS, 0, 0, 0, 0], "{first:?}");
    assert_eq!(first.photos_incomplete.len(), 1, "{first:?}");
    assert_balance(&first, PHOTOS);

    // g3 is known now but still lacks its rating, which is refused again.
    let second = run(&fake, &env, &input()).unwrap();
    assert_eq!(counts(&second), [0, 0, 1, PHOTOS - 1, 0], "{second:?}");
    assert_eq!(second.photos_incomplete.len(), 1, "{second:?}");
    assert_eq!(second.updates, 0);
    assert_balance(&second, PHOTOS);
}

#[test]
fn lost_state_counts_found_photos_as_existing_and_balances() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    run(&fake, &env, &input()).unwrap();
    std::fs::remove_file(&env.state_file).unwrap();
    let s = run(&fake, &env, &input()).unwrap();
    assert_eq!(s.photos_existing, PHOTOS, "{s:?}");
    assert_eq!(s.photos_updated + s.photos_unchanged, 0, "{s:?}");
    assert_balance(&s, PHOTOS);
}

#[test]
fn a_skipped_photo_keeps_the_balance_in_every_run() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.import_failed_for = Some("g2.jpg".into());
    let fake = FakeLightCraft::start(config);
    let first = run(&fake, &env, &input()).unwrap();
    assert_eq!(counts(&first), [PHOTOS - 1, 0, 0, 0, 1], "{first:?}");
    assert_balance(&first, PHOTOS);

    // g2 is refused again; the three known photos are unchanged.
    let second = run(&fake, &env, &input()).unwrap();
    assert_eq!(counts(&second), [0, 0, 0, PHOTOS - 1, 1], "{second:?}");
    assert_balance(&second, PHOTOS);
}

#[test]
fn a_run_without_photos_balances_at_zero() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let s = run(&fake, &env, &WriteInput::default()).unwrap();
    assert_eq!(counts(&s), [0; 5]);
}
