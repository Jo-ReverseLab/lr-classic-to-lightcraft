//! Acceptance tests for reading tone curves and the crop geometry (P6-3).
//!
//! Contract (Lead, P6-3):
//! - `ToneCurvePV2012` → `Curves.master`; `ToneCurvePV2012Red/Green/Blue` →
//!   `Curves.red/green/blue`; the old `ToneCurve` only when no
//!   `ToneCurvePV2012` exists.
//! - Point lists (`x1, y1, x2, y2, …`) are normalised from 0..255 to 0..1.
//! - A straight identity line `(0,0)–(255,255)` means "no curve".
//! - The three channel curves are read only when all three and the master are
//!   present; otherwise no channel curve is read.
//! - A broken curve list (odd number of values, a non-number) follows the
//!   P6-1 error path: the photo keeps no settings and is listed in
//!   `PhotoData::develop_invalid`; the run continues.
//! - `CropLeft/Top/Right/Bottom` become `CropGeometry::{x0,y0,x1,y1}` and
//!   `CropAngle` becomes `CropGeometry::angle`, in LightCraft's oriented,
//!   rotated frame (`lightroom_catalog.rs:737-765`, documented in
//!   `docs/research/develop-mapping.md` §4.9). A missing `HasCrop` is inferred
//!   from any crop field. The stored angle turns the other way, and a mirrored
//!   orientation flips that sign again.
//! - `CropAngle` is no longer a control (`crop.angle`) and no crop field is
//!   reported as unmapped. The unverified aspect/flip keys stay assumptions
//!   and are reported.
//!
//! The expected frames are derived from the documented rule, not from a real
//! catalog. Every test keeps the mandatory invariants: the catalog checksum is
//! the same before and after reading, the report balances, and a second read
//! is identical.

use lr_reader::{Catalog, PhotoData, checksum};
use lrcat_fixtures::develop::{
    self as dev, crop as c, curve as cv, edge as e, settings, write_crop_catalog_with_files,
    write_curves_catalog_with_files,
};
use model::{CropGeometry, CurvePoint, Develop, Report};
use std::path::Path;

/// Opens the catalog, reads the photos and checks the mandatory invariants.
fn read(cat: &Path) -> (PhotoData, Report) {
    let before = checksum(cat).unwrap();
    let catalog = Catalog::open(cat).unwrap();
    let mut report = Report::new();
    let data = catalog.photo_data(&mut report).expect("read photos");
    assert_eq!(
        checksum(cat).unwrap(),
        before,
        "catalog changed during read"
    );
    assert_eq!(
        report.read_count(),
        data.photos.len() + report.skipped_count(),
        "read rows = returned photos + skipped rows"
    );
    for _ in &data.photos {
        report.converted();
    }
    assert!(
        report.is_balanced(),
        "read = converted + skipped: read {}, converted {}, skipped {}",
        report.read_count(),
        report.converted_count(),
        report.skipped_count()
    );
    (data, report)
}

fn develop(data: &PhotoData, id: i64) -> &Develop {
    data.photos
        .iter()
        .find(|p| p.lightroom_id == id)
        .and_then(|p| p.develop.as_ref())
        .unwrap_or_else(|| panic!("photo {id} must have development settings"))
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "expected {expected}, got {actual}"
    );
}

/// The points of a stored 0..255 list, normalised to 0..1.
fn points(list: &[i64]) -> Vec<CurvePoint> {
    (0..list.len())
        .step_by(2)
        .map(|i| CurvePoint {
            x: list[i] as f64 / 255.0,
            y: list[i + 1] as f64 / 255.0,
        })
        .collect()
}

fn assert_points(actual: &[CurvePoint], store: &[i64]) {
    let expected = points(store);
    assert_eq!(
        actual.len(),
        expected.len(),
        "point count: {actual:?} vs {expected:?}"
    );
    for (a, e) in actual.iter().zip(&expected) {
        close(a.x, e.x);
        close(a.y, e.y);
    }
}

fn assert_rect(crop: &CropGeometry, x0: f64, y0: f64, x1: f64, y1: f64) {
    close(crop.x0, x0);
    close(crop.y0, y0);
    close(crop.x1, x1);
    close(crop.y1, y1);
}

#[test]
fn a_master_curve_is_normalised_to_the_unit_range() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_curves_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);
    let d = develop(&data, cv::MASTER_PHOTO);

    assert_points(&d.curves.master, &cv::MASTER_255);
    assert!(d.curves.red.is_empty());
    assert!(d.curves.green.is_empty());
    assert!(d.curves.blue.is_empty());
    assert!(
        d.unmapped.is_empty(),
        "a read curve is not unmapped: {:?}",
        d.unmapped
    );
    assert!(
        !d.controls.contains_key("crop.angle"),
        "a tone curve is not a control"
    );
}

#[test]
fn channel_curves_need_the_master_and_all_three() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_curves_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);

    // Master plus one channel: the master is read, no channel is.
    let one = develop(&data, cv::ONE_CHANNEL_PHOTO);
    assert_points(&one.curves.master, &cv::MASTER_255);
    assert!(one.curves.red.is_empty());
    assert!(one.curves.green.is_empty());
    assert!(one.curves.blue.is_empty());
    assert!(
        one.unmapped.is_empty(),
        "a channel curve Lightroom itself ignores is not a loss: {:?}",
        one.unmapped
    );

    // All three channels and the master: all four are read.
    let all = develop(&data, cv::ALL_CHANNELS_PHOTO);
    assert_points(&all.curves.master, &cv::MASTER_255);
    assert_points(&all.curves.red, &cv::RED_255);
    assert_points(&all.curves.green, &cv::GREEN_255);
    assert_points(&all.curves.blue, &cv::BLUE_255);
    assert!(all.unmapped.is_empty(), "{:?}", all.unmapped);

    // All three channels without a master: no channel is read.
    let no_master = develop(&data, cv::CHANNELS_WITHOUT_MASTER_PHOTO);
    assert!(no_master.curves.master.is_empty());
    assert!(no_master.curves.red.is_empty());
    assert!(no_master.curves.green.is_empty());
    assert!(no_master.curves.blue.is_empty());
    assert!(no_master.unmapped.is_empty(), "{:?}", no_master.unmapped);
}

#[test]
fn a_straight_identity_line_means_no_curve() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_curves_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);
    let d = develop(&data, cv::IDENTITY_PHOTO);

    assert!(
        d.curves.master.is_empty(),
        "identity line must not become a curve: {:?}",
        d.curves.master
    );
    assert!(d.curves.red.is_empty());
    assert!(d.curves.green.is_empty());
    assert!(d.curves.blue.is_empty());
    assert!(d.unmapped.is_empty(), "{:?}", d.unmapped);
}

#[test]
fn the_old_curve_is_used_only_without_a_2012_curve() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_curves_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);

    let old = develop(&data, cv::OLD_PHOTO);
    assert_points(&old.curves.master, &cv::OLD_255);
    assert!(
        !old.unmapped.iter().any(|n| n.starts_with("ToneCurve")),
        "the old curve is read, not reported: {:?}",
        old.unmapped
    );

    let both = develop(&data, cv::OLD_AND_NEW_PHOTO);
    assert_points(&both.curves.master, &cv::NEW_255);
    assert!(
        !both.unmapped.iter().any(|n| n.starts_with("ToneCurve")),
        "{:?}",
        both.unmapped
    );
}

#[test]
fn a_broken_curve_list_invalidates_the_row_and_the_run_continues() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_curves_catalog_with_files(dir.path()).unwrap();
    let (data, report) = read(&cat);

    assert_eq!(
        data.develop_invalid,
        vec![cv::ODD_LIST_PHOTO, cv::BROKEN_POINT_PHOTO],
        "an odd or non-numeric point list is an unreadable row"
    );
    for &id in &[cv::ODD_LIST_PHOTO, cv::BROKEN_POINT_PHOTO] {
        let photo = data
            .photos
            .iter()
            .find(|p| p.lightroom_id == id)
            .unwrap_or_else(|| panic!("photo {id} must stay in the catalog"));
        assert!(photo.develop.is_none(), "photo {id} must have no settings");
    }

    // The valid rows before and after the broken ones are still read.
    assert_points(
        &develop(&data, cv::MASTER_PHOTO).curves.master,
        &cv::MASTER_255,
    );
    assert_points(
        &develop(&data, cv::CHANNELS_WITHOUT_MASTER_PHOTO)
            .curves
            .master,
        &[],
    );
    assert!(report.is_balanced());
}

#[test]
fn a_upright_crop_keeps_its_normalised_edges() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_crop_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);
    let d = develop(&data, c::PLAIN_PHOTO);

    let crop = d.crop.expect("stored crop edges must become a crop");
    assert_rect(&crop, c::LEFT, c::TOP, c::RIGHT, c::BOTTOM);
    close(crop.angle, 0.0);
    assert!(
        !d.unmapped.iter().any(|n| n.starts_with("Crop")),
        "read crop fields are not unmapped: {:?}",
        d.unmapped
    );
    assert!(
        !d.controls.contains_key("crop.angle"),
        "the angle lives in the geometry, not in a control"
    );
}

#[test]
fn the_angle_turns_the_other_way_on_an_upright_photo() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_crop_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);

    let positive = develop(&data, c::ANGLE_POSITIVE_PHOTO).crop.expect("crop");
    close(positive.angle, -c::POSITIVE_ANGLE);

    let negative = develop(&data, c::ANGLE_NEGATIVE_PHOTO).crop.expect("crop");
    close(negative.angle, -c::NEGATIVE_ANGLE);

    for id in [c::ANGLE_POSITIVE_PHOTO, c::ANGLE_NEGATIVE_PHOTO] {
        let d = develop(&data, id);
        assert!(
            !d.unmapped.iter().any(|n| n == "CropAngle"),
            "CropAngle must not stay unmapped: {:?}",
            d.unmapped
        );
        assert!(!d.controls.contains_key("crop.angle"));
    }
}

#[test]
fn a_mirrored_orientation_turns_the_angle_back() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_crop_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);

    let normal = develop(&data, c::ANGLE_POSITIVE_PHOTO).crop.expect("crop");
    let mirrored = develop(&data, c::FLIP_H_ANGLE_PHOTO).crop.expect("crop");
    close(normal.angle, -c::POSITIVE_ANGLE);
    close(mirrored.angle, c::FLIP_ANGLE);
    assert_eq!(
        normal.angle.signum(),
        -mirrored.angle.signum(),
        "a mirror flips the sign of the straighten angle"
    );
}

#[test]
fn a_quarter_turn_maps_the_edges_into_the_oriented_frame() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_crop_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);
    let crop = develop(&data, c::ROTATE90_PHOTO).crop.expect("crop");

    // Reference values of the documented rule for `BC` and a 1.5 stored aspect.
    assert_rect(&crop, 0.1, 0.1, 0.8, 0.6);
    close(crop.angle, 0.0);
}

#[test]
fn a_horizontal_mirror_maps_the_edges_into_the_oriented_frame() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_crop_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);
    let crop = develop(&data, c::FLIP_H_PHOTO).crop.expect("crop");

    // `BA` mirrors x: 1 - right .. 1 - left; y stays.
    assert_rect(&crop, 0.4, 0.2, 0.9, 0.9);
    close(crop.angle, 0.0);
}

#[test]
fn an_angle_alone_means_the_photo_is_cropped() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_crop_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);
    let d = develop(&data, c::ANGLE_ONLY_PHOTO);

    let crop = d
        .crop
        .expect("a crop angle alone means the photo is cropped");
    close(crop.angle, -c::ANGLE_ONLY);
    assert!(
        !d.unmapped.iter().any(|n| n == "CropAngle"),
        "{:?}",
        d.unmapped
    );
}

#[test]
fn a_photo_without_crop_fields_keeps_no_crop() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_crop_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);
    let d = develop(&data, c::NONE_PHOTO);

    assert!(
        d.crop.is_none(),
        "no crop field means no crop: {:?}",
        d.crop
    );
    assert!(d.unmapped.is_empty(), "{:?}", d.unmapped);
}

#[test]
fn the_unverified_aspect_and_flip_keys_are_reported_not_mapped() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_crop_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);
    let d = develop(&data, c::ASSUMPTIONS_PHOTO);

    assert!(d.crop.is_some(), "the edges still become a crop");
    let mut got = d.unmapped.clone();
    got.sort();
    let mut want: Vec<String> = c::UNMAPPED_ASSUMPTIONS
        .iter()
        .map(|s| s.to_string())
        .collect();
    want.sort();
    assert_eq!(got, want, "each assumption reported once");
    assert_eq!(d.unmapped.len(), c::UNMAPPED_ASSUMPTIONS.len());
    for key in ["crop.aspect", "crop.flip_h", "crop.flip_v"] {
        assert!(!d.partial.contains_key(key), "{key} must not be mapped");
    }
}

#[test]
fn a_second_read_is_identical_and_creates_no_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_crop_catalog_with_files(dir.path()).unwrap();
    let (first, first_report) = read(&cat);
    let (second, second_report) = read(&cat);

    assert_eq!(first, second, "a second read must be identical");
    assert_eq!(first_report, second_report);
}

#[test]
fn reading_curves_and_crop_leaves_the_catalog_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_curves_catalog_with_files(dir.path()).unwrap();
    let before = checksum(&cat).unwrap();
    let catalog = Catalog::open(&cat).unwrap();
    let mut report = Report::new();
    let _ = catalog.photo_data(&mut report).unwrap();
    assert_eq!(checksum(&cat).unwrap(), before);
    for suffix in ["-wal", "-shm", "-journal"] {
        let side = format!("{}{suffix}", cat.display());
        assert!(!Path::new(&side).exists(), "reading must not create {side}");
    }
}

/// Keeps the fixture constants honest: they are a 0..255 pair list.
#[test]
fn the_stored_curve_fixtures_are_valid_pair_lists() {
    for list in [
        cv::MASTER_255.as_slice(),
        cv::RED_255.as_slice(),
        cv::GREEN_255.as_slice(),
        cv::BLUE_255.as_slice(),
        cv::OLD_255.as_slice(),
        cv::NEW_255.as_slice(),
    ] {
        assert_eq!(list.len() % 2, 0, "{list:?}");
        assert!(list.iter().all(|v| (0..=255).contains(v)), "{list:?}");
    }
    let _ = settings();
    let _ = e::ALL_SLIDERS_PHOTO;
    let _ = dev::lua_list(&cv::IDENTITY_255);
}
