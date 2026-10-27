//! Acceptance tests for titles in the CLI dry run (task P5b-7c).
//!
//! Fixture: `lrcat_fixtures::write_title_catalog_with_files` (17 converted
//! photos, 3 skipped), see `lrcat_fixtures::title_edge`.
//!
//! report.json, area `photos`: integers `titles` (converted photos with a
//! title) and `titles_unreadable` (converted photos with a broken blob;
//! skipped photos, NULL, missing and title-less XMP do not count). stdout,
//! matching the always-present lines of `copyrights`/`creators`: exactly one
//! line `titles: N` and one line `titles unreadable: N`, also when 0. No title
//! text in stdout, stderr or report.json. Photo balance unchanged. A catalog
//! without the table or column reports 0 and 0 without an error.
//!
//! Always: dry run, catalog byte-identical, second run identical, nothing
//! written, `--apply` still writes nothing to the catalog.

use lrcat_fixtures::title_edge::{self as e, Variant};
use lrcat_fixtures::{
    write_small_catalog_with_files, write_title_catalog_with_files,
    write_title_catalog_with_files_variant,
};
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

fn line<'a>(text: &'a str, key: &str) -> &'a str {
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with(&format!("{key}:")))
        .collect();
    assert_eq!(lines.len(), 1, "exactly one {key} line: {text}");
    lines[0].trim()
}

#[test]
fn counts_titles_and_unreadable_blobs() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_title_catalog_with_files(dir.path()).unwrap();
    let (out, _err, v, _) = run(&cat);
    assert_eq!(v["photos"]["titles"], e::TITLES);
    assert_eq!(v["photos"]["titles_unreadable"], e::UNREADABLE);
    assert_eq!(line(&out, "titles"), format!("titles: {}", e::TITLES));
    assert_eq!(
        line(&out, "titles unreadable"),
        format!("titles unreadable: {}", e::UNREADABLE)
    );
    // Broken blobs never skip a photo: the balance is that of the fixture.
    assert_eq!(v["photos"]["converted"], e::CONVERTED);
    let skipped = v["photos"]["skipped"].as_array().unwrap().len() as i64;
    assert_eq!(skipped, e::SKIPPED);
    assert_eq!(v["photos"]["read"], e::CONVERTED + e::SKIPPED);
}

#[test]
fn catalog_without_titles_reports_zero() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_small_catalog_with_files(dir.path()).unwrap();
    let (out, _err, v, _) = run(&cat);
    assert_eq!(v["photos"]["titles"], 0);
    assert_eq!(v["photos"]["titles_unreadable"], 0);
    assert_eq!(line(&out, "titles"), "titles: 0");
    assert_eq!(line(&out, "titles unreadable"), "titles unreadable: 0");
}

#[test]
fn missing_table_or_column_gives_zero_and_no_error() {
    for variant in [Variant::NoTable, Variant::NoColumn] {
        let dir = tempfile::tempdir().unwrap();
        let cat = write_title_catalog_with_files_variant(dir.path(), variant).unwrap();
        let (out, _err, v, _) = run(&cat);
        assert_eq!(v["photos"]["titles"], 0, "{variant:?}");
        assert_eq!(v["photos"]["titles_unreadable"], 0, "{variant:?}");
        assert_eq!(line(&out, "titles"), "titles: 0");
        assert_eq!(v["photos"]["converted"], e::CONVERTED);
        assert_eq!(v["photos"]["read"], e::CONVERTED + e::SKIPPED);
    }
}

#[test]
fn no_title_text_in_any_output() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_title_catalog_with_files(dir.path()).unwrap();
    let (out, err, _v, raw) = run(&cat);
    let extra = ["K\u{f6}ln", "\u{1f305}", "Tom &", "Jerry", "Deutsch", "<b>"];
    for t in e::ALL_TEXTS.into_iter().chain(extra) {
        for (name, s) in [("stdout", &out), ("stderr", &err), ("report", &raw)] {
            assert!(!s.contains(t), "{name} contains text {t:?}");
        }
    }
}

#[test]
fn catalog_unchanged_second_run_identical_and_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_title_catalog_with_files(dir.path()).unwrap();
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
    let a = run(&cat);
    let b = run(&cat);
    assert_eq!(a, b, "second run differs");
    assert_eq!(a.2["catalog_unchanged"], true);
    assert_eq!(a.2["dry_run"], true);
    assert_eq!(before, fs::read(&cat).unwrap());
    assert_eq!(files, listing(dir.path()));
}

#[test]
fn apply_is_still_refused_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_title_catalog_with_files(dir.path()).unwrap();
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
