//! Acceptance tests for collections in the CLI dry run (task P5b-3a).
//!
//! Fixtures: `lrcat_fixtures::write_collection_catalog_with_files`
//! (see `lrcat_fixtures::collection_edge`) and the small fixture.
//!
//! report.json:
//! - `collections`: `read`, `converted`, `skipped` (list of `{reason}`) like
//!   the other areas, plus integers `albums`, `folders` and `duplicate_names`
//!   (converted albums or folders that repeat the name of an earlier
//!   sibling below the same parent: the two "Twin" albums count as 1).
//! - `collection_members`: `read`, `converted`, `skipped` like the others.
//! - Both areas balance: read == converted + len(skipped).
//!
//! stdout: the lines
//! `collections: read N, would convert N, skipped N` and
//! `collection members: read N, would convert N, skipped N`, and for each
//! skipped item a reason below the heading `skipped collections:` or
//! `skipped collection members:` as for the other areas (reasons grouped, a
//! leading `<word> <id>: ` removed). Names of
//! collections appear in neither stdout, stderr nor report.json.
//!
//! Always: dry run, catalog byte-identical, second run identical, nothing
//! written, `--apply` without `--library` exits 2 and writes nothing.

use lrcat_fixtures::collection_edge as e;
use lrcat_fixtures::{small, write_collection_catalog_with_files, write_small_catalog_with_files};
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

/// Removes a leading `word <digits>: ` such as `collection 12: `.
fn strip_id_prefix(reason: &str) -> &str {
    if let Some((head, rest)) = reason.split_once(": ")
        && let Some((word, id)) = head.split_once(' ')
        && !word.is_empty()
        && word.bytes().all(|b| b.is_ascii_alphabetic())
        && !id.is_empty()
        && id.bytes().all(|b| b.is_ascii_digit())
    {
        return rest;
    }
    reason
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
fn edge_fixture_counts_and_balance() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_collection_catalog_with_files(dir.path()).unwrap();
    let (out, _err, v, _) = run(&cat);
    check(&v, "collections", e::COLLECTIONS, e::CONVERTED, e::SKIPPED);
    check(
        &v,
        "collection_members",
        e::MEMBER_ROWS,
        e::MEMBERS_CONVERTED,
        e::MEMBERS_SKIPPED,
    );
    assert_eq!(v["collections"]["albums"], e::ALBUMS);
    assert_eq!(v["collections"]["folders"], e::FOLDERS);
    assert_eq!(v["collections"]["duplicate_names"], e::DUPLICATE_NAMES);
    assert!(has_line(
        &out,
        &format!(
            "collections: read {}, would convert {}, skipped {}",
            e::COLLECTIONS,
            e::CONVERTED,
            e::SKIPPED
        )
    ));
    assert!(has_line(
        &out,
        &format!(
            "collection members: read {}, would convert {}, skipped {}",
            e::MEMBER_ROWS,
            e::MEMBERS_CONVERTED,
            e::MEMBERS_SKIPPED
        )
    ));
}

#[test]
fn small_fixture_counts_with_missing_originals() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_small_catalog_with_files(dir.path()).unwrap();
    let (out, _err, v, _) = run(&cat);
    // Set 1 and albums 2, 3, 4 are converted.
    check(&v, "collections", small::COLLECTIONS, 4, 5);
    assert_eq!(v["collections"]["albums"], 3);
    assert_eq!(v["collections"]["folders"], 1);
    assert_eq!(v["collections"]["duplicate_names"], 0);
    // Member rows: (2,1) (2,2) (3,3) (3,4) (4,5) (7,1) (8,6). Photos 2 and 3
    // have no original (skipped), 7 and 8 are skipped collections.
    check(&v, "collection_members", small::COLLECTION_IMAGES, 3, 4);
    assert!(has_line(
        &out,
        "collections: read 9, would convert 4, skipped 5"
    ));
    assert!(has_line(
        &out,
        "collection members: read 7, would convert 3, skipped 4"
    ));
}

#[test]
fn members_of_photos_skipped_by_the_cli_are_skipped_members() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_small_catalog_with_files(dir.path()).unwrap();
    let (_out, _err, v, _) = run(&cat);
    let reasons: Vec<String> = v["collection_members"]["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["reason"].as_str().unwrap().to_lowercase())
        .collect();
    // Photo 2 (album 2) and photo 3 (album 3) are missing originals.
    for photo in ["2", "3"] {
        let n = reasons
            .iter()
            .filter(|r| {
                r.split(|c: char| !c.is_ascii_digit()).any(|t| t == photo)
                    && !r.contains("collection skipped")
            })
            .count();
        assert!(n >= 1, "photo {photo}: {reasons:?}");
    }
    assert_eq!(
        reasons
            .iter()
            .filter(|r| r.contains("collection skipped"))
            .count(),
        2
    );
}

#[test]
fn no_collection_name_in_any_output() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_collection_catalog_with_files(dir.path()).unwrap();
    let (out, err, _v, raw) = run(&cat);
    let mut names: Vec<String> = e::SKIPPED_NAMES.iter().map(|s| s.to_string()).collect();
    names.extend(e::NAMES.iter().map(|(_, n, _)| n.to_string()));
    // Short names such as "Empty" could occur in ordinary words; the
    // others are distinctive.
    names.retain(|n| n.trim().len() > 4);
    for name in &names {
        for (what, text) in [("stdout", &out), ("stderr", &err), ("report", &raw)] {
            assert!(!text.contains(name.as_str()), "{what} has {name:?}");
        }
    }
    let small_dir = tempfile::tempdir().unwrap();
    let cat = write_small_catalog_with_files(small_dir.path()).unwrap();
    let (out, err, _v, raw) = run(&cat);
    for name in [
        "Set Alpha",
        "Coll Beta",
        "Coll Gamma",
        "Coll Delta",
        "Smart Epsilon",
    ] {
        for text in [&out, &err, &raw] {
            assert!(!text.contains(name), "output has {name:?}");
        }
    }
}

#[test]
fn skipped_collection_reasons_are_listed_in_stdout() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_collection_catalog_with_files(dir.path()).unwrap();
    let (out, _err, v, _) = run(&cat);
    assert!(has_line(&out, "skipped collections:"), "{out}");
    assert!(has_line(&out, "skipped collection members:"), "{out}");
    for area in ["collections", "collection_members"] {
        for s in v[area]["skipped"].as_array().unwrap() {
            // stdout groups reasons and drops a leading `<word> <digits>: `.
            let reason = strip_id_prefix(s["reason"].as_str().unwrap());
            assert!(out.contains(reason), "{reason:?} missing from stdout");
        }
    }
}

#[test]
fn catalog_without_collections_reports_zero() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_small_catalog_with_files(dir.path()).unwrap();
    lrcat_fixtures::clear_collections(&cat).unwrap();
    let (out, _err, v, _) = run(&cat);
    check(&v, "collections", 0, 0, 0);
    check(&v, "collection_members", 0, 0, 0);
    assert_eq!(v["collections"]["albums"], 0);
    assert_eq!(v["collections"]["folders"], 0);
    assert_eq!(v["collections"]["duplicate_names"], 0);
    assert!(has_line(
        &out,
        "collections: read 0, would convert 0, skipped 0"
    ));
}

#[test]
fn catalog_is_unchanged_and_second_run_is_identical() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_collection_catalog_with_files(dir.path()).unwrap();
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
    let cat = write_collection_catalog_with_files(dir.path()).unwrap();
    let before = fs::read(&cat).unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd.path())
        .arg(&cat)
        .arg("--apply")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(fs::read_dir(cwd.path()).unwrap().count(), 0);
    assert_eq!(fs::read(&cat).unwrap(), before);
}

#[test]
fn dry_run_writes_no_file_without_report_option() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_collection_catalog_with_files(dir.path()).unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd.path())
        .arg(&cat)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(fs::read_dir(cwd.path()).unwrap().count(), 0);
}
