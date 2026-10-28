//! Checks of the caption fixture itself.

use lrcat_fixtures::caption_edge as e;
use lrcat_fixtures::write_caption_catalog_with_files;
use rusqlite::{Connection, OpenFlags, types::Value};

#[test]
fn rows_match_the_documentation() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_caption_catalog_with_files(dir.path()).unwrap();
    let c = Connection::open_with_flags(&cat, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let rows = |image: i64| -> Vec<(i64, Value)> {
        let mut st = c
            .prepare(
                "SELECT id_local, caption FROM AgLibraryIPTC WHERE image = ?1 ORDER BY id_local",
            )
            .unwrap();
        st.query_map([image], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert_eq!(rows(e::UMLAUT_ID)[0].1, Value::Text(e::UMLAUT_TEXT.into()));
    assert_eq!(rows(e::PADDED_ID)[0].1, Value::Text(e::PADDED_TEXT.into()));
    assert_eq!(rows(e::EMPTY_ID)[0].1, Value::Text(String::new()));
    assert_eq!(rows(e::NULL_ID)[0].1, Value::Null);
    assert!(rows(e::NO_ROW_ID).is_empty());
    let dup = rows(e::DUPLICATE_ID);
    assert_eq!(dup.len(), 2);
    assert_eq!(dup[0], (100, Value::Text(e::DUPLICATE_TEXT.into())));
    assert_eq!(rows(e::DUPLICATE_NULL_FIRST_ID)[0], (200, Value::Null));
    assert_eq!(rows(e::NON_TEXT_ID)[0].1, Value::Integer(42));
    assert_eq!(rows(2).len() + rows(20).len(), 2);
}

#[test]
fn other_fixtures_have_the_empty_table() {
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_small_catalog_with_files(dir.path()).unwrap();
    let c = Connection::open_with_flags(&cat, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let n: i64 = c
        .query_row("SELECT COUNT(*) FROM AgLibraryIPTC", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
}
