//! Acceptance tests for planning white-balance settings (P6-4, phase 6a).
//!
//! Contract checked here (Lead, P6-4):
//! - `WhiteBalance` (`Develop::white_balance.mode`, LightCraft spelling) is
//!   written through `develop.merge` as `wb.mode`.
//! - `Temperature`, `Tint`, `IncrementalTemperature` and
//!   `IncrementalTint` are never part of a command; only the mode is carried
//!   while the probe is not implemented. Their names live in
//!   `Develop::unmapped` and are not sent.
//! - `wb.scale: "kelvin"` is added only for `WhiteBalanceHint::Kelvin`; the
//!   reader never produces that hint in P6-4 (no original probe).
//! - A hint without a mode plans no merge: nothing to write, nothing to
//!   report as a command.
//! - Sliders are still written even when the white balance is not carried.
//!
//! The tests are black-box against `lc_writer::plan`, like `tests/plan.rs`.

#![allow(clippy::needless_update)]

use lc_writer::plan::{Plan, WriteInput, plan};
use lc_writer::state::State;
use model::{Develop, Photo, Rating, WhiteBalanceHint};
use serde_json::{Value, json};

fn photo_with(gid: &str, lr_id: i64, develop: Develop) -> Photo {
    Photo::new(
        format!("/Users/example/Pictures/{gid}.jpg"),
        lr_id,
        Rating::default(),
    )
    .with_global_id(gid)
    .with_develop(develop)
}

fn white_balance(mode: Option<&str>, hint: WhiteBalanceHint) -> Develop {
    let mut develop = Develop::default();
    develop.white_balance.mode = mode.map(str::to_owned);
    develop.white_balance.hint = hint;
    develop.source_hash = Some([0x77; 32]);
    develop
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

fn develop_params(lines: &[(String, Value)]) -> Vec<(&str, &Value)> {
    lines
        .iter()
        .filter(|(c, _)| c.starts_with("develop."))
        .map(|(c, p)| (c.as_str(), p))
        .collect()
}

#[test]
fn an_undecidable_absolute_pair_writes_the_mode_and_no_scale() {
    let mut develop = white_balance(Some("cloudy"), WhiteBalanceHint::Undecidable);
    develop.unmapped = vec!["Temperature".to_string(), "Tint".to_string()];
    let input = WriteInput {
        photos: vec![photo_with("g-wb-abs", 4, develop)],
        ..WriteInput::default()
    };
    let state = State::default();
    let plan = plan(&input, &state, &Default::default());
    let lines = lines(&plan, &state);

    assert_eq!(
        lines.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(),
        vec!["library.import", "library.select", "develop.merge"]
    );
    assert_eq!(
        lines[2].1,
        json!({"settings": {"wb": {"mode": "cloudy"}}}),
        "no temperature, no tint, no scale"
    );
    assert!(
        lines[2].1.get("ids").is_none(),
        "develop.merge acts on the selection"
    );
}

#[test]
fn a_relative_pair_writes_only_the_mode() {
    let mut develop = white_balance(Some("shade"), WhiteBalanceHint::Relative);
    develop.unmapped = vec![
        "IncrementalTemperature".to_string(),
        "IncrementalTint".to_string(),
    ];
    let input = WriteInput {
        photos: vec![photo_with("g-wb-rel", 5, develop)],
        ..WriteInput::default()
    };
    let state = State::default();
    let lines = lines(&plan(&input, &state, &Default::default()), &state);

    assert_eq!(
        lines.last().unwrap().1,
        json!({"settings": {"wb": {"mode": "shade"}}})
    );
}

#[test]
fn only_the_kelvin_hint_adds_the_scale() {
    // The reader does not produce this hint in P6-4; the writer contract from
    // P6-2 must still hold once the probe is released.
    let input = WriteInput {
        photos: vec![photo_with(
            "g-wb-kelvin",
            6,
            white_balance(Some("daylight"), WhiteBalanceHint::Kelvin),
        )],
        ..WriteInput::default()
    };
    let state = State::default();
    let lines = lines(&plan(&input, &state, &Default::default()), &state);

    assert_eq!(
        lines.last().unwrap().1,
        json!({"settings": {"wb": {"mode": "daylight", "scale": "kelvin"}}})
    );
}

#[test]
fn a_hint_without_a_mode_plans_no_command() {
    let mut develop = white_balance(None, WhiteBalanceHint::Undecidable);
    develop.unmapped = vec!["Temperature".to_string(), "Tint".to_string()];
    let input = WriteInput {
        photos: vec![photo_with("g-wb-hint-only", 7, develop)],
        ..WriteInput::default()
    };
    let state = State::default();
    let plan = plan(&input, &state, &Default::default());

    assert!(
        develop_params(&lines(&plan, &state)).is_empty(),
        "a hint alone is not a merge: nothing would be written"
    );
}

#[test]
fn sliders_are_still_written_when_the_white_balance_is_not_carried() {
    let mut develop = white_balance(Some("custom"), WhiteBalanceHint::Relative);
    develop.controls.insert("light.exposure".to_string(), 0.75);
    develop.unmapped = vec![
        "IncrementalTemperature".to_string(),
        "IncrementalTint".to_string(),
    ];
    let input = WriteInput {
        photos: vec![photo_with("g-wb-slider", 8, develop)],
        ..WriteInput::default()
    };
    let state = State::default();
    let plan = plan(&input, &state, &Default::default());
    let lines = lines(&plan, &state);
    let develop_commands = develop_params(&lines);

    assert_eq!(develop_commands.len(), 2, "{develop_commands:?}");
    assert_eq!(develop_commands[0].0, "develop.set");
    assert_eq!(
        develop_commands[0].1,
        &json!({"values": {"light.exposure": 0.75}, "ids": ["lr:g-wb-slider"]})
    );
    assert_eq!(develop_commands[1].0, "develop.merge");
    assert_eq!(
        develop_commands[1].1,
        &json!({"settings": {"wb": {"mode": "custom"}}})
    );
}

#[test]
fn temperature_tint_and_their_values_never_reach_the_script() {
    let mut develop = white_balance(Some("cloudy"), WhiteBalanceHint::Undecidable);
    develop.controls.insert("light.contrast".to_string(), 12.0);
    develop.unmapped = vec![
        "Temperature".to_string(),
        "Tint".to_string(),
        "IncrementalTemperature".to_string(),
        "IncrementalTint".to_string(),
    ];
    let input = WriteInput {
        photos: vec![photo_with("g-wb-names", 9, develop)],
        ..WriteInput::default()
    };
    let state = State::default();
    let script = plan(&input, &state, &Default::default()).to_script(&state);

    for forbidden in [
        "Temperature",
        "Tint",
        "IncrementalTemperature",
        "IncrementalTint",
        "5500",
        "-20",
    ] {
        assert!(
            !script.contains(forbidden),
            "{forbidden} must not be part of a command: {script}"
        );
    }
    assert!(script.contains("wb"));
    assert!(script.contains("cloudy"));
}

#[test]
fn two_photos_keep_their_own_mode_and_never_share_a_merge() {
    let absolute = photo_with(
        "g-wb-a",
        1,
        white_balance(Some("tungsten"), WhiteBalanceHint::Undecidable),
    );
    let relative = photo_with(
        "g-wb-b",
        2,
        white_balance(Some("flash"), WhiteBalanceHint::Relative),
    );
    let input = WriteInput {
        photos: vec![absolute, relative],
        ..WriteInput::default()
    };
    let state = State::default();
    let lines = lines(&plan(&input, &state, &Default::default()), &state);
    let merges: Vec<&Value> = develop_params(&lines)
        .into_iter()
        .filter(|(c, _)| *c == "develop.merge")
        .map(|(_, p)| p)
        .collect();

    assert_eq!(merges.len(), 2, "{merges:?}");
    assert_eq!(
        merges[0],
        &json!({"settings": {"wb": {"mode": "tungsten"}}})
    );
    assert_eq!(merges[1], &json!({"settings": {"wb": {"mode": "flash"}}}));
    assert_ne!(merges[0], merges[1]);
}
