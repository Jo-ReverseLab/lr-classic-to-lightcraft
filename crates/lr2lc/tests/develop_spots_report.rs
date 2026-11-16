//! Acceptance tests for spot removal (heal/clone), P6-7b.
//!
//! Contract under test (agreed with the lead):
//! - `Develop` gains `spots`. `RetouchAreas` is the current container: one
//!   entry becomes one spot. `SpotType = "clone"` maps to clone, everything
//!   else to heal. The first entry of `Masks` carries the shape:
//!   `Mask/Ellipse` (centre from `X`/`Y`, `SizeX` scaled by the smaller of
//!   aspect and 1), `Mask/CircularGradient` (centre of the box, half its width
//!   scaled the same way) and `Mask/Paint` (every dab after its first token is
//!   a point, `Radius` is the size as stored). Any other shape is reported as
//!   `Spot: <kind>` with the `Mask/` prefix dropped. A size outside
//!   0.001..0.25 is clamped.
//! - `Feather` and `Opacity` are fractions multiplied by 100 and clamped to
//!   0..100 (defaults 0 and 100). The source is explicit when `SourceState`
//!   is missing or `sourceSetExplicitly`; then `SourceX` and `OffsetY` (both
//!   absolute positions) become the offset from the first point, or null when
//!   missing or the same position.
//! - `RetouchInfo` is read only when `RetouchAreas` yields no spot. Each line
//!   is `key = value, …` with `centerX`, `centerY`, `radius`, `spotType`,
//!   `sourceState`, `sourceX`, `sourceY`, `opacity`; `size = radius ×
//!   min(aspect, 1)`, `feather = 0`. A line that does not parse is reported
//!   as `Spot: legacy line`.
//! - Nothing is dropped silently: `Spot: no mask`, `Spot: empty brush`,
//!   `Spot: <kind>` and `Spot: legacy line` appear in `develop.unmapped` and
//!   count in `develop.unmapped_count`.
//! - `report.json` gains `develop.spots`: converted photos with at least one
//!   spot. The summary contains `photos with spots N`.
//! - Spots are written only through `develop.merge`, after `library.select`,
//!   as `{mode, points: [{x, y}], size, feather, opacity, source_offset}`.
//!   `develop.set` never carries spots. The develop hash makes a second
//!   `--apply` plan nothing.
//!
//! Unconfirmed assumptions are marked in the fixture module (`spots`): the
//! brush radius is not aspect-scaled, feather and opacity are clamped, a
//! missing `SourceState` counts as explicit, and the unknown-shape report
//! drops the `Mask/` prefix.
//!
//! Every fixture is generated; nothing here touches a real catalog. The tests
//! need product code that does not exist yet, so they are ignored until
//! P6-7b lands.

use lrcat_fixtures::develop::{settings, spots as s, write_develop_catalog_with_files};
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

    fn spots() -> Setup {
        let photos = tempfile::tempdir().unwrap();
        let catalog = s::write_spots_catalog_with_files(photos.path()).unwrap();
        Setup::new(catalog, photos)
    }

    fn without_spots() -> Setup {
        let photos = tempfile::tempdir().unwrap();
        let catalog = write_develop_catalog_with_files(
            photos.path(),
            "develop-no-spots.lrcat",
            &[(5, settings().number("Exposure2012", 1.0).build())],
        )
        .unwrap();
        Setup::new(catalog, photos)
    }

    fn report_path(&self) -> PathBuf {
        self.work.path().join("report.json")
    }

    /// A dry run over the given catalog, optionally writing a script.
    fn dry(&self, report: &Path, script: Option<&Path>) -> Output {
        let mut args: Vec<OsString> = vec![
            self.catalog.as_os_str().to_owned(),
            OsString::from("--anonymize"),
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
            OsString::from("--anonymize"),
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

/// Splits a script into `(photo id, settings)` for every `develop.merge`,
/// checking the `library.select` that must precede it, and the
/// `develop.set` parameters.
fn split_develop(lines: &[(String, Value)]) -> (Vec<(String, Value)>, Vec<Value>) {
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
                merges.push((id.to_owned(), params["settings"].clone()));
            }
            "develop.set" => sets.push(params.clone()),
            _ => {}
        }
    }
    (merges, sets)
}

fn merge_for(merges: &[(String, Value)], photo: i64) -> &Value {
    let id = format!("lr:image-{photo:08}");
    merges
        .iter()
        .find(|(merge_id, _)| *merge_id == id)
        .map(|(_, settings)| settings)
        .unwrap_or_else(|| panic!("no develop.merge for {id}: {merges:?}"))
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

/// Photo 1: a heal ellipse with an explicit source and a clone brush.
fn expected_areas_photo() -> Value {
    json!({
        "spots": [
            {
                "mode": "heal",
                "points": [{"x": 0.4, "y": 0.3}],
                "size": 0.04,
                "feather": 50.0,
                "opacity": 80.0,
                "source_offset": {"x": 0.2, "y": 0.2}
            },
            {
                "mode": "clone",
                "points": [{"x": 0.1, "y": 0.2}, {"x": 0.3, "y": 0.4}],
                "size": 0.05,
                "feather": 25.0,
                "opacity": 100.0,
                "source_offset": {"x": 0.3, "y": 0.3}
            }
        ]
    })
}

/// Photo 4: `RetouchInfo` only. The third line is unreadable and dropped with
/// a report entry, so only two spots remain.
fn expected_legacy_photo() -> Value {
    json!({
        "spots": [
            {
                "mode": "heal",
                "points": [{"x": 0.25, "y": 0.75}],
                "size": 0.04,
                "feather": 0.0,
                "opacity": 50.0,
                "source_offset": {"x": 0.25, "y": -0.15}
            },
            {
                "mode": "clone",
                "points": [{"x": 0.6, "y": 0.2}],
                "size": 0.1,
                "feather": 0.0,
                "opacity": 100.0,
                "source_offset": null
            }
        ]
    })
}

/// Photo 5: only the circle from `RetouchAreas`; the `RetouchInfo` line is
/// ignored while the current container yields a spot.
fn expected_both_photo() -> Value {
    json!({
        "spots": [{
            "mode": "heal",
            "points": [{"x": 0.5, "y": 0.4}],
            "size": 0.1,
            "feather": 0.0,
            "opacity": 100.0,
            "source_offset": null
        }]
    })
}

/// Photo 7: one linear mask and one spot in the same settings text.
fn expected_mixed_photo() -> Value {
    json!({
        "masks": [{
            "id": 1,
            "name": s::MIXED_MASK_NAME,
            "visible": true,
            "invert": false,
            "components": [{
                "op": "add",
                "invert": false,
                "shape": {
                    "kind": "linear",
                    "start": {"x": 0.0, "y": 0.0},
                    "end": {"x": 1.0, "y": 1.0}
                }
            }],
            "adjust": {"amount": 100.0}
        }],
        "spots": [{
            "mode": "heal",
            "points": [{"x": 0.5, "y": 0.5}],
            "size": 0.1,
            "feather": 0.0,
            "opacity": 100.0,
            "source_offset": null
        }]
    })
}

/// Photo 10: the size clamps, the feather/opacity clamps and a source at the
/// target's own position.
fn expected_clamp_photo() -> Value {
    json!({
        "spots": [
            {
                "mode": "heal",
                "points": [{"x": 0.2, "y": 0.2}],
                "size": 0.25,
                "feather": 0.0,
                "opacity": 100.0,
                "source_offset": null
            },
            {
                "mode": "heal",
                "points": [{"x": 0.8, "y": 0.8}],
                "size": 0.001,
                "feather": 100.0,
                "opacity": 0.0,
                "source_offset": null
            }
        ]
    })
}

#[test]
fn spots_are_counted_and_the_summary_names_them() {
    let s = Setup::spots();
    let (report, stdout) = s.dry_ok_full(&s.report_path());
    assert_balanced(&report);
    assert_eq!(report["dry_run"], json!(true));

    let d = develop(&report);
    assert_eq!(n(&d["spots"]), s::PHOTOS_WITH_SPOTS);
    assert_eq!(n(&d["photos"]), s::PHOTOS_WITH_SETTINGS);
    assert_eq!(n(&d["planned"]), s::PLANNED_PHOTOS);
    // The mask fields keep their meaning next to the new spot count.
    assert_eq!(n(&d["masks"]), s::PHOTOS_WITH_MASKS);

    let expected = format!("photos with spots {}", s::PHOTOS_WITH_SPOTS);
    assert!(
        stdout.contains(&expected),
        "summary must contain `{expected}`:\n{stdout}"
    );
}

#[test]
fn unsupported_spots_are_reported_by_name_only() {
    let s = Setup::spots();
    let (report, stdout) = s.dry_ok_full(&s.report_path());
    let d = develop(&report);
    assert_eq!(n(&d["unmapped_count"]), s::UNMAPPED.len());
    assert_eq!(names(&report), s::UNMAPPED);

    // Values, coordinates and file names must not leak into either output.
    let raw = fs::read_to_string(s.report_path()).unwrap();
    for forbidden in ["centerX", "Mask/Ellipse", "0.08", "IMG_0001"] {
        assert!(!raw.contains(forbidden), "report leaks `{forbidden}`");
        assert!(!stdout.contains(forbidden), "summary leaks `{forbidden}`");
    }
}

#[test]
fn script_contains_the_exact_lightcraft_spot_json() {
    let s = Setup::spots();
    let report_path = s.work.path().join("report.json");
    let script = s.work.path().join("plan.jsonl");
    let output = s.dry(&report_path, Some(&script));
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let (merges, sets) = split_develop(&script_lines(&script));
    assert_eq!(merges.len(), s::MERGED_PHOTOS, "{merges:?}");
    for (id, _) in &merges {
        let known = [
            "lr:image-00000001",
            "lr:image-00000004",
            "lr:image-00000005",
            "lr:image-00000007",
            "lr:image-00000010",
        ];
        assert!(known.contains(&id.as_str()), "unexpected merge for {id}");
    }

    assert_json_close(
        merge_for(&merges, s::AREAS_PHOTO),
        &expected_areas_photo(),
        "photo 1",
    );
    assert_json_close(
        merge_for(&merges, s::LEGACY_PHOTO),
        &expected_legacy_photo(),
        "photo 4",
    );
    assert_json_close(
        merge_for(&merges, s::BOTH_PHOTO),
        &expected_both_photo(),
        "photo 5",
    );
    assert_json_close(
        merge_for(&merges, s::MIXED_PHOTO),
        &expected_mixed_photo(),
        "photo 7",
    );
    assert_json_close(
        merge_for(&merges, s::CLAMP_PHOTO),
        &expected_clamp_photo(),
        "photo 10",
    );

    // The spot-free photo keeps its slider, and spots never travel through
    // develop.set.
    assert_eq!(sets.len(), 1, "{sets:?}");
    assert_eq!(sets[0]["values"], json!({"light.exposure": 0.5}));
    assert!(
        sets.iter().all(|p| !p.to_string().contains("spots")),
        "develop.set must never carry spots: {sets:?}"
    );
}

#[test]
fn retouch_info_is_used_only_when_retouch_areas_is_empty() {
    let s = Setup::spots();
    let report_path = s.work.path().join("report.json");
    let script = s.work.path().join("plan.jsonl");
    let output = s.dry(&report_path, Some(&script));
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (merges, _) = split_develop(&script_lines(&script));

    let legacy = merge_for(&merges, s::LEGACY_PHOTO);
    let spots = legacy["spots"].as_array().unwrap();
    assert_eq!(
        spots.len(),
        2,
        "only the two readable lines convert: {legacy}"
    );
    assert_eq!(spots[0]["feather"], json!(0.0), "legacy feather is 0");
    assert_eq!(spots[1]["feather"], json!(0.0), "legacy feather is 0");

    // Photo 5 has both containers; the current one wins, so the legacy line
    // (at 0.9/0.9) must not appear as a second spot.
    let both = merge_for(&merges, s::BOTH_PHOTO);
    let spots = both["spots"].as_array().unwrap();
    assert_eq!(
        spots.len(),
        1,
        "RetouchInfo must be ignored when RetouchAreas converts: {both}"
    );
    assert_eq!(spots[0]["points"], json!([{"x": 0.5, "y": 0.4}]));
}

#[test]
fn two_dry_runs_are_identical_and_leave_the_catalog_unchanged() {
    let s = Setup::spots();
    let before = fs::read(&s.catalog).unwrap();
    let first_path = s.work.path().join("first.json");
    let second_path = s.work.path().join("second.json");

    let first = s.dry_ok(&first_path);
    assert_balanced(&first);
    assert_eq!(first["catalog_unchanged"], json!(true));
    assert!(!s.state_file().exists(), "a dry run writes no mapping file");
    assert_eq!(n(&develop(&first)["spots"]), s::PHOTOS_WITH_SPOTS);

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
fn apply_writes_spots_once_and_a_second_run_writes_nothing() {
    use fake_lightcraft::{Config, FakeLightCraft};
    use lc_test_support::fake_lightcraft;

    let s = Setup::spots();
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
    assert_eq!(n(&develop(&report)["spots"]), s::PHOTOS_WITH_SPOTS);
    assert_eq!(n(&develop(&report)["planned"]), s::PLANNED_PHOTOS);
    assert_eq!(n(&develop(&report)["written"]), s::PLANNED_PHOTOS);

    let commands = fake.commands();
    let merges = commands
        .iter()
        .filter(|(c, _)| c == "develop.merge")
        .count();
    let sets = commands.iter().filter(|(c, _)| c == "develop.set").count();
    assert_eq!(merges, s::MERGED_PHOTOS, "{commands:?}");
    assert_eq!(sets, 1, "{commands:?}");
    for (command, params) in &commands {
        if command == "develop.set" {
            assert!(
                !params.to_string().contains("spots"),
                "develop.set must never carry spots: {params}"
            );
        }
    }

    // A second run re-reads the spots but sends no develop command again.
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
    assert_eq!(n(&develop(&report)["spots"]), s::PHOTOS_WITH_SPOTS);
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
fn a_catalog_without_spots_reports_zero_and_sends_no_spots_key() {
    let s = Setup::without_spots();
    let report_path = s.work.path().join("report.json");
    let script = s.work.path().join("plan.jsonl");
    let before = fs::read(&s.catalog).unwrap();
    let output = s.dry(&report_path, Some(&script));
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(before, fs::read(&s.catalog).unwrap(), "catalog changed");

    let report = read_report(&report_path);
    assert_balanced(&report);
    assert_eq!(n(&develop(&report)["spots"]), 0);
    assert!(names(&report).is_empty(), "{report}");

    // The command stream must not grow a `spots` key when there is none.
    let raw = fs::read_to_string(&script).unwrap();
    assert!(
        !raw.contains("\"spots\""),
        "a spot-free catalog must not send a spots key: {raw}"
    );
}
