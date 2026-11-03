//! Golden `report.json` for a medium catalog (task T20).
//!
//! The test runs a dry run over a generated catalog of 1 000 photos (50
//! folders, 100 keywords, 4 collections) and compares the complete report with
//! the stored file `tests/fixtures/golden/report-medium.json`. It pins the key
//! set and every count, so an optimisation that accidentally changes the
//! output fails here. It contains no time measurement: runtime is guarded by
//! `performance_guard.rs`.
//!
//! The stored file contains generated data only. `tool_version` is normalised
//! to `"normalized"` so a version bump does not fail the test; every other
//! value is compared exactly.
//!
//! Regenerate the golden file after an intentional report change with:
//!
//! ```text
//! UPDATE_GOLDEN=1 cargo test -p lr2lc --test report_golden
//! ```

#[path = "perf_support/mod.rs"]
mod perf_support;

use lrcat_fixtures::{CatalogShape, write_performance_catalog_with_files};
use perf_support::{
    assert_catalog_unchanged, assert_no_state_file, assert_photo_subcounts,
    assert_report_invariants, dry_run, exit_ok, read_report,
};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

/// A medium catalog: large enough to exercise the whole reader, small enough
/// for the normal test suite.
const SHAPE: CatalogShape = CatalogShape {
    photos: 1_000,
    folders: 50,
    keywords: 100,
    collections: 4,
};

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/golden/report-medium.json")
}

/// Replaces the only value that may legitimately change between releases.
fn normalize(mut report: Value) -> Value {
    report["tool_version"] = json!("normalized");
    report
}

#[test]
fn medium_catalog_report_matches_the_golden_file() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = write_performance_catalog_with_files(dir.path(), SHAPE).unwrap();
    let before = fs::read(&catalog).unwrap();
    let report_path = dir.path().join("report.json");

    let output = dry_run(&catalog, &report_path);
    exit_ok(&output);
    let report = read_report(&report_path);
    assert_report_invariants(&report);
    assert_photo_subcounts(&report);
    assert_catalog_unchanged(&catalog, &before);

    // Pin the shape explicitly: a generator change must not silently alter
    // what the golden file describes.
    assert_eq!(report["photos"]["read"], SHAPE.photos);
    assert_eq!(report["photos"]["converted"], SHAPE.photos);
    assert_eq!(report["folders"]["read"], SHAPE.folders);
    assert_eq!(report["keywords"]["read"], SHAPE.keywords);
    assert_eq!(report["collections"]["read"], SHAPE.collections);

    // The second dry run changes nothing and gives the identical report.
    let second_path = dir.path().join("report-second.json");
    let second = dry_run(&catalog, &second_path);
    exit_ok(&second);
    assert_eq!(
        fs::read(&second_path).unwrap(),
        fs::read(&report_path).unwrap(),
        "two dry runs over the same catalog produced different reports"
    );
    assert_catalog_unchanged(&catalog, &before);
    assert_no_state_file(dir.path());

    let actual = normalize(report);
    let golden = golden_path();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        if let Some(parent) = golden.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut text = serde_json::to_string_pretty(&actual).unwrap();
        text.push('\n');
        fs::write(&golden, text).unwrap();
        eprintln!("updated golden file {}", golden.display());
        return;
    }

    let expected = normalize(read_report(&golden));
    if let Some(difference) = first_difference(&expected, &actual, "report") {
        panic!(
            "report.json no longer matches {}: {difference}\n\
             If the change is intentional, regenerate with:\n\
             UPDATE_GOLDEN=1 cargo test -p lr2lc --test report_golden\n\
             expected:\n{}\nactual:\n{}",
            golden.display(),
            serde_json::to_string_pretty(&expected).unwrap(),
            serde_json::to_string_pretty(&actual).unwrap(),
        );
    }
}

/// Returns a readable description of the first difference, or `None` when the
/// two values are equal.
fn first_difference(expected: &Value, actual: &Value, path: &str) -> Option<String> {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            for (key, value) in e {
                match a.get(key) {
                    Some(actual_value) => {
                        let child = format!("{path}.{key}");
                        if let Some(d) = first_difference(value, actual_value, &child) {
                            return Some(d);
                        }
                    }
                    None => return Some(format!("{path}.{key}: missing in the actual report")),
                }
            }
            for key in a.keys() {
                if !e.contains_key(key) {
                    return Some(format!("{path}.{key}: unexpected key in the actual report"));
                }
            }
            None
        }
        (Value::Array(e), Value::Array(a)) => {
            if e.len() != a.len() {
                return Some(format!(
                    "{path}: expected {} entries, actual {}",
                    e.len(),
                    a.len()
                ));
            }
            for (index, (expected_item, actual_item)) in e.iter().zip(a).enumerate() {
                let child = format!("{path}[{index}]");
                if let Some(d) = first_difference(expected_item, actual_item, &child) {
                    return Some(d);
                }
            }
            None
        }
        _ if expected != actual => Some(format!("{path}: expected {expected}, actual {actual}")),
        _ => None,
    }
}
