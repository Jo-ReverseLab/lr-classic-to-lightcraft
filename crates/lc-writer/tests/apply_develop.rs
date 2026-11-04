//! Acceptance tests for writing development settings (P6-2, phase 6a) over
//! the control port, against the stateful fake LightCraft.
//!
//! API under test: `lc_writer::apply::apply` (see `tests/apply.rs`).
//!
//! Contract checked here (Lead, P6-2):
//! - a photo with sliders only gets `develop.set {values, ids:[<LightCraft
//!   id>]}` and no `library.select` directly before it;
//! - a photo with structured settings gets `library.select {ids:[id],
//!   active:id}` directly before `develop.merge {settings}`; the merge carries
//!   no `ids` (the fake enforces the selection gate like `photo.*`);
//! - a second run with the same input sends no `develop.*` command;
//! - a mapping file whose photo entry has no `develop_hash` (written by an
//!   older lr2lc) loads and leads to exactly one write, not to an error;
//! - after a successful write the mapping file records `develop_hash` as the
//!   lowercase hex SHA-256 of the source text (64 characters);
//! - a new photo beside an unchanged known one gets the only develop steps;
//! - `Undecidable` white balance writes `wb.mode` only: no `scale`, no
//!   temperature, no tint; unmapped names never reach a command.
//!
//! The tests use no catalog file: the input is built from `model::Photo`.
//! The catalog-unchanged and report checks are in `crates/lr2lc/tests`.

// `..WriteInput::default()` stays valid when the struct gains fields.
#![allow(clippy::needless_update)]

use lc_writer::apply::apply;
use lc_writer::control::{ControlClient, WriteError};
use lc_writer::plan::WriteInput;
use lc_writer::state::{State, state_path};
use model::{Develop, MergeValue, Photo, Rating, WhiteBalanceHint};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use fake_lightcraft::{Config, FakeLightCraft};
use lc_test_support::fake_lightcraft;

const PHOTO_DIR: &str = "/Users/example/Pictures";

fn photo(gid: &str, lr_id: i64, develop: Develop) -> Photo {
    Photo::new(format!("{PHOTO_DIR}/{gid}.jpg"), lr_id, Rating::default())
        .with_global_id(gid)
        .with_develop(develop)
}

fn hash(byte: u8) -> [u8; 32] {
    [byte; 32]
}

fn hex(source: [u8; 32]) -> String {
    source.iter().map(|b| format!("{b:02x}")).collect()
}

fn sliders(byte: u8) -> Develop {
    let mut develop = Develop::default();
    develop.controls.insert("light.exposure".to_string(), 1.25);
    develop.controls.insert("light.contrast".to_string(), -10.0);
    develop.source_hash = Some(hash(byte));
    develop
}

fn structured(byte: u8) -> Develop {
    let mut develop = Develop::default();
    develop
        .partial
        .insert("hdr.enabled".to_string(), MergeValue::Bool(true));
    develop
        .partial
        .insert("optics.remove_ca".to_string(), MergeValue::Bool(false));
    develop.white_balance.mode = Some("daylight".to_string());
    develop.white_balance.hint = WhiteBalanceHint::Kelvin;
    develop.source_hash = Some(hash(byte));
    develop
}

struct Env {
    _dir: tempfile::TempDir,
    library: PathBuf,
    state_file: PathBuf,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Develop.lightcraft");
    Env {
        state_file: state_path(&library),
        library,
        _dir: dir,
    }
}

fn input(photos: Vec<Photo>) -> WriteInput {
    WriteInput {
        photos,
        ..WriteInput::default()
    }
}

fn run(
    fake: &FakeLightCraft,
    env: &Env,
    input: &WriteInput,
) -> Result<lc_writer::apply::ApplySummary, WriteError> {
    let mut state = State::load(&env.state_file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    apply(&mut client, input, &mut state, &env.state_file)
}

fn assert_clean(summary: &lc_writer::apply::ApplySummary) {
    assert!(summary.photos_skipped.is_empty(), "{summary:?}");
    assert!(summary.photos_incomplete.is_empty(), "{summary:?}");
    assert!(summary.keywords_not_created.is_empty(), "{summary:?}");
}

fn develop_commands(fake: &FakeLightCraft, from: usize) -> Vec<(String, Value)> {
    fake.commands()[from..]
        .iter()
        .filter(|(c, _)| c.starts_with("develop."))
        .cloned()
        .collect()
}

fn state_json(env: &Env) -> Value {
    serde_json::from_str(&std::fs::read_to_string(&env.state_file).unwrap()).unwrap()
}

fn photo_entry<'a>(state: &'a Value, gid: &str) -> &'a Value {
    &state["photos"][gid]
}

fn remove_develop_hash(path: &Path, gid: &str) {
    let mut state: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let removed = state["photos"][gid]
        .as_object_mut()
        .expect("photo entry")
        .remove("develop_hash");
    assert!(
        removed.is_some(),
        "the first run must record develop_hash, so an old file can lack it"
    );
    std::fs::write(path, serde_json::to_string(&state).unwrap()).unwrap();
}

#[test]
fn sliders_are_written_with_develop_set_and_no_select_before_it() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let summary = run(&fake, &env, &input(vec![photo("g1", 1, sliders(0x11))])).unwrap();
    assert_clean(&summary);

    let commands = fake.commands();
    let set_at = commands
        .iter()
        .position(|(c, _)| c == "develop.set")
        .expect("develop.set");
    assert_eq!(
        commands[set_at - 1].0,
        "library.import",
        "only sliders: develop.set must not need a selection"
    );
    let id = fake.photos()[&format!("{PHOTO_DIR}/g1.jpg")];
    assert_eq!(commands[set_at].1["values"]["light.exposure"], 1.25);
    assert_eq!(commands[set_at].1["ids"], serde_json::json!([id]));

    let saved = state_json(&env);
    let entry = photo_entry(&saved, "g1");
    let recorded = entry["develop_hash"]
        .as_str()
        .unwrap_or_else(|| panic!("no develop_hash recorded: {entry}"));
    assert_eq!(recorded, hex(hash(0x11)));
    assert_eq!(recorded.len(), 64);
    assert!(
        recorded
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "lowercase hex: {recorded}"
    );
}

#[test]
fn structured_settings_are_selected_then_merged_without_ids() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let summary = run(&fake, &env, &input(vec![photo("g2", 2, structured(0x22))])).unwrap();
    assert_clean(&summary);

    let commands = fake.commands();
    let merge_at = commands
        .iter()
        .position(|(c, _)| c == "develop.merge")
        .expect("develop.merge");
    let id = fake.photos()[&format!("{PHOTO_DIR}/g2.jpg")];
    assert_eq!(commands[merge_at - 1].0, "library.select");
    assert_eq!(
        commands[merge_at - 1].1,
        serde_json::json!({"ids": [id], "active": id})
    );
    assert_eq!(
        commands[merge_at].1,
        serde_json::json!({"settings": {
            "hdr": {"enabled": true},
            "optics": {"remove_ca": false},
            "wb": {"mode": "daylight", "scale": "kelvin"},
        }}),
        "nested settings, no ids"
    );
    assert!(
        commands[merge_at].1.get("ids").is_none(),
        "develop.merge acts on the selection"
    );
}

#[test]
fn a_second_run_with_unchanged_input_sends_no_develop_command() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let photos = || {
        vec![
            photo("g1", 1, sliders(0x11)),
            photo("g2", 2, structured(0x22)),
        ]
    };
    run(&fake, &env, &input(photos())).unwrap();
    assert_eq!(develop_commands(&fake, 0).len(), 2);

    let before = fake.commands().len();
    let summary = run(&fake, &env, &input(photos())).unwrap();
    assert_clean(&summary);
    assert!(
        develop_commands(&fake, before).is_empty(),
        "the same input must not be written twice"
    );
}

#[test]
fn a_mapping_file_without_a_develop_hash_gets_one_write_not_an_error() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    run(&fake, &env, &input(vec![photo("g1", 1, sliders(0x11))])).unwrap();
    remove_develop_hash(&env.state_file, "g1");

    let before = fake.commands().len();
    let mut state = State::load(&env.state_file).expect("a file without develop_hash loads");
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let summary = apply(
        &mut client,
        &input(vec![photo("g1", 1, sliders(0x11))]),
        &mut state,
        &env.state_file,
    )
    .expect("an older mapping file is not an error");
    assert_clean(&summary);
    let develop = develop_commands(&fake, before);
    assert_eq!(develop.len(), 1, "{develop:?}");
    assert_eq!(develop[0].0, "develop.set");
    assert_eq!(
        photo_entry(&state_json(&env), "g1")["develop_hash"],
        serde_json::json!(hex(hash(0x11))),
        "the field is recorded again"
    );
}

#[test]
fn a_changed_hash_of_a_known_photo_is_left_alone_without_the_flag() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    run(&fake, &env, &input(vec![photo("g1", 1, sliders(0x11))])).unwrap();

    let before = fake.commands().len();
    // Same photo, different settings text: a different source hash.
    run(&fake, &env, &input(vec![photo("g1", 1, sliders(0x12))])).unwrap();
    assert!(
        develop_commands(&fake, before).is_empty(),
        "a known photo changed in Lightroom is never silently overwritten"
    );
}

#[test]
fn a_new_photo_beside_a_known_one_gets_the_only_develop_commands() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    run(&fake, &env, &input(vec![photo("g1", 1, sliders(0x11))])).unwrap();

    let before = fake.commands().len();
    let summary = run(
        &fake,
        &env,
        &input(vec![
            photo("g1", 1, sliders(0x11)),
            photo("g2", 2, structured(0x22)),
        ]),
    )
    .unwrap();
    assert_clean(&summary);
    let develop = develop_commands(&fake, before);
    assert_eq!(develop.len(), 1, "{develop:?}");
    let new_id = fake.photos()[&format!("{PHOTO_DIR}/g2.jpg")];
    assert_eq!(develop[0].0, "develop.merge");
    assert_eq!(
        photo_entry(&state_json(&env), "g1")["develop_hash"],
        serde_json::json!(hex(hash(0x11)))
    );
    assert_eq!(
        photo_entry(&state_json(&env), "g2")["develop_hash"],
        serde_json::json!(hex(hash(0x22)))
    );
    // The merge is preceded by a select of the new photo.
    let all = fake.commands();
    let merge_at = all.iter().rposition(|(c, _)| c == "develop.merge").unwrap();
    assert_eq!(all[merge_at - 1].1["ids"], serde_json::json!([new_id]));
}

#[test]
fn undecidable_white_balance_writes_the_mode_but_no_temperature_or_scale() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let mut develop = Develop::default();
    develop.white_balance.mode = Some("cloudy".to_string());
    develop.white_balance.hint = WhiteBalanceHint::Undecidable;
    develop.unmapped = vec!["Temperature".to_string(), "Tint".to_string()];
    develop.source_hash = Some(hash(0x66));

    let summary = run(&fake, &env, &input(vec![photo("g3", 3, develop)])).unwrap();
    assert_clean(&summary);
    let develop = develop_commands(&fake, 0);
    assert_eq!(develop.len(), 1, "{develop:?}");
    assert_eq!(develop[0].0, "develop.merge");
    assert_eq!(
        develop[0].1,
        serde_json::json!({"settings": {"wb": {"mode": "cloudy"}}})
    );
    let all = format!("{:?}", fake.commands()).to_lowercase();
    assert!(!all.contains("temperature"), "{all}");
    assert!(!all.contains("tint"), "{all}");
    assert!(!all.contains("kelvin"), "{all}");
}

#[test]
fn the_develop_hash_is_the_source_hash_not_the_settings_values() {
    // Two different texts, same visible sliders: the hash must differ.
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let mut first = sliders(0x11);
    let mut second = sliders(0x11);
    second.unmapped.push("MysterySlider".to_string());
    first.source_hash = Some(hash(0xa0));
    second.source_hash = Some(hash(0xb0));
    run(&fake, &env, &input(vec![photo("g4", 4, first)])).unwrap();
    let state = state_json(&env);
    assert_eq!(
        photo_entry(&state, "g4")["develop_hash"],
        serde_json::json!(hex(hash(0xa0)))
    );

    let before = fake.commands().len();
    run(&fake, &env, &input(vec![photo("g4", 4, second)])).unwrap();
    assert!(
        develop_commands(&fake, before).is_empty(),
        "a different source text on a known photo is not applied without opt-in"
    );
}

#[test]
fn a_photo_without_settings_records_no_develop_hash_and_writes_nothing() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let plain =
        Photo::new(format!("{PHOTO_DIR}/g5.jpg"), 5, Rating::default()).with_global_id("g5");
    let summary = run(&fake, &env, &input(vec![plain])).unwrap();
    assert_clean(&summary);
    assert!(develop_commands(&fake, 0).is_empty());
    let state = state_json(&env);
    assert!(
        photo_entry(&state, "g5")["develop_hash"].is_null(),
        "no settings, no hash: {state}"
    );
}

#[test]
fn the_plan_of_a_finished_run_has_no_develop_command_left() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let photos = || {
        vec![
            photo("g1", 1, sliders(0x11)),
            photo("g2", 2, structured(0x22)),
        ]
    };
    run(&fake, &env, &input(photos())).unwrap();
    let state = State::load(&env.state_file).unwrap();
    let plan = lc_writer::plan::plan(&input(photos()), &state, &BTreeSet::new());
    let script = plan.to_script(&state);
    assert!(
        !script.contains("develop."),
        "nothing left to write: {script}"
    );
}
