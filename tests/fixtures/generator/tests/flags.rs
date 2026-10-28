//! Checks of the flag fixture itself.

use lrcat_fixtures::{flag_edge, write_flag_catalog_with_files};
use rusqlite::{Connection, OpenFlags, types::Value};

#[test]
fn flag_fixture_stores_the_documented_values() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_flag_catalog_with_files(dir.path()).unwrap();
    let c = Connection::open_with_flags(&cat, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let n: i64 = c
        .query_row("SELECT COUNT(*) FROM Adobe_images", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, flag_edge::PHOTOS);
    let pick = |id: i64| -> Value {
        c.query_row(
            "SELECT pick FROM Adobe_images WHERE id_local = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(pick(4), Value::Integer(1));
    assert_eq!(pick(5), Value::Integer(-1));
    assert_eq!(pick(6), Value::Integer(0));
    assert_eq!(pick(7), Value::Null);
    assert_eq!(pick(8), Value::Real(1.0));
    assert_eq!(pick(9), Value::Real(-1.0));
    assert_eq!(pick(10), Value::Integer(2));
    assert!(matches!(pick(11), Value::Text(_)));
    assert_eq!(pick(12), Value::Real(0.5));
    assert_eq!(pick(1), Value::Integer(0));
    assert_eq!(pick(20), Value::Integer(1));
    let bad: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM AgLibraryCollectionImage WHERE pick != 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(bad, 0);
}
