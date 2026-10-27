//! Checks of the title fixture itself (task P5b-7c).

use flate2::read::ZlibDecoder;
use lrcat_fixtures::title_edge::{self as e, Variant};
use lrcat_fixtures::{write_title_catalog_with_files, write_title_catalog_with_files_variant};
use rusqlite::{Connection, OpenFlags, types::Value};
use std::io::Read;

fn open(p: &std::path::Path) -> Connection {
    Connection::open_with_flags(p, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

fn rows(c: &Connection, image: i64) -> Vec<(i64, Value)> {
    let mut st = c
        .prepare(
            "SELECT id_local, xmp FROM Adobe_AdditionalMetadata WHERE image = ?1 ORDER BY id_local",
        )
        .unwrap();
    st.query_map([image], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

fn unpack(v: &Value) -> Vec<u8> {
    let Value::Blob(b) = v else {
        panic!("not a blob: {v:?}")
    };
    let mut out = Vec::new();
    ZlibDecoder::new(&b[4..]).read_to_end(&mut out).unwrap();
    assert_eq!(
        u32::from_be_bytes(b[..4].try_into().unwrap()) as usize,
        out.len()
    );
    out
}

#[test]
fn blobs_match_the_documentation() {
    let dir = tempfile::tempdir().unwrap();
    let c = open(&write_title_catalog_with_files(dir.path()).unwrap());
    let xmp = |id: i64| String::from_utf8(unpack(&rows(&c, id)[0].1)).unwrap();
    assert!(xmp(e::UMLAUT_ID).contains(e::UMLAUT_TITLE));
    assert!(xmp(e::UMLAUT_ID).contains("xml:lang=\"x-default\""));
    assert!(xmp(e::ENTITY_ID).contains("&amp;"));
    assert!(xmp(e::NO_TITLE_ID).contains("dc:description"));
    assert!(!xmp(e::NO_TITLE_ID).contains("dc:title"));
    assert!(xmp(e::WITH_DESCRIPTION_ID).contains("desc text"));
    assert!(unpack(&rows(&c, e::NOT_UTF8_ID)[0].1).contains(&0xff));
    assert!(matches!(&rows(&c, e::SHORT_ID)[0].1, Value::Blob(b) if b.len() < 4));
    assert!(matches!(&rows(&c, e::NOT_ZLIB_ID)[0].1, Value::Blob(b) if b.len() > 4));
    assert!(rows(&c, e::NO_ROW_ID).is_empty());
    assert_eq!(rows(&c, e::NULL_XMP_ID)[0].1, Value::Null);
    let dup = rows(&c, e::DUPLICATE_ID);
    assert_eq!(dup.iter().map(|r| r.0).collect::<Vec<_>>(), vec![100, 101]);
    assert!(
        String::from_utf8(unpack(&dup[0].1))
            .unwrap()
            .contains("first")
    );
    assert_eq!(rows(&c, e::NULL_FIRST_ID)[0], (200, Value::Null));
    assert!(matches!(rows(&c, e::BROKEN_FIRST_ID)[0].1, Value::Blob(ref b) if b.len() == 2));
    assert_eq!(rows(&c, 2).len() + rows(&c, 20).len(), 2);
}

#[test]
fn variants_drop_table_or_column() {
    let dir = tempfile::tempdir().unwrap();
    let c = open(&write_title_catalog_with_files_variant(dir.path(), Variant::NoTable).unwrap());
    let n: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'Adobe_AdditionalMetadata'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
    drop(c);
    let dir = tempfile::tempdir().unwrap();
    let c = open(&write_title_catalog_with_files_variant(dir.path(), Variant::NoColumn).unwrap());
    let n: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('Adobe_AdditionalMetadata') WHERE name = 'xmp'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn other_fixtures_have_no_xmp_blobs() {
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_small_catalog_with_files(dir.path()).unwrap();
    let n: i64 = open(&cat)
        .query_row(
            "SELECT COUNT(*) FROM Adobe_AdditionalMetadata WHERE xmp IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
}
