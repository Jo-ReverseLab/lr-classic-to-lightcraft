//! Acceptance tests for local corrections (masks), P6-7a.
//!
//! Contract under test (agreed with the lead):
//! - `Develop` gains `masks`. One correction of the four containers
//!   (`MaskGroupBasedCorrections`, `GradientBasedCorrections`,
//!   `CircularGradientBasedCorrections`, `PaintBasedCorrections`) becomes one
//!   mask, in that container order. The name is `CorrectionName`, or `Mask N`
//!   with N counted per photo over the converted masks. `visible` follows
//!   `CorrectionActive` (missing/true is visible; an inactive correction is
//!   skipped). The mask `invert` comes from the correction-level
//!   `MaskInverted`; a component's `invert` from the component's
//!   `MaskInverted`.
//! - Supported shapes: linear, radial (radii in long-edge units from the
//!   photo's `fileWidth`/`fileHeight`), brush, luminance range, depth range,
//!   subject and sky. Unsupported shapes and `Local*` sliders without a
//!   counterpart are reported in `develop.unmapped` as `Mask: <kind>`, never
//!   dropped silently.
//! - `report.json`, top-level area `develop`, gains `masks`: the number of
//!   converted photos with at least one mask. The summary contains
//!   `photos with masks N`.
//! - Masks are written only through `develop.merge`, after
//!   `library.select`, in LightCraft's JSON form. `develop.set` never carries
//!   masks.
//!
//! The expected JSON numbers for the luminance range follow LightCraft's
//! `lightness_to_range` (perceptual lightness to log-luminance 0..1); they
//! are compared with a small tolerance. Unconfirmed assumptions are marked in
//! the fixture module: `Mask: empty brush`, `Mask: no components` and
//! `Mask: UnknownShape` (the `Mask/` prefix is dropped).
//!
//! Every fixture is generated; nothing here touches a real catalog. The
//! mandatory invariants (checksum, balance, dry run, identical second run)
//! are checked in the dedicated tests. The mask path (P6-7a) is implemented;
//! spots (P6-7b) are not covered by this file.

use lrcat_fixtures::develop::{masks as m, settings, write_develop_catalog_with_files};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Setup {
    _photos: tempfile::TempDir,
    work: tempfile::TempDir,
    catalog: PathBuf,
    library: PathBuf,
}

impl Setup {
    fn new(catalog: PathBuf, photos: tempfile::TempDir) -> Setup {
        let work = tempfile::tempdir().unwrap();
        let library = work.path().join("Test.lightcraft");
        Setup {
            _photos: photos,
            work,
            catalog,
            library,
        }
    }

    fn masks() -> Setup {
        let photos = tempfile::tempdir().unwrap();
        let catalog = m::write_masks_catalog_with_files(photos.path()).unwrap();
        Setup::new(catalog, photos)
    }

    fn edges() -> Setup {
        let photos = tempfile::tempdir().unwrap();
        let catalog = m::write_mask_edges_catalog_with_files(photos.path()).unwrap();
        Setup::new(catalog, photos)
    }

    fn many_unmapped() -> Setup {
        let photos = tempfile::tempdir().unwrap();
        let catalog = m::write_many_unmapped_catalog_with_files(photos.path()).unwrap();
        Setup::new(catalog, photos)
    }

    fn report_path(&self) -> PathBuf {
        self.work.path().join("report.json")
    }

    /// A dry run over the given catalog, optionally writing a script.
    fn dry(&self, report: &Path, script: Option<&Path>) -> Output {
        let mut args: Vec<OsString> = vec![
            self.catalog.as_os_str().to_owned(),
            OsString::from("--report"),
            report.as_os_str().to_owned(),
        ];
        if let Some(script) = script {
            args.push(OsString::from("--script"));
            args.push(script.as_os_str().to_owned());
        }
        Command::new(env!("CARGO_BIN_EXE_lr2lc"))
            .current_dir(self.work.path())
            .args(args)
            .output()
            .unwrap()
    }

    fn dry_ok(&self, report: &Path) -> Value {
        self.dry_ok_full(report).0
    }

    /// A successful dry run: the report and the summary text.
    fn dry_ok_full(&self, report: &Path) -> (Value, String) {
        let before = fs::read(&self.catalog).unwrap();
        let output = self.dry(report, None);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(before, fs::read(&self.catalog).unwrap(), "catalog changed");
        (
            read_report(report),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )
    }

    fn state_file(&self) -> PathBuf {
        let mut name = self.library.file_name().unwrap().to_os_string();
        name.push(".lr2lc.json");
        self.work.path().join(name)
    }

    fn apply(&self, port: u16) -> Output {
        let port_text = port.to_string();
        let report = self.report_path();
        let args: Vec<OsString> = vec![
            self.catalog.as_os_str().to_owned(),
            OsString::from("--library"),
            self.library.as_os_str().to_owned(),
            OsString::from("--port"),
            OsString::from(port_text),
            OsString::from("--report"),
            report.as_os_str().to_owned(),
            OsString::from("--apply"),
        ];
        Command::new(env!("CARGO_BIN_EXE_lr2lc"))
            .current_dir(self.work.path())
            .args(args)
            .output()
            .unwrap()
    }
}

fn read_report(path: &Path) -> Value {
    let raw = fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read report: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("report is not JSON: {e}"))
}

fn n(value: &Value) -> usize {
    value
        .as_u64()
        .unwrap_or_else(|| panic!("not a number: {value}")) as usize
}

fn develop(report: &Value) -> &Value {
    report
        .get("develop")
        .unwrap_or_else(|| panic!("no develop area in the report: {report}"))
}

fn assert_balanced(report: &Value) {
    let photos = &report["photos"];
    assert_eq!(
        n(&photos["read"]),
        n(&photos["converted"]) + photos["skipped"].as_array().unwrap().len(),
        "read = converted + skipped: {photos}"
    );
}

fn names(report: &Value) -> Vec<String> {
    develop(report)["unmapped"]
        .as_array()
        .unwrap_or_else(|| panic!("develop.unmapped is not an array: {}", develop(report)))
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

fn script_lines(path: &Path) -> Vec<(String, Value)> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| {
            let v: Value = serde_json::from_str(line).unwrap();
            (
                v["params"]["command"].as_str().unwrap().to_owned(),
                v["params"]["params"].clone(),
            )
        })
        .collect()
}

/// Compares JSON values exactly in shape and text, and numbers within 1e-9.
fn assert_json_close(actual: &Value, expected: &Value, path: &str) {
    match (actual, expected) {
        (Value::Object(a), Value::Object(e)) => {
            assert_eq!(
                a.len(),
                e.len(),
                "{path}: different keys: actual {actual} vs expected {expected}"
            );
            for (key, expected_value) in e {
                let actual_value = a
                    .get(key)
                    .unwrap_or_else(|| panic!("{path}: missing key {key}: {actual}"));
                assert_json_close(actual_value, expected_value, &format!("{path}.{key}"));
            }
        }
        (Value::Array(a), Value::Array(e)) => {
            assert_eq!(
                a.len(),
                e.len(),
                "{path}: different lengths: actual {actual} vs expected {expected}"
            );
            for (index, (a, e)) in a.iter().zip(e).enumerate() {
                assert_json_close(a, e, &format!("{path}[{index}]"));
            }
        }
        (Value::Number(a), Value::Number(e)) => {
            let (a, e) = (
                a.as_f64().expect("a JSON number"),
                e.as_f64().expect("a JSON number"),
            );
            assert!((a - e).abs() <= 1e-9, "{path}: {a} != {e}");
        }
        _ => assert_eq!(actual, expected, "{path}"),
    }
}

fn expected_all_kinds() -> Value {
    json!({
        "masks": [
            {
                "id": 1,
                "name": m::LUMINANCE_NAME,
                "visible": true,
                "invert": false,
                "components": [{
                    "op": "add",
                    "invert": false,
                    "shape": {
                        "kind": "luminanceRange",
                        "lo": 0.24097984927065022,
                        "hi": 0.6076465159373169,
                        "lo_feather": 0.24097984927065022,
                        "hi_feather": 0.0590201507293497
                    }
                }],
                "adjust": {"amount": 100.0}
            },
            {
                "id": 2,
                "name": m::DEPTH_NAME,
                "visible": true,
                "invert": false,
                "components": [{
                    "op": "add",
                    "invert": false,
                    "shape": {"kind": "depthRange", "lo": 0.2, "hi": 0.8, "feather": 0.1}
                }],
                "adjust": {"amount": 100.0}
            },
            {
                "id": 3,
                "name": m::SUBJECT_NAME,
                "visible": true,
                "invert": false,
                "components": [{
                    "op": "add",
                    "invert": false,
                    "shape": {"kind": "subject"}
                }],
                "adjust": {"amount": 100.0}
            },
            {
                "id": 4,
                "name": m::SKY_NAME,
                "visible": true,
                "invert": false,
                "components": [{
                    "op": "add",
                    "invert": false,
                    "shape": {"kind": "sky"}
                }],
                "adjust": {"amount": 100.0}
            },
            {
                "id": 5,
                "name": m::LINEAR_NAME,
                "visible": true,
                "invert": true,
                "components": [{
                    "op": "add",
                    "invert": false,
                    "shape": {
                        "kind": "linear",
                        "start": {"x": 0.1, "y": 0.2},
                        "end": {"x": 0.9, "y": 0.8}
                    }
                }],
                "adjust": {"amount": 65.0, "exposure": 2.0, "contrast": 25.0}
            },
            {
                "id": 6,
                "name": m::RADIAL_NAME,
                "visible": true,
                "invert": false,
                "components": [{
                    "op": "add",
                    "invert": false,
                    "shape": {
                        "kind": "radial",
                        "center": {"x": 0.5, "y": 0.35},
                        "rx": 0.3,
                        "ry": 0.125,
                        "angle": 15.0,
                        "feather": 40.0,
                        "invert": false
                    }
                }],
                "adjust": {"amount": 100.0}
            },
            {
                "id": 7,
                "name": m::DEFAULT_MASK_NAME,
                "visible": true,
                "invert": false,
                "components": [{
                    "op": "add",
                    "invert": false,
                    "shape": {
                        "kind": "brush",
                        "strokes": [{
                            "points": [{"x": 0.1, "y": 0.2}, {"x": 0.3, "y": 0.4}],
                            "size": 0.05,
                            "feather": 40.0,
                            "flow": 75.0,
                            "density": 80.0,
                            "erase": false
                        }]
                    }
                }],
                "adjust": {"amount": 100.0}
            }
        ]
    })
}

fn expected_two_components() -> Value {
    json!({
        "masks": [{
            "id": 1,
            "name": m::TWO_COMPONENTS_DEFAULT_NAME,
            "visible": true,
            "invert": false,
            "components": [
                {
                    "op": "add",
                    "invert": true,
                    "shape": {
                        "kind": "linear",
                        "start": {"x": 0.0, "y": 0.0},
                        "end": {"x": 1.0, "y": 1.0}
                    }
                },
                {
                    "op": "subtract",
                    "invert": false,
                    "shape": {
                        "kind": "radial",
                        "center": {"x": 0.4, "y": 0.5},
                        "rx": 0.1,
                        "ry": 0.2,
                        "angle": -30.0,
                        "feather": 25.0,
                        "invert": true
                    }
                }
            ],
            "adjust": {"amount": 100.0}
        }]
    })
}

#[test]
fn masks_are_counted_and_the_summary_names_them() {
    let s = Setup::masks();
    let (report, stdout) = s.dry_ok_full(&s.report_path());
    assert_balanced(&report);
    assert_eq!(report["dry_run"], json!(true));

    let d = develop(&report);
    assert_eq!(n(&d["masks"]), m::PHOTOS_WITH_MASKS);
    assert_eq!(n(&d["photos"]), 3, "photos 4, 5 and 6 carry settings: {d}");

    let expected = format!("photos with masks {}", m::PHOTOS_WITH_MASKS);
    assert!(
        stdout.contains(&expected),
        "summary must contain `{expected}`:\n{stdout}"
    );
}

#[test]
fn unsupported_masks_are_reported_by_name_only() {
    let s = Setup::masks();
    let (report, stdout) = s.dry_ok_full(&s.report_path());
    let d = develop(&report);
    assert_eq!(n(&d["unmapped_count"]), m::UNMAPPED.len());
    assert_eq!(names(&report), m::UNMAPPED);

    let raw = fs::read_to_string(s.report_path()).unwrap();
    for forbidden in ["3737", "IMG_0004"] {
        assert!(!raw.contains(forbidden), "report leaks `{forbidden}`");
        assert!(!stdout.contains(forbidden), "summary leaks `{forbidden}`");
    }
}

#[test]
fn two_dry_runs_are_identical_and_leave_the_catalog_unchanged() {
    let s = Setup::masks();
    let before = fs::read(&s.catalog).unwrap();
    let first_path = s.work.path().join("first.json");
    let second_path = s.work.path().join("second.json");

    let first = s.dry_ok(&first_path);
    assert_balanced(&first);
    assert_eq!(first["catalog_unchanged"], json!(true));
    assert!(!s.state_file().exists(), "a dry run writes no mapping file");
    assert_eq!(n(&develop(&first)["masks"]), m::PHOTOS_WITH_MASKS);

    let second = s.dry_ok(&second_path);
    assert_eq!(
        fs::read(&first_path).unwrap(),
        fs::read(&second_path).unwrap(),
        "two dry runs over the same catalog produced different reports"
    );
    assert_eq!(develop(&first), develop(&second), "develop area drifted");
    assert_eq!(before, fs::read(&s.catalog).unwrap(), "catalog changed");
    assert!(!s.state_file().exists(), "a dry run writes no mapping file");
}

#[test]
fn script_contains_the_exact_lightcraft_mask_json() {
    let s = Setup::masks();
    let report_path = s.work.path().join("report.json");
    let script = s.work.path().join("plan.jsonl");
    let output = s.dry(&report_path, Some(&script));
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let lines = script_lines(&script);
    let mut merges: Vec<(String, Value)> = Vec::new();
    let mut sets: Vec<Value> = Vec::new();
    for (index, (command, params)) in lines.iter().enumerate() {
        match command.as_str() {
            "develop.merge" => {
                let previous = index
                    .checked_sub(1)
                    .and_then(|i| lines.get(i))
                    .unwrap_or_else(|| panic!("develop.merge without a preceding line"));
                assert_eq!(
                    previous.0, "library.select",
                    "develop.merge must follow library.select"
                );
                let id = previous.1["ids"][0]
                    .as_str()
                    .unwrap_or_else(|| panic!("select carries no string id: {}", previous.1));
                merges.push((id.to_owned(), params.clone()));
            }
            "develop.set" => sets.push(params.clone()),
            _ => {}
        }
    }

    assert_eq!(merges.len(), 2, "{merges:?}");
    for (id, params) in &merges {
        let expected = match id.as_str() {
            "lr:image-00000004" => expected_all_kinds(),
            "lr:image-00000005" => expected_two_components(),
            other => panic!("unexpected develop.merge for {other}"),
        };
        assert!(
            params.get("ids").is_none(),
            "develop.merge takes no ids: {params}"
        );
        assert_json_close(&params["settings"], &expected, id);
    }

    // The mask-free photo keeps its slider, and masks never travel through
    // develop.set.
    assert_eq!(sets.len(), 1, "{sets:?}");
    assert_eq!(sets[0]["values"], json!({"light.exposure": 0.5}));
    assert!(
        sets.iter().all(|p| !p.to_string().contains("masks")),
        "develop.set must never carry masks: {sets:?}"
    );
}

#[test]
fn edge_cases_are_reported_and_the_blend_modes_map() {
    let s = Setup::edges();
    let report_path = s.work.path().join("report.json");
    let script = s.work.path().join("plan.jsonl");
    let output = s.dry(&report_path, Some(&script));
    assert_eq!(
        output.status.code(),
        Some(0),
        "empty dabs or a missing list must not crash:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = read_report(&report_path);
    assert_balanced(&report);
    assert_eq!(n(&develop(&report)["masks"]), m::EDGE_PHOTOS_WITH_MASKS);
    assert_eq!(
        n(&develop(&report)["unmapped_count"]),
        m::EDGE_UNMAPPED.len()
    );
    assert_eq!(names(&report), m::EDGE_UNMAPPED);

    let lines = script_lines(&script);
    let mut merges: Vec<(String, Value)> = Vec::new();
    for (index, (command, params)) in lines.iter().enumerate() {
        if command == "develop.merge" {
            assert_eq!(lines[index - 1].0, "library.select");
            let id = lines[index - 1].1["ids"][0].as_str().unwrap().to_owned();
            merges.push((id, params.clone()));
        }
    }
    assert_eq!(merges.len(), 4, "{merges:?}");

    let blend = merges
        .iter()
        .find(|(id, _)| id == "lr:image-00000009")
        .map(|(_, params)| params)
        .unwrap_or_else(|| panic!("no merge for the blend-modes photo: {merges:?}"));
    let masks = blend["settings"]["masks"].as_array().unwrap();
    assert_eq!(masks.len(), 1);
    let components = masks[0]["components"].as_array().unwrap();
    let ops: Vec<&str> = components
        .iter()
        .map(|c| c["op"].as_str().unwrap())
        .collect();
    assert_eq!(ops, ["add", "subtract", "intersect"], "{components:?}");
    for component in components {
        assert_eq!(component["shape"]["kind"], json!("linear"));
    }

    let erasing = merges
        .iter()
        .find(|(id, _)| id == "lr:image-00000011")
        .map(|(_, params)| params)
        .unwrap_or_else(|| panic!("no merge for the erasing brush: {merges:?}"));
    let stroke = &erasing["settings"]["masks"][0]["components"][0]["shape"]["strokes"][0];
    assert_eq!(stroke["erase"], json!(true));
    assert_eq!(stroke["density"], json!(50.0));

    // CorrectionAmount is clamped to 0..200 after the ×100 conversion.
    for (id, amount) in [("lr:image-00000012", 200.0), ("lr:image-00000013", 0.0)] {
        let params = merges
            .iter()
            .find(|(merge_id, _)| merge_id == id)
            .map(|(_, params)| params)
            .unwrap_or_else(|| panic!("no merge for {id}: {merges:?}"));
        assert_eq!(
            params["settings"]["masks"][0]["adjust"]["amount"],
            json!(amount)
        );
    }
}

#[test]
fn the_unmapped_detail_list_is_capped() {
    let s = Setup::many_unmapped();
    let report = s.dry_ok(&s.report_path());
    assert_balanced(&report);
    let d = develop(&report);
    assert_eq!(n(&d["unmapped_count"]), m::MANY_UNMAPPED_COUNT);
    let listed = d["unmapped"].as_array().unwrap();
    assert_eq!(listed.len(), 20, "the detail list is capped: {listed:?}");
    let mut sorted = names(&report);
    let given = sorted.clone();
    sorted.sort_unstable();
    assert_eq!(given, sorted, "the listed names are sorted");
    assert!(
        listed
            .iter()
            .all(|v| v.as_str().unwrap().starts_with("Mask: AI mask (")),
        "{listed:?}"
    );
    assert_eq!(
        n(&d["masks"]),
        0,
        "an unsupported mask is no converted mask"
    );
}

#[test]
fn apply_writes_masks_once_and_a_second_run_writes_nothing() {
    use fake_lightcraft::{Config, FakeLightCraft};
    use lc_test_support::fake_lightcraft;

    let s = Setup::masks();
    let before = fs::read(&s.catalog).unwrap();
    let fake = FakeLightCraft::start(Config::new(&s.library));

    let first = s.apply(fake.port());
    assert_eq!(
        first.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(before, fs::read(&s.catalog).unwrap(), "catalog changed");
    let report = read_report(&s.report_path());
    assert_balanced(&report);
    assert_eq!(n(&develop(&report)["masks"]), m::PHOTOS_WITH_MASKS);
    assert_eq!(n(&develop(&report)["written"]), 3);

    let commands = fake.commands();
    let merges = commands
        .iter()
        .filter(|(c, _)| c == "develop.merge")
        .count();
    let sets = commands.iter().filter(|(c, _)| c == "develop.set").count();
    assert_eq!(merges, 2, "{commands:?}");
    assert_eq!(sets, 1, "{commands:?}");
    for (command, params) in &commands {
        if command == "develop.set" {
            assert!(
                !params.to_string().contains("masks"),
                "develop.set must never carry masks: {params}"
            );
        }
    }

    // A second run re-reads the masks but sends no develop command again.
    let after = fake.commands().len();
    let second = s.apply(fake.port());
    assert_eq!(
        second.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let report = read_report(&s.report_path());
    assert_balanced(&report);
    assert_eq!(n(&develop(&report)["masks"]), m::PHOTOS_WITH_MASKS);
    assert_eq!(n(&develop(&report)["planned"]), 0);
    assert_eq!(n(&develop(&report)["written"]), 0);
    assert!(
        fake.commands()[after..]
            .iter()
            .all(|(c, _)| !c.starts_with("develop.")),
        "a second run must not write develop settings again"
    );
    assert_eq!(before, fs::read(&s.catalog).unwrap(), "catalog changed");
}

#[test]
fn a_fixture_without_mask_rows_is_still_fine() {
    // The plain settings helper produces no mask container at all; this
    // guards against an accidental "masks" key or a changed count.
    let photos = tempfile::tempdir().unwrap();
    let catalog = write_develop_catalog_with_files(
        photos.path(),
        "develop-no-masks.lrcat",
        &[(5, settings().number("Exposure2012", 1.0).build())],
    )
    .unwrap();
    let s = Setup::new(catalog, photos);
    let report = s.dry_ok(&s.report_path());
    assert_balanced(&report);
    assert_eq!(n(&develop(&report)["masks"]), 0);
    assert!(names(&report).is_empty(), "{report}");
}
