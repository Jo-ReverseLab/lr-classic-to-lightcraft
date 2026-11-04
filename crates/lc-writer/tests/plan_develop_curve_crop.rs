//! Acceptance tests for planning tone curves and crop geometry (P6-3).
//!
//! API under test: `lc_writer::plan` (see `tests/plan.rs`) with
//! `model::Develop::{curves, crop}`.
//!
//! Contract checked here (Lead, P6-3):
//! - non-empty `Develop.curves` becomes one `develop.merge` whose `curve`
//!   object holds the four point lists in LightCraft's form: an array of
//!   `{"x": <0..1>, "y": <0..1>}` objects (LightCraft's `Point` derives
//!   `Serialize`, `crates/geom/src/lib.rs:21-24`; the XMP mapper writes the
//!   same shape, `crates/engine/src/crs.rs:181-197`). Empty channel lists are
//!   not sent (`crs.rs:335-350` writes only present curves).
//! - `Develop.crop = Some` becomes one `develop.merge` with
//!   `crop.geometry = {"rect": {"x0","y0","x1","y1"}, "angle"}` (LightCraft's
//!   `Crop`/`CropGeometry`, `crates/develop/src/settings.rs:815-822` and
//!   `crates/geom/src/crop.rs:13-21`; the catalog mapper emits exactly this
//!   nesting, `crates/engine/src/lightroom_catalog.rs:745-765`).
//! - The angle is written only inside `crop.geometry`, never again as the
//!   `crop.angle` control.
//! - The merge is preceded by `library.select` and carries no `ids`; sliders
//!   are still one `develop.set` before it. A photo with sliders only needs no
//!   selection.
//! - A known photo with the same source hash plans nothing; a changed hash is
//!   only reported (`develop_not_applied`) until `overwrite_develop`, which
//!   plans exactly one write.
//!
//! Planning is a pure function: it only builds commands and never connects to
//! the control port. The second-run and apply checks are in
//! `tests/apply_develop.rs`.

// `..WriteInput::default()` stays valid when the struct gains fields.
#![allow(clippy::needless_update)]

use lc_writer::plan::{Plan, WriteInput, plan};
use lc_writer::state::State;
use model::{CropGeometry, CurvePoint, Develop, Photo, Rating};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn photo_with(gid: &str, lr_id: i64, develop: Develop) -> Photo {
    Photo::new(
        format!("/Users/example/Pictures/{gid}.jpg"),
        lr_id,
        Rating::default(),
    )
    .with_global_id(gid)
    .with_develop(develop)
}

fn hash(byte: u8) -> [u8; 32] {
    [byte; 32]
}

fn hex(source: [u8; 32]) -> String {
    source.iter().map(|b| format!("{b:02x}")).collect()
}

fn point(x: f64, y: f64) -> CurvePoint {
    CurvePoint { x, y }
}

fn master_curve() -> Vec<CurvePoint> {
    vec![point(0.0, 0.0), point(0.25, 0.35), point(1.0, 1.0)]
}

fn red_curve() -> Vec<CurvePoint> {
    vec![point(0.0, 0.0), point(0.5, 0.4), point(1.0, 1.0)]
}

fn curve_develop() -> Develop {
    let mut develop = Develop::default();
    develop.curves.master = master_curve();
    develop.source_hash = Some(hash(0x11));
    develop
}

fn all_channels_develop() -> Develop {
    let mut develop = Develop::default();
    develop.curves.master = master_curve();
    develop.curves.red = red_curve();
    develop.curves.green = vec![point(0.0, 0.0), point(1.0, 0.5)];
    develop.curves.blue = vec![point(0.0, 0.1), point(1.0, 1.0)];
    develop.source_hash = Some(hash(0x12));
    develop
}

fn crop_develop() -> Develop {
    Develop {
        crop: Some(CropGeometry {
            x0: 0.1,
            y0: 0.2,
            x1: 0.9,
            y1: 0.8,
            angle: 0.0,
        }),
        source_hash: Some(hash(0x22)),
        ..Develop::default()
    }
}

fn sliders() -> Develop {
    let mut develop = Develop::default();
    develop.controls.insert("light.exposure".to_string(), 1.25);
    develop.controls.insert("light.contrast".to_string(), -10.0);
    develop.source_hash = Some(hash(0x33));
    develop
}

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
    let path = dir.path().join("develop-curve-crop.lr2lc.json");
    std::fs::write(&path, file.to_string()).unwrap();
    State::load(&path).expect("a mapping file, with or without develop_hash, must load")
}

fn none() -> BTreeSet<String> {
    BTreeSet::new()
}

fn lines(plan: &Plan, state: &State) -> Vec<(String, Value)> {
    plan.to_script(state)
        .lines()
        .map(|line| {
            let v: Value = serde_json::from_str(line).expect("each line is valid JSON");
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
fn a_curve_becomes_a_merge_after_a_select_with_lightcraft_point_objects() {
    let state = State::default();
    let input = WriteInput {
        photos: vec![photo_with("g-curve", 4, curve_develop())],
        ..WriteInput::default()
    };
    let plan = plan(&input, &state, &none());
    let lines = lines(&plan, &state);

    assert_eq!(
        commands(&lines),
        vec!["library.import", "library.select", "develop.merge"],
        "a curve is merge-only and needs the selection"
    );
    assert_eq!(
        lines[1].1,
        json!({"ids": ["lr:g-curve"], "active": "lr:g-curve"})
    );
    assert_eq!(
        lines[2].1,
        json!({"settings": {"curve": {"master": [
            {"x": 0.0, "y": 0.0},
            {"x": 0.25, "y": 0.35},
            {"x": 1.0, "y": 1.0},
        ]}}}),
        "points are x/y objects in 0..1, not strings"
    );
    assert!(lines[2].1.get("ids").is_none());
}

#[test]
fn only_non_empty_curves_are_sent() {
    let state = State::default();
    let input = WriteInput {
        photos: vec![photo_with("g-master", 5, curve_develop())],
        ..WriteInput::default()
    };
    let plan = plan(&input, &state, &none());
    let lines = lines(&plan, &state);
    let merge = &lines.last().unwrap().1;
    let curve = merge["settings"]["curve"].as_object().unwrap();
    assert_eq!(
        curve.keys().collect::<Vec<_>>(),
        vec!["master"],
        "empty channel lists would add keys without a value"
    );
}

#[test]
fn all_four_curves_are_written_in_one_merge() {
    let state = State::default();
    let input = WriteInput {
        photos: vec![photo_with("g-all-curves", 6, all_channels_develop())],
        ..WriteInput::default()
    };
    let plan = plan(&input, &state, &none());
    let lines = lines(&plan, &state);
    let merge = &lines.last().unwrap().1;
    assert_eq!(
        merge["settings"]["curve"]["red"],
        json!([{"x": 0.0, "y": 0.0}, {"x": 0.5, "y": 0.4}, {"x": 1.0, "y": 1.0}])
    );
    assert_eq!(
        merge["settings"]["curve"]["green"],
        json!([{"x": 0.0, "y": 0.0}, {"x": 1.0, "y": 0.5}])
    );
    assert_eq!(
        merge["settings"]["curve"]["blue"],
        json!([{"x": 0.0, "y": 0.1}, {"x": 1.0, "y": 1.0}])
    );
}

#[test]
fn a_crop_becomes_a_nested_geometry_merge() {
    let state = State::default();
    let input = WriteInput {
        photos: vec![photo_with("g-crop", 7, crop_develop())],
        ..WriteInput::default()
    };
    let plan = plan(&input, &state, &none());
    let lines = lines(&plan, &state);

    assert_eq!(
        commands(&lines),
        vec!["library.import", "library.select", "develop.merge"]
    );
    assert_eq!(
        lines[2].1,
        json!({"settings": {"crop": {"geometry": {
            "rect": {"x0": 0.1, "y0": 0.2, "x1": 0.9, "y1": 0.8},
            "angle": 0.0,
        }}}}),
        "rect and angle are nested exactly as LightCraft's CropGeometry"
    );
    assert!(lines[2].1.get("ids").is_none());
}

#[test]
fn a_zero_angle_is_still_written_inside_the_geometry() {
    let state = State::default();
    let input = WriteInput {
        photos: vec![photo_with("g-zero", 8, crop_develop())],
        ..WriteInput::default()
    };
    let plan = plan(&input, &state, &none());
    let lines = lines(&plan, &state);
    let geometry = &lines.last().unwrap().1["settings"]["crop"]["geometry"];
    assert_eq!(geometry["angle"], json!(0.0));
    assert!(geometry.get("rect").is_some());
}

#[test]
fn sliders_are_written_before_the_curve_crop_merge() {
    let state = State::default();
    let mut develop = sliders();
    develop.curves.master = master_curve();
    develop.crop = Some(CropGeometry {
        x0: 0.1,
        y0: 0.2,
        x1: 0.9,
        y1: 0.8,
        angle: -2.5,
    });
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
        ],
        "the slider command comes first, the selection only guards the merge"
    );
    let merge = &lines[3].1["settings"];
    assert_eq!(
        merge["curve"]["master"],
        json!([{"x": 0.0, "y": 0.0}, {"x": 0.25, "y": 0.35}, {"x": 1.0, "y": 1.0}])
    );
    assert_eq!(merge["crop"]["geometry"]["angle"], json!(-2.5));
    assert!(lines[3].1.get("ids").is_none());
}

#[test]
fn the_angle_is_not_written_a_second_time_as_a_control() {
    let state = State::default();
    let input = WriteInput {
        photos: vec![photo_with("g-angle", 10, crop_develop())],
        ..WriteInput::default()
    };
    let plan = plan(&input, &state, &none());
    let script = plan.to_script(&state);
    assert!(
        !script.contains("crop.angle"),
        "the angle lives in crop.geometry only: {script}"
    );
    let lines = lines(&plan, &state);
    assert!(
        develop_params(&lines)
            .iter()
            .all(|(c, _)| *c != "develop.set"),
        "a crop alone needs no slider command"
    );
}

#[test]
fn sliders_only_need_no_selection() {
    let state = State::default();
    let input = WriteInput {
        photos: vec![photo_with("g-slider", 11, sliders())],
        ..WriteInput::default()
    };
    let plan = plan(&input, &state, &none());
    let lines = lines(&plan, &state);
    assert_eq!(commands(&lines), vec!["library.import", "develop.set"]);
}

#[test]
fn a_known_photo_with_the_same_hash_plans_no_curve_or_crop_command() {
    let state = state_with("g-known", 12, Some(&hex(hash(0x44))));
    let mut develop = curve_develop();
    develop.crop = crop_develop().crop;
    develop.source_hash = Some(hash(0x44));
    let input = WriteInput {
        photos: vec![photo_with("g-known", 12, develop)],
        ..WriteInput::default()
    };
    let plan = plan(&input, &state, &none());
    assert!(
        plan.steps.is_empty(),
        "the same source text must not be written twice: {:?}",
        plan.steps
    );
}

#[test]
fn a_changed_hash_needs_the_overwrite_flag_and_is_written_once_with_it() {
    let state = state_with("g-known", 13, Some(&hex(hash(0x55))));
    let mut develop = crop_develop();
    develop.curves.master = master_curve();
    develop.source_hash = Some(hash(0x66));

    let input = WriteInput {
        photos: vec![photo_with("g-known", 13, develop)],
        ..WriteInput::default()
    };
    let default_plan = plan(&input, &state, &none());
    assert!(
        develop_params(&lines(&default_plan, &state)).is_empty(),
        "a changed hash is never silently overwritten"
    );
    assert_eq!(default_plan.develop_not_applied(), 1);

    let overwrite = WriteInput {
        overwrite_develop: true,
        ..input
    };
    let plan = plan(&overwrite, &state, &none());
    let lines = lines(&plan, &state);
    let develop_commands = develop_params(&lines);
    assert_eq!(develop_commands.len(), 1, "{develop_commands:?}");
    assert_eq!(develop_commands[0].0, "develop.merge");
    assert_eq!(plan.develop_overwritten(), 1);
    assert_eq!(plan.develop_planned(), 1);
}

#[test]
fn planning_is_pure_and_only_returns_commands() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = dir.path().join("never-written.lr2lc.json");
    let input = WriteInput {
        photos: vec![photo_with("g-pure", 14, crop_develop())],
        ..WriteInput::default()
    };
    let state = State::default();
    let plan = plan(&input, &state, &none());
    assert!(!plan.steps.is_empty());
    let script = plan.to_script(&state);
    assert!(script.contains("develop.merge"), "{script}");
    assert!(
        !state_file.exists(),
        "planning a dry run must not create a mapping file"
    );
}
