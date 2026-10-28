//! Acceptance test: a catalog without a table the readers
//! need ends the run with exit 1 (read error), names the table on stderr
//! and writes no report.

use std::process::Command;

#[test]
fn missing_collection_table_exits_1_without_report() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dir.path().join("damaged.lrcat");
    lrcat_fixtures::write_small_catalog_without(&cat, "AgLibraryCollection", None).unwrap();
    let report = dir.path().join("report.json");
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(dir.path())
        .arg(&cat)
        .arg("--report")
        .arg(&report)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {err}");
    assert!(err.contains("AgLibraryCollection"), "stderr: {err}");
    assert!(!report.exists(), "no report may be written");
}

#[test]
fn missing_synonym_table_exits_1_without_report() {
    // the synonym table is required too.
    let dir = tempfile::tempdir().unwrap();
    let cat = dir.path().join("damaged.lrcat");
    lrcat_fixtures::write_small_catalog_without(&cat, "AgLibraryKeywordSynonym", None).unwrap();
    let report = dir.path().join("report.json");
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(dir.path())
        .arg(&cat)
        .arg("--report")
        .arg(&report)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {err}");
    assert!(err.contains("AgLibraryKeywordSynonym"), "stderr: {err}");
    assert!(!report.exists(), "no report may be written");
}
