//! Acceptance tests for copyright and creator in the CLI dry run (task P5b-7b).
//!
//! Fixtures: `lrcat_fixtures::write_rights_catalog_with_files` (17 converted
//! photos, 3 skipped) and `write_copyright_only_catalog_with_files`; see
//! `lrcat_fixtures::rights_edge`.
//!
//! report.json, area `photos`: integers `copyrights` and `creators` = converted
//! photos with a value. Skipped photos, empty, NULL, dangling and missing rows
//! do not count. stdout: always exactly one line `copyrights: N` and one line
//! `creators: N`. No text of either value in stdout, stderr or report.json.
//! Photo balance unchanged. A catalog without the creator tables reports
//! `creators: 0` and still counts copyrights.
//!
//! Always: dry run, catalog byte-identical, second run identical, nothing
//! written, `--apply` writes nothing to the catalog.

use lrcat_fixtures::rights_edge as e;
use lrcat_fixtures::{
    write_copyright_only_catalog_with_files, write_rights_catalog_with_files,
    write_small_catalog_with_files,
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
#[ignore = "waiting for P5b-7b"]
fn counts_converted_photos_with_copyright_and_creator() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_rights_catalog_with_files(dir.path()).unwrap();
    let (out, _err, v, _) = run(&cat);
    assert_eq!(v["photos"]["copyrights"], e::COPYRIGHTS);
    assert_eq!(v["photos"]["creators"], e::CREATORS);
    assert_eq!(
        line(&out, "copyrights"),
        format!("copyrights: {}", e::COPYRIGHTS)
    );
    assert_eq!(line(&out, "creators"), format!("creators: {}", e::CREATORS));
    assert_eq!(v["photos"]["converted"], e::CONVERTED);
    let skipped = v["photos"]["skipped"].as_array().unwrap().len() as i64;
    assert_eq!(skipped, e::SKIPPED);
    assert_eq!(v["photos"]["read"], e::CONVERTED + e::SKIPPED);
}

#[test]
#[ignore = "waiting for P5b-7b"]
fn catalog_without_rights_reports_zero() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_small_catalog_with_files(dir.path()).unwrap();
    let (out, _err, v, _) = run(&cat);
    assert_eq!(v["photos"]["copyrights"], 0);
    assert_eq!(v["photos"]["creators"], 0);
    assert_eq!(line(&out, "copyrights"), "copyrights: 0");
    assert_eq!(line(&out, "creators"), "creators: 0");
}

#[test]
#[ignore = "waiting for P5b-7b"]
fn missing_creator_tables_give_zero_creators_and_no_error() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_copyright_only_catalog_with_files(dir.path()).unwrap();
    let (out, _err, v, _) = run(&cat);
    assert_eq!(v["photos"]["creators"], 0);
    assert_eq!(line(&out, "creators"), "creators: 0");
    assert_eq!(v["photos"]["copyrights"], e::COPYRIGHTS);
    assert_eq!(v["photos"]["converted"], e::CONVERTED);
    assert_eq!(v["photos"]["read"], e::CONVERTED + e::SKIPPED);
}

#[test]
#[ignore = "waiting for P5b-7b"]
fn no_rights_text_in_any_output() {
    for creators in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let cat = if creators {
            write_rights_catalog_with_files(dir.path())
        } else {
            write_copyright_only_catalog_with_files(dir.path())
        }
        .unwrap();
        let (out, err, _v, raw) = run(&cat);
        let extra = [
            "K\u{f6}ln",
            "\u{1f305}",
            "M\u{fc}ller",
            "\u{1f4f7}",
            "\u{a9}",
        ];
        for t in e::ALL_TEXTS.into_iter().chain(extra) {
            for (name, s) in [("stdout", &out), ("stderr", &err), ("report", &raw)] {
                assert!(!s.contains(t), "{name} contains text {t:?}");
            }
        }
    }
}

#[test]
#[ignore = "waiting for P5b-7b"]
fn catalog_unchanged_second_run_identical_and_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_rights_catalog_with_files(dir.path()).unwrap();
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
#[ignore = "waiting for P5b-7b"]
fn apply_is_still_refused_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_rights_catalog_with_files(dir.path()).unwrap();
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
