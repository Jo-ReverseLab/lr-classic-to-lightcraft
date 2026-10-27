//! Acceptance tests for reading copyright and creator (task P5b-7b).
//!
//! Switched off with `cfg(any())` until the API exists; the developer removes
//! that line and the `ignore` attributes.
//!
//! Expected API: `Photo::copyright` and `Photo::creator` (`Option<String>`),
//! filled by `Catalog::photo_data`:
//! - copyright from `AgLibraryIPTC.copyright`, same rules as the caption:
//!   byte for byte, NULL / empty / non-text / no row give `None`, the row
//!   with the smallest `id_local` per image decides;
//! - creator from `AgHarvestedIptcMetadata.creatorRef` ->
//!   `AgInternedIptcCreator.value` (`id_local`): smallest harvested
//!   `id_local` per image decides; creatorRef NULL, a dangling creatorRef,
//!   an empty or non-text value give `None` and never skip the photo;
//! - catalogs without the two creator tables open fine, creator is `None`
//!   for every photo and the copyright still works;
//! - neither value changes which photos are returned or the counts;
//! - the catalog stays unchanged; a second read is identical; the `Report`
//!   contains no text of either value.
//!
//! Fixtures: `lrcat_fixtures::write_rights_catalog_with_files` and
//! `write_copyright_only_catalog_with_files`, see `lrcat_fixtures::rights_edge`.

use lr_reader::{Catalog, PhotoData, checksum};
use lrcat_fixtures::rights_edge as e;
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
    let cat = lrcat_fixtures::write_rights_catalog_with_files(dir.path()).unwrap();
    let (data, report) = read_at(&cat);
    (dir, cat, data, report)
}

fn photo(data: &PhotoData, id: i64) -> &model::Photo {
    data.photos
        .iter()
        .find(|p| p.lightroom_id == id)
        .unwrap_or_else(|| panic!("photo {id} not returned"))
}

#[test]
fn copyright_is_returned_byte_for_byte() {
    let (_d, _c, data, _) = read();
    assert_eq!(
        photo(&data, e::UMLAUT_ID).copyright.as_deref(),
        Some(e::UMLAUT_COPYRIGHT)
    );
    assert_eq!(
        photo(&data, e::PADDED_ID).copyright.as_deref(),
        Some(e::PADDED_COPYRIGHT)
    );
}

#[test]
fn copyright_empty_null_missing_and_non_text_give_none() {
    let (_d, _c, data, _) = read();
    for id in [
        e::EMPTY_ID,
        e::TWO_HARVESTED_ID,
        e::NO_ROW_ID,
        e::NON_TEXT_ID,
        1,
        12,
    ] {
        assert_eq!(photo(&data, id).copyright, None, "photo {id}");
    }
}

#[test]
fn copyright_smallest_id_local_wins() {
    let (_d, _c, data, _) = read();
    assert_eq!(
        photo(&data, e::DUPLICATE_ID).copyright.as_deref(),
        Some(e::DUPLICATE_COPYRIGHT)
    );
    assert_eq!(photo(&data, e::DUPLICATE_NULL_FIRST_ID).copyright, None);
}

#[test]
fn creator_is_resolved_through_the_interned_table() {
    let (_d, _c, data, _) = read();
    assert_eq!(
        photo(&data, e::UMLAUT_ID).creator.as_deref(),
        Some(e::UMLAUT_CREATOR)
    );
    for id in e::SHARED_CREATOR_IDS {
        assert_eq!(photo(&data, id).creator.as_deref(), Some(e::FIRST_CREATOR));
    }
}

#[test]
fn creator_smallest_harvested_id_local_wins() {
    let (_d, _c, data, _) = read();
    assert_eq!(
        photo(&data, e::TWO_HARVESTED_ID).creator.as_deref(),
        Some(e::FIRST_CREATOR)
    );
    // Photo 9: row 400 has creatorRef NULL and wins over row 401.
    assert_eq!(photo(&data, e::DUPLICATE_ID).creator, None);
}

#[test]
fn creator_null_dangling_empty_non_text_and_missing_give_none_without_skipping() {
    let (_d, _c, data, _) = read();
    // 5: creatorRef NULL, 6: dangling ref, 10: empty value, 11: integer value,
    // 8: no row, 14: no row.
    for id in [
        e::PADDED_ID,
        e::EMPTY_ID,
        e::DUPLICATE_NULL_FIRST_ID,
        e::NON_TEXT_ID,
        e::NO_ROW_ID,
        14,
    ] {
        assert_eq!(photo(&data, id).creator, None, "photo {id}");
    }
}

#[test]
fn photos_and_counts_are_unchanged_by_rights_data() {
    let (_d, _c, data, report) = read();
    let plain_dir = tempfile::tempdir().unwrap();
    let plain = lrcat_fixtures::write_small_catalog_with_files(plain_dir.path()).unwrap();
    let (plain_data, plain_report) = read_at(&plain);
    let ids = |d: &PhotoData| -> Vec<i64> { d.photos.iter().map(|p| p.lightroom_id).collect() };
    assert_eq!(ids(&data), ids(&plain_data));
    assert_eq!(report.read_count(), plain_report.read_count());
    assert_eq!(report.skipped_count(), plain_report.skipped_count());
    // Virtual copy 20 is not returned, whatever its values.
    assert!(!ids(&data).contains(&20));
    assert!(
        plain_data
            .photos
            .iter()
            .all(|p| p.copyright.is_none() && p.creator.is_none())
    );
}

#[test]
fn missing_creator_tables_are_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_copyright_only_catalog_with_files(dir.path()).unwrap();
    let (data, report) = read_at(&cat);
    let (_d, _c, full, full_report) = read();
    assert!(data.photos.iter().all(|p| p.creator.is_none()));
    assert_eq!(
        photo(&data, e::UMLAUT_ID).copyright.as_deref(),
        Some(e::UMLAUT_COPYRIGHT)
    );
    assert_eq!(data.photos.len(), full.photos.len());
    assert_eq!(report.read_count(), full_report.read_count());
    assert_eq!(report.skipped_count(), full_report.skipped_count());
}

#[test]
fn rights_text_stays_out_of_the_report() {
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
