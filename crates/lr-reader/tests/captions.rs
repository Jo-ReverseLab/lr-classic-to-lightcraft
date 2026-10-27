//! Acceptance tests for reading captions (task P5b-7).
//!
//! Switched off with `cfg(any())` until the API exists; the developer removes
//! that line and the `ignore` attributes.
//!
//! Expected API: `Photo::caption: Option<String>`, filled by
//! `Catalog::photo_data` from `AgLibraryIPTC.caption` (joined by `image`):
//! - the stored text byte for byte (no trim, no normalisation);
//! - NULL, empty string and non-text values give `None`;
//! - several rows for one image: the row with the smallest `id_local` counts
//!   (if its caption is NULL the result is `None`);
//! - no row: `None`;
//! - captions do not change which photos are returned or the counts;
//! - the catalog stays unchanged; a second read gives identical data;
//! - nothing in the `Report` contains caption text.
//!
//! Fixture: `lrcat_fixtures::write_caption_catalog_with_files`, see
//! `lrcat_fixtures::caption_edge`.
#![cfg(any())]

use lr_reader::{Catalog, checksum};
use lrcat_fixtures::caption_edge as e;
use lrcat_fixtures::write_caption_catalog_with_files;
use model::Report;
use std::path::PathBuf;

fn read() -> (tempfile::TempDir, PathBuf, lr_reader::PhotoData, Report) {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_caption_catalog_with_files(dir.path()).unwrap();
    let before = checksum(&cat).unwrap();
    let catalog = Catalog::open(&cat).unwrap();
    let mut report = Report::new();
    let data = catalog.photo_data(&mut report).expect("read photos");
    assert_eq!(checksum(&cat).unwrap(), before, "catalog changed");
    (dir, cat, data, report)
}

fn caption(data: &lr_reader::PhotoData, id: i64) -> Option<String> {
    data.photos
        .iter()
        .find(|p| p.lightroom_id == id)
        .unwrap_or_else(|| panic!("photo {id} not returned"))
        .caption
        .clone()
}

#[test]
#[ignore = "waiting for P5b-7"]
fn text_is_returned_byte_for_byte() {
    let (_d, _c, data, _) = read();
    assert_eq!(
        caption(&data, e::UMLAUT_ID).as_deref(),
        Some(e::UMLAUT_TEXT)
    );
    assert_eq!(
        caption(&data, e::PADDED_ID).as_deref(),
        Some(e::PADDED_TEXT)
    );
}

#[test]
#[ignore = "waiting for P5b-7"]
fn empty_null_missing_row_and_non_text_give_none() {
    let (_d, _c, data, _) = read();
    for id in [e::EMPTY_ID, e::NULL_ID, e::NO_ROW_ID, e::NON_TEXT_ID, 1, 12] {
        assert_eq!(caption(&data, id), None, "photo {id}");
    }
}

#[test]
#[ignore = "waiting for P5b-7"]
fn smallest_id_local_wins_with_duplicate_rows() {
    let (_d, _c, data, _) = read();
    assert_eq!(
        caption(&data, e::DUPLICATE_ID).as_deref(),
        Some(e::DUPLICATE_TEXT)
    );
    assert_eq!(caption(&data, e::DUPLICATE_NULL_FIRST_ID), None);
}

#[test]
#[ignore = "waiting for P5b-7"]
fn captions_do_not_change_photos_or_counts() {
    let (_d, _c, data, report) = read();
    let plain_dir = tempfile::tempdir().unwrap();
    let plain = lrcat_fixtures::write_small_catalog_with_files(plain_dir.path()).unwrap();
    let mut plain_report = Report::new();
    let plain_data = Catalog::open(&plain)
        .unwrap()
        .photo_data(&mut plain_report)
        .unwrap();
    let ids = |d: &lr_reader::PhotoData| -> Vec<i64> {
        d.photos.iter().map(|p| p.lightroom_id).collect()
    };
    assert_eq!(ids(&data), ids(&plain_data));
    assert_eq!(report.read_count(), plain_report.read_count());
    assert_eq!(report.skipped_count(), plain_report.skipped_count());
    // Virtual copy 20 is not returned, whatever its caption.
    assert!(!ids(&data).contains(&20));
    assert!(plain_data.photos.iter().all(|p| p.caption.is_none()));
}

#[test]
#[ignore = "waiting for P5b-7"]
fn caption_text_stays_out_of_the_report() {
    let (_d, _c, _data, report) = read();
    for item in report.skipped_items() {
        for t in e::ALL_TEXTS {
            assert!(!item.reason.contains(t), "{}", item.reason);
        }
    }
}

#[test]
#[ignore = "waiting for P5b-7"]
fn second_read_is_identical() {
    let (_d, cat, a, _) = read();
    let mut report = Report::new();
    let b = Catalog::open(&cat)
        .unwrap()
        .photo_data(&mut report)
        .unwrap();
    assert_eq!(a, b);
}
