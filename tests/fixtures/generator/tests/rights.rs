//! Checks of the rights fixture itself.

use lrcat_fixtures::rights_edge as e;
use lrcat_fixtures::{write_copyright_only_catalog_with_files, write_rights_catalog_with_files};
use rusqlite::{Connection, OpenFlags, types::Value};

fn open(p: &std::path::Path) -> Connection {
    Connection::open_with_flags(p, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

#[test]
fn copyright_rows_match_the_documentation() {
    let dir = tempfile::tempdir().unwrap();
    let c = open(&write_rights_catalog_with_files(dir.path()).unwrap());
    let rows = |image: i64| -> Vec<(i64, Value)> {
        let mut st = c
            .prepare(
                "SELECT id_local, copyright FROM AgLibraryIPTC WHERE image = ?1 ORDER BY id_local",
            )
            .unwrap();
        st.query_map([image], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert_eq!(
        rows(e::UMLAUT_ID)[0].1,
        Value::Text(e::UMLAUT_COPYRIGHT.into())
    );
    assert_eq!(
        rows(e::PADDED_ID)[0].1,
        Value::Text(e::PADDED_COPYRIGHT.into())
    );
    assert_eq!(rows(e::EMPTY_ID)[0].1, Value::Text(String::new()));
    assert_eq!(rows(e::NO_ROW_ID).len(), 0);
    assert_eq!(rows(e::DUPLICATE_ID)[0], (100, Value::Text("first".into())));
    assert_eq!(rows(e::DUPLICATE_NULL_FIRST_ID)[0], (200, Value::Null));
    assert_eq!(rows(e::NON_TEXT_ID)[0].1, Value::Integer(42));
}

#[test]
fn creator_tables_match_the_documentation() {
    let dir = tempfile::tempdir().unwrap();
    let c = open(&write_rights_catalog_with_files(dir.path()).unwrap());
    let harvested = |image: i64| -> Vec<(i64, Option<i64>)> {
        let mut st = c
            .prepare("SELECT id_local, creatorRef FROM AgHarvestedIptcMetadata WHERE image = ?1 ORDER BY id_local")
            .unwrap();
        st.query_map([image], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert_eq!(harvested(e::UMLAUT_ID), vec![(1, Some(503))]);
    assert_eq!(harvested(e::PADDED_ID), vec![(2, None)]);
    assert_eq!(harvested(e::EMPTY_ID), vec![(3, Some(999))]);
    assert_eq!(
        harvested(e::TWO_HARVESTED_ID),
        vec![(300, Some(501)), (301, Some(502))]
    );
    assert_eq!(
        harvested(e::DUPLICATE_ID),
        vec![(400, None), (401, Some(501))]
    );
    assert!(harvested(e::NO_ROW_ID).is_empty());
    let missing: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM AgInternedIptcCreator WHERE id_local = 999",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(missing, 0);
    let umlaut: String = c
        .query_row(
            "SELECT value FROM AgInternedIptcCreator WHERE id_local = 503",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(umlaut, e::UMLAUT_CREATOR);
}

#[test]
fn copyright_only_fixture_has_no_creator_tables() {
    let dir = tempfile::tempdir().unwrap();
    let c = open(&write_copyright_only_catalog_with_files(dir.path()).unwrap());
    let n: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name IN ('AgHarvestedIptcMetadata','AgInternedIptcCreator')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
}
