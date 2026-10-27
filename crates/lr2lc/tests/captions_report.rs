//! Acceptance tests for captions in the CLI dry run (task P5b-7).
//!
//! Fixture: `lrcat_fixtures::write_caption_catalog_with_files` (17 converted
//! photos, 3 skipped; 3 converted photos carry a caption, see
//! `lrcat_fixtures::caption_edge`).
//!
//! report.json, area `photos`: integer `captions` = converted photos with a
//! caption. Skipped photos, empty, NULL and missing rows do not count.
//! stdout: always exactly one line `captions: N`. No caption text anywhere
//! in stdout, stderr or report.json. Photo balance unchanged.
//!
//! Always: dry run, catalog byte-identical, second run identical, nothing
//! written, `--apply` writes nothing to the catalog.

use lrcat_fixtures::caption_edge as e;
use lrcat_fixtures::{write_caption_catalog_with_files, write_small_catalog_with_files};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

fn run(cat: &Path, extra: &[&str]) -> (String, String, Value, String) {
    let cwd = tempfile::tempdir().unwrap();
    let report = cwd.path().join("report.json");
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd.path())
        .arg(cat)
        .args(extra)
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

fn captions_line(text: &str) -> &str {
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("captions:"))
        .collect();
    assert_eq!(lines.len(), 1, "exactly one captions line: {text}");
    lines[0].trim()
}

#[test]
#[ignore = "waiting for P5b-7"]
fn counts_converted_photos_with_a_caption() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_caption_catalog_with_files(dir.path()).unwrap();
    let (out, _err, v, _) = run(&cat, &[]);
    assert_eq!(v["photos"]["captions"], e::CAPTIONS);
    assert_eq!(captions_line(&out), format!("captions: {}", e::CAPTIONS));
    // Skipped photos with a caption (2, 20) are not counted; balance holds.
    assert_eq!(v["photos"]["converted"], e::CONVERTED);
    assert_eq!(v["photos"]["skipped"], e::SKIPPED);
    assert_eq!(v["photos"]["read"], e::CONVERTED + e::SKIPPED);
}

#[test]
#[ignore = "waiting for P5b-7"]
fn catalog_without_captions_reports_zero() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_small_catalog_with_files(dir.path()).unwrap();
    let (out, _err, v, _) = run(&cat, &[]);
    assert_eq!(v["photos"]["captions"], 0);
    assert_eq!(captions_line(&out), "captions: 0");
}

#[test]
#[ignore = "waiting for P5b-7"]
fn no_caption_text_in_any_output() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_caption_catalog_with_files(dir.path()).unwrap();
    let (out, err, _v, raw) = run(&cat, &[]);
    for t in e::ALL_TEXTS.into_iter().chain(["K\u{f6}ln", "\u{1f305}"]) {
        for (name, s) in [("stdout", &out), ("stderr", &err), ("report", &raw)] {
            assert!(!s.contains(t), "{name} contains caption text {t:?}");
        }
    }
}

#[test]
#[ignore = "waiting for P5b-7"]
fn catalog_unchanged_second_run_identical_and_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_caption_catalog_with_files(dir.path()).unwrap();
    let before = fs::read(&cat).unwrap();
    let listing = |p: &Path| {
        let mut v: Vec<_> = fs::read_dir(p)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        v.sort();
        v
    };
    let files = listing(dir.path());
    let a = run(&cat, &[]);
    let b = run(&cat, &[]);
    assert_eq!(a, b, "second run differs");
    assert_eq!(a.2["catalog_unchanged"], true);
    assert_eq!(a.2["dry_run"], true);
    assert_eq!(before, fs::read(&cat).unwrap());
    assert_eq!(files, listing(dir.path()));
}

#[test]
#[ignore = "waiting for P5b-7"]
fn apply_is_still_refused_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_caption_catalog_with_files(dir.path()).unwrap();
    let before = fs::read(&cat).unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd.path())
        .arg(&cat)
        .arg("--apply")
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    assert_eq!(before, fs::read(&cat).unwrap());
    assert_eq!(fs::read_dir(cwd.path()).unwrap().count(), 0);
}
