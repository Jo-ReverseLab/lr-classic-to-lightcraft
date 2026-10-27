#![cfg(any())]
//! Acceptance tests for running the plan over the control port (task P5-4c),
//! module `lc_writer::apply`, against a stateful fake LightCraft.
//!
//! API under test:
//! - `apply(&mut ControlClient, &WriteInput, &mut State, &Path /* state file */)
//!   -> Result<ApplySummary, WriteError>`;
//! - `ApplySummary { keywords_created, photos_imported, photos_existing,
//!   updates, not_saved: usize, skipped: Vec<model::Skipped> }` (all pub).
//!
//! Rules checked (see the lead's design for P5-4c):
//! - the first command is `keyword.list {}`; its answer is accepted as an
//!   array of `a|b` texts, an array of `{name, children}` objects, or an
//!   object with a `keywords` field holding either; any other answer is
//!   `WriteError::Protocol` before the first writing command;
//! - keywords are created parents first, before any photo is imported;
//! - every import is exactly `{paths:[p], mode:"add"}`; its LightCraft id
//!   comes from `duplicates[].existing` (counts as existing) or from the
//!   single `imported` element (number, `{id}` or `{photoId}`); a non-zero
//!   `failed` or no id skips the photo and all its later steps;
//! - right after an import the photo is recorded with
//!   `PhotoFields::imported_default()` and the state file is saved; after
//!   all updates of the photo it is recorded with the full `snapshot`;
//! - every update carries `ids: [<LightCraft id>]`;
//! - `Outcome::AppliedNotSaved` counts as success (and in `not_saved`);
//! - `WriteError::Command` skips only the affected photo/keyword, with a
//!   reason that names the command and no catalog text;
//! - `ConnectionLost` and `Protocol` abort at once; nothing is repeated; the
//!   state file holds everything finished so far.
//!
//! A second run with an unchanged input sends only read commands.
//!
//! STATUS: waiting for P5-4c. The whole file is disabled by the crate-level
//! `cfg(any())` above. The developer may ONLY remove the `#![cfg(any())]`
//! line. Any other change to this file needs the test author.

use lc_writer::apply::{ApplySummary, apply};
use lc_writer::control::{ControlClient, WriteError};
use lc_writer::plan::WriteInput;
use lc_writer::state::{PhotoFields, State, snapshot};
use model::{ColorLabel, Keyword, Photo, Rating};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[path = "support/fake_lightcraft.rs"]
mod fake_lightcraft;
use fake_lightcraft::{
    Config, FakeLightCraft, Failure, ImportIdStyle, KeywordListFormat, READ_COMMANDS,
};

const PHOTO_DIR: &str = "/Users/example/Pictures";

fn kw(parts: &[&str]) -> Keyword {
    Keyword::new(parts.iter().map(|s| s.to_string()).collect()).unwrap()
}

fn photo_path(gid: &str) -> String {
    format!("{PHOTO_DIR}/{gid}.jpg")
}

fn photo(gid: &str, lr_id: i64, rating: u8) -> Photo {
    Photo::new(photo_path(gid), lr_id, Rating::new(rating).unwrap()).with_global_id(gid)
}

/// Four photos. Updates of the first run: g1 rate + setMeta, g3 rate,
/// g4 rate + label = 5; g2 is only imported. Keywords to create: `Travel`,
/// `Travel|Rome` (implicit parent first) and `Café ☕` = 3.
fn input() -> WriteInput {
    let g1 = photo("g1", 1, 3);
    let g2 = photo("g2", 2, 0);
    let g3 = photo("g3", 3, 5);
    let g4 = photo("g4", 4, 2).with_label(ColorLabel::Blue);
    let mut photo_keywords = BTreeMap::new();
    photo_keywords.insert(1, vec![kw(&["Travel", "Rome"]), kw(&["Café ☕"])]);
    WriteInput {
        photos: vec![g1, g2, g3, g4],
        keywords: vec![kw(&["Travel", "Rome"]), kw(&["Café ☕"])],
        photo_keywords,
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

fn run_with(
    fake: &FakeLightCraft,
    env: &Env,
    input: &WriteInput,
    state: &mut State,
) -> Result<ApplySummary, WriteError> {
    let mut client = ControlClient::connect(fake.port()).unwrap();
    apply(&mut client, input, state, &env.state_file)
}

/// One run with the state loaded from (and saved to) the state file.
fn run(fake: &FakeLightCraft, env: &Env, input: &WriteInput) -> Result<ApplySummary, WriteError> {
    let mut state = State::load(&env.state_file).unwrap();
    run_with(fake, env, input, &mut state)
}

fn texts(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn expected_keywords() -> BTreeSet<String> {
    texts(&["Travel", "Travel|Rome", "Café ☕"])
}

fn is_read(command: &str) -> bool {
    READ_COMMANDS.contains(&command)
}

fn first_run_fake(env: &Env) -> FakeLightCraft {
    FakeLightCraft::start(Config::new(&env.library))
}

// ---------------------------------------------------------------- first run

#[test]
fn first_run_creates_keywords_imports_photos_and_sets_fields() {
    let env = env();
    let fake = first_run_fake(&env);
    let summary = run(&fake, &env, &input()).unwrap();

    assert_eq!(summary.keywords_created, 3);
    assert_eq!(summary.photos_imported, 4);
    assert_eq!(summary.photos_existing, 0);
    assert_eq!(summary.updates, 5);
    assert_eq!(summary.not_saved, 0);
    assert!(summary.skipped.is_empty(), "{:?}", summary.skipped);

    assert_eq!(fake.keywords(), expected_keywords());
    let photos = fake.photos();
    assert_eq!(photos.len(), 4);
    let mut ids: Vec<i64> = photos.values().copied().collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 4, "every photo has its own id");
    let ratings = fake.ratings();
    assert_eq!(ratings[&photos[&photo_path("g1")]], 3);
    assert_eq!(ratings[&photos[&photo_path("g3")]], 5);
    assert_eq!(ratings[&photos[&photo_path("g4")]], 2);
    assert!(!ratings.contains_key(&photos[&photo_path("g2")]));
}

#[test]
fn first_run_records_every_photo_and_keyword_in_the_saved_state() {
    let env = env();
    let fake = first_run_fake(&env);
    let input = input();
    let mut state = State::default();
    run_with(&fake, &env, &input, &mut state).unwrap();

    let saved = State::load(&env.state_file).unwrap();
    assert_eq!(saved, state, "the state file matches the state in memory");
    let photos = fake.photos();
    for p in &input.photos {
        let gid = p.global_id.as_deref().unwrap();
        let entry = saved.photo(gid).unwrap_or_else(|| panic!("{gid} not recorded"));
        assert_eq!(entry.lightroom_id, p.lightroom_id);
        assert_eq!(entry.lightcraft_id, serde_json::json!(photos[&photo_path(gid)]));
        let keywords = input.photo_keywords.get(&p.lightroom_id).cloned().unwrap_or_default();
        assert_eq!(entry.fields, snapshot(p, &keywords), "{gid}");
    }
    for k in expected_keywords() {
        assert!(saved.keyword(&k).is_some(), "keyword {k} not recorded");
    }
}

#[test]
fn keywords_are_created_parents_first_and_before_any_import() {
    let env = env();
    let fake = first_run_fake(&env);
    run(&fake, &env, &input()).unwrap();
    let commands = fake.commands();
    assert_eq!(commands[0].0, "keyword.list", "the first command reads the keywords");
    assert_eq!(commands[0].1, serde_json::json!({}));
    let names: Vec<&str> = commands.iter().map(|(c, _)| c.as_str()).collect();
    let last_create = names.iter().rposition(|c| *c == "keyword.create").unwrap();
    let first_import = names.iter().position(|c| *c == "library.import").unwrap();
    assert!(last_create < first_import);
    let travel = commands
        .iter()
        .position(|(c, p)| c == "keyword.create" && p["name"] == "Travel")
        .unwrap();
    let rome = commands
        .iter()
        .position(|(c, p)| c == "keyword.create" && p["name"] == "Rome")
        .unwrap();
    assert!(travel < rome, "parent before child");
}

#[test]
fn every_import_is_exactly_one_path_in_add_mode() {
    let env = env();
    let fake = first_run_fake(&env);
    run(&fake, &env, &input()).unwrap();
    let imports: Vec<serde_json::Value> = fake
        .commands()
        .into_iter()
        .filter(|(c, _)| c == "library.import")
        .map(|(_, p)| p)
        .collect();
    assert_eq!(imports.len(), 4);
    for (i, params) in imports.iter().enumerate() {
        let gid = format!("g{}", i + 1);
        assert_eq!(
            params,
            &serde_json::json!({"paths": [photo_path(&gid)], "mode": "add"})
        );
    }
}

#[test]
fn every_update_names_the_photo_by_its_lightcraft_id() {
    let env = env();
    let fake = first_run_fake(&env);
    run(&fake, &env, &input()).unwrap();
    let known: BTreeSet<i64> = fake.photos().values().copied().collect();
    let updates: Vec<_> = fake
        .commands()
        .into_iter()
        .filter(|(c, _)| c.starts_with("photo."))
        .collect();
    assert_eq!(updates.len(), 5);
    for (command, params) in updates {
        let ids = params["ids"].as_array().unwrap_or_else(|| panic!("{command} has no ids"));
        assert_eq!(ids.len(), 1, "{command}");
        assert!(known.contains(&ids[0].as_i64().unwrap()), "{command}: {ids:?}");
    }
}

#[test]
fn only_known_commands_are_sent() {
    let env = env();
    let fake = first_run_fake(&env);
    run(&fake, &env, &input()).unwrap();
    for (command, _) in fake.commands() {
        assert!(
            is_read(&command)
                || ["keyword.create", "library.import", "photo.rate", "photo.flag",
                    "photo.label", "photo.setMeta"].contains(&command.as_str()),
            "unexpected command {command}"
        );
    }
}

// --------------------------------------------------------------- second run

#[test]
fn second_run_with_unchanged_input_sends_only_read_commands() {
    let env = env();
    let fake = first_run_fake(&env);
    run(&fake, &env, &input()).unwrap();
    let before = fake.commands().len();
    let summary = run(&fake, &env, &input()).unwrap();
    assert_eq!(summary.keywords_created, 0);
    assert_eq!(summary.photos_imported, 0);
    assert_eq!(summary.photos_existing, 0);
    assert_eq!(summary.updates, 0);
    assert!(summary.skipped.is_empty());
    let second: Vec<String> = fake.commands()[before..].iter().map(|(c, _)| c.clone()).collect();
    assert!(!second.is_empty(), "the keyword list is still read");
    assert!(second.iter().all(|c| is_read(c)), "{second:?}");
    assert_eq!(fake.photos().len(), 4);
    assert_eq!(fake.keywords(), expected_keywords());
}

#[test]
fn second_run_does_not_change_the_state_file() {
    let env = env();
    let fake = first_run_fake(&env);
    run(&fake, &env, &input()).unwrap();
    let before = std::fs::read(&env.state_file).unwrap();
    run(&fake, &env, &input()).unwrap();
    assert_eq!(std::fs::read(&env.state_file).unwrap(), before);
}

#[test]
fn lost_state_file_finds_photos_and_keywords_again_without_duplicates() {
    let env = env();
    let fake = first_run_fake(&env);
    run(&fake, &env, &input()).unwrap();
    std::fs::remove_file(&env.state_file).unwrap();

    let before = fake.commands().len();
    let summary = run(&fake, &env, &input()).unwrap();
    assert_eq!(summary.photos_imported, 0);
    assert_eq!(summary.photos_existing, 4, "reported as duplicates by LightCraft");
    assert_eq!(summary.keywords_created, 0, "keyword.list shows them");
    assert!(summary.skipped.is_empty(), "{:?}", summary.skipped);
    assert_eq!(fake.photos().len(), 4);
    assert_eq!(fake.keywords(), expected_keywords());
    let creates = fake.commands()[before..]
        .iter()
        .filter(|(c, _)| c == "keyword.create")
        .count();
    assert_eq!(creates, 0);

    // The state knows the existing ids again.
    let state = State::load(&env.state_file).unwrap();
    let photos = fake.photos();
    for gid in ["g1", "g2", "g3", "g4"] {
        let entry = state.photo(gid).unwrap();
        assert_eq!(entry.lightcraft_id, serde_json::json!(photos[&photo_path(gid)]));
    }
}

#[test]
fn a_changed_rating_sends_exactly_one_update() {
    let env = env();
    let fake = first_run_fake(&env);
    run(&fake, &env, &input()).unwrap();
    let mut changed = input();
    changed.photos[2] = photo("g3", 3, 1);
    let before = fake.commands().len();
    let summary = run(&fake, &env, &changed).unwrap();
    assert_eq!(summary.updates, 1);
    let writes: Vec<_> = fake.commands()[before..]
        .iter()
        .filter(|(c, _)| !is_read(c))
        .cloned()
        .collect();
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert_eq!(writes[0].0, "photo.rate");
    assert_eq!(writes[0].1["rating"], 1);
}

// ------------------------------------------------------- answer formats

#[test]
fn import_id_may_be_an_object_with_id_or_photo_id() {
    for style in [ImportIdStyle::ObjectId, ImportIdStyle::ObjectPhotoId] {
        let env = env();
        let mut config = Config::new(&env.library);
        config.import_ids = style.clone();
        let fake = FakeLightCraft::start(config);
        let summary = run(&fake, &env, &input()).unwrap();
        assert_eq!(summary.photos_imported, 4, "{style:?}");
        assert_eq!(summary.updates, 5, "{style:?}");
        assert!(summary.skipped.is_empty(), "{style:?}");
        let state = State::load(&env.state_file).unwrap();
        let photos = fake.photos();
        assert_eq!(
            state.photo("g1").unwrap().lightcraft_id,
            serde_json::json!(photos[&photo_path("g1")]),
            "{style:?}"
        );
    }
}

#[test]
fn every_keyword_list_format_is_understood() {
    for format in [
        KeywordListFormat::Texts,
        KeywordListFormat::Nested,
        KeywordListFormat::WrappedTexts,
        KeywordListFormat::WrappedNested,
    ] {
        let env = env();
        let mut config = Config::new(&env.library);
        config.keywords = texts(&["Travel", "Travel|Rome"]);
        config.keyword_list = format.clone();
        let fake = FakeLightCraft::start(config);
        let summary = run(&fake, &env, &input()).unwrap();
        assert_eq!(summary.keywords_created, 1, "{format:?}: only `Café ☕` is new");
        let created: Vec<String> = fake
            .commands()
            .into_iter()
            .filter(|(c, _)| c == "keyword.create")
            .map(|(_, p)| p["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(created, ["Café ☕"], "{format:?}");
        assert!(summary.skipped.is_empty(), "{format:?}");
    }
}

#[test]
fn unknown_keyword_list_format_is_a_protocol_error_before_any_write() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.keyword_list = KeywordListFormat::Garbage;
    let fake = FakeLightCraft::start(config);
    let e = run(&fake, &env, &input()).unwrap_err();
    assert!(matches!(e, WriteError::Protocol(_)), "{e:?}");
    assert!(fake.write_commands().is_empty(), "{:?}", fake.write_commands());
    assert!(fake.photos().is_empty());
    assert!(fake.keywords().is_empty());
}

// ------------------------------------------------------------ interruptions

/// Counting the lines of the first run: 1 keyword.list, 2-4 keyword.create,
/// 5 import g1, 6 rate g1, 7 setMeta g1, 8 import g2, 9 import g3, 10 rate g3.
fn lost_at(line: usize, applies: bool) -> (Env, FakeLightCraft) {
    let env = env();
    let mut config = Config::new(&env.library);
    config.close_at = Some(line);
    config.close_applies = applies;
    let fake = FakeLightCraft::start(config);
    (env, fake)
}

fn assert_no_command_twice(fake: &FakeLightCraft) {
    let lines: Vec<String> = fake
        .received()
        .iter()
        .map(|r| {
            let mut v: serde_json::Value = serde_json::from_str(&r.line).unwrap();
            v.as_object_mut().unwrap().remove("id");
            v.to_string()
        })
        .filter(|l| !l.contains("\"command\":\"keyword.list\""))
        .collect();
    let unique: BTreeSet<&String> = lines.iter().collect();
    assert_eq!(unique.len(), lines.len(), "a command was sent twice: {lines:?}");
}

#[test]
fn lost_connection_aborts_the_run_and_keeps_finished_photos() {
    let (env, fake) = lost_at(9, false);
    let e = run(&fake, &env, &input()).unwrap_err();
    match e {
        WriteError::ConnectionLost { command } => assert_eq!(command, "library.import"),
        other => panic!("{other:?}"),
    }
    assert_no_command_twice(&fake);
    assert_eq!(fake.commands().len(), 9, "nothing was sent after the loss");

    let state = State::load(&env.state_file).unwrap();
    let inp = input();
    for (gid, lr) in [("g1", 1), ("g2", 2)] {
        let entry = state.photo(gid).unwrap_or_else(|| panic!("{gid} lost"));
        let keywords = inp.photo_keywords.get(&lr).cloned().unwrap_or_default();
        assert_eq!(entry.fields, snapshot(&inp.photos[lr as usize - 1], &keywords));
    }
    assert!(state.photo("g3").is_none());
    assert!(state.photo("g4").is_none());
    for k in expected_keywords() {
        assert!(state.keyword(&k).is_some(), "keyword {k} not saved");
    }
}

#[test]
fn run_after_a_lost_import_finishes_the_rest_without_duplicates() {
    let (env, fake) = lost_at(9, false);
    run(&fake, &env, &input()).unwrap_err();
    let summary = run(&fake, &env, &input()).unwrap();
    assert_eq!(summary.keywords_created, 0);
    assert_eq!(summary.photos_imported, 2, "g3 and g4");
    assert_eq!(summary.photos_existing, 0);
    assert!(summary.skipped.is_empty());
    assert_eq!(fake.photos().len(), 4);
    assert_eq!(fake.keywords(), expected_keywords());
    let imports_of_g1 = fake
        .commands()
        .iter()
        .filter(|(c, p)| c == "library.import" && p["paths"][0] == photo_path("g1"))
        .count();
    assert_eq!(imports_of_g1, 1, "a finished photo is not imported again");

    // And a further run has nothing to do.
    let before = fake.commands().len();
    run(&fake, &env, &input()).unwrap();
    assert!(fake.commands()[before..].iter().all(|(c, _)| is_read(c)));
}

#[test]
fn run_after_an_import_that_took_effect_but_got_no_reply_finds_the_duplicate() {
    let (env, fake) = lost_at(9, true);
    run(&fake, &env, &input()).unwrap_err();
    assert_eq!(fake.photos().len(), 3, "g3 was imported but never confirmed");
    let summary = run(&fake, &env, &input()).unwrap();
    assert_eq!(summary.photos_existing, 1, "g3 is reported as a duplicate");
    assert_eq!(summary.photos_imported, 1, "g4");
    assert!(summary.skipped.is_empty(), "{:?}", summary.skipped);
    assert_eq!(fake.photos().len(), 4);
    let state = State::load(&env.state_file).unwrap();
    assert_eq!(
        state.photo("g3").unwrap().lightcraft_id,
        serde_json::json!(fake.photos()[&photo_path("g3")])
    );
    let before = fake.commands().len();
    run(&fake, &env, &input()).unwrap();
    assert!(fake.commands()[before..].iter().all(|(c, _)| is_read(c)));
}

#[test]
fn photo_is_recorded_right_after_its_import_when_a_later_update_is_lost() {
    // Line 6 is `photo.rate` of g1: the import before it must already be saved.
    let (env, fake) = lost_at(6, false);
    let e = run(&fake, &env, &input()).unwrap_err();
    assert!(matches!(&e, WriteError::ConnectionLost { command } if command == "photo.rate"), "{e:?}");
    let state = State::load(&env.state_file).unwrap();
    let entry = state.photo("g1").expect("g1 recorded after its import");
    assert_eq!(entry.fields, PhotoFields::imported_default());
    assert_eq!(entry.lightcraft_id, serde_json::json!(fake.photos()[&photo_path("g1")]));

    let summary = run(&fake, &env, &input()).unwrap();
    assert_eq!(summary.photos_imported, 3, "g1 is not imported again");
    assert_eq!(summary.photos_existing, 0);
    assert_eq!(summary.updates, 5);
    assert_eq!(fake.photos().len(), 4);
    assert_eq!(fake.ratings()[&fake.photos()[&photo_path("g1")]], 3);
    // The lost `photo.rate` has an unknown outcome and is sent again by the
    // next run (setting a rating is idempotent); the import is not.
}

// ------------------------------------------------------------------ errors

#[test]
fn error_reply_for_one_photo_skips_only_that_photo() {
    let env = env();
    let mut config = Config::new(&env.library);
    // g1 is the first import, so it gets the first id.
    config.failures.push(Failure {
        command: "photo.rate".into(),
        photo_id: Some(1001),
        message: "rating refused for /Users/example/Pictures/g1.jpg".into(),
    });
    let fake = FakeLightCraft::start(config);
    let summary = run(&fake, &env, &input()).unwrap();

    assert_eq!(summary.skipped.len(), 1, "{:?}", summary.skipped);
    let reason = &summary.skipped[0].reason;
    assert!(reason.contains("photo.rate"), "{reason}");
    assert!(!reason.contains("example") && !reason.contains("g1.jpg"), "{reason}");
    assert_eq!(summary.photos_imported, 4);
    assert_eq!(summary.updates, 3, "g3 rate, g4 rate and label");
    // No later step for g1: its setMeta never went out.
    let g1_meta = fake
        .commands()
        .iter()
        .filter(|(c, p)| c == "photo.setMeta" && p["ids"][0] == 1001)
        .count();
    assert_eq!(g1_meta, 0);
    // The others were written completely.
    let photos = fake.photos();
    assert_eq!(fake.ratings()[&photos[&photo_path("g3")]], 5);
    assert_eq!(fake.ratings()[&photos[&photo_path("g4")]], 2);
    let state = State::load(&env.state_file).unwrap();
    let inp = input();
    assert_eq!(state.photo("g3").unwrap().fields, snapshot(&inp.photos[2], &[]));
    // g1 stays at its post-import state, so a later run retries it.
    assert_eq!(state.photo("g1").unwrap().fields, PhotoFields::imported_default());
}

#[test]
fn error_reply_for_a_keyword_skips_that_keyword_and_the_run_goes_on() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.failures.push(Failure {
        command: "keyword.create".into(),
        photo_id: None,
        message: "refused".into(),
    });
    let fake = FakeLightCraft::start(config);
    let summary = run(&fake, &env, &input()).unwrap();
    assert_eq!(summary.keywords_created, 0);
    assert!(!summary.skipped.is_empty());
    for s in &summary.skipped {
        assert!(s.reason.contains("keyword.create"), "{}", s.reason);
        assert!(!s.reason.contains("Café") && !s.reason.contains("Rome"), "{}", s.reason);
    }
    assert_eq!(summary.photos_imported, 4, "photos are still written");
    let state = State::load(&env.state_file).unwrap();
    for k in expected_keywords() {
        assert!(state.keyword(&k).is_none(), "{k} was not created");
    }
}

#[test]
fn failed_import_skips_the_photo_and_its_later_steps() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.import_failed_for = Some("g3.jpg".into());
    let fake = FakeLightCraft::start(config);
    let summary = run(&fake, &env, &input()).unwrap();
    assert_eq!(summary.photos_imported, 3);
    assert_eq!(summary.skipped.len(), 1, "{:?}", summary.skipped);
    assert_eq!(summary.updates, 4, "g1 rate + setMeta, g4 rate + label");
    let state = State::load(&env.state_file).unwrap();
    assert!(state.photo("g3").is_none(), "an unimported photo is not recorded");
    assert!(
        fake.commands()
            .iter()
            .all(|(c, p)| !(c.starts_with("photo.") && p["ids"].is_null())),
        "every update names a photo"
    );
}

#[test]
fn not_saved_replies_count_as_success() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.not_saved_updates = true;
    let fake = FakeLightCraft::start(config);
    let summary = run(&fake, &env, &input()).unwrap();
    assert_eq!(summary.updates, 5);
    assert_eq!(summary.not_saved, 5);
    assert!(summary.skipped.is_empty(), "{:?}", summary.skipped);
    let state = State::load(&env.state_file).unwrap();
    let inp = input();
    let kws = inp.photo_keywords[&1].clone();
    assert_eq!(state.photo("g1").unwrap().fields, snapshot(&inp.photos[0], &kws));
}

#[test]
fn nothing_to_do_still_reads_the_keyword_list_only() {
    let env = env();
    let fake = first_run_fake(&env);
    let empty = WriteInput::default();
    let summary = run(&fake, &env, &empty).unwrap();
    assert_eq!(summary.photos_imported + summary.updates + summary.keywords_created, 0);
    assert!(fake.write_commands().is_empty());
}
