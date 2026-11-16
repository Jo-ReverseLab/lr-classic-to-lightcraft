//! Acceptance tests for the `--report` target.
//!
//! `--report FILE` must never destroy the catalog or any other user file:
//! - FILE is the catalog or its `-wal`/`-journal`/`-shm` companion (also via
//!   another path to the same file) -> exit 2 before reading, stderr
//!   mentions "catalog", the file stays byte-identical.
//! - FILE exists and is not an earlier lr2lc report (JSON object with key
//!   "tool_version") -> exit 2 before reading, stderr contains "exists",
//!   the file stays byte-identical.
//! - FILE does not exist, or is an earlier report -> written, exit 0.

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

fn companion(cat: &Path, suffix: &str) -> PathBuf {
    let mut name = cat.file_name().unwrap().to_os_string();
    name.push(suffix);
    cat.with_file_name(name)
}

/// First placeholder photo file below `dir` (anything but the catalog).
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

/// Runs with `--report target`; expects exit 2 and `word` in stderr, and the
/// protected file `guard` unchanged.
fn assert_refused(cwd: &Path, cat: &Path, target: &OsStr, guard: &Path, word: &str) {
    let before = fs::read(guard).unwrap();
    let out = run(
        cwd,
        &[
            cat.as_os_str(),
            OsStr::new("--anonymize"),
            OsStr::new("--report"),
            target,
        ],
    );
    assert_eq!(out.status.code(), Some(2), "stderr: {}", stderr(&out));
    assert!(stderr(&out).contains(word), "stderr: {}", stderr(&out));
    assert_eq!(fs::read(guard).unwrap(), before, "file was modified");
}

#[test]
fn report_pointing_at_catalog_is_refused() {
    let (dir, cat) = with_files();
    assert_refused(dir.path(), &cat, cat.as_os_str(), &cat, "catalog");
}

#[test]
fn report_pointing_at_wal_is_refused() {
    let (dir, cat) = with_files();
    let wal = companion(&cat, "-wal");
    fs::write(&wal, b"").unwrap();
    assert_refused(dir.path(), &cat, wal.as_os_str(), &wal, "catalog");
}

#[test]
fn report_pointing_at_journal_is_refused() {
    let (dir, cat) = with_files();
    let j = companion(&cat, "-journal");
    fs::write(&j, b"").unwrap();
    assert_refused(dir.path(), &cat, j.as_os_str(), &j, "catalog");
}

#[test]
fn report_pointing_at_shm_is_refused() {
    let (dir, cat) = with_files();
    let shm = companion(&cat, "-shm");
    fs::write(&shm, b"").unwrap();
    assert_refused(dir.path(), &cat, shm.as_os_str(), &shm, "catalog");
}

#[test]
fn report_via_detour_path_to_catalog_is_refused() {
    let (dir, cat) = with_files();
    fs::create_dir(dir.path().join("sub")).unwrap();
    let detour = dir.path().join("sub").join("..").join("small.lrcat");
    assert_refused(dir.path(), &cat, detour.as_os_str(), &cat, "catalog");
}

#[test]
fn relative_report_path_to_catalog_is_refused() {
    let (dir, cat) = with_files();
    assert_refused(dir.path(), &cat, OsStr::new("small.lrcat"), &cat, "catalog");
}

#[cfg(unix)]
#[test]
fn symlink_to_catalog_as_report_is_refused() {
    let (dir, cat) = with_files();
    let link = dir.path().join("link.json");
    std::os::unix::fs::symlink(&cat, &link).unwrap();
    assert_refused(dir.path(), &cat, link.as_os_str(), &cat, "catalog");
}

#[test]
fn report_pointing_at_photo_is_refused() {
    let (dir, cat) = with_files();
    let photo = find_photo(dir.path()).expect("fixture has photo files");
    assert_refused(dir.path(), &cat, photo.as_os_str(), &photo, "exists");
}

#[test]
fn report_pointing_at_arbitrary_text_file_is_refused() {
    let (dir, cat) = with_files();
    let other = dir.path().join("notes.txt");
    fs::write(&other, b"my precious notes\n").unwrap();
    assert_refused(dir.path(), &cat, other.as_os_str(), &other, "exists");
}

#[test]
fn report_pointing_at_json_without_tool_version_is_refused() {
    let (dir, cat) = with_files();
    let other = dir.path().join("data.json");
    fs::write(&other, br#"{"hello": 1}"#).unwrap();
    assert_refused(dir.path(), &cat, other.as_os_str(), &other, "exists");
}

#[test]
fn report_pointing_at_json_array_is_refused() {
    let (dir, cat) = with_files();
    let other = dir.path().join("list.json");
    fs::write(&other, br#"["tool_version"]"#).unwrap();
    assert_refused(dir.path(), &cat, other.as_os_str(), &other, "exists");
}

#[test]
fn new_report_path_is_written() {
    let (dir, cat) = with_files();
    let report = dir.path().join("new-report.json");
    let out = run(
        dir.path(),
        &[
            cat.as_os_str(),
            OsStr::new("--anonymize"),
            OsStr::new("--report"),
            report.as_os_str(),
        ],
    );
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let v: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert!(v["tool_version"].is_string());
}

#[test]
fn earlier_report_is_overwritten_by_second_run() {
    let (dir, cat) = with_files();
    let report = dir.path().join("report.json");
    let args = [
        cat.as_os_str(),
        OsStr::new("--anonymize"),
        OsStr::new("--report"),
        report.as_os_str(),
    ];
    assert_eq!(run(dir.path(), &args).status.code(), Some(0));
    // Corrupt the content but keep it a valid earlier report.
    fs::write(&report, br#"{"tool_version": "old", "marker": true}"#).unwrap();
    let out = run(dir.path(), &args);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let v: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert!(v.get("marker").is_none(), "report was not overwritten");
    assert!(v["tool_version"].is_string());
    assert_eq!(v["dry_run"], true);
}
