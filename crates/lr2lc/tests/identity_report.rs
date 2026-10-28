//! Acceptance tests for stable identity in the CLI (task P5-8).
//!
//! The dry run never prints or stores global ids (they are internal
//! identity, not report content): neither stdout nor report.json contains
//! them. Skipped rows appear with the reasons `photo {id}: missing global id`,
//! `photo {id}: duplicate global id` and the keyword equivalents; the
//! numbers still add up and nothing is written without `--apply`.

use lrcat_fixtures::{set_keyword_global_id, set_photo_global_id, small};
use std::fs;
use std::path::Path;
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
    assert!(
        fs::read_dir(cwd.path()).unwrap().count() == 1,
        "only the report file may be written"
    );
    let raw = fs::read_to_string(&report).unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        serde_json::from_str(&raw).unwrap(),
        raw,
    )
}

fn reasons(v: &serde_json::Value, area: &str) -> Vec<String> {
    v[area]["skipped"]
        .as_array()
        .unwrap_or_else(|| panic!("{area}.skipped"))
        .iter()
        .map(|s| s["reason"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn global_ids_never_appear_in_output() {
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_small_catalog_with_files(dir.path()).unwrap();
    set_photo_global_id(&cat, 1, "zyx-gid-photo-1").unwrap();
    set_photo_global_id(&cat, 4, "zyx-gid-photo-1").unwrap(); // duplicate
    set_photo_global_id(&cat, 5, "").unwrap();
    set_keyword_global_id(&cat, 2, "zyx-gid-kw-2").unwrap();
    set_keyword_global_id(&cat, 10, "zyx-gid-kw-2").unwrap(); // duplicate
    set_keyword_global_id(&cat, 9, " ").unwrap();
    let (text, v, raw) = run_report(&cat);
    // The machine-readable skip codes legitimately contain "global_id";
    // remove exactly those codes before searching for leaks.
    let codes = ["missing_global_id", "duplicate_global_id", "no_global_id"];
    let strip = |s: &str| {
        codes
            .iter()
            .fold(s.to_string(), |acc, c| acc.replace(c, ""))
    };
    assert!(
        raw.contains("\"missing_global_id\"") && raw.contains("\"duplicate_global_id\""),
        "expected skip codes missing from report.json"
    );
    let (text_s, raw_s) = (strip(&text), strip(&raw));
    for secret in ["zyx-gid", "image-0000", "kw-0000", "global_id", "id_global"] {
        assert!(!text_s.contains(secret), "stdout leaks {secret:?}");
        assert!(!raw_s.contains(secret), "report.json leaks {secret:?}");
    }
    let photos = reasons(&v, "photos");
    assert!(
        photos.contains(&"photo 4: duplicate global id".to_string()),
        "{photos:?}"
    );
    assert!(
        photos.contains(&"photo 5: missing global id".to_string()),
        "{photos:?}"
    );
    let kws = reasons(&v, "keywords");
    assert!(
        kws.contains(&"keyword 10: duplicate global id".to_string()),
        "{kws:?}"
    );
    assert!(
        kws.contains(&"keyword 9: missing global id".to_string()),
        "{kws:?}"
    );
    // The readable summary names the reasons too.
    assert!(text.contains("duplicate global id"), "stdout: {text}");
    assert!(text.contains("missing global id"), "stdout: {text}");
}

#[test]
fn numbers_add_up_and_catalog_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_small_catalog_with_files(dir.path()).unwrap();
    set_photo_global_id(&cat, 5, "").unwrap();
    set_keyword_global_id(&cat, 3, "").unwrap(); // Italy: 3 keywords lost
    let before = fs::read(&cat).unwrap();
    let (_, v1, _) = run_report(&cat);
    let (_, v2, _) = run_report(&cat);
    assert_eq!(fs::read(&cat).unwrap(), before, "catalog bytes changed");
    assert_eq!(v1["catalog_unchanged"], true);
    assert_eq!(v1, v2, "second run must give the same report");
    assert_eq!(v1["photos"]["read"], small::PHOTOS);
    for area in ["photos", "keywords"] {
        let a = &v1[area];
        assert_eq!(
            a["read"].as_i64().unwrap(),
            a["converted"].as_i64().unwrap() + a["skipped"].as_array().unwrap().len() as i64,
            "{area}"
        );
    }
    let kws = reasons(&v1, "keywords");
    assert!(
        kws.contains(&"keyword 3: missing global id".to_string()),
        "{kws:?}"
    );
    assert!(
        kws.contains(&"keyword 4: parent keyword 3 is skipped".to_string()),
        "{kws:?}"
    );
}
