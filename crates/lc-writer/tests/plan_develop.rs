//! Acceptance tests for planning development settings (P6-2, phase 6a).
//!
//! API under test: `lc_writer::plan` (see `tests/plan.rs`) with
//! `model::Develop` on `Photo::develop` and a `develop_hash` on the recorded
//! photo entry of the mapping file.
//!
//! Contract checked here (Lead, P6-2):
//! - `Photo.develop = None` and an empty `Develop` plan nothing;
//! - a photo with `controls` gets one `develop.set` whose `values` are the
//!   control map and whose `ids` name the photo (a placeholder `lr:<gid>` for
//!   a photo this plan imports); no `library.select` precedes it;
//! - any structured part (`partial`, `curves`, `crop`, `white_balance.mode`)
//!   gives `library.select {ids:[id], active:id}` then
//!   `develop.merge {settings}` without `ids`; `partial` keys are dotted
//!   paths and are nested in the JSON (`hdr.enabled` -> `{"hdr":{"enabled":..}}`);
//! - `white_balance.mode` -> `wb.mode`; hint `Kelvin` adds
//!   `{"wb":{"scale":"kelvin"}}`; `Undecidable`/`Relative`/`None` write no
//!   temperature, tint or scale;
//! - per photo the order is `develop.set` before `develop.merge`;
//! - a known photo whose recorded `develop_hash` equals the source hash gets
//!   no develop step; a different hash gets none by default (the report
//!   counts it, see the CLI tests); a mapping file without the field loads
//!   and plans one write;
//! - a new photo beside unchanged known ones gets the only develop steps;
//! - `Temperature`/`Tint` and `Develop::unmapped` never appear in a command.
//!
//! The mapping file is hand-written JSON so the tests stay black-box: the only
//! new field is `develop_hash`, the lowercase hex SHA-256 of the Lightroom
//! settings text, next to `lightroom_id`/`lightcraft_id`/`fields`.

// `..WriteInput::default()` stays valid when the struct gains fields.
#![allow(clippy::needless_update)]

use lc_writer::plan::{Plan, WriteInput, plan};
use lc_writer::state::State;
use model::{Develop, MergeValue, Photo, Rating, WhiteBalanceHint};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn photo(gid: &str, lr_id: i64) -> Photo {
    Photo::new(
        format!("/Users/example/Pictures/{gid}.jpg"),
        lr_id,
        Rating::default(),
    )
    .with_global_id(gid)
}

fn photo_with(gid: &str, lr_id: i64, develop: Develop) -> Photo {
    photo(gid, lr_id).with_develop(develop)
}

fn hash(byte: u8) -> [u8; 32] {
    [byte; 32]
}

/// The lowercase hex spelling the mapping file stores.
fn hex(source: [u8; 32]) -> String {
    source.iter().map(|b| format!("{b:02x}")).collect()
}

fn sliders() -> Develop {
    let mut develop = Develop::default();
    develop.controls.insert("light.contrast".to_string(), -10.0);
    develop.controls.insert("light.exposure".to_string(), 1.25);
    develop.source_hash = Some(hash(0x11));
    develop
}

fn structured() -> Develop {
    let mut develop = Develop::default();
    develop
        .partial
        .insert("treatment".to_string(), MergeValue::Text("bw".to_string()));
    develop
        .partial
        .insert("hdr.enabled".to_string(), MergeValue::Bool(true));
    develop
        .partial
        .insert("optics.remove_ca".to_string(), MergeValue::Bool(false));
    develop.white_balance.mode = Some("daylight".to_string());
    develop.white_balance.hint = WhiteBalanceHint::Kelvin;
    develop.source_hash = Some(hash(0x22));
    develop
}

/// A mapping file with one known photo, as the writer recorded it: import
/// defaults (the fixture photos carry no rating, flag, label, text or
/// keyword) and optionally a develop hash.
fn state_with(gid: &str, lr_id: i64, develop_hash: Option<&str>) -> State {
    let mut entry = json!({
        "lightroom_id": lr_id,
        "lightcraft_id": 100 + lr_id,
        "fields": {
            "rating": 0,
            "flag": "none",
            "label": null,
            "title": null,
            "caption": null,
            "copyright": null,
            "creator": null,
            "keywords": [],
        },
    });
    if let Some(h) = develop_hash {
        entry["develop_hash"] = json!(h);
    }
    let mut photos = serde_json::Map::new();
    photos.insert(gid.to_string(), entry);
    let file = json!({"version": 1, "photos": photos, "keywords": {}});
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("develop.lr2lc.json");
    std::fs::write(&path, file.to_string()).unwrap();
    State::load(&path).expect("a mapping file, with or without develop_hash, must load")
}

fn none() -> BTreeSet<String> {
    BTreeSet::new()
}

/// The script lines as `(command, params)` pairs.
fn lines(plan: &Plan, state: &State) -> Vec<(String, Value)> {
    plan.to_script(state)
        .lines()
        .map(|line| {
            let v: Value = serde_json::from_str(line).expect("each line is valid JSON");
            assert_eq!(v["method"], json!("engine.execute"));
            (
                v["params"]["command"].as_str().unwrap().to_string(),
                v["params"]["params"].clone(),
            )
        })
        .collect()
}

fn commands(lines: &[(String, Value)]) -> Vec<&str> {
    lines.iter().map(|(c, _)| c.as_str()).collect()
}

fn develop_params(lines: &[(String, Value)]) -> Vec<(&str, &Value)> {
    lines
        .iter()
        .filter(|(c, _)| c.starts_with("develop."))
        .map(|(c, p)| (c.as_str(), p))
        .collect()
}

#[test]
fn sliders_are_one_develop_set_with_values_and_the_photo_id_and_no_select() {
    let state = State::default();
    let input = WriteInput {
        photos: vec![photo_with("g-slider", 4, sliders())],
        ..WriteInput::default()
    };
    let plan = plan(&input, &state, &none());
    let lines = lines(&plan, &state);

    assert_eq!(commands(&lines), vec!["library.import", "develop.set"]);
    let set = &lines[1].1;
    assert_eq!(
        set["values"],
        json!({"light.contrast": -10.0, "light.exposure": 1.25}),
        "the control map is passed on unchanged"
    );
    assert_eq!(
        set["ids"],
        json!(["lr:g-slider"]),
        "develop.set names the photo directly, like an update"
    );
}

#[test]
fn structured_settings_become_a_nested_merge_after_a_select_without_ids() {
    let state = State::default();
    let input = WriteInput {
        photos: vec![photo_with("g-merge", 7, structured())],
        ..WriteInput::default()
    };
    let plan = plan(&input, &state, &none());
    let lines = lines(&plan, &state);

    assert_eq!(
        commands(&lines),
        vec!["library.import", "library.select", "develop.merge"]
    );
    assert_eq!(
        lines[1].1,
        json!({"ids": ["lr:g-merge"], "active": "lr:g-merge"})
    );
    let merge = &lines[2].1;
    assert_eq!(
        merge,
        &json!({"settings": {
            "treatment": "bw",
            "hdr": {"enabled": true},
            "optics": {"remove_ca": false},
            "wb": {"mode": "daylight", "scale": "kelvin"},
        }}),
        "dotted partial paths are nested and no ids are sent"
    );
    assert!(
        merge.get("ids").is_none(),
        "develop.merge acts on the selection, not on ids"
    );
}

#[test]
fn sliders_are_written_before_the_merge_of_the_same_photo() {
    let state = State::default();
    let mut develop = sliders();
    develop
        .partial
        .insert("hdr.enabled".to_string(), MergeValue::Bool(true));
    let input = WriteInput {
        photos: vec![photo_with("g-both", 9, develop)],
        ..WriteInput::default()
    };
    let plan = plan(&input, &state, &none());
    let lines = lines(&plan, &state);
    assert_eq!(
        commands(&lines),
        vec![
            "library.import",
            "develop.set",
            "library.select",
            "develop.merge"
        ]
    );
}

#[test]
fn without_develop_settings_or_with_an_empty_develop_nothing_is_planned() {
    let empty = Develop {
        source_hash: Some(hash(0x33)),
        ..Develop::default()
    };
    let input = WriteInput {
        photos: vec![photo("g-none", 1), photo_with("g-empty", 2, empty)],
        ..WriteInput::default()
    };
    let plan = plan(&input, &State::default(), &none());
    assert!(
        develop_params(&lines(&plan, &State::default())).is_empty(),
        "no controls, partial, curve, crop or white-balance mode: no command"
    );
}

#[test]
fn a_known_photo_with_the_same_develop_hash_gets_no_develop_step() {
    let state = state_with("g-known", 3, Some(&hex(hash(0x44))));
    let mut develop = sliders();
    develop.source_hash = Some(hash(0x44));
    let input = WriteInput {
        photos: vec![photo_with("g-known", 3, develop)],
        ..WriteInput::default()
    };
    let plan = plan(&input, &state, &none());
    assert!(
        plan.steps.is_empty(),
        "fields equal and develop hash equal: no command at all"
    );
    assert!(lines(&plan, &state).is_empty());
}

#[test]
fn a_changed_develop_hash_is_not_written_by_default() {
    let state = state_with("g-known", 3, Some(&hex(hash(0x44))));
    let mut develop = sliders();
    develop.source_hash = Some(hash(0x55));
    let input = WriteInput {
        photos: vec![photo_with("g-known", 3, develop)],
        ..WriteInput::default()
    };
    let plan = plan(&input, &state, &none());
    assert!(
        develop_params(&lines(&plan, &state)).is_empty(),
        "a changed hash on a known photo is only reported (develop_not_applied)"
    );
}

#[test]
fn a_state_file_without_a_develop_hash_loads_and_plans_one_write() {
    let state = state_with("g-known", 3, None);
    let input = WriteInput {
        photos: vec![photo_with("g-known", 3, sliders())],
        ..WriteInput::default()
    };
    let plan = plan(&input, &state, &none());
    let lines = lines(&plan, &state);
    let develop = develop_params(&lines);
    assert_eq!(develop.len(), 1, "one write, not an error and not a skip");
    assert_eq!(develop[0].0, "develop.set");
}

#[test]
fn a_new_photo_beside_an_unchanged_known_one_gets_the_only_develop_steps() {
    let state = state_with("g-known", 3, Some(&hex(hash(0x44))));
    let mut known = sliders();
    known.source_hash = Some(hash(0x44));
    let input = WriteInput {
        photos: vec![
            photo_with("g-known", 3, known),
            photo_with("g-new", 8, sliders()),
        ],
        ..WriteInput::default()
    };
    let plan = plan(&input, &state, &none());
    let lines = lines(&plan, &state);
    let develop = develop_params(&lines);
    assert_eq!(develop.len(), 1, "{develop:?}");
    assert_eq!(
        develop[0].1["ids"],
        json!(["lr:g-new"]),
        "only the new photo is written"
    );
}

#[test]
fn white_balance_mode_is_a_merge_and_undecidable_writes_no_scale_or_temperature() {
    let mut develop = Develop::default();
    develop.white_balance.mode = Some("cloudy".to_string());
    develop.white_balance.hint = WhiteBalanceHint::Undecidable;
    develop.unmapped = vec!["Temperature".to_string(), "Tint".to_string()];
    develop.source_hash = Some(hash(0x66));
    let input = WriteInput {
        photos: vec![photo_with("g-wb", 11, develop)],
        ..WriteInput::default()
    };
    let plan = plan(&input, &State::default(), &none());
    let lines = lines(&plan, &State::default());
    assert_eq!(
        commands(&lines),
        vec!["library.import", "library.select", "develop.merge"]
    );
    assert_eq!(
        lines[2].1,
        json!({"settings": {"wb": {"mode": "cloudy"}}}),
        "Undecidable writes the mode, never a temperature, tint or scale"
    );
}

#[test]
fn relative_white_balance_writes_no_scale_and_no_temperature() {
    let mut develop = Develop::default();
    develop.white_balance.mode = Some("custom".to_string());
    develop.white_balance.hint = WhiteBalanceHint::Relative;
    develop.source_hash = Some(hash(0x67));
    let input = WriteInput {
        photos: vec![photo_with("g-rel", 12, develop)],
        ..WriteInput::default()
    };
    let state = State::default();
    let plan = plan(&input, &state, &none());
    let lines = lines(&plan, &state);
    assert_eq!(
        lines.last().unwrap().1,
        json!({"settings": {"wb": {"mode": "custom"}}})
    );
}

#[test]
fn temperature_tint_and_unmapped_names_never_reach_a_command() {
    let mut develop = sliders();
    develop.unmapped = vec![
        "Temperature".to_string(),
        "Tint".to_string(),
        "MysterySlider".to_string(),
    ];
    let input = WriteInput {
        photos: vec![photo_with("g-unmapped", 13, develop)],
        ..WriteInput::default()
    };
    let state = State::default();
    let script = plan(&input, &state, &none()).to_script(&state);
    for forbidden in ["Temperature", "Tint", "MysterySlider"] {
        assert!(
            !script.contains(forbidden),
            "{forbidden} must not be part of a command: {script}"
        );
    }
    let plan = plan(&input, &state, &none());
    let lines = lines(&plan, &state);
    let develop = develop_params(&lines);
    assert_eq!(develop.len(), 1, "only the slider command is planned");
    assert_eq!(develop[0].0, "develop.set");
}

#[test]
fn the_control_map_stays_sorted_and_has_no_duplicates() {
    let mut develop = Develop::default();
    develop.controls.insert("light.whites".to_string(), 3.0);
    develop.controls.insert("light.blacks".to_string(), -3.0);
    develop.controls.insert("light.exposure".to_string(), 0.5);
    let input = WriteInput {
        photos: vec![photo_with("g-order", 14, develop)],
        ..WriteInput::default()
    };
    let state = State::default();
    let plan = plan(&input, &state, &none());
    let lines = lines(&plan, &state);
    let values = lines[1].1["values"].as_object().unwrap().clone();
    let keys: Vec<&String> = values.keys().collect();
    assert_eq!(
        keys,
        vec!["light.blacks", "light.exposure", "light.whites"],
        "BTreeMap order, no duplicates"
    );
}

#[test]
fn two_photos_get_their_own_develop_commands() {
    let input = WriteInput {
        photos: vec![
            photo_with("g-a", 1, sliders()),
            photo_with("g-b", 2, structured()),
        ],
        ..WriteInput::default()
    };
    let state = State::default();
    let plan = plan(&input, &state, &none());
    let lines = lines(&plan, &state);
    let develop: Vec<(&str, &Value)> = develop_params(&lines);
    assert_eq!(develop.len(), 2);
    assert_eq!(develop[0].0, "develop.set");
    assert_eq!(develop[1].0, "develop.merge");
    assert_eq!(develop[0].1["ids"], json!(["lr:g-a"]));
    assert_eq!(develop[1].1["settings"]["wb"]["mode"], json!("daylight"));
}

/// Keeps the helper honest: the test fixture hash must be plain lowercase hex.
#[test]
fn the_develop_hash_spelling_is_lowercase_hex() {
    let text = hex(hash(0xab));
    assert_eq!(text.len(), 64);
    assert!(
        text.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    assert_eq!(text, "ab".repeat(32));
}

/// The state helper must not accidentally record a develop hash twice.
#[test]
fn the_mapping_file_helper_round_trips() {
    let state = state_with("g-x", 5, Some(&hex(hash(0x00))));
    let entry = state.photo("g-x").unwrap();
    assert_eq!(entry.lightroom_id, 5);
}
