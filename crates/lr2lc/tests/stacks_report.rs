//! Acceptance tests for stacks in the CLI dry run.
//!
//! Fixtures: `lrcat_fixtures::write_stack_catalog_with_files` (see
//! `lrcat_fixtures::stack_edge`), the video fixture (no stack tables) and the
//! small fixture (no stack tables).
//!
//! report.json:
//! - `stacks`: `read`, `converted`, `skipped` (list of `{reason}`), plus the
//!   integers `collapsed` and `no_top`.
//! - `stack_members`: `read`, `converted`, `skipped` like the others.
//! - Both areas balance: read == converted + len(skipped).
//!
//! stdout: the lines `stacks: read N, would convert N, skipped N` and
//! `stack members: read N, would convert N, skipped N`.
//!
//! Always: dry run, catalog byte-identical, second run identical, nothing
//! written, `--apply` without `--library` exits 2 and writes nothing.

use lrcat_fixtures::stack_edge as e;
use lrcat_fixtures::{write_small_catalog_with_files, write_stack_catalog_with_files};
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
        .arg("--anonymize")
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

fn has_line(text: &str, want: &str) -> bool {
    text.lines().any(|l| l.trim() == want)
}

fn check(v: &Value, area: &str, read: i64, converted: i64, skipped: i64) {
    let a = &v[area];
    assert_eq!(a["read"], read, "{area}.read");
    assert_eq!(a["converted"], converted, "{area}.converted");
    let list = a["skipped"].as_array().unwrap();
    assert_eq!(list.len() as i64, skipped, "{area}.skipped");
    assert!(
        list.iter()
            .all(|s| !s["reason"].as_str().unwrap().is_empty())
    );
    assert_eq!(read, converted + list.len() as i64, "{area} balance");
}

#[test]
fn stack_fixture_counts_and_balance() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_stack_catalog_with_files(dir.path()).unwrap();
    let (out, _err, v, _) = run(&cat);
    check(&v, "stacks", e::STACKS, e::CONVERTED, e::SKIPPED);
    check(
        &v,
        "stack_members",
        e::MEMBER_ROWS,
        e::MEMBERS_CONVERTED,
        e::MEMBERS_SKIPPED,
    );
    assert_eq!(v["stacks"]["collapsed"], e::COLLAPSED);
    assert_eq!(v["stacks"]["no_top"], e::NO_TOP);
    assert!(has_line(
        &out,
        &format!(
            "stacks: read {}, would convert {}, skipped {}",
            e::STACKS,
            e::CONVERTED,
            e::SKIPPED
        )
    ));
    assert!(has_line(
        &out,
        &format!(
            "stack members: read {}, would convert {}, skipped {}",
            e::MEMBER_ROWS,
            e::MEMBERS_CONVERTED,
            e::MEMBERS_SKIPPED
        )
    ));
}

#[test]
fn skipped_stacks_name_their_ids_and_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_stack_catalog_with_files(dir.path()).unwrap();
    let (_out, _err, v, _) = run(&cat);
    let reasons: Vec<String> = v["stacks"]["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["reason"].as_str().unwrap().to_lowercase())
        .collect();
    for id in e::SKIPPED_STACKS {
        let want = id.to_string();
        assert!(
            reasons
                .iter()
                .any(|r| r.contains("fewer than two converted photos")
                    && r.split(|c: char| !c.is_ascii_digit()).any(|t| t == want)),
            "stack {id}: {reasons:?}"
        );
    }
}

#[test]
fn catalog_without_stack_tables_reports_zero() {
    for variant in ["small", "dropped"] {
        let dir = tempfile::tempdir().unwrap();
        let cat = if variant == "small" {
            write_small_catalog_with_files(dir.path()).unwrap()
        } else {
            let cat = write_stack_catalog_with_files(dir.path()).unwrap();
            lrcat_fixtures::drop_stack_tables(&cat).unwrap();
            cat
        };
        let (out, _err, v, _) = run(&cat);
        check(&v, "stacks", 0, 0, 0);
        check(&v, "stack_members", 0, 0, 0);
        assert_eq!(v["stacks"]["collapsed"], 0, "{variant}");
        assert_eq!(v["stacks"]["no_top"], 0, "{variant}");
        assert!(
            has_line(&out, "stacks: read 0, would convert 0, skipped 0"),
            "{variant}: {out}"
        );
    }
}

#[test]
fn nothing_but_ids_in_reasons() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_stack_catalog_with_files(dir.path()).unwrap();
    let (out, err, _v, raw) = run(&cat);
    for text in [&out, &err, &raw] {
        assert!(!text.contains("IMG_0004"), "a file name leaked");
    }
}

#[test]
fn catalog_is_unchanged_and_second_run_is_identical() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_stack_catalog_with_files(dir.path()).unwrap();
    let before = fs::read(&cat).unwrap();
    let (out1, _e1, v1, raw1) = run(&cat);
    let (out2, _e2, v2, raw2) = run(&cat);
    assert_eq!(fs::read(&cat).unwrap(), before, "catalog bytes changed");
    assert_eq!(v1["catalog_unchanged"], true);
    assert_eq!(out1, out2);
    assert_eq!(v1, v2);
    assert_eq!(raw1, raw2);
    assert_eq!(v1["dry_run"], true);
}

#[test]
fn apply_without_library_exits_2_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_stack_catalog_with_files(dir.path()).unwrap();
    let before = fs::read(&cat).unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd.path())
        .arg(&cat)
        .arg("--apply")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(fs::read(&cat).unwrap(), before);
    assert_eq!(fs::read_dir(cwd.path()).unwrap().count(), 0);
}
