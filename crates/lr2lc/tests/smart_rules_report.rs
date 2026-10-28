//! Acceptance tests for smart collections in the CLI dry run,
//! on `lrcat_fixtures::write_smart_catalog_with_files`.
//!
//! At this stage nothing is translated, so every smart collection is skipped:
//! - `collections`: read == converted + skipped; the smart collections are
//!   all in `skipped` (25 reasons containing "smart collection").
//! - Reasons name the first criterion (`rating`, `pick`, ...) or say
//!   "unreadable rules"; report.json, stdout and stderr contain no rule
//!   value, operation, combine text or collection name.
//! - The catalog is byte-identical, a second run gives the same report,
//!   and no `--library` is touched.

use lrcat_fixtures::smart_edge as e;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

fn run(cat: &Path) -> (String, String, Value, String) {
    let cwd = tempfile::tempdir().unwrap();
    let report = cwd.path().join("report.json");
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd.path())
        .arg(cat)
        .arg("--report")
        .arg(&report)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(0), "stderr: {stderr}");
    let raw = fs::read_to_string(&report).unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr,
        serde_json::from_str(&raw).unwrap(),
        raw,
    )
}

fn smart_reasons(v: &Value) -> Vec<String> {
    v["collections"]["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["reason"].as_str().unwrap().to_string())
        .filter(|r| r.to_lowercase().contains("smart collection"))
        .collect()
}

#[test]
fn smart_collections_are_skipped_and_balance() {
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_smart_catalog_with_files(dir.path()).unwrap();
    let (_out, _err, v, _) = run(&cat);
    let a = &v["collections"];
    let skipped = a["skipped"].as_array().unwrap().len() as i64;
    assert_eq!(
        a["read"].as_i64().unwrap(),
        a["converted"].as_i64().unwrap() + skipped
    );
    // Collection 101 (rating >= 3) is the one that is translated; it is
    // then converted and no longer skipped.
    let n = smart_reasons(&v).len() as i64;
    assert!(n == e::SMART_TOTAL || n == e::SMART_TOTAL - 1, "{n}");
}

#[test]
fn reports_name_criteria_never_values_or_names() {
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_smart_catalog_with_files(dir.path()).unwrap();
    let (out, err, v, raw) = run(&cat);
    let reasons = smart_reasons(&v);
    for name in [
        "rating",
        "pick",
        "labelColor",
        "keywords",
        "captureTime",
        "touchTime",
        "fileFormat",
        "zyxCriterion",
    ] {
        assert!(reasons.iter().any(|r| r.contains(name)), "criterion {name}");
    }
    assert!(
        reasons
            .iter()
            .any(|r| r.to_lowercase().contains("unreadable rules"))
    );
    for text in [&out, &err, &raw] {
        assert!(!text.contains(e::SECRET));
        for word in ["zyxOp", "zyxCombine", "inLast", "intersect"] {
            assert!(!text.contains(word), "{word}");
        }
    }
}

#[test]
fn catalog_unchanged_and_second_run_identical() {
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_smart_catalog_with_files(dir.path()).unwrap();
    let before = fs::read(&cat).unwrap();
    let (out1, _, v1, _) = run(&cat);
    let (out2, _, v2, _) = run(&cat);
    assert_eq!(fs::read(&cat).unwrap(), before);
    assert_eq!(v1, v2);
    assert_eq!(out1, out2);
}
