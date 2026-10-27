//! Acceptance tests for keywords and ratings in the CLI dry run (task P5-2).
//!
//! Fixture: `write_keyword_edge_catalog_with_files` (small fixture plus
//! keyword edge cases; photos 2, 3 and 20 have missing originals).
//!
//! Expected behaviour of `lr2lc <CATALOG> [--report FILE]`:
//! - report.json gains `keywords`, `keyword_assignments` (same shape as
//!   `photos`: `{"read", "converted", "skipped": [{"reason"}]}`, balanced) and
//!   `ratings` (keys "0".."5", always all six, sum == `photos.converted`).
//! - Keywords: read 17, converted 12, skipped 5 (ids 12, 14, 16, 17, 18).
//! - Assignments are resolved against the photos that were converted (photos
//!   with missing originals count as skipped photos): read 37, converted 26,
//!   skipped 11 (ids 2, 3, 20, 22, 23, 31..=36).
//! - Ratings (NULL counts as 0) of the 17 converted photos:
//!   0: 6, 1: 3, 2: 2, 3: 3, 4: 2, 5: 1.
//! - stdout has `keywords: read N, would convert M, skipped K`,
//!   `keyword assignments: ...`, `skipped keywords:` / `skipped keyword
//!   assignments:` groups (like the other areas) and a line
//!   `ratings: 0: a, 1: b, 2: c, 3: d, 4: e, 5: f`.
//! - Neither stdout nor report.json contains keyword names or paths.

use lrcat_fixtures::{keyword_edge, small, write_keyword_edge_catalog_with_files};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PHOTOS_OK: i64 = small::PHOTOS - small::MISSING_PHOTO_IDS.len() as i64;
const KEYWORDS_SKIPPED: i64 = keyword_edge::KEYWORDS - keyword_edge::KEYWORDS_CONVERTED;
/// Small fixture: assignments 2, 3, 20, 22, 23 hit photos 2, 3 or 20.
const ASSIGNMENTS_ON_MISSING_PHOTOS: i64 = 5;
/// Extra rows 31..=36 are skipped, 37 is converted.
const ASSIGNMENTS_CONVERTED: i64 = small::ASSIGNMENTS - ASSIGNMENTS_ON_MISSING_PHOTOS + 1;
const ASSIGNMENTS_SKIPPED: i64 = keyword_edge::ASSIGNMENTS - ASSIGNMENTS_CONVERTED;
const RATINGS: [i64; 6] = [6, 3, 2, 3, 2, 1];

/// Names that must never appear in any output.
const SECRET_NAMES: [&str; 10] = [
    "Colosseum",
    "Süßwasser",
    "Köln",
    "🌅",
    "Sunrise",
    "Bad|Name",
    "Alice",
    "Under Empty",
    "Deep Under",
    "Family",
];

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_keyword_edge_catalog_with_files(dir.path()).unwrap();
    (dir, cat)
}

fn run(cwd: &Path, args: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn report_of(cat: &Path) -> (String, serde_json::Value, String) {
    let cwd = tempfile::tempdir().unwrap();
    let report = cwd.path().join("report.json");
    let out = run(
        cwd.path(),
        &[cat.as_os_str(), "--report".as_ref(), report.as_os_str()],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let raw = fs::read_to_string(&report).unwrap();
    let v = serde_json::from_str(&raw).unwrap();
    (stdout(&out), v, raw)
}

fn check_area(v: &serde_json::Value, area: &str, read: i64, conv: i64, skipped: i64) {
    let a = &v[area];
    assert_eq!(a["read"], read, "{area}.read");
    assert_eq!(a["converted"], conv, "{area}.converted");
    let list = a["skipped"]
        .as_array()
        .unwrap_or_else(|| panic!("{area}.skipped"));
    assert_eq!(list.len() as i64, skipped, "{area}.skipped length");
    for s in list {
        assert!(!s["reason"].as_str().unwrap().is_empty());
    }
    assert_eq!(read, conv + list.len() as i64, "{area} balance");
}

fn count_line(text: &str, area: &str, read: i64, conv: i64, skipped: i64) -> bool {
    let want = format!("{area}: read {read}, would convert {conv}, skipped {skipped}");
    text.lines().any(|l| l.trim() == want)
}

fn reasons(v: &serde_json::Value, area: &str) -> Vec<String> {
    v[area]["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["reason"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn report_has_keyword_and_assignment_areas() {
    let (_dir, cat) = fixture();
    let (_, v, _) = report_of(&cat);
    check_area(
        &v,
        "keywords",
        keyword_edge::KEYWORDS,
        keyword_edge::KEYWORDS_CONVERTED,
        KEYWORDS_SKIPPED,
    );
    check_area(
        &v,
        "keyword_assignments",
        keyword_edge::ASSIGNMENTS,
        ASSIGNMENTS_CONVERTED,
        ASSIGNMENTS_SKIPPED,
    );
    // Existing areas are unchanged.
    check_area(&v, "photos", small::PHOTOS, PHOTOS_OK, 3);
}

#[test]
fn skip_reasons_name_ids_only() {
    let (_dir, cat) = fixture();
    let (_, v, _) = report_of(&cat);
    let mut kw = reasons(&v, "keywords");
    kw.sort();
    let mut want = vec![
        "keyword 12: name is empty",
        "keyword 14: name contains a vertical bar",
        "keyword 16: name is empty",
        "keyword 17: parent keyword 16 is skipped",
        "keyword 18: parent keyword 17 is skipped",
    ];
    want.sort();
    assert_eq!(kw, want);
    let asg = reasons(&v, "keyword_assignments");
    for id in [2, 3, 20, 22, 23, 34, 35] {
        assert!(
            asg.iter()
                .any(|r| r.starts_with(&format!("assignment {id}: photo "))),
            "assignment {id} in {asg:?}"
        );
    }
    for id in [31, 32, 36] {
        assert!(
            asg.iter().any(|r| r
                == &format!(
                    "assignment {id}: keyword {} is skipped",
                    match id {
                        31 => 14,
                        32 => 16,
                        _ => 12,
                    }
                )),
            "assignment {id} in {asg:?}"
        );
    }
    // Assignment 33 has a skipped photo and a skipped keyword: one entry.
    assert_eq!(
        asg.iter()
            .filter(|r| r.starts_with("assignment 33: "))
            .count(),
        1
    );
}

#[test]
fn ratings_distribution_in_report() {
    let (_dir, cat) = fixture();
    let (_, v, _) = report_of(&cat);
    let r = v["ratings"].as_object().expect("ratings object");
    assert_eq!(r.len(), 6, "exactly the keys 0..=5: {r:?}");
    for (star, want) in RATINGS.iter().enumerate() {
        assert_eq!(r[&star.to_string()], *want, "ratings.{star}");
    }
    let sum: i64 = r.values().map(|x| x.as_i64().unwrap()).sum();
    assert_eq!(sum, v["photos"]["converted"].as_i64().unwrap());
    assert_eq!(sum, PHOTOS_OK);
}

#[test]
fn text_output_shows_keywords_assignments_and_ratings() {
    let (_dir, cat) = fixture();
    let cwd = tempfile::tempdir().unwrap();
    let out = run(cwd.path(), &[cat.as_os_str()]);
    assert_eq!(out.status.code(), Some(0));
    let text = stdout(&out);
    assert!(
        count_line(
            &text,
            "keywords",
            keyword_edge::KEYWORDS,
            keyword_edge::KEYWORDS_CONVERTED,
            KEYWORDS_SKIPPED
        ),
        "stdout: {text}"
    );
    assert!(
        count_line(
            &text,
            "keyword assignments",
            keyword_edge::ASSIGNMENTS,
            ASSIGNMENTS_CONVERTED,
            ASSIGNMENTS_SKIPPED
        ),
        "stdout: {text}"
    );
    assert!(text.contains("skipped keywords:"), "stdout: {text}");
    assert!(
        text.contains("skipped keyword assignments:"),
        "stdout: {text}"
    );
    let want = format!(
        "ratings: 0: {}, 1: {}, 2: {}, 3: {}, 4: {}, 5: {}",
        RATINGS[0], RATINGS[1], RATINGS[2], RATINGS[3], RATINGS[4], RATINGS[5]
    );
    assert!(text.lines().any(|l| l.trim() == want), "stdout: {text}");
    // Existing lines stay.
    assert!(count_line(&text, "photos", small::PHOTOS, PHOTOS_OK, 3));
    assert!(text.to_lowercase().contains("dry run"));
}

#[test]
fn no_keyword_names_or_paths_in_any_output() {
    let (_dir, cat) = fixture();
    let (text, _, raw) = report_of(&cat);
    for name in SECRET_NAMES {
        assert!(!text.contains(name), "stdout leaks {name:?}: {text}");
        assert!(!raw.contains(name), "report leaks {name:?}");
    }
}

#[test]
fn catalog_unchanged_and_second_run_identical() {
    let (_dir, cat) = fixture();
    let before = fs::read(&cat).unwrap();
    let (text1, v1, _) = report_of(&cat);
    let (text2, v2, _) = report_of(&cat);
    assert_eq!(fs::read(&cat).unwrap(), before, "catalog bytes changed");
    assert_eq!(v1["catalog_unchanged"], true);
    assert_eq!(text1, text2);
    assert_eq!(v1["keywords"], v2["keywords"]);
    assert_eq!(v1["keyword_assignments"], v2["keyword_assignments"]);
    assert_eq!(v1["ratings"], v2["ratings"]);
}

#[test]
fn ratings_sum_matches_photos_on_small_fixture() {
    // All six keys always exist. Here the plain small fixture is used and the
    // sum rule is checked against the photo count; one empty-named keyword is skipped.
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_small_catalog_with_files(dir.path()).unwrap();
    let (_, v, _) = report_of(&cat);
    let r = v["ratings"].as_object().unwrap();
    assert_eq!(r.len(), 6);
    let sum: i64 = r.values().map(|x| x.as_i64().unwrap()).sum();
    assert_eq!(sum, PHOTOS_OK);
    // One keyword has an empty name and is skipped (see the P5-1 reader test).
    check_area(&v, "keywords", small::KEYWORDS, small::KEYWORDS - 1, 1);
}
