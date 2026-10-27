//! Acceptance tests for reading collections (task P4-10).
//!
//! Expected API: `Catalog::collections(&self, report: &mut model::Report)
//! -> Result<(), ReadError>`. Every `AgLibraryCollection` row calls
//! `report.read()` and `report.skipped(reason)`; the reader never calls
//! `report.converted()` (user decision F-002: all collections count as
//! skipped). Reasons (exact, never containing the collection name):
//! - `systemOnly != 0` (wins over creationId):
//!   "collection {id}: system collection is not converted"
//! - `com.adobe.ag.library.collection`: "collection {id}: collection is not converted"
//! - `com.adobe.ag.library.group`: "collection {id}: collection set is not converted"
//! - `com.adobe.ag.library.smart_collection`:
//!   "collection {id}: smart collection is not converted"
//! - anything else: "collection {id}: unknown collection type is not converted"

use lr_reader::{Catalog, checksum};
use lrcat_fixtures::small;
use model::Report;
use std::path::{Path, PathBuf};

fn small_fixture(dir: &Path) -> PathBuf {
    let path = dir.join("small.lrcat");
    lrcat_fixtures::write_small_catalog(&path).expect("generate fixture");
    path
}

fn read_collections(path: &Path) -> Report {
    let catalog = Catalog::open(path).unwrap();
    let mut report = Report::new();
    catalog.collections(&mut report).expect("read collections");
    report
}

fn reasons(report: &Report) -> Vec<String> {
    report
        .skipped_items()
        .iter()
        .map(|s| s.reason.clone())
        .collect()
}

fn count_of(report: &Report, reason: &str) -> usize {
    reasons(report).iter().filter(|r| *r == reason).count()
}

#[test]
fn all_collections_are_read_and_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let report = read_collections(&small_fixture(dir.path()));
    assert_eq!(report.read_count() as i64, small::COLLECTIONS);
    assert_eq!(report.converted_count(), 0);
    assert_eq!(report.skipped_count() as i64, small::COLLECTIONS);
    assert!(report.is_balanced());
}

#[test]
fn skip_reasons_are_exact_per_kind() {
    let dir = tempfile::tempdir().unwrap();
    let report = read_collections(&small_fixture(dir.path()));
    // System collections: ids 7 and 8 (8 has the plain creationId; system wins).
    for id in [7, 8] {
        let r = format!("collection {id}: system collection is not converted");
        assert_eq!(count_of(&report, &r), 1, "{r}");
    }
    for id in [2, 3, 4] {
        let r = format!("collection {id}: collection is not converted");
        assert_eq!(count_of(&report, &r), 1, "{r}");
    }
    assert_eq!(
        count_of(&report, "collection 1: collection set is not converted"),
        1
    );
    for id in [5, 6] {
        let r = format!("collection {id}: smart collection is not converted");
        assert_eq!(count_of(&report, &r), 1, "{r}");
    }
    assert_eq!(
        count_of(
            &report,
            "collection 9: unknown collection type is not converted"
        ),
        1
    );
}

#[test]
fn reason_totals_match_fixture_constants() {
    let dir = tempfile::tempdir().unwrap();
    let report = read_collections(&small_fixture(dir.path()));
    let all = reasons(&report);
    let n = |suffix: &str| all.iter().filter(|r| r.ends_with(suffix)).count() as i64;
    assert_eq!(
        n(": system collection is not converted"),
        small::COLLECTIONS_SYSTEM
    );
    assert_eq!(n(": collection is not converted"), small::COLLECTIONS_PLAIN);
    assert_eq!(
        n(": collection set is not converted"),
        small::COLLECTION_SETS
    );
    assert_eq!(
        n(": smart collection is not converted"),
        small::COLLECTIONS_SMART
    );
    assert_eq!(
        n(": unknown collection type is not converted"),
        small::COLLECTIONS_UNKNOWN
    );
}

#[test]
fn no_collection_name_appears_in_any_reason() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    let names: Vec<String> = {
        let c = rusqlite::Connection::open(&path).unwrap();
        let mut st = c.prepare("SELECT name FROM AgLibraryCollection").unwrap();
        st.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(names.len() as i64, small::COLLECTIONS);
    let report = read_collections(&path);
    for reason in reasons(&report) {
        for name in &names {
            assert!(
                !reason.contains(name.as_str()),
                "reason {reason:?} has name"
            );
        }
    }
}

#[test]
fn reading_collections_does_not_change_the_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    let before = checksum(&path).unwrap();
    let _ = read_collections(&path);
    assert_eq!(before, checksum(&path).unwrap());
}

#[test]
fn reading_twice_gives_identical_results() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    let a = reasons(&read_collections(&path));
    let b = reasons(&read_collections(&path));
    assert_eq!(a, b);
}

#[test]
fn catalog_without_collections_reads_zero() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch("DELETE FROM AgLibraryCollectionImage; DELETE FROM AgLibraryCollection;")
            .unwrap();
    }
    let report = read_collections(&path);
    assert_eq!(report.read_count(), 0);
    assert_eq!(report.skipped_count(), 0);
    assert!(report.is_balanced());
}
