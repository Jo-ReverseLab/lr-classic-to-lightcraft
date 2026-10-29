//! Acceptance tests for opening a catalog safely.
//!
//! Expected API:
//! - `lr_reader::Catalog::open(path: &Path) -> Result<Catalog, ReadError>`
//!   opens strictly read-only (SQLite `mode=ro` / immutable) and checks
//!   `Adobe_DBVersion`. Only `1500000` is accepted; anything else yields
//!   `ReadError::UnsupportedVersion(String)`. A missing file or a file that
//!   is not a SQLite database yields `ReadError::Open(..)`.
//! - `Catalog::version(&self) -> &str`
//! - `lr_reader::checksum(path: &Path) -> std::io::Result<[u8; 32]>`
//!   (SHA-256 of the file).

use lr_reader::{Catalog, ReadError, checksum};
use std::fs;
use std::path::{Path, PathBuf};

fn small_fixture(dir: &Path) -> PathBuf {
    let path = dir.join("small.lrcat");
    lrcat_fixtures::write_small_catalog(&path).expect("generate fixture");
    path
}

#[test]
fn opens_small_fixture_and_reports_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    let catalog = Catalog::open(&path).expect("open");
    assert_eq!(catalog.version(), "1500000");
}

// `Adobe_DBVersion` is stored as text by Lightroom, but the reader accepts
// the same logical value `1500000` stored as a real number or a blob (see
// `catalog.rs` where each `rusqlite::types::Value` arm is handled). These
// tests pin that the generator variants and the reader agree.
#[test]
fn opens_when_dbversion_is_stored_as_real() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    lrcat_fixtures::set_dbversion_type(&path, lrcat_fixtures::DbVersionType::Real)
        .expect("rewrite version as real");
    let catalog = Catalog::open(&path).expect("open with real version");
    assert_eq!(catalog.version(), "1500000");
}

#[test]
fn opens_when_dbversion_is_stored_as_blob() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    lrcat_fixtures::set_dbversion_type(&path, lrcat_fixtures::DbVersionType::Blob)
        .expect("rewrite version as blob");
    let catalog = Catalog::open(&path).expect("open with blob version");
    assert_eq!(catalog.version(), "1500000");
}

#[test]
fn rejects_unsupported_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    // Same-length in-place edit of the stored version string, so no SQLite
    // dependency is needed here.
    let mut bytes = fs::read(&path).unwrap();
    let needle = b"Adobe_DBVersionstring1500000";
    let pos = bytes
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("version record present in fixture");
    let start = pos + needle.len() - 7;
    bytes[start..start + 7].copy_from_slice(b"1400000");
    fs::write(&path, bytes).unwrap();

    match Catalog::open(&path) {
        Err(ReadError::UnsupportedVersion(v)) => assert_eq!(v, "1400000"),
        other => panic!("expected UnsupportedVersion, got {:?}", other.map(|_| ())),
    }
}

#[test]
fn missing_file_is_open_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("does-not-exist.lrcat");
    assert!(matches!(Catalog::open(&path), Err(ReadError::Open(_))));
    assert!(!path.exists(), "opening must not create the file");
}

#[test]
fn text_file_is_open_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.lrcat");
    fs::write(&path, "this is not a database, just some text\n".repeat(50)).unwrap();
    assert!(matches!(Catalog::open(&path), Err(ReadError::Open(_))));
}

#[test]
fn checksum_is_unchanged_by_open_and_drop() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    let before = checksum(&path).unwrap();
    let catalog = Catalog::open(&path).expect("open");
    drop(catalog);
    let after = checksum(&path).unwrap();
    assert_eq!(before, after);
}

#[cfg(unix)]
#[test]
fn opens_read_only_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
    let before = checksum(&path).unwrap();
    let catalog = Catalog::open(&path).expect("open read-only file");
    assert_eq!(catalog.version(), "1500000");
    drop(catalog);
    assert_eq!(before, checksum(&path).unwrap());
}

#[test]
fn leaves_no_sidecar_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    let before: Vec<_> = names(dir.path());
    let catalog = Catalog::open(&path).expect("open");
    // Check while the connection is still open, then after drop.
    assert_eq!(before, names(dir.path()), "files appeared while open");
    drop(catalog);
    let after = names(dir.path());
    assert_eq!(before, after, "files appeared after close");
    for n in &after {
        assert!(
            !(n.ends_with("-journal") || n.ends_with("-wal") || n.ends_with("-shm")),
            "unexpected sidecar file {n}"
        );
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}
