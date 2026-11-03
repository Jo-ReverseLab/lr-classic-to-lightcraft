//! Acceptance tests for reading folders.
//!
//! Expected API:
//! - `Catalog::folders(&self, report: &mut model::Report)
//!   -> Result<Vec<model::Folder>, ReadError>`. Every folder row calls
//!   `report.read()`; a folder without a valid root folder is reported via
//!   `report.skipped(..)` (reason contains "root" and the folder id, no path)
//!   instead of failing. The reader never calls `report.converted()`.
//!   Each folder's path is the `absolutePath` of its root folder
//!   (`AgLibraryRootFolder`) followed by `pathFromRoot` (`AgLibraryFolder`).
//!   Root folders themselves appear as folders with an empty `pathFromRoot`
//!   when such a row exists. The path is assembled from the stored parts
//!   only, without any file system access.

use lr_reader::{Catalog, checksum};
use model::Report;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const ROOTS: [&str; 2] = ["/Users/example/Pictures", "C:\\Users\\example\\Pictures"];

fn small_fixture(dir: &Path) -> PathBuf {
    let path = dir.join("small.lrcat");
    lrcat_fixtures::write_small_catalog(&path).expect("generate fixture");
    path
}

fn folder_paths(catalog: &Catalog) -> Vec<String> {
    let mut report = Report::new();
    catalog
        .folders(&mut report)
        .expect("read folders")
        .iter()
        .map(|f| f.path.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn folder_count_matches_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(&small_fixture(dir.path())).unwrap();
    assert_eq!(
        folder_paths(&catalog).len() as i64,
        lrcat_fixtures::small::FOLDERS
    );
}

#[test]
fn every_path_starts_with_a_root_folder_path() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(&small_fixture(dir.path())).unwrap();
    for p in folder_paths(&catalog) {
        assert!(
            ROOTS.iter().any(|r| p.starts_with(r)),
            "path {p:?} does not start with a known root"
        );
    }
}

#[test]
fn paths_are_assembled_from_both_parts() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(&small_fixture(dir.path())).unwrap();
    let paths = folder_paths(&catalog);
    // Normalise separators so the check does not depend on the join style.
    let norm: Vec<String> = paths
        .iter()
        .map(|p| p.replace('\\', "/").trim_end_matches('/').to_string())
        .collect();
    for expected in [
        "/Users/example/Pictures/2024/Rome",
        "C:/Users/example/Pictures/Trips",
    ] {
        assert!(
            norm.iter().any(|p| p == expected),
            "missing folder {expected}"
        );
    }
    // Root folders appear on their own (empty pathFromRoot).
    for root in ["/Users/example/Pictures", "C:/Users/example/Pictures"] {
        assert!(norm.iter().any(|p| p == root), "missing root {root}");
    }
}

#[test]
fn no_duplicate_paths() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(&small_fixture(dir.path())).unwrap();
    let paths = folder_paths(&catalog);
    let unique: HashSet<_> = paths.iter().collect();
    assert_eq!(unique.len(), paths.len());
}

#[test]
fn reading_folders_does_not_change_the_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    let before = checksum(&path).unwrap();
    {
        let catalog = Catalog::open(&path).unwrap();
        let mut report = Report::new();
        let _ = catalog.folders(&mut report).expect("read folders");
    }
    assert_eq!(before, checksum(&path).unwrap());
}

#[test]
fn every_folder_row_is_counted_as_read() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(&small_fixture(dir.path())).unwrap();
    let mut report = Report::new();
    let folders = catalog.folders(&mut report).unwrap();
    assert_eq!(report.read_count() as i64, lrcat_fixtures::small::FOLDERS);
    assert_eq!(report.skipped_count(), 0);
    assert_eq!(
        report.converted_count(),
        0,
        "reader must not call converted()"
    );
    assert_eq!(folders.len(), report.read_count());
}

#[test]
fn broken_root_folder_is_skipped_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    let broken_id: i64;
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        broken_id = c
            .query_row(
                "SELECT id_local FROM AgLibraryFolder WHERE pathFromRoot <> '' ORDER BY id_local LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        c.execute(
            "UPDATE AgLibraryFolder SET rootFolder = 987654 WHERE id_local = ?1",
            [broken_id],
        )
        .unwrap();
    }
    let catalog = Catalog::open(&path).unwrap();
    let mut report = Report::new();
    let folders = catalog.folders(&mut report).expect("must not fail");
    assert_eq!(report.read_count() as i64, lrcat_fixtures::small::FOLDERS);
    assert_eq!(report.skipped_count(), 1);
    assert_eq!(folders.len(), report.read_count() - 1);
    let reason = &report.skipped_items()[0].reason;
    assert!(reason.contains("root"), "reason: {reason}");
    assert!(reason.contains(&broken_id.to_string()), "reason: {reason}");
    assert!(
        !reason.contains('/') && !reason.contains('\\'),
        "no paths: {reason}"
    );
    // read = converted + skipped once the caller marks the rest converted.
    for _ in &folders {
        report.converted();
    }
    assert!(report.is_balanced());
}

#[test]
fn folder_identity_is_the_path_not_an_id() {
    // Folders have no global id in the model: their identity is the assembled
    // path. A folder moved or renamed in Lightroom (a different pathFromRoot
    // or root) is therefore seen as a different folder, with no memory of the
    // old path. The folder row id staying the same does not keep the identity.
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    let catalog = Catalog::open(&path).unwrap();
    let before: HashSet<String> = folder_paths(&catalog).into_iter().collect();
    // Pick a real sub-folder (one with a non-empty pathFromRoot).
    let row_id: i64 = {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.query_row(
            "SELECT id_local FROM AgLibraryFolder \
             WHERE pathFromRoot <> '' ORDER BY id_local LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    // Rename the folder in the catalog: same row id, new path.
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute(
            "UPDATE AgLibraryFolder SET pathFromRoot = 'renamed/' WHERE id_local = ?1",
            [row_id],
        )
        .unwrap();
    }
    let catalog = Catalog::open(&path).unwrap();
    let after: HashSet<String> = folder_paths(&catalog).into_iter().collect();

    // The row id is unchanged, but the path changed: exactly one path left
    // (the old one) and exactly one arrived (the new one, ending in `renamed`).
    let gone: Vec<&String> = before.difference(&after).collect();
    let arrived: Vec<&String> = after.difference(&before).collect();
    assert_eq!(
        gone.len(),
        1,
        "exactly one old path should disappear: {gone:?}"
    );
    assert_eq!(
        arrived.len(),
        1,
        "exactly one new path should appear: {arrived:?}"
    );
    assert!(
        arrived[0].ends_with("renamed"),
        "the new path should end with `renamed`: {}",
        arrived[0]
    );
    assert_eq!(before.len(), after.len(), "no folder was added or removed");
}
