//! Acceptance tests for pick/reject counts in the CLI dry run (task P5b-1).
//!
//! Expected behaviour of `lr2lc <CATALOG> [--report FILE]` on the flag
//! fixture (`lrcat_fixtures::write_flag_catalog_with_files`, counts in
//! `lrcat_fixtures::flag_edge`):
//! - report.json, area `photos`: `flags` is an object with the integer fields
//!   `pick`, `reject`, `none`, `unknown`. They count only converted photos
//!   (not virtual copies, not photos with a missing original). `none` does
//!   not include `unknown`: pick + reject + none + unknown = converted.
//!   Unknown means the stored value is neither NULL, 0, 1 nor -1 (1.0 and
//!   -1.0 are valid); such photos are converted with no flag.
//! - stdout: always one line
//!   `flags: pick P, reject R, none N, unknown U` (also on catalogs without
//!   any flag).
//! - The photo balance is unchanged (read = converted + skipped) and no new
//!   skipped reason appears; reports contain no ids of flagged photos beyond
//!   the existing skip reasons.
//! - Without `--apply` nothing is written; the catalog is unchanged; a second
//!   run gives identical output.

use lrcat_fixtures::{
    flag_edge, small, write_flag_catalog_with_files, write_small_catalog_with_files,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn run_report(cat: &Path) -> (String, serde_json::Value, String) {
    let cwd = tempfile::tempdir().unwrap();
    let report = cwd.path().join("report.json");
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd.path())
        .arg(cat)
        .arg("--report")
        .arg(&report)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let raw = fs::read_to_string(&report).unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        serde_json::from_str(&raw).unwrap(),
        raw,
    )
}

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_flag_catalog_with_files(dir.path()).unwrap();
    (dir, cat)
}

#[test]
#[ignore = "waiting for P5b-1"]
fn report_json_counts_converted_photos_per_flag() {
    let (_d, cat) = fixture();
    let (_, v, _) = run_report(&cat);
    let f = &v["photos"]["flags"];
    assert_eq!(f["pick"], flag_edge::PICK);
    assert_eq!(f["reject"], flag_edge::REJECT);
    assert_eq!(f["none"], flag_edge::NONE);
    assert_eq!(f["unknown"], flag_edge::UNKNOWN);
    assert_eq!(f.as_object().unwrap().len(), 4, "no other fields: {f}");
}

#[test]
#[ignore = "waiting for P5b-1"]
fn flag_counts_add_up_to_converted() {
    let (_d, cat) = fixture();
    let (_, v, _) = run_report(&cat);
    let f = &v["photos"]["flags"];
    let sum: i64 = ["pick", "reject", "none", "unknown"]
        .iter()
        .map(|k| f[*k].as_i64().unwrap())
        .sum();
    assert_eq!(sum, v["photos"]["converted"].as_i64().unwrap());
    assert_eq!(sum, flag_edge::CONVERTED);
}

#[test]
#[ignore = "waiting for P5b-1"]
fn skipped_photos_with_flags_are_not_counted() {
    // Photo 20 (virtual copy, pick), photo 2 (missing, reject) and photo 3
    // (missing, unknown value) are skipped and must not show up in the counts.
    let (_d, cat) = fixture();
    let (_, v, _) = run_report(&cat);
    assert_eq!(
        v["photos"]["skipped"].as_array().unwrap().len() as i64,
        flag_edge::SKIPPED
    );
    assert_eq!(v["photos"]["flags"]["pick"], flag_edge::PICK);
    assert_eq!(v["photos"]["flags"]["reject"], flag_edge::REJECT);
    assert_eq!(v["photos"]["flags"]["unknown"], flag_edge::UNKNOWN);
}

#[test]
#[ignore = "waiting for P5b-1"]
fn stdout_has_the_flags_line() {
    let (_d, cat) = fixture();
    let (text, _, _) = run_report(&cat);
    let want = format!(
        "flags: pick {}, reject {}, none {}, unknown {}",
        flag_edge::PICK,
        flag_edge::REJECT,
        flag_edge::NONE,
        flag_edge::UNKNOWN
    );
    assert_eq!(
        text.lines().filter(|l| l.trim() == want).count(),
        1,
        "stdout: {text}"
    );
}

#[test]
#[ignore = "waiting for P5b-1"]
fn photo_balance_is_unchanged() {
    let (_d, cat) = fixture();
    let (text, v, _) = run_report(&cat);
    let want = format!(
        "photos: read {}, would convert {}, skipped {}",
        flag_edge::PHOTOS,
        flag_edge::CONVERTED,
        flag_edge::SKIPPED
    );
    assert!(text.lines().any(|l| l.trim() == want), "stdout: {text}");
    let p = &v["photos"];
    assert_eq!(p["read"], flag_edge::PHOTOS);
    assert_eq!(p["converted"], flag_edge::CONVERTED);
    let skipped = p["skipped"].as_array().unwrap();
    assert_eq!(skipped.len() as i64, flag_edge::SKIPPED);
    // Unknown flag values add no skip reason: only the virtual copy and the
    // two missing originals are skipped.
    let mut ids: Vec<String> = skipped
        .iter()
        .map(|s| {
            s["reason"]
                .as_str()
                .unwrap()
                .split(':')
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    ids.sort();
    assert_eq!(ids, ["photo 2", "photo 20", "photo 3"]);
    for id in flag_edge::UNKNOWN_IDS {
        assert!(!ids.contains(&format!("photo {id}")));
    }
}

#[test]
#[ignore = "waiting for P5b-1"]
fn catalog_without_flags_prints_a_zero_line() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_small_catalog_with_files(dir.path()).unwrap();
    let (text, v, _) = run_report(&cat);
    let converted = v["photos"]["converted"].as_i64().unwrap();
    assert!(converted > 0 && converted < small::PHOTOS);
    let f = &v["photos"]["flags"];
    assert_eq!(f["pick"], 0);
    assert_eq!(f["reject"], 0);
    assert_eq!(f["unknown"], 0);
    assert_eq!(f["none"], converted);
    let want = format!("flags: pick 0, reject 0, none {converted}, unknown 0");
    assert!(text.lines().any(|l| l.trim() == want), "stdout: {text}");
}

#[test]
#[ignore = "waiting for P5b-1"]
fn catalog_unchanged_second_run_identical_and_nothing_written() {
    let (_d, cat) = fixture();
    let before = fs::read(&cat).unwrap();
    let dir = cat.parent().unwrap().to_path_buf();
    let listing = |p: &Path| {
        let mut v: Vec<_> = fs::read_dir(p)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        v.sort();
        v
    };
    let files_before = listing(&dir);
    let (text1, v1, raw1) = run_report(&cat);
    let (text2, v2, raw2) = run_report(&cat);
    assert_eq!(text1, text2);
    assert_eq!(v1, v2);
    assert_eq!(raw1, raw2);
    assert_eq!(v1["catalog_unchanged"], true);
    assert_eq!(v1["dry_run"], true);
    assert_eq!(before, fs::read(&cat).unwrap());
    assert_eq!(files_before, listing(&dir));
}

#[test]
fn apply_is_still_refused_and_writes_nothing() {
    let (_d, cat) = fixture();
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
