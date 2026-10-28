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
