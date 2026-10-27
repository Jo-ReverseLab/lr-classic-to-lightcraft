//! Acceptance tests for reading titles (task P5b-7c).
//!
//! Switched off with `cfg(any())` until the API exists; the developer removes
//! that line and the `ignore` attributes.
//!
//! Expected API: `Photo::title` (`Option<String>`), filled by
//! `Catalog::photo_data`, and `PhotoData::title_unreadable: Vec<i64>`
//! (catalog ids of the returned photos with a broken blob, ascending; same
//! pattern as `unknown_flag`). Source: `Adobe_AdditionalMetadata.xmp`, a blob
//! of 4 header bytes plus a zlib stream of XMP (the length is not used). The
//! row with the smallest `id_local` per image decides.
//! - Title = `dc:title` (`rdf:Alt`): the `rdf:li` with `xml:lang="x-default"`,
//!   else the first `rdf:li`. XML entities (named and numeric) are decoded.
//!   Empty, whitespace-only or missing title: `None`. Otherwise the text is
//!   returned unchanged.
//! - A broken blob (shorter than 4 bytes, not zlib, not UTF-8) gives `None`,
//!   never skips the photo, and lists the photo in `title_unreadable`. A valid
//!   XMP without `dc:title`, a NULL `xmp` and a missing row are not unreadable.
//! - Table `Adobe_AdditionalMetadata` or column `xmp` missing: open succeeds,
//!   every title is `None`, `title_unreadable` is empty.
//! - Photos and counts do not change; the catalog stays unchanged; a second
//!   read is identical; the `Report` contains no title text.
//!
//! Fixtures: `lrcat_fixtures::write_title_catalog_with_files[_variant]`, see
//! `lrcat_fixtures::title_edge`.

use lr_reader::{Catalog, PhotoData, checksum};
use lrcat_fixtures::title_edge::{self as e, Variant};
use model::Report;
use std::path::{Path, PathBuf};

fn read_at(cat: &Path) -> (PhotoData, Report) {
    let before = checksum(cat).unwrap();
    let mut report = Report::new();
    let data = Catalog::open(cat)
        .unwrap()
        .photo_data(&mut report)
        .expect("read photos");
    assert_eq!(checksum(cat).unwrap(), before, "catalog changed");
    (data, report)
}

fn read() -> (tempfile::TempDir, PathBuf, PhotoData, Report) {
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_title_catalog_with_files(dir.path()).unwrap();
    let (data, report) = read_at(&cat);
    (dir, cat, data, report)
}

fn title(data: &PhotoData, id: i64) -> Option<&str> {
    data.photos
        .iter()
        .find(|p| p.lightroom_id == id)
        .unwrap_or_else(|| panic!("photo {id} not returned"))
        .title
        .as_deref()
}

#[test]
fn x_default_wins_over_earlier_entries() {
    let (_d, _c, data, _) = read();
    assert_eq!(title(&data, e::UMLAUT_ID), Some(e::UMLAUT_TITLE));
    assert_eq!(
        title(&data, e::WITH_DESCRIPTION_ID),
        Some(e::WITH_DESCRIPTION_TITLE)
    );
    assert_eq!(title(&data, e::PLAIN_ID), Some(e::PLAIN_TITLE));
}

#[test]
fn without_x_default_the_first_entry_wins() {
    let (_d, _c, data, _) = read();
    assert_eq!(title(&data, e::OTHER_LANG_ID), Some(e::OTHER_LANG_TITLE));
    assert_eq!(title(&data, e::MULTI_ID), Some(e::MULTI_TITLE));
}

#[test]
fn xml_entities_are_decoded() {
    let (_d, _c, data, _) = read();
    assert_eq!(title(&data, e::ENTITY_ID), Some(e::ENTITY_TITLE));
}

#[test]
fn empty_blank_missing_title_null_and_no_row_give_none() {
    let (_d, _c, data, _) = read();
    for id in [
        e::EMPTY_ID,
        e::BLANK_ID,
        e::NO_TITLE_ID,
        e::NULL_XMP_ID,
        e::NO_ROW_ID,
        e::NULL_FIRST_ID,
    ] {
        assert_eq!(title(&data, id), None, "photo {id}");
    }
}

#[test]
fn smallest_id_local_wins() {
    let (_d, _c, data, _) = read();
    assert_eq!(title(&data, e::DUPLICATE_ID), Some(e::DUPLICATE_TITLE));
    assert_eq!(title(&data, e::NULL_FIRST_ID), None);
    assert_eq!(title(&data, e::BROKEN_FIRST_ID), None);
}

#[test]
fn broken_blobs_give_none_keep_the_photo_and_are_listed() {
    let (_d, _c, data, _) = read();
    for id in e::UNREADABLE_IDS {
        assert_eq!(title(&data, id), None, "photo {id}");
    }
    assert_eq!(data.title_unreadable, e::UNREADABLE_IDS.to_vec());
    // Missing title, NULL and empty are not "unreadable"; skipped photos
    // (20 has a broken blob) are never listed.
    assert!(!data.title_unreadable.contains(&e::NO_TITLE_ID));
    assert!(!data.title_unreadable.contains(&20));
}

#[test]
#[ignore = "waiting for P5b-7c"]
fn photos_and_counts_are_unchanged_by_titles() {
    let (_d, _c, data, report) = read();
    let plain_dir = tempfile::tempdir().unwrap();
    let plain = lrcat_fixtures::write_small_catalog_with_files(plain_dir.path()).unwrap();
    let (plain_data, plain_report) = read_at(&plain);
    let ids = |d: &PhotoData| -> Vec<i64> { d.photos.iter().map(|p| p.lightroom_id).collect() };
    assert_eq!(ids(&data), ids(&plain_data));
    assert_eq!(report.read_count(), plain_report.read_count());
    assert_eq!(report.skipped_count(), plain_report.skipped_count());
    assert!(plain_data.photos.iter().all(|p| p.title.is_none()));
    assert!(plain_data.title_unreadable.is_empty());
    let with = data.photos.iter().filter(|p| p.title.is_some()).count();
    assert_eq!(with as i64, e::TITLES + 1); // photo 2 is returned by the reader; only the CLI skips it (missing file)
    for id in e::SKIPPED_IDS {
        assert!(title(&data, id).is_none() || id == 2 || id == 3);
    }
}

#[test]
fn missing_table_or_column_is_not_an_error() {
    let (_d, _c, full, full_report) = read();
    for variant in [Variant::NoTable, Variant::NoColumn] {
        let dir = tempfile::tempdir().unwrap();
        let cat =
            lrcat_fixtures::write_title_catalog_with_files_variant(dir.path(), variant).unwrap();
        let (data, report) = read_at(&cat);
        assert!(data.photos.iter().all(|p| p.title.is_none()), "{variant:?}");
        assert!(data.title_unreadable.is_empty(), "{variant:?}");
        assert_eq!(data.photos.len(), full.photos.len());
        assert_eq!(report.read_count(), full_report.read_count());
        assert_eq!(report.skipped_count(), full_report.skipped_count());
    }
}

#[test]
fn title_text_stays_out_of_the_report() {
    let (_d, _c, _data, report) = read();
    for item in report.skipped_items() {
        for t in e::ALL_TEXTS {
            assert!(!item.reason.contains(t), "{}", item.reason);
        }
    }
}

#[test]
fn second_read_is_identical() {
    let (_d, cat, a, _) = read();
    let (b, _) = read_at(&cat);
    assert_eq!(a, b);
}
