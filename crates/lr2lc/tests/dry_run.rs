//! Acceptance tests for the CLI dry run.
//!
//! Expected behaviour of `lr2lc [OPTIONS] <CATALOG>`:
//! - Options: `--map OLD=NEW` (repeatable, invalid -> exit 2), `--report FILE`
//!   (writes report.json there; without it no file is written), `--apply`
//!   (needs `--library`; without it: exit 2, nothing read or written), `-h`, `-V`.
//! - stdout contains a line `folders: read N, would convert N, skipped N`, a
//!   line `photos: read N, would convert N, skipped N`, a line
//!   `collections: read N, would convert N, skipped N` (sets and
//!   plain collections are converted, the rest is skipped), and the words
//!   "dry run".
//! - report.json: `{"tool_version", "catalog_version", "dry_run": true,
//!   "catalog_unchanged": true, "folders": {"read", "converted", "skipped":
//!   [{"reason"}]}, "photos": {same}, "collections": {same}}` with read == converted + len(skipped).
//! - Exit 0 even when items are skipped; 1 on runtime errors (missing
//!   catalog, pending Lightroom changes); 2 on invalid arguments.

use lrcat_fixtures::small;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const COLLECTIONS: i64 = small::COLLECTIONS;
/// Small fixture: set 1, albums 2, 3, 4 and the quick collection 7 are
/// converted; smart 5 and 6, quick 8 ("more than one") and unknown 9 are
/// skipped.
const COLLECTIONS_OK: i64 = 5;
const COLLECTIONS_SKIPPED: i64 = COLLECTIONS - COLLECTIONS_OK;

const PHOTOS_OK: i64 = small::PHOTOS - small::MISSING_PHOTO_IDS.len() as i64;
const PHOTOS_MISSING: i64 = small::MISSING_PHOTO_IDS.len() as i64;

fn run(cwd: &Path, args: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap()
}

fn os(s: &str) -> &std::ffi::OsStr {
    std::ffi::OsStr::new(s)
}

/// Sorted names of all entries below `dir`, recursively, with file sizes.
fn tree(dir: &Path) -> Vec<(String, u64)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, u64)>) {
        for e in fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
            let meta = e.metadata().unwrap();
            out.push((rel, if meta.is_file() { meta.len() } else { 0 }));
            if meta.is_dir() {
                walk(base, &p, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn with_files() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_small_catalog_with_files(dir.path()).unwrap();
    (dir, cat)
}

fn count_line(text: &str, area: &str, read: i64, conv: i64, skipped: i64) -> bool {
    let want = format!("{area}: read {read}, would convert {conv}, skipped {skipped}");
    text.lines().any(|l| l.trim() == want)
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

#[test]
fn dry_run_exits_zero_and_prints_counts() {
    let (dir, cat) = with_files();
    let cwd = tempfile::tempdir().unwrap();
    let out = run(cwd.path(), &[cat.as_os_str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        count_line(&text, "folders", small::FOLDERS, small::FOLDERS, 0),
        "stdout: {text}"
    );
    assert!(
        count_line(&text, "photos", small::PHOTOS, PHOTOS_OK, PHOTOS_MISSING),
        "stdout: {text}"
    );
    assert!(
        count_line(
            &text,
            "collections",
            COLLECTIONS,
            COLLECTIONS_OK,
            COLLECTIONS_SKIPPED
        ),
        "stdout: {text}"
    );
    assert!(text.to_lowercase().contains("dry run"), "stdout: {text}");
    drop(dir);
}

#[test]
fn report_file_has_documented_structure_and_balance() {
    let (dir, cat) = with_files();
    let cwd = tempfile::tempdir().unwrap();
    let report = cwd.path().join("report.json");
    let out = run(
        cwd.path(),
        &[
            cat.as_os_str(),
            os("--anonymize"),
            os("--report"),
            report.as_os_str(),
        ],
    );
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let v: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(v["tool_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["catalog_version"], "1500000");
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["catalog_unchanged"], true);
    check_area(&v, "folders", small::FOLDERS, small::FOLDERS, 0);
    check_area(&v, "photos", small::PHOTOS, PHOTOS_OK, PHOTOS_MISSING);
    // Set 1, albums 2..=4 and quick collection 7 are converted.
    check_area(
        &v,
        "collections",
        COLLECTIONS,
        COLLECTIONS_OK,
        COLLECTIONS_SKIPPED,
    );
    let reasons: Vec<&str> = v["collections"]["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["reason"].as_str().unwrap())
        .collect();
    for name in [
        "Set Alpha",
        "Coll Beta",
        "Smart Epsilon",
        "Sys Eta",
        "Odd Iota",
    ] {
        assert!(
            reasons.iter().all(|r| !r.contains(name)),
            "name {name} leaked"
        );
    }
    drop(dir);
}

#[test]
fn report_does_not_leak_paths() {
    let (dir, cat) = with_files();
    let cwd = tempfile::tempdir().unwrap();
    let report = cwd.path().join("report.json");
    run(
        cwd.path(),
        &[
            cat.as_os_str(),
            os("--anonymize"),
            os("--report"),
            report.as_os_str(),
        ],
    );
    let text = fs::read_to_string(&report).unwrap();
    assert!(!text.contains(small::MACOS_ROOT_DIR), "report: {text}");
    assert!(!text.contains(small::WINDOWS_ROOT_DIR), "report: {text}");
    drop(dir);
}

#[test]
fn without_report_option_no_file_is_created() {
    let (dir, cat) = with_files();
    let cwd = tempfile::tempdir().unwrap();
    let cat_before = tree(dir.path());
    let out = run(cwd.path(), &[cat.as_os_str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(tree(dir.path()), cat_before, "catalog folder changed");
    assert!(tree(cwd.path()).is_empty(), "working directory changed");
}

#[test]
fn catalog_checksum_is_unchanged() {
    let (dir, cat) = with_files();
    let cwd = tempfile::tempdir().unwrap();
    let before = lr_reader_checksum(&cat);
    let report = cwd.path().join("r.json");
    run(
        cwd.path(),
        &[
            cat.as_os_str(),
            os("--anonymize"),
            os("--report"),
            report.as_os_str(),
        ],
    );
    assert_eq!(before, lr_reader_checksum(&cat));
    drop(dir);
}

/// Byte-level comparison is enough here; avoids a dependency on lr-reader.
fn lr_reader_checksum(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}

#[test]
fn second_run_gives_identical_numbers() {
    let (dir, cat) = with_files();
    let cwd = tempfile::tempdir().unwrap();
    let a = stdout(&run(cwd.path(), &[cat.as_os_str()]));
    let b = stdout(&run(cwd.path(), &[cat.as_os_str()]));
    assert_eq!(a, b);
    drop(dir);
}

#[test]
fn apply_without_library_fails_and_writes_nothing() {
    let (dir, cat) = with_files();
    let cwd = tempfile::tempdir().unwrap();
    let report = cwd.path().join("report.json");
    let before = tree(dir.path());
    let out = run(
        cwd.path(),
        &[
            cat.as_os_str(),
            os("--apply"),
            os("--anonymize"),
            os("--report"),
            report.as_os_str(),
        ],
    );
    assert!(!out.status.success(), "--apply without --library must fail");
    assert!(
        !stderr(&out).trim().is_empty(),
        "needs an explanatory message"
    );
    assert!(!report.exists(), "no report may be written");
    assert!(tree(cwd.path()).is_empty());
    assert_eq!(tree(dir.path()), before);
}

#[test]
fn missing_catalog_exits_one() {
    let cwd = tempfile::tempdir().unwrap();
    let nope = cwd.path().join("does-not-exist.lrcat");
    let out = run(cwd.path(), &[nope.as_os_str()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(!stderr(&out).trim().is_empty());
    assert!(tree(cwd.path()).is_empty());
}

#[test]
fn pending_wal_exits_one_and_mentions_lightroom() {
    let (dir, cat) = with_files();
    let cwd = tempfile::tempdir().unwrap();
    let mut name = cat.file_name().unwrap().to_os_string();
    name.push("-wal");
    fs::write(cat.with_file_name(name), b"pending").unwrap();
    let before = tree(dir.path());
    let out = run(cwd.path(), &[cat.as_os_str()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("Lightroom"),
        "stderr: {}",
        stderr(&out)
    );
    assert_eq!(tree(dir.path()), before);
}

/// Moves both photo roots from the catalog directory into a new directory.
fn move_photos(from: &Path) -> tempfile::TempDir {
    let to = tempfile::tempdir().unwrap();
    for d in [small::MACOS_ROOT_DIR, small::WINDOWS_ROOT_DIR] {
        fs::rename(from.join(d), to.path().join(d)).unwrap();
    }
    to
}

#[test]
fn without_mapping_moved_photos_are_all_skipped() {
    let (dir, cat) = with_files();
    let _moved = move_photos(dir.path());
    let cwd = tempfile::tempdir().unwrap();
    let out = run(cwd.path(), &[cat.as_os_str()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        count_line(&text, "photos", small::PHOTOS, 0, small::PHOTOS),
        "stdout: {text}"
    );
}

#[test]
fn map_option_finds_moved_photos() {
    let (dir, cat) = with_files();
    let moved = move_photos(dir.path());
    let cwd = tempfile::tempdir().unwrap();
    let spec = format!("{}={}", dir.path().display(), moved.path().display());
    let out = run(cwd.path(), &[cat.as_os_str(), os("--map"), os(&spec)]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        count_line(&text, "photos", small::PHOTOS, PHOTOS_OK, PHOTOS_MISSING),
        "stdout: {text}"
    );
    assert!(
        count_line(&text, "folders", small::FOLDERS, small::FOLDERS, 0),
        "stdout: {text}"
    );
}

#[test]
fn map_option_can_be_repeated() {
    let (dir, cat) = with_files();
    let moved = move_photos(dir.path());
    let cwd = tempfile::tempdir().unwrap();
    let a = format!(
        "{}={}",
        dir.path().join(small::MACOS_ROOT_DIR).display(),
        moved.path().join(small::MACOS_ROOT_DIR).display()
    );
    let b = format!(
        "{}={}",
        dir.path().join(small::WINDOWS_ROOT_DIR).display(),
        moved.path().join(small::WINDOWS_ROOT_DIR).display()
    );
    let out = run(
        cwd.path(),
        &[cat.as_os_str(), os("--map"), os(&a), os("--map"), os(&b)],
    );
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(
        count_line(
            &stdout(&out),
            "photos",
            small::PHOTOS,
            PHOTOS_OK,
            PHOTOS_MISSING
        ),
        "stdout: {}",
        stdout(&out)
    );
}

#[test]
fn invalid_map_exits_two() {
    let (dir, cat) = with_files();
    let cwd = tempfile::tempdir().unwrap();
    let out = run(
        cwd.path(),
        &[cat.as_os_str(), os("--map"), os("no-equals-sign")],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(tree(cwd.path()).is_empty());
    drop(dir);
}

#[test]
fn skipped_reasons_in_report_do_not_name_paths() {
    let (dir, cat) = with_files();
    let cwd = tempfile::tempdir().unwrap();
    let report = cwd.path().join("report.json");
    run(
        cwd.path(),
        &[
            cat.as_os_str(),
            os("--anonymize"),
            os("--report"),
            report.as_os_str(),
        ],
    );
    let v: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    for s in v["photos"]["skipped"].as_array().unwrap() {
        let r = s["reason"].as_str().unwrap();
        assert!(!r.contains(dir.path().to_str().unwrap()), "reason: {r}");
    }
}

// --- Empty mapping target ---------------------------------------
// The CLI rejects an empty or whitespace-only new prefix: exit 2, stderr
// mentions "empty", nothing is read or written. The library keeps allowing it.

#[test]
fn map_with_empty_target_exits_two_and_mentions_empty() {
    let (dir, cat) = with_files();
    let cwd = tempfile::tempdir().unwrap();
    let cat_before = tree(dir.path());
    let out = run(cwd.path(), &[cat.as_os_str(), os("--map"), os("/x=")]);
    assert_eq!(out.status.code(), Some(2), "stderr: {}", stderr(&out));
    assert!(
        stderr(&out).to_lowercase().contains("empty"),
        "stderr: {}",
        stderr(&out)
    );
    assert!(stdout(&out).trim().is_empty(), "nothing may be reported");
    assert!(tree(cwd.path()).is_empty());
    assert_eq!(tree(dir.path()), cat_before, "catalog folder changed");
}

#[test]
fn map_with_whitespace_only_target_exits_two() {
    let (dir, cat) = with_files();
    let cwd = tempfile::tempdir().unwrap();
    let out = run(cwd.path(), &[cat.as_os_str(), os("--map"), os("/x=   ")]);
    assert_eq!(out.status.code(), Some(2), "stderr: {}", stderr(&out));
    assert!(stderr(&out).to_lowercase().contains("empty"));
    assert!(tree(cwd.path()).is_empty());
    drop(dir);
}

#[test]
fn map_with_empty_target_writes_no_report_file() {
    let (dir, cat) = with_files();
    let cwd = tempfile::tempdir().unwrap();
    let report = cwd.path().join("report.json");
    let out = run(
        cwd.path(),
        &[
            cat.as_os_str(),
            os("--map"),
            os("/x="),
            os("--anonymize"),
            os("--report"),
            report.as_os_str(),
        ],
    );
    assert_eq!(out.status.code(), Some(2), "stderr: {}", stderr(&out));
    assert!(!report.exists(), "report file must not be created");
    assert!(tree(cwd.path()).is_empty());
    drop(dir);
}

#[test]
fn map_with_empty_target_among_valid_maps_is_still_rejected() {
    let (dir, cat) = with_files();
    let cwd = tempfile::tempdir().unwrap();
    let out = run(
        cwd.path(),
        &[
            cat.as_os_str(),
            os("--map"),
            os("/a=/b"),
            os("--map"),
            os("/x="),
        ],
    );
    assert_eq!(out.status.code(), Some(2), "stderr: {}", stderr(&out));
    drop(dir);
}

#[test]
fn map_with_non_empty_target_stays_valid() {
    let (dir, cat) = with_files();
    let cwd = tempfile::tempdir().unwrap();
    let out = run(cwd.path(), &[cat.as_os_str(), os("--map"), os("/x=/y")]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("dry run"));
    drop(dir);
}
