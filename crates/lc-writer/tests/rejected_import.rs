//! An import LightCraft answers with an error but finishes anyway (an
//! original it had to download from the cloud first).
//!
//! Rules:
//! - right after a rejected `library.import`, lr2lc asks once with
//!   `library.importPreview` whether the file is in the library now; if so
//!   the photo is taken over like any photo found while importing;
//! - otherwise the photo is skipped (`rejected`) with LightCraft's message
//!   as `detail`, never in `reason`;
//! - a photo the app added later is found by the check before the next run
//!   and taken over: never a second record.

use lc_writer::apply::{ApplySummary, apply};
use lc_writer::control::ControlClient;
use lc_writer::existing::check_library;
use lc_writer::plan::WriteInput;
use lc_writer::progress::NoProgress;
use lc_writer::state::State;
use model::{Photo, Rating, SkipCode};
use std::path::PathBuf;

use fake_lightcraft::{Config, Failure, FakeLightCraft};
use lc_test_support::fake_lightcraft;

const CLOUD: &str = "/Users/example/Pictures/cloud/IMG_0001.CR3";

fn input() -> WriteInput {
    WriteInput {
        photos: vec![Photo::new(CLOUD, 1, Rating::new(4).unwrap()).with_global_id("g1")],
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

/// Checks, then writes, like a front end does.
fn run(fake: &FakeLightCraft, env: &Env) -> ApplySummary {
    let mut state = State::load(&env.state_file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let mut input = input();
    let check = check_library(&mut client, &input, &state, &mut NoProgress).unwrap();
    input.existing = check.existing;
    input.removed = check.removed;
    apply(&mut client, &input, &mut state, &env.state_file).unwrap()
}

fn imports(fake: &FakeLightCraft) -> usize {
    fake.commands()
        .iter()
        .filter(|(c, _)| c == "library.import")
        .count()
}

#[test]
fn an_import_finished_despite_an_error_is_taken_over_in_the_same_run() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.import_rejected_but_added_for = Some("IMG_0001".into());
    let fake = FakeLightCraft::start(config);
    let s = run(&fake, &env);
    assert!(s.photos_skipped.is_empty(), "{:?}", s.photos_skipped);
    assert_eq!((s.photos_existing, s.photos_imported), (1, 0));
    assert_eq!(s.existing.found_while_importing, 1);
    let id = fake.photos()[CLOUD];
    assert_eq!(fake.photo_fields(id).unwrap().rating, 4, "empty, filled");
    let state = State::load(&env.state_file).unwrap();
    assert_eq!(
        state.photo("g1").unwrap().lightcraft_id,
        serde_json::json!(id)
    );

    let s = run(&fake, &env);
    assert_eq!(imports(&fake), 1, "never imported again");
    assert_eq!(s.photos_unchanged, 1);
    assert_eq!(fake.photos().len(), 1);
}

#[test]
fn an_import_the_app_finishes_later_is_found_by_the_next_check() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.failures.push(Failure {
        command: "library.import".into(),
        photo_id: None,
        message: "could not read \"/Users/example/Pictures/cloud/IMG_0001.CR3\": timed out".into(),
    });
    let fake = FakeLightCraft::start(config);
    let s = run(&fake, &env);
    let [skipped] = &s.photos_skipped[..] else {
        panic!("{:?}", s.photos_skipped)
    };
    assert_eq!(skipped.code, SkipCode::Rejected);
    assert_eq!(
        skipped.reason,
        "photo 1: LightCraft rejected `library.import`"
    );
    // The app's message is kept apart: it quotes a path.
    assert!(
        skipped
            .detail
            .as_deref()
            .is_some_and(|d| d.contains("timed out")),
        "{skipped:?}"
    );
    let preview_of_the_file = fake
        .commands()
        .iter()
        .filter(|(c, p)| c == "library.importPreview" && p["paths"] == serde_json::json!([CLOUD]))
        .count();
    assert_eq!(
        preview_of_the_file, 2,
        "the check, then once after the error"
    );

    // LightCraft finishes the import after all.
    let id = fake.add_photo(CLOUD);
    let s = run(&fake, &env);
    assert_eq!((s.photos_existing, s.existing.found), (1, 1));
    assert_eq!(s.existing.found_while_importing, 0);
    assert_eq!(imports(&fake), 1, "not imported a second time");
    assert_eq!(fake.photos().len(), 1);
    assert_eq!(fake.photo_fields(id).unwrap().rating, 4);
}
