//! Acceptance tests for reading folders.
//!
//! Expected API:
//! - `Catalog::folders(&self) -> Result<Vec<model::Folder>, ReadError>`.
//!   Each folder's path is the `absolutePath` of its root folder
//!   (`AgLibraryRootFolder`) followed by `pathFromRoot` (`AgLibraryFolder`).
//!   Root folders themselves appear as folders with an empty `pathFromRoot`
//!   when such a row exists. The path is assembled from the stored parts
//!   only, without any file system access.

use lr_reader::{Catalog, checksum};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const ROOTS: [&str; 2] = ["/Users/example/Pictures", "C:\\Users\\example\\Pictures"];

fn small_fixture(dir: &Path) -> PathBuf {
    let path = dir.join("small.lrcat");
    lrcat_fixtures::write_small_catalog(&path).expect("generate fixture");
    path
}

fn folder_paths(catalog: &Catalog) -> Vec<String> {
    catalog
        .folders()
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
        let _ = catalog.folders().expect("read folders");
    }
    assert_eq!(before, checksum(&path).unwrap());
}
