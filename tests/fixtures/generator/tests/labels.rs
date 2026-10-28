//! Checks of the colour label fixtures themselves.

use lrcat_fixtures::label_edge::{self, Fixture};
use lrcat_fixtures::write_label_catalog_with_files;
use rusqlite::{Connection, OpenFlags, types::Value};

fn label(c: &Connection, id: i64) -> Value {
    c.query_row(
        "SELECT colorLabels FROM Adobe_images WHERE id_local = ?1",
        [id],
        |r| r.get(0),
    )
    .unwrap()
}

fn text(s: &str) -> Value {
    Value::Text(s.to_string())
}

#[test]
fn every_fixture_stores_the_documented_labels() {
    for f in [
        Fixture::En,
        Fixture::De,
        Fixture::Mixed,
        Fixture::Odd,
        Fixture::Empty,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let cat = write_label_catalog_with_files(dir.path(), f).unwrap();
        let c = Connection::open_with_flags(&cat, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM Adobe_images", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, label_edge::PHOTOS);
        let listed = label_edge::values(f);
        for (id, want) in &listed {
            assert_eq!(label(&c, *id), text(want), "{f:?} photo {id}");
        }
        assert_eq!(label(&c, label_edge::NULL_ID), Value::Null);
        let rest: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM Adobe_images WHERE colorLabels = ''",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rest, label_edge::PHOTOS - 1 - listed.len() as i64, "{f:?}");
    }
}

#[test]
fn odd_fixture_has_nfd_and_trailing_space() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_label_catalog_with_files(dir.path(), Fixture::Odd).unwrap();
    let c = Connection::open_with_flags(&cat, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(label(&c, 4), text("Gru\u{308}n"));
    assert_eq!(label(&c, 6), text("Red "));
    let dir2 = tempfile::tempdir().unwrap();
    let de = write_label_catalog_with_files(dir2.path(), Fixture::De).unwrap();
    let c = Connection::open_with_flags(&de, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(label(&c, 6), text("Gr\u{fc}n"));
}
