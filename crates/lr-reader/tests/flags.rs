//! Acceptance tests for reading pick/reject flags (task P5b-1).
//!
//! Expected behaviour (needs `model::Flag`, `model::Photo::flag`):
//! - `Catalog::photos` maps `Adobe_images.pick`: 1 = `Flag::Pick`,
//!   -1 = `Flag::Reject`, 0 or NULL = `Flag::None`. INTEGER and REAL values
//!   are equivalent (1.0, -1.0). Any other value (2, 0.5, text) gives
//!   `Flag::None`; the photo is still returned (not skipped) and nothing is
//!   added to the photo report's skipped list. The count of such "unknown
//!   flag" photos is made visible by the CLI (see the CLI tests); how the
//!   reader hands it over is left to the developer.
//! - The flag of a skipped photo (virtual copy) is not read; the skipped list
//!   and `report.read()` count stay as before (read = every row).
//! - `AgLibraryCollectionImage.pick` is never read: every row of that table
//!   is 1 in the fixture, and no photo may get a flag from it.
//! - Nothing else about a photo changes; the catalog is not modified.
//! - Fixture: `lrcat_fixtures::write_flag_catalog_with_files`, ids in
//!   `lrcat_fixtures::flag_edge`. (The fixture column `pick` is nullable,
//!   unlike in a real catalog, so that NULL can be tested.)

use lr_reader::{Catalog, checksum};
use lrcat_fixtures::{flag_edge, small, write_flag_catalog_with_files, write_small_catalog};
use model::{Flag, Photo, Report};
use std::path::{Path, PathBuf};

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_flag_catalog_with_files(dir.path()).unwrap();
    (dir, cat)
}

fn read_photos(path: &Path) -> (Vec<Photo>, Report) {
    let catalog = Catalog::open(path).unwrap();
    let mut report = Report::new();
    let photos = catalog.photos(&mut report).expect("read photos");
    (photos, report)
}

fn flag_of(photos: &[Photo], id: i64) -> Flag {
    photos
        .iter()
        .find(|p| p.lightroom_id == id)
        .unwrap_or_else(|| panic!("photo {id} not returned"))
        .flag
}

macro_rules! flag_test {
    ($name:ident, $id:expr, $want:expr) => {
        #[test]
        fn $name() {
            let (_d, cat) = fixture();
            let (photos, _) = read_photos(&cat);
            assert_eq!(flag_of(&photos, $id), $want);
        }
    };
}

flag_test!(integer_one_is_pick, 4, Flag::Pick);
flag_test!(integer_minus_one_is_reject, 5, Flag::Reject);
flag_test!(integer_zero_is_none, 6, Flag::None);
flag_test!(null_is_none, 7, Flag::None);
flag_test!(real_one_is_pick, 8, Flag::Pick);
flag_test!(real_minus_one_is_reject, 9, Flag::Reject);
flag_test!(integer_two_is_none, 10, Flag::None);
flag_test!(text_is_none, 11, Flag::None);
flag_test!(real_half_is_none, 12, Flag::None);
// Photos 2 and 3 have missing originals; the reader does not check files.
flag_test!(missing_original_keeps_its_reject, 2, Flag::Reject);
flag_test!(missing_original_unknown_value_is_none, 3, Flag::None);

#[test]
fn unknown_values_do_not_skip_the_photo() {
    let (_d, cat) = fixture();
    let (photos, report) = read_photos(&cat);
    for id in flag_edge::UNKNOWN_IDS {
        assert!(photos.iter().any(|p| p.lightroom_id == id), "photo {id}");
    }
    let reasons: Vec<_> = report.skipped_items().iter().map(|s| &s.reason).collect();
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(reasons[0].starts_with("photo 20:") && reasons[0].contains("virtual copy"));
}

#[test]
fn skipped_virtual_copy_with_pick_is_not_returned_and_counts_as_read() {
    let (_d, cat) = fixture();
    let (photos, report) = read_photos(&cat);
    assert!(photos.iter().all(|p| p.lightroom_id != 20));
    assert_eq!(report.read_count() as i64, flag_edge::PHOTOS);
    assert_eq!(report.skipped_count(), 1);
    assert_eq!(photos.len() as i64, flag_edge::PHOTOS - 1);
}

#[test]
fn collection_pick_is_not_read() {
    let (_d, cat) = fixture();
    let (photos, _) = read_photos(&cat);
    let listed: Vec<i64> = flag_edge::PICK_VALUES.iter().map(|v| v.0).collect();
    for p in photos.iter().filter(|p| !listed.contains(&p.lightroom_id)) {
        assert_eq!(p.flag, Flag::None, "photo {}", p.lightroom_id);
    }
    let picks: Vec<i64> = photos
        .iter()
        .filter(|p| p.flag == Flag::Pick)
        .map(|p| p.lightroom_id)
        .collect();
    assert_eq!(picks, flag_edge::PICK_IDS);
    let rejects: Vec<i64> = photos
        .iter()
        .filter(|p| p.flag == Flag::Reject)
        .map(|p| p.lightroom_id)
        .collect();
    // Photo 2 (missing original) is returned by the reader and is a reject.
    assert_eq!(rejects, [2, 5, 9]);
}

#[test]
fn small_fixture_has_no_flags() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.lrcat");
    write_small_catalog(&path).unwrap();
    let (photos, _) = read_photos(&path);
    assert_eq!(photos.len() as i64, small::PHOTOS - small::VIRTUAL_COPIES);
    assert!(photos.iter().all(|p| p.flag == Flag::None));
}

#[test]
fn flags_do_not_change_other_fields_and_catalog_is_unchanged() {
    let (_d, cat) = fixture();
    let before = checksum(&cat).unwrap();
    let (a, _) = read_photos(&cat);
    let (b, _) = read_photos(&cat);
    assert_eq!(a, b, "second read must be identical");
    assert_eq!(checksum(&cat).unwrap(), before);
    // Same photos as in the small fixture, apart from the flag.
    let dir = tempfile::tempdir().unwrap();
    let small_cat = dir.path().join("small.lrcat");
    write_small_catalog(&small_cat).unwrap();
    let (plain, _) = read_photos(&small_cat);
    assert_eq!(plain.len(), a.len());
    for (x, y) in plain.iter().zip(&a) {
        assert_eq!(x.lightroom_id, y.lightroom_id);
        assert_eq!(x.rating, y.rating);
        assert_eq!(x.global_id, y.global_id);
    }
}
