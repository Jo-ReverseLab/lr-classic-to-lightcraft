//! Helpers shared by the performance guard, the performance measurements and
//! the golden-report test (task T20). Test infrastructure only: every catalog
//! is generated, nothing here touches a real Lightroom catalog.
//!
//! Each test target includes this module with
//! `#[path = "perf_support/mod.rs"] mod perf_support;` and uses a subset of
//! the helpers, so unused items are allowed.
#![allow(dead_code)]

use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

/// The nine catalog areas of `report.json`, in output order.
pub const AREAS: [&str; 9] = [
    "folders",
    "photos",
    "collections",
    "collection_members",
    "stacks",
    "stack_members",
    "keywords",
    "keyword_assignments",
    "keyword_synonyms",
];

/// Runs `lr2lc <catalog> --report <report>` as a dry run (never `--apply`).
pub fn dry_run(catalog: &Path, report: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .arg(catalog)
        .arg("--anonymize")
        .arg("--report")
        .arg(report)
        .output()
        .expect("lr2lc starts")
}

/// Like [`dry_run`], but the report keeps names (no `--anonymize`).
pub fn dry_run_named(catalog: &Path, report: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .arg(catalog)
        .arg("--report")
        .arg(report)
        .output()
        .expect("lr2lc starts")
}

/// Decodes captured output for failure messages.
pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Asserts a successful run and returns its stdout.
pub fn exit_ok(output: &Output) -> String {
    assert_eq!(
        output.status.code(),
        Some(0),
        "lr2lc failed\nstdout:\n{}\nstderr:\n{}",
        text(&output.stdout),
        text(&output.stderr)
    );
    text(&output.stdout)
}

/// Reads `report.json`.
pub fn read_report(path: &Path) -> Value {
    let raw = fs::read_to_string(path).unwrap_or_else(|e| {
        panic!("cannot read {}: {e}", path.display());
    });
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
}

/// The invariants that hold for every report, independent of its content:
/// a dry run that verified the checksum, and every area balanced
/// (`read == converted + len(skipped)`), with no empty reason.
pub fn assert_report_invariants(report: &Value) {
    assert_eq!(report["dry_run"], true, "the guard runs without --apply");
    assert_eq!(
        report["catalog_unchanged"], true,
        "the checksum before and after the run must match"
    );
    for area in AREAS {
        let a = &report[area];
        let read = a["read"].as_u64().unwrap_or_else(|| {
            panic!("{area}.read is not an integer: {a}");
        });
        let converted = a["converted"].as_u64().unwrap_or_else(|| {
            panic!("{area}.converted is not an integer: {a}");
        });
        let skipped = a["skipped"]
            .as_array()
            .unwrap_or_else(|| panic!("{area}.skipped is not an array: {a}"));
        for s in skipped {
            assert!(
                !s["reason"].as_str().unwrap_or("").is_empty(),
                "{area}: a skip has no reason: {s}"
            );
            assert!(
                !s["code"].as_str().unwrap_or("").is_empty(),
                "{area}: a skip has no code: {s}"
            );
        }
        let by_code: u64 = a["by_code"]
            .as_object()
            .unwrap_or_else(|| panic!("{area}.by_code is not an object: {a}"))
            .values()
            .map(|n| n.as_u64().unwrap_or(0))
            .sum();
        assert_eq!(
            by_code,
            skipped.len() as u64,
            "{area}.by_code does not sum to the skipped list"
        );
        assert_eq!(
            read,
            converted + skipped.len() as u64,
            "{area}: read != converted + skipped"
        );
    }
}

/// Asserts that the photo sub-counters add up to `photos.converted`.
pub fn assert_photo_subcounts(report: &Value) {
    let converted = report["photos"]["converted"].as_u64().unwrap();
    let sum = |obj: &Value, keys: &[&str]| -> u64 {
        keys.iter().map(|k| obj[k].as_u64().unwrap_or(0)).sum()
    };
    assert_eq!(
        sum(
            &report["photos"]["flags"],
            &["pick", "reject", "none", "unknown"]
        ),
        converted,
        "flags do not add up to photos.converted"
    );
    assert_eq!(
        sum(
            &report["photos"]["labels"],
            &["red", "yellow", "green", "blue", "purple", "none", "custom"]
        ),
        converted,
        "labels do not add up to photos.converted"
    );
    assert_eq!(
        report["ratings"]
            .as_object()
            .unwrap()
            .values()
            .map(|n| n.as_u64().unwrap_or(0))
            .sum::<u64>(),
        converted,
        "ratings do not add up to photos.converted"
    );
}

/// Asserts that a dry run without `--library` created no mapping file.
pub fn assert_no_state_file(dir: &Path) {
    for entry in fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        assert!(
            !name.ends_with(".lr2lc.json"),
            "a dry run must not write a mapping file, found {name}"
        );
    }
}

/// Asserts that the catalog file is byte-identical to `before`.
pub fn assert_catalog_unchanged(catalog: &Path, before: &[u8]) {
    assert_eq!(
        fs::read(catalog).unwrap(),
        before,
        "the catalog changed during the run"
    );
}
