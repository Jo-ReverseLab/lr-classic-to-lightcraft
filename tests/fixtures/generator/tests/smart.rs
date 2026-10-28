//! The smart collection fixture has the documented counts.

use lrcat_fixtures::{small, smart_edge as e, write_smart_catalog};
use rusqlite::Connection;

#[test]
fn smart_fixture_counts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("smart.lrcat");
    write_smart_catalog(&path).unwrap();
    let c = Connection::open(&path).unwrap();
    let n = |sql: &str| -> i64 { c.query_row(sql, [], |r| r.get(0)).unwrap() };
    assert_eq!(
        n("SELECT COUNT(*) FROM AgLibraryCollection"),
        small::COLLECTIONS + e::ADDED
    );
    assert_eq!(
        n("SELECT COUNT(*) FROM AgLibraryCollection WHERE creationId LIKE '%smart_collection'"),
        e::SMART_TOTAL
    );
    assert_eq!(e::READABLE.len() as i64, e::READABLE_COUNT);
    assert_eq!(e::SMART_TOTAL, small::COLLECTIONS_SMART + e::ADDED);
    assert_eq!(
        e::UNREADABLE_COUNT,
        small::COLLECTIONS_SMART + e::UNREADABLE.len() as i64 + 1
    );
    assert_eq!(e::READABLE_COUNT + e::UNREADABLE_COUNT, e::SMART_TOTAL);
}

#[test]
fn smart_translate_fixture_counts() {
    use lrcat_fixtures::smart_translate as t;
    let dir = tempfile::tempdir().unwrap();
    let path = lrcat_fixtures::write_smart_translate_catalog_with_files(dir.path()).unwrap();
    let c = Connection::open(&path).unwrap();
    let total: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM AgLibraryCollection WHERE creationId LIKE '%smart_collection'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(total, e::SMART_TOTAL + t::ADDED);
    assert_eq!(t::TRANSLATABLE.len(), t::RULES.len());
    assert_eq!(t::NEW_SKIPPED.len() as i64 + 3, t::ADDED);
    let parent: Option<i64> = c
        .query_row(
            "SELECT parent FROM AgLibraryCollection WHERE id_local = 202",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(parent, Some(t::PARENT_SET));
}
