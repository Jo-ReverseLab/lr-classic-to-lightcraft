//! Acceptance tests for detecting changes Lightroom has not yet merged into
//! the catalog file.
//!
//! Expected API:
//! - `Catalog::open(path)` looks for `<catalog>-wal` and `<catalog>-journal`
//!   next to the catalog (the full file name plus the suffix, e.g.
//!   `foo.lrcat-wal`) before opening. If either exists and is NOT empty, it
//!   returns `ReadError::PendingChanges { path }`, where `path` is the
//!   offending side file. Its `Display` text asks the user to close Lightroom
//!   (it contains the word "Lightroom").
//! - Missing or zero-length side files are not an error.
//! - Nothing is written: the catalog and the side files stay byte-identical,
//!   and no new files appear.

use lr_reader::{Catalog, ReadError, checksum};
use std::fs;
use std::path::{Path, PathBuf};

fn small_fixture(dir: &Path) -> PathBuf {
    let path = dir.join("small.lrcat");
    lrcat_fixtures::write_small_catalog(&path).expect("generate fixture");
    path
}

fn side_file(catalog: &Path, suffix: &str) -> PathBuf {
    let mut name = catalog.file_name().unwrap().to_os_string();
    name.push(suffix);
    catalog.with_file_name(name)
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn expect_pending(path: &Path, side: &Path) {
    match Catalog::open(path) {
        Err(err @ ReadError::PendingChanges { .. }) => {
            let ReadError::PendingChanges { path: reported } = &err else {
                unreachable!()
            };
            assert_eq!(reported, side, "error must name the pending side file");
            assert!(
                err.to_string().contains("Lightroom"),
                "message must tell the user to close Lightroom: {err}"
            );
        }
        other => panic!("expected PendingChanges, got {:?}", other.map(|_| ())),
    }
}

#[test]
fn non_empty_wal_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    let wal = side_file(&path, "-wal");
    fs::write(&wal, b"pretend these are uncommitted pages").unwrap();
    expect_pending(&path, &wal);
}

#[test]
fn non_empty_journal_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    let journal = side_file(&path, "-journal");
    fs::write(&journal, b"pretend this is a rollback journal").unwrap();
    expect_pending(&path, &journal);
}

#[test]
fn empty_wal_opens_normally() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    fs::write(side_file(&path, "-wal"), b"").unwrap();
    let catalog = Catalog::open(&path).expect("empty -wal is not an error");
    assert_eq!(catalog.version(), "1500000");
}

#[test]
fn empty_journal_opens_normally() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    fs::write(side_file(&path, "-journal"), b"").unwrap();
    let catalog = Catalog::open(&path).expect("empty -journal is not an error");
    assert_eq!(catalog.version(), "1500000");
}

#[test]
fn opens_normally_without_side_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    assert!(!side_file(&path, "-wal").exists());
    assert!(!side_file(&path, "-journal").exists());
    let catalog = Catalog::open(&path).expect("open");
    assert_eq!(catalog.version(), "1500000");
}

#[test]
fn rejection_leaves_catalog_and_wal_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    let wal = side_file(&path, "-wal");
    fs::write(&wal, b"pretend these are uncommitted pages").unwrap();

    let catalog_before = checksum(&path).unwrap();
    let wal_before = fs::read(&wal).unwrap();
    let files_before = names(dir.path());

    assert!(matches!(
        Catalog::open(&path),
        Err(ReadError::PendingChanges { .. })
    ));

    assert_eq!(catalog_before, checksum(&path).unwrap());
    assert_eq!(wal_before, fs::read(&wal).unwrap());
    assert_eq!(files_before, names(dir.path()), "no files may appear");
}

#[test]
fn both_side_files_non_empty_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    fs::write(side_file(&path, "-wal"), b"wal data").unwrap();
    fs::write(side_file(&path, "-journal"), b"journal data").unwrap();
    assert!(matches!(
        Catalog::open(&path),
        Err(ReadError::PendingChanges { .. })
    ));
}

#[test]
fn side_files_of_other_catalogs_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    // Belongs to a different catalog name, so it is irrelevant here.
    fs::write(dir.path().join("other.lrcat-wal"), b"not ours").unwrap();
    Catalog::open(&path).expect("unrelated side file must not matter");
}

// A non-empty `-shm` (shared-memory) file next to the catalog is **not**
// rejected today: only `-wal` and `-journal` are checked. This pins the
// current behaviour. A `-shm` file only ever accompanies a `-wal` file in
// WAL mode, so a lone `-shm` is a stale leftover and the existing `-wal`
// check already catches a live WAL. Whether a lone `-shm` should also be
// refused is a decision for the owner, not changed here.
#[test]
fn non_empty_shm_is_not_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    let shm = side_file(&path, "-shm");
    fs::write(&shm, b"pretend this is shared memory").unwrap();
    let catalog = Catalog::open(&path).expect("a lone -shm is not checked");
    assert_eq!(catalog.version(), "1500000");
    // Nothing is written: the catalog and the -shm stay byte-identical.
    let before = checksum(&path).unwrap();
    let shm_before = fs::read(&shm).unwrap();
    drop(catalog);
    assert_eq!(before, checksum(&path).unwrap());
    assert_eq!(shm_before, fs::read(&shm).unwrap());
}
