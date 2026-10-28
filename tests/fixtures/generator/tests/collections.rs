//! Checks of the collection fixture itself.

use lrcat_fixtures::collection_edge as e;
use lrcat_fixtures::write_collection_catalog_with_files;
use rusqlite::{Connection, OpenFlags};

fn open(p: &std::path::Path) -> Connection {
    Connection::open_with_flags(p, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

fn count(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn row_counts_match_the_constants() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_collection_catalog_with_files(dir.path()).unwrap();
    let c = open(&cat);
    assert_eq!(
        count(&c, "SELECT count(*) FROM AgLibraryCollection"),
        e::COLLECTIONS
    );
    assert_eq!(
        count(&c, "SELECT count(*) FROM AgLibraryCollectionImage"),
        e::MEMBER_ROWS
    );
    assert_eq!(e::CONVERTED + e::SKIPPED, e::COLLECTIONS);
    assert_eq!(e::FOLDERS + e::ALBUMS, e::CONVERTED);
    assert_eq!(e::MEMBERS_CONVERTED + e::MEMBERS_SKIPPED, e::MEMBER_ROWS);
    assert_eq!(e::NAMES.len() as i64, e::CONVERTED);
    let kept: usize = e::MEMBERS.iter().map(|(_, m)| m.len()).sum();
    assert_eq!(kept as i64, e::MEMBERS_CONVERTED);
}

#[test]
fn names_are_stored_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_collection_catalog_with_files(dir.path()).unwrap();
    let c = open(&cat);
    for (id, name, parent) in e::NAMES {
        let (n, p): (String, Option<i64>) = c
            .query_row(
                "SELECT name, parent FROM AgLibraryCollection WHERE id_local = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(n.as_bytes(), name.as_bytes(), "name of {id}");
        assert_eq!(p, parent, "parent of {id}");
    }
}

#[test]
fn system_only_has_real_integer_and_null_forms() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_collection_catalog_with_files(dir.path()).unwrap();
    let c = open(&cat);
    let ty = |id: i64| -> String {
        c.query_row(
            "SELECT typeof(systemOnly) FROM AgLibraryCollection WHERE id_local = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(ty(14), "real");
    assert_eq!(ty(15), "integer");
    assert_eq!(ty(2), "real");
    assert_eq!(ty(3), "null");
}

#[test]
fn member_photos_are_as_described() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_collection_catalog_with_files(dir.path()).unwrap();
    let c = open(&cat);
    assert_eq!(
        count(&c, "SELECT count(*) FROM Adobe_images WHERE id_local = 999"),
        0
    );
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM Adobe_images WHERE id_local = 20 AND masterImage IS NOT NULL"
        ),
        1
    );
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM AgLibraryCollectionImage WHERE collection = 7 AND image = 15"
        ),
        2
    );
}
