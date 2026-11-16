//! Acceptance tests for catalog detection by file identity.
//!
//! A hard link to the catalog has a different path but is the same file.
//! `--report` pointing at it must be refused with the reason "catalog"
//! (not merely "exists"); a hard link to any other non-empty file must
//! keep the "exists" reason (no false alarms).

#![cfg(unix)]

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run(cwd: &Path, args: &[&OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn with_files() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_small_catalog_with_files(dir.path()).unwrap();
    (dir, cat)
}

fn find_photo(dir: &Path) -> Option<PathBuf> {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            if let Some(f) = find_photo(&p) {
                return Some(f);
            }
        } else if p.extension().is_none_or(|x| x != "lrcat") {
            return Some(p);
        }
    }
    None
}

/// Runs with `--report target`; expects exit 2, `word` in stderr, and the
/// catalog bytes unchanged.
fn assert_refused(cwd: &Path, cat: &Path, target: &Path, guard: &Path, word: &str) {
    let before = fs::read(cat).unwrap();
    let guard_before = fs::read(guard).unwrap();
    let out = run(
        cwd,
        &[
            cat.as_os_str(),
            OsStr::new("--anonymize"),
            OsStr::new("--report"),
            target.as_os_str(),
        ],
    );
    assert_eq!(out.status.code(), Some(2), "stderr: {}", stderr(&out));
    assert!(stderr(&out).contains(word), "stderr: {}", stderr(&out));
    assert_eq!(fs::read(cat).unwrap(), before, "catalog was modified");
    assert_eq!(fs::read(guard).unwrap(), guard_before, "file was modified");
}

#[test]
fn hard_link_to_catalog_as_report_is_refused_as_catalog() {
    let (dir, cat) = with_files();
    let link = dir.path().join("hardlink.json");
    fs::hard_link(&cat, &link).unwrap();
    assert_refused(dir.path(), &cat, &link, &link, "catalog");
}

#[test]
fn hard_link_to_photo_as_report_is_refused_as_exists() {
    let (dir, cat) = with_files();
    let photo = find_photo(dir.path()).expect("fixture has photo files");
    if fs::metadata(&photo).unwrap().len() == 0 {
        fs::write(&photo, b"not empty\n").unwrap();
    }
    let link = dir.path().join("photo-link.json");
    fs::hard_link(&photo, &link).unwrap();
    assert_refused(dir.path(), &cat, &link, &link, "exists");
    let out = run(
        dir.path(),
        &[
            cat.as_os_str(),
            OsStr::new("--anonymize"),
            OsStr::new("--report"),
            link.as_os_str(),
        ],
    );
    assert!(
        !stderr(&out).contains("catalog"),
        "false alarm: {}",
        stderr(&out)
    );
}

#[test]
fn hard_link_to_catalog_in_subdirectory_is_refused_as_catalog() {
    let (dir, cat) = with_files();
    let sub = dir.path().join("elsewhere");
    fs::create_dir(&sub).unwrap();
    let link = sub.join("report.json");
    fs::hard_link(&cat, &link).unwrap();
    assert_refused(dir.path(), &cat, &link, &link, "catalog");
}
