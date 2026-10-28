//! Acceptance tests for the selection before photo commands (task P5-4e),
//! against the stateful fake LightCraft, which behaves like the real app:
//! `photo.rate`, `photo.flag`, `photo.label` and `photo.setMeta` fail with
//! `no photos selected` while nothing is selected, `library.select` sets the
//! selection, and an import selects the imported photo.
//!
//! Rules:
//! - before the FIRST update of a photo the writer sends exactly one
//!   `library.select {ids:[<lightcraft id>], active:<lightcraft id>}`,
//!   directly before that update, also for a photo that was just imported
//!   (never rely on the import's side effect) and for a photo found as a
//!   duplicate;
//! - a photo without updates gets no select;
//! - no `photo.*` command is sent unless the latest select of the same run
//!   named exactly that photo;
//! - `library.select` is not an update: `updates` does not count it;
//! - a rejected select puts the photo in `photos_incomplete`, its updates are
//!   not sent, the run goes on, the balance holds; a lost connection aborts.
//!

use lc_writer::apply::{ApplySummary, apply};
use lc_writer::control::{ControlClient, WriteError};
use lc_writer::plan::WriteInput;
use lc_writer::state::State;
use model::{ColorLabel, Photo, Rating};
use serde_json::{Value, json};
use std::path::PathBuf;

#[path = "support/fake_lightcraft.rs"]
mod fake_lightcraft;
use fake_lightcraft::{Config, Failure, FakeLightCraft};

fn photo(gid: &str, lr_id: i64, rating: u8) -> Photo {
    Photo::new(
        format!("/Users/example/Pictures/{gid}.jpg"),
        lr_id,
        Rating::new(rating).unwrap(),
    )
    .with_global_id(gid)
}

/// Four photos, imported in this order as 1001..1004. g2 needs no update
/// after its import.
fn input() -> WriteInput {
    WriteInput {
        photos: vec![
            photo("g1", 1, 3),
            photo("g2", 2, 0),
            photo("g3", 3, 5),
            photo("g4", 4, 2).with_label(ColorLabel::Blue),
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

/// One run; returns the result and the commands of this run only.
fn run(
    fake: &FakeLightCraft,
    env: &Env,
    input: &WriteInput,
) -> (Result<ApplySummary, WriteError>, Vec<(String, Value)>) {
    let before = fake.commands().len();
    let mut state = State::load(&env.state_file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let result = apply(&mut client, input, &mut state, &env.state_file);
    (result, fake.commands()[before..].to_vec())
}

/// The commands that matter here: everything except the reads.
fn names(commands: &[(String, Value)]) -> Vec<&str> {
    commands
        .iter()
        .map(|(c, _)| c.as_str())
        .filter(|c| *c != "keyword.list")
        .collect()
}

fn is_select(command: &str) -> bool {
    command == "library.select"
}

fn selects(commands: &[(String, Value)]) -> Vec<&Value> {
    commands
        .iter()
        .filter(|(c, _)| is_select(c))
        .map(|(_, p)| p)
        .collect()
}

fn select_params(id: i64) -> Value {
    json!({"ids": [id], "active": id})
}

/// Every `photo.*` command of the run follows a select naming its photo,
/// with no other select in between; an import resets nothing.
fn assert_always_selected(commands: &[(String, Value)]) {
    let mut selected: Option<i64> = None;
    for (command, params) in commands {
        if is_select(command) {
            assert_eq!(params["ids"].as_array().map(Vec::len), Some(1), "{params}");
            assert_eq!(params["ids"][0], params["active"], "{params}");
            selected = params["ids"][0].as_i64();
        } else if command.starts_with("photo.") {
            assert_eq!(
                params["ids"][0].as_i64(),
                selected,
                "{command} {params} sent without a select for its photo"
            );
            assert!(selected.is_some());
        }
    }
}

fn seeded(env: &Env) -> FakeLightCraft {
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let (first, _) = run(&fake, env, &input());
    first.unwrap();
    // The app was restarted since: nothing is selected, as in a real
    // second session. Photo commands now need the writer's own select.
    fake.clear_selection();
    fake
}

#[test]
fn known_photos_get_one_select_each_right_before_their_first_update() {
    let env = env();
    let fake = seeded(&env);
    let mut changed = input();
    changed.photos[0] = photo("g1", 1, 1);
    changed.photos[2] = photo("g3", 3, 2).with_label(ColorLabel::Red);
    let (s, commands) = run(&fake, &env, &changed);
    let s = s.unwrap();
    assert_eq!(
        names(&commands),
        [
            "library.select",
            "photo.rate",
            "library.select",
            "photo.rate",
            "photo.label"
        ]
    );
    let sel = selects(&commands);
    assert_eq!(sel, [&select_params(1001), &select_params(1003)]);
    assert_eq!(s.updates, 3, "library.select is not an update: {s:?}");
    assert!(s.photos_incomplete.is_empty(), "{s:?}");
    assert_eq!(fake.ratings()[&1001], 1);
    assert_eq!(fake.ratings()[&1003], 2);
    assert_always_selected(&commands);
}

#[test]
fn a_photo_without_updates_gets_no_select() {
    let env = env();
    let fake = seeded(&env);
    let mut changed = input();
    changed.photos[1] = photo("g2", 2, 4); // only g2 changes
    let (s, commands) = run(&fake, &env, &changed);
    s.unwrap();
    assert_eq!(names(&commands), ["library.select", "photo.rate"]);
    assert_eq!(selects(&commands), [&select_params(1002)]);
}

#[test]
fn a_second_run_without_changes_sends_no_select() {
    let env = env();
    let fake = seeded(&env);
    let (s, commands) = run(&fake, &env, &input());
    let s = s.unwrap();
    assert!(names(&commands).is_empty(), "{commands:?}");
    assert_eq!(s.updates, 0);
}

#[test]
fn the_first_run_selects_before_the_updates_of_every_imported_photo() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let (s, commands) = run(&fake, &env, &input());
    let s = s.unwrap();
    // g1 rate, g2 nothing, g3 rate, g4 rate + label; the import selects
    // already, but the writer selects explicitly all the same.
    assert_eq!(
        names(&commands),
        [
            "library.import",
            "library.select",
            "photo.rate",
            "library.import",
            "library.import",
            "library.select",
            "photo.rate",
            "library.import",
            "library.select",
            "photo.rate",
            "photo.label",
        ]
    );
    assert_eq!(
        selects(&commands),
        [
            &select_params(1001),
            &select_params(1003),
            &select_params(1004)
        ]
    );
    assert_eq!(s.updates, 4, "{s:?}");
    assert!(s.photos_incomplete.is_empty(), "{s:?}");
    assert_always_selected(&commands);
}

#[test]
fn a_new_photo_added_later_is_selected_after_its_import() {
    let env = env();
    let fake = seeded(&env);
    let mut more = input();
    more.photos.push(photo("g5", 5, 4));
    let (s, commands) = run(&fake, &env, &more);
    s.unwrap();
    assert_eq!(
        names(&commands),
        ["library.import", "library.select", "photo.rate"]
    );
    assert_eq!(selects(&commands), [&select_params(1005)]);
}

#[test]
fn photos_found_as_duplicates_are_selected_before_their_updates() {
    let env = env();
    let fake = seeded(&env);
    std::fs::remove_file(&env.state_file).unwrap(); // the library keeps the photos
    let (s, commands) = run(&fake, &env, &input());
    let s = s.unwrap();
    assert_eq!(s.photos_existing, 4, "{s:?}");
    // Duplicates select nothing: every photo needs its own select.
    assert_eq!(selects(&commands).len(), 3, "{commands:?}");
    assert!(s.photos_incomplete.is_empty(), "{s:?}");
    assert_always_selected(&commands);
}

#[test]
fn every_photo_command_follows_a_select_naming_its_photo() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let (first, commands) = run(&fake, &env, &input());
    first.unwrap();
    assert_always_selected(&commands);
    let mut changed = input();
    changed.photos[0] = photo("g1", 1, 0);
    changed.photos[3] = photo("g4", 4, 5).with_label(ColorLabel::Green);
    let (second, commands) = run(&fake, &env, &changed);
    second.unwrap();
    assert!(commands.iter().any(|(c, _)| c.starts_with("photo.")));
    assert_always_selected(&commands);
}

#[test]
fn a_rejected_select_makes_the_photo_incomplete_and_the_run_goes_on() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.failures.push(Failure {
        command: "library.select".into(),
        photo_id: Some(1003),
        message: "refused".into(),
    });
    let fake = FakeLightCraft::start(config);
    let (s, commands) = run(&fake, &env, &input());
    let s = s.unwrap();
    // g3 is imported but never updated; g1 and g4 are complete.
    assert_eq!(s.photos_imported, 4, "{s:?}");
    assert_eq!(s.photos_incomplete.len(), 1, "{s:?}");
    assert!(s.photos_skipped.is_empty(), "{s:?}");
    assert_eq!(s.updates, 3, "g1 rate, g4 rate + label: {s:?}");
    assert!(!fake.ratings().contains_key(&1003));
    assert_eq!(fake.ratings()[&1001], 3);
    assert_eq!(fake.ratings()[&1004], 2);
    assert!(
        !commands
            .iter()
            .any(|(c, p)| c.starts_with("photo.") && p["ids"][0] == 1003),
        "no update for a photo whose select was rejected"
    );
    assert_always_selected(&commands);
    let total = s.photos_imported
        + s.photos_existing
        + s.photos_updated
        + s.photos_unchanged
        + s.photos_skipped.len();
    assert_eq!(total, 4, "{s:?}");

    // The select is refused again: g3 stays incomplete, nothing is sent
    // for it and nothing is claimed as done.
    let (again, _) = run(&fake, &env, &input());
    let again = again.unwrap();
    assert_eq!(again.photos_incomplete.len(), 1, "{again:?}");
    assert_eq!(again.updates, 0, "{again:?}");
}

#[test]
fn a_rejected_select_for_a_known_photo_counts_it_as_updated_and_incomplete() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.failures.push(Failure {
        command: "library.select".into(),
        photo_id: Some(1001),
        message: "refused".into(),
    });
    let fake = FakeLightCraft::start(config);
    // g1 is imported first and its select is refused; run 2 retries it.
    let (first, _) = run(&fake, &env, &input());
    assert_eq!(first.unwrap().photos_incomplete.len(), 1);
    let (second, commands) = run(&fake, &env, &input());
    let second = second.unwrap();
    assert_eq!(second.photos_updated, 1, "{second:?}");
    assert_eq!(second.photos_incomplete.len(), 1, "{second:?}");
    assert_eq!(second.photos_unchanged, 3, "{second:?}");
    assert_eq!(second.updates, 0, "{second:?}");
    assert_eq!(names(&commands), ["library.select"], "{commands:?}");
}

#[test]
fn a_connection_lost_during_select_aborts_the_run() {
    let env = env();
    let mut config = Config::new(&env.library);
    // keyword.list (1), library.import (2), library.select (3).
    config.close_at = Some(3);
    let fake = FakeLightCraft::start(config);
    let (result, commands) = run(&fake, &env, &input());
    match result.unwrap_err() {
        WriteError::ConnectionLost { command } => assert_eq!(command, "library.select"),
        other => panic!("expected ConnectionLost, got {other:?}"),
    }
    assert_eq!(names(&commands), ["library.import", "library.select"]);
}
