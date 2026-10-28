//! Acceptance tests for keyword attribute counts in the CLI dry run.
//!
//! Expected behaviour of `lr2lc <CATALOG> [--report FILE]`:
//! - report.json: the `keywords` object keeps `read`, `converted`,
//!   `skipped` and gains six integer fields (flattened, like
//!   `photos.videos`): `persons`, `not_exported`, `no_parents_on_export`,
//!   `no_synonyms_on_export`, `unknown_type`, `unused`. They count converted
//!   keywords only.
//! - stdout has one line (always, also with zeros):
//!   `keyword attributes: persons P, not exported E, no parents on export N,
//!   no synonyms on export S, unknown type U, unused X` (single line).
//! - `unused` = converted keywords without any converted assignment (an
//!   assignment to a skipped photo does not count). Unused keywords stay in
//!   the converted count.
//! - Only numbers; no keyword names appear in stdout or report.json.

use lrcat_fixtures::{keyword_attr, small, write_keyword_attr_catalog_with_files};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn run_report(cat: &Path, extra: &[&str]) -> (String, serde_json::Value, String) {
    let cwd = tempfile::tempdir().unwrap();
    let report = cwd.path().join("report.json");
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd.path())
        .arg(cat)
        .arg("--report")
        .arg(&report)
        .args(extra)
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

fn attrs() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_keyword_attr_catalog_with_files(dir.path()).unwrap();
    (dir, cat)
}

#[test]
fn report_json_has_the_attribute_counts() {
    let (_d, cat) = attrs();
    let (_, v, _) = run_report(&cat, &[]);
    let k = &v["keywords"];
    assert_eq!(k["persons"], keyword_attr::PERSONS);
    assert_eq!(k["not_exported"], keyword_attr::NOT_EXPORTED);
    assert_eq!(k["no_parents_on_export"], keyword_attr::NO_PARENTS);
    assert_eq!(k["no_synonyms_on_export"], keyword_attr::NO_SYNONYMS);
    assert_eq!(k["unknown_type"], keyword_attr::UNKNOWN_TYPE);
    assert_eq!(k["unused"], keyword_attr::UNUSED);
}

#[test]
fn existing_keyword_fields_are_unchanged() {
    let (_d, cat) = attrs();
    let (_, v, _) = run_report(&cat, &[]);
    let k = &v["keywords"];
    assert_eq!(k["read"], keyword_attr::KEYWORDS);
    // Unused keywords are converted; the skipped ones are 12 and 28.
    assert_eq!(k["converted"], keyword_attr::KEYWORDS_CONVERTED);
    assert_eq!(k["skipped"].as_array().unwrap().len(), 2);
    assert_eq!(v["keyword_assignments"]["read"], keyword_attr::ASSIGNMENTS);
}

#[test]
fn small_catalog_has_zero_counts() {
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_small_catalog_with_files(dir.path()).unwrap();
    let (text, v, _) = run_report(&cat, &[]);
    let k = &v["keywords"];
    for field in [
        "persons",
        "not_exported",
        "no_parents_on_export",
        "no_synonyms_on_export",
        "unknown_type",
    ] {
        assert_eq!(k[field], 0, "{field}");
    }
    // Only small keyword 11 is unused.
    assert_eq!(k["unused"], 1);
    assert_eq!(k["read"], small::KEYWORDS);
    assert!(
        text.lines().any(|l| l.trim()
            == "keyword attributes: persons 0, not exported 0, no parents on export 0, \
                no synonyms on export 0, unknown type 0, unused 1"),
        "stdout: {text}"
    );
}

#[test]
fn stdout_has_the_attribute_line() {
    let (_d, cat) = attrs();
    let (text, _, _) = run_report(&cat, &[]);
    let want = format!(
        "keyword attributes: persons {}, not exported {}, no parents on export {}, \
         no synonyms on export {}, unknown type {}, unused {}",
        keyword_attr::PERSONS,
        keyword_attr::NOT_EXPORTED,
        keyword_attr::NO_PARENTS,
        keyword_attr::NO_SYNONYMS,
        keyword_attr::UNKNOWN_TYPE,
        keyword_attr::UNUSED
    );
    assert!(text.lines().any(|l| l.trim() == want), "stdout: {text}");
}

#[test]
fn keyword_names_never_appear_in_output() {
    let (_d, cat) = attrs();
    let (text, _, raw) = run_report(&cat, &[]);
    for secret in keyword_attr::NAMES
        .iter()
        .chain(&["Zyx-Attr", "weird", "zyx-kind"])
    {
        assert!(!text.contains(secret), "stdout leaks {secret:?}");
        assert!(!raw.contains(secret), "report leaks {secret:?}");
    }
}

#[test]
fn dry_run_leaves_catalog_unchanged_and_second_run_agrees() {
    let (_d, cat) = attrs();
    let before = fs::read(&cat).unwrap();
    let (_, v1, _) = run_report(&cat, &[]);
    let (_, v2, _) = run_report(&cat, &[]);
    assert_eq!(fs::read(&cat).unwrap(), before, "catalog bytes changed");
    assert_eq!(v1["catalog_unchanged"], true);
    assert_eq!(v1["dry_run"], true);
    assert_eq!(v1["keywords"], v2["keywords"]);
}

#[test]
fn keyword_numbers_add_up() {
    let (_d, cat) = attrs();
    let (_, v, _) = run_report(&cat, &[]);
    let k = &v["keywords"];
    let skipped = k["skipped"].as_array().unwrap().len() as i64;
    assert_eq!(
        k["read"].as_i64().unwrap(),
        k["converted"].as_i64().unwrap() + skipped
    );
    // The attribute counts never exceed the converted keywords.
    for field in [
        "persons",
        "not_exported",
        "no_parents_on_export",
        "no_synonyms_on_export",
        "unknown_type",
        "unused",
    ] {
        assert!(k[field].as_i64().unwrap() <= k["converted"].as_i64().unwrap());
    }
}
