//! Acceptance tests for keyword synonyms in the CLI dry run (synonyms are counted, not converted).
//!
//! Expected behaviour of `lr2lc <CATALOG> [--report FILE]` (synonyms are converted with their keyword):
//! - report.json gains `keyword_synonyms` (`{"read", "converted", "skipped":
//!   [{"reason"}]}`, read == converted + len(skipped)).
//! - Keyword edge catalog: read 6, converted 2 (both on keyword 3), skipped 4:
//!   `synonym 3: keyword 16 is skipped`, `synonym 4: keyword 999 is unknown`,
//!   `synonym 5: name is empty`, `synonym 6: name is empty`.
//! - Small catalog: 0 / 0 / 0.
//! - stdout has `keyword synonyms: read N, would convert M, skipped K` and,
//!   if K > 0, a group `skipped keyword synonyms:`.
//! - Neither stdout nor report.json contains synonym texts.
//! - Other areas stay unchanged.

use lrcat_fixtures::{keyword_edge, small, write_keyword_edge_catalog_with_files};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn run_report(cat: &Path) -> (String, serde_json::Value, String) {
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

fn edge() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_keyword_edge_catalog_with_files(dir.path()).unwrap();
    (dir, cat)
}

fn reasons(v: &serde_json::Value) -> Vec<String> {
    v["keyword_synonyms"]["skipped"]
        .as_array()
        .expect("keyword_synonyms.skipped")
        .iter()
        .map(|s| s["reason"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn report_counts_converted_and_skipped_synonyms() {
    let (_dir, cat) = edge();
    let (_, v, _) = run_report(&cat);
    let a = &v["keyword_synonyms"];
    assert_eq!(a["read"], keyword_edge::SYNONYMS);
    assert_eq!(a["converted"], keyword_edge::SYNONYMS_CONVERTED);
    assert_eq!(
        a["skipped"].as_array().unwrap().len() as i64,
        keyword_edge::SYNONYMS - keyword_edge::SYNONYMS_CONVERTED
    );
    assert_eq!(
        reasons(&v),
        vec![
            "synonym 3: keyword 16 is skipped",
            "synonym 4: keyword 999 is unknown",
            "synonym 5: name is empty",
            "synonym 6: name is empty",
        ]
    );
}

#[test]
fn small_catalog_has_zero_synonyms() {
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_small_catalog_with_files(dir.path()).unwrap();
    let (text, v, _) = run_report(&cat);
    let a = &v["keyword_synonyms"];
    assert_eq!(a["read"], 0);
    assert_eq!(a["converted"], 0);
    assert_eq!(a["skipped"].as_array().expect("skipped array").len(), 0);
    assert!(
        text.lines()
            .any(|l| l.trim() == "keyword synonyms: read 0, would convert 0, skipped 0"),
        "stdout: {text}"
    );
}

#[test]
fn text_output_shows_counts_and_skipped_group() {
    let (_dir, cat) = edge();
    let (text, _, _) = run_report(&cat);
    let want = format!(
        "keyword synonyms: read {}, would convert {}, skipped {}",
        keyword_edge::SYNONYMS,
        keyword_edge::SYNONYMS_CONVERTED,
        keyword_edge::SYNONYMS - keyword_edge::SYNONYMS_CONVERTED
    );
    assert!(text.lines().any(|l| l.trim() == want), "stdout: {text}");
    assert!(text.contains("skipped keyword synonyms:"), "stdout: {text}");
    // The text output groups reasons without the "<kind> <id>: " prefix in
    // every area; the full reason is checked in report.json elsewhere.
    assert!(text.contains("keyword 999 is unknown"), "stdout: {text}");
}

#[test]
fn synonym_texts_never_appear_in_output() {
    let (_dir, cat) = edge();
    let (text, _, raw) = run_report(&cat);
    for secret in keyword_edge::SYNONYM_TEXTS.iter().chain(&["Zyx", "zyx"]) {
        assert!(!text.contains(secret), "stdout leaks {secret:?}");
        assert!(!raw.contains(secret), "report leaks {secret:?}");
    }
}

#[test]
fn other_areas_unchanged_and_catalog_untouched() {
    let (_dir, cat) = edge();
    let before = fs::read(&cat).unwrap();
    let (_, v1, _) = run_report(&cat);
    let (_, v2, _) = run_report(&cat);
    assert_eq!(fs::read(&cat).unwrap(), before, "catalog bytes changed");
    assert_eq!(v1["catalog_unchanged"], true);
    assert_eq!(v1["keywords"]["read"], keyword_edge::KEYWORDS);
    assert_eq!(v1["keyword_assignments"]["read"], keyword_edge::ASSIGNMENTS);
    assert_eq!(v1["photos"]["read"], small::PHOTOS);
    // A second run gives the same synonym numbers (no duplicates).
    assert_eq!(v1["keyword_synonyms"], v2["keyword_synonyms"]);
}
