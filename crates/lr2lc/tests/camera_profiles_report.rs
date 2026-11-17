//! Acceptance tests for camera profiles (P6-5, phase 6a).
//!
//! Contract under test (agreed with the lead):
//! - The reader keeps the `CameraProfile` name byte for byte in
//!   `Develop::camera_profile`. A missing or empty name is `None`. A value of
//!   the wrong Lua type is reported through `Develop::unmapped` as
//!   `CameraProfile` and leaves `camera_profile = None`.
//! - `CameraProfile` is no longer a generic unmapped name; `Look` stays
//!   unmapped.
//! - `report.json`, top-level area `develop`, gains:
//!   - `camera_profiles`: object mapping profile name to the number of photos
//!     that use it, keys sorted, at most 20 entries. Names and counts only,
//!     never rendering values.
//!   - `camera_profiles_count`: number of distinct profile names (may exceed
//!     the 20 listed).
//!   - `photos_with_profile`: number of converted photos with a non-empty
//!     profile name.
//! - The readable summary names the number of photos with a profile as
//!   `photos with camera profile N`.
//! - A photo that carries only a profile plans no develop command, so for the
//!   profile-only fixture `develop.planned == 0`.
//!
//! Fixture: `lrcat_fixtures::develop::write_camera_profile_catalog_with_files`
//! with invented names. The mandatory invariants are checked in the last
//! test: checksum before/after, `read == converted + skipped`, no mapping file
//! without `--apply`, and an identical second dry run.
//!
//! P6-5 is implemented: the reader fills `Develop::camera_profile`, the plan
//! counts the names, and the report and summary carry them. These tests run
//! with the normal suite.

#[path = "perf_support/mod.rs"]
mod perf_support;

use lrcat_fixtures::develop::{profile as p, write_camera_profile_catalog_with_files};
use perf_support::{
    assert_catalog_unchanged, assert_no_state_file, assert_report_invariants, dry_run,
    dry_run_named, exit_ok, read_report,
};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let catalog = write_camera_profile_catalog_with_files(dir.path()).unwrap();
    let report = dir.path().join("report.json");
    (dir, catalog, report)
}

fn run(catalog: &Path, report: &Path) -> (Output, Value) {
    let output = dry_run_named(catalog, report);
    exit_ok(&output);
    (output, read_report(report))
}

fn develop(report: &Value) -> &Value {
    report
        .get("develop")
        .unwrap_or_else(|| panic!("no develop area: {report}"))
}

fn profiles(report: &Value) -> &serde_json::Map<String, Value> {
    develop(report)["camera_profiles"]
        .as_object()
        .unwrap_or_else(|| {
            panic!(
                "develop.camera_profiles is not an object: {}",
                develop(report)
            )
        })
}

fn count(report: &Value, key: &str) -> usize {
    develop(report)[key]
        .as_u64()
        .unwrap_or_else(|| panic!("develop.{key} is not a number: {}", develop(report)))
        as usize
}

#[test]
fn profiles_are_aggregated_by_name_and_keep_their_bytes() {
    let (_dir, catalog, report_path) = setup();
    let (_out, report) = run(&catalog, &report_path);

    let got = profiles(&report);
    let mut names: Vec<&String> = got.keys().collect();
    names.sort();
    let mut expected = [
        p::BYTE_EXACT.to_owned(),
        p::STUDIO.to_owned(),
        p::PROFILE_ONLY.to_owned(),
    ];
    expected.sort();
    assert_eq!(
        names,
        expected.iter().collect::<Vec<_>>(),
        "profile names must be reported byte for byte and sorted: {got:?}"
    );

    // Two byte-exact photos, three studio photos (incl. the Look row), one
    // profile-only photo.
    let expected_counts = [(p::STUDIO, 3), (p::BYTE_EXACT, 2), (p::PROFILE_ONLY, 1)];
    for (name, want) in expected_counts {
        assert_eq!(
            got.get(name).and_then(Value::as_u64),
            Some(want),
            "{name}: {got:?}"
        );
    }
    assert_eq!(got.len(), p::DISTINCT_PROFILES);
    assert_eq!(
        count(&report, "camera_profiles_count"),
        p::DISTINCT_PROFILES
    );
    assert_eq!(
        count(&report, "photos_with_profile"),
        p::PHOTOS_WITH_PROFILE
    );

    // Only names and counts, never a rendering value.
    for (name, value) in got {
        assert!(!name.is_empty(), "an empty name must not be reported");
        assert!(value.is_u64(), "{name} maps to a non-count value: {value}");
    }
    assert!(
        got.len() <= 20,
        "at most 20 profile entries may be listed: {got:?}"
    );
}

#[test]
fn missing_and_empty_profiles_are_not_reported() {
    let (_dir, catalog, report_path) = setup();
    let (_out, report) = run(&catalog, &report_path);

    let got = profiles(&report);
    assert!(
        !got.contains_key(""),
        "an empty CameraProfile must not become a profile name: {got:?}"
    );
    // Photo 4 has no field and photo 5 is empty; neither may add an entry.
    assert_eq!(got.len(), p::DISTINCT_PROFILES, "{got:?}");
    assert_eq!(
        count(&report, "photos_with_profile"),
        p::PHOTOS_WITH_PROFILE
    );
}

#[test]
fn a_wrongly_typed_profile_is_unmapped_and_creates_no_entry() {
    let (_dir, catalog, report_path) = setup();
    let (_out, report) = run(&catalog, &report_path);

    let unmapped = develop(&report)["unmapped"]
        .as_array()
        .unwrap_or_else(|| panic!("develop.unmapped is not a list: {}", develop(&report)));
    let names: Vec<&str> = unmapped.iter().filter_map(Value::as_str).collect();
    assert!(
        names.contains(&"CameraProfile"),
        "a wrongly typed CameraProfile must be reported as unmapped: {names:?}"
    );
    assert!(
        names.contains(&"Look"),
        "Look is not converted and stays unmapped: {names:?}"
    );
    assert_eq!(
        names,
        p::UNMAPPED_NAMES,
        "no other field of this fixture may be unmapped"
    );
    assert_eq!(
        count(&report, "photos_with_profile"),
        p::PHOTOS_WITH_PROFILE
    );
}

#[test]
fn a_look_name_is_not_a_camera_profile() {
    let (_dir, catalog, report_path) = setup();
    let (_out, report) = run(&catalog, &report_path);

    let got = profiles(&report);
    assert!(
        !got.contains_key(p::LOOK),
        "a Look name must not be reported as a camera profile: {got:?}"
    );
}

#[test]
fn a_profile_alone_plans_no_develop_command() {
    let (_dir, catalog, report_path) = setup();
    let (_out, report) = run(&catalog, &report_path);

    let d = develop(&report);
    assert_eq!(
        d["planned"].as_u64(),
        Some(0),
        "a profile must not trigger a develop command: {d}"
    );
    assert_eq!(
        d["written"].as_u64(),
        Some(0),
        "a dry run writes nothing: {d}"
    );
    assert!(
        d["photos"].as_u64().unwrap_or(0) >= p::PHOTOS_WITH_PROFILE as u64,
        "photos with settings must still be counted: {d}"
    );
}

#[test]
fn the_summary_names_the_photos_with_a_profile() {
    let (_dir, catalog, report_path) = setup();
    let (output, _report) = run(&catalog, &report_path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected = format!("photos with camera profile {}", p::PHOTOS_WITH_PROFILE);
    assert!(
        stdout.contains(&expected),
        "summary must contain `{expected}`:\n{stdout}"
    );
}

#[test]
fn the_mandatory_invariants_hold_for_the_profile_fixture() {
    let (_dir, catalog, report_path) = setup();
    let before = fs::read(&catalog).unwrap();

    let (_out, first) = run(&catalog, &report_path);
    assert_report_invariants(&first);
    assert_catalog_unchanged(&catalog, &before);
    assert_no_state_file(_dir.path());

    // A second dry run yields the identical report: no duplicates, no drift.
    let second_path = _dir.path().join("report-second.json");
    let (_out2, second) = run(&catalog, &second_path);
    assert_eq!(
        fs::read(&report_path).unwrap(),
        fs::read(&second_path).unwrap(),
        "two dry runs over the same catalog produced different reports"
    );
    assert_eq!(
        profiles(&first),
        profiles(&second),
        "camera profile aggregation is not stable"
    );
    assert_catalog_unchanged(&catalog, &before);
    assert_no_state_file(_dir.path());
}

#[test]
fn an_anonymized_report_counts_profiles_without_their_names() {
    let (_dir, catalog, report_path) = setup();
    let output = dry_run(&catalog, &report_path);
    exit_ok(&output);
    let report = read_report(&report_path);
    assert!(profiles(&report).is_empty(), "{}", develop(&report));
    assert_eq!(
        count(&report, "camera_profiles_count"),
        p::DISTINCT_PROFILES
    );
    let text = std::fs::read_to_string(&report_path).unwrap();
    assert!(!text.contains(p::STUDIO));
}
