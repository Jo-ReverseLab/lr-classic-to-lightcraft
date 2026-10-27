//! Acceptance tests for the schema check in `Catalog::open` (task P4-11).
//!
//! Expected API: `ReadError::MissingSchema { table: String, column:
//! Option<String> }`, returned by `Catalog::open` right after the version
//! check when a table or column that the readers query is absent. The
//! Display message names the table and, if given, the column.
//!
//! Required tables: Adobe_images, AgLibraryFile, AgLibraryFolder,
//! AgLibraryRootFolder, AgLibraryCollection. Required columns are those
//! used by the SELECTs in folders.rs, photos.rs and collections.rs.
//! Names are compared exactly (as stored in the generated fixtures).

use lr_reader::{Catalog, ReadError, checksum};
use std::path::{Path, PathBuf};

fn damaged(dir: &Path, table: &str, column: Option<&str>) -> PathBuf {
    let path = dir.join("damaged.lrcat");
    lrcat_fixtures::write_small_catalog_without(&path, table, column).expect("generate fixture");
    path
}

fn expect_missing(path: &Path, table: &str, column: Option<&str>) -> String {
    let before = checksum(path).unwrap();
    let err = match Catalog::open(path) {
        Err(e) => e,
        Ok(_) => panic!("open must fail for {table}/{column:?}"),
    };
    let msg = err.to_string();
    match err {
        ReadError::MissingSchema {
            table: t,
            column: c,
        } => {
            assert_eq!(t, table);
            assert_eq!(c.as_deref(), column);
        }
        other => panic!("expected MissingSchema, got {other:?}"),
    }
    assert!(msg.contains(table), "message: {msg}");
    if let Some(c) = column {
        assert!(msg.contains(c), "message: {msg}");
    }
    assert_eq!(checksum(path).unwrap(), before, "catalog changed");
    msg
}

#[test]
fn normal_fixture_still_opens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.lrcat");
    lrcat_fixtures::write_small_catalog(&path).unwrap();
    let before = checksum(&path).unwrap();
    Catalog::open(&path).expect("open");
    assert_eq!(checksum(&path).unwrap(), before);
}

#[test]
fn missing_collection_table_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = damaged(dir.path(), "AgLibraryCollection", None);
    expect_missing(&path, "AgLibraryCollection", None);
}

#[test]
fn missing_file_table_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = damaged(dir.path(), "AgLibraryFile", None);
    expect_missing(&path, "AgLibraryFile", None);
}

#[test]
fn missing_images_table_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = damaged(dir.path(), "Adobe_images", None);
    expect_missing(&path, "Adobe_images", None);
}

#[test]
fn missing_root_folder_table_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = damaged(dir.path(), "AgLibraryRootFolder", None);
    expect_missing(&path, "AgLibraryRootFolder", None);
}

#[test]
fn missing_path_from_root_column_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = damaged(dir.path(), "AgLibraryFolder", Some("pathFromRoot"));
    expect_missing(&path, "AgLibraryFolder", Some("pathFromRoot"));
}

#[test]
fn missing_rating_column_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = damaged(dir.path(), "Adobe_images", Some("rating"));
    expect_missing(&path, "Adobe_images", Some("rating"));
}

#[test]
fn missing_collection_column_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = damaged(dir.path(), "AgLibraryCollection", Some("systemOnly"));
    expect_missing(&path, "AgLibraryCollection", Some("systemOnly"));
}

#[test]
fn error_comes_from_open_not_from_reading() {
    // `expect_missing` calls only `Catalog::open`; no reader function is
    // involved, so a passing test proves the check happens at open time.
    let dir = tempfile::tempdir().unwrap();
    let path = damaged(dir.path(), "AgLibraryCollection", None);
    let msg = expect_missing(&path, "AgLibraryCollection", None);
    assert!(!msg.starts_with("cannot read catalog"), "message: {msg}");
}

#[test]
fn missing_synonym_table_is_reported() {
    // P5-3: AgLibraryKeywordSynonym (columns id_local, keyword) is required.
    let dir = tempfile::tempdir().unwrap();
    let path = damaged(dir.path(), "AgLibraryKeywordSynonym", None);
    expect_missing(&path, "AgLibraryKeywordSynonym", None);
}

#[test]
fn missing_synonym_keyword_column_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = damaged(dir.path(), "AgLibraryKeywordSynonym", Some("keyword"));
    expect_missing(&path, "AgLibraryKeywordSynonym", Some("keyword"));
}

#[test]
fn missing_pick_column_is_reported() {
    // P5b-1: Adobe_images.pick is required.
    let dir = tempfile::tempdir().unwrap();
    let path = damaged(dir.path(), "Adobe_images", Some("pick"));
    expect_missing(&path, "Adobe_images", Some("pick"));
}

#[test]
fn missing_color_labels_column_is_reported() {
    // P5b-2: Adobe_images.colorLabels is required.
    let dir = tempfile::tempdir().unwrap();
    let path = damaged(dir.path(), "Adobe_images", Some("colorLabels"));
    expect_missing(&path, "Adobe_images", Some("colorLabels"));
}

#[test]
fn missing_iptc_table_is_reported() {
    // P5b-7: AgLibraryIPTC (columns image, caption) is required.
    let dir = tempfile::tempdir().unwrap();
    let path = damaged(dir.path(), "AgLibraryIPTC", None);
    expect_missing(&path, "AgLibraryIPTC", None);
}

#[test]
fn missing_iptc_caption_column_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = damaged(dir.path(), "AgLibraryIPTC", Some("caption"));
    expect_missing(&path, "AgLibraryIPTC", Some("caption"));
}

#[test]
fn missing_iptc_image_column_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = damaged(dir.path(), "AgLibraryIPTC", Some("image"));
    expect_missing(&path, "AgLibraryIPTC", Some("image"));
}
