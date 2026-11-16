//! Acceptance tests for white balance in the reader (P6-4, phase 6a).
//!
//! Contract checked here (Lead, P6-4, the probe is not implemented yet):
//! - `WhiteBalance` is normalised to LightCraft's `wb.mode` spelling
//!   (`asShot`, `auto`, `daylight`, `cloudy`, `shade`, `tungsten`,
//!   `fluorescent`, `flash`, `custom`); the stored spelling is not kept.
//! - A named mode carries the white balance: its stored temperature and tint
//!   are dropped and the hint is `None`.
//! - With mode `custom`, `IncrementalTemperature`/`IncrementalTint` set
//!   `WhiteBalanceHint::Relative` and an absolute pair sets
//!   `WhiteBalanceHint::Undecidable`; the values are kept in
//!   `Develop::white_balance`, never in `controls`, `partial` or `unmapped`.
//!   (The report counts these photos; see the CLI tests.)
//! - A mode without a temperature stays `None`: nothing is invented.
//! - Reading never opens an original photo, even when the placeholder files
//!   are unreadable.
//!
//! Mandatory in every test: the catalog checksum is unchanged, the report
//! balances (`read == converted + skipped`), and a second read is identical.
//! The fixture is `lrcat_fixtures::develop` with invented names and values
//! only.

use lr_reader::{Catalog, PhotoData, checksum};
use lrcat_fixtures::develop::{edge as e, write_white_balance_catalog_with_files};
use model::{Develop, Report, WhiteBalanceHint};
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
        .unwrap_or_else(|| panic!("photo {id} has no development settings"))
}

#[test]
fn every_stored_mode_becomes_the_lightcraft_spelling() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_white_balance_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);

    for (id, stored, expected) in e::WB_MODES {
        let d = develop(&data, id);
        assert_eq!(
            d.white_balance.mode.as_deref(),
            Some(expected),
            "photo {id} stores {stored:?}"
        );
        // A named mode carries the white balance; stored numbers (photos 4
        // and 5) are dropped, as in LightCraft's own import.
        assert_eq!(
            d.white_balance.hint,
            WhiteBalanceHint::None,
            "photo {id}: a named mode carries no temperature"
        );
        assert_eq!(
            (d.white_balance.temperature, d.white_balance.tint),
            (None, None),
            "photo {id}"
        );
        assert!(
            d.controls.is_empty() && d.partial.is_empty(),
            "photo {id}: a white-balance mode is not a slider or merge value"
        );
        assert!(
            d.unmapped.is_empty(),
            "photo {id}: WhiteBalance is mapped, nothing is unmapped"
        );
    }

    // Names outside the list fall back to `custom`; the stored spelling and
    // the invented name are not kept anywhere.
    for id in [e::WB_CUSTOM_PHOTO, e::WB_UNKNOWN_PHOTO] {
        let d = develop(&data, id);
        assert_eq!(d.white_balance.mode.as_deref(), Some("custom"));
        assert!(d.controls.is_empty() && d.partial.is_empty());
        assert!(d.unmapped.is_empty());
    }
}

#[test]
fn a_custom_kelvin_pair_is_kept_for_the_lightcraft_scale_check() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_white_balance_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);
    let d = develop(&data, e::WB_ABSOLUTE_PHOTO);

    assert_eq!(d.white_balance.mode.as_deref(), Some("custom"));
    assert_eq!(
        d.white_balance.hint,
        WhiteBalanceHint::Undecidable,
        "a Kelvin value is carried only when LightCraft edits the photo in Kelvin"
    );
    assert_eq!(d.white_balance.temperature, Some(e::WB_TEMPERATURE));
    assert_eq!(d.white_balance.tint, Some(e::WB_TINT));
    assert!(
        d.controls.is_empty() && d.partial.is_empty(),
        "temperature and tint are neither controls nor merge values"
    );
    assert!(d.unmapped.is_empty(), "{:?}", d.unmapped);
}

#[test]
fn a_custom_incremental_pair_is_relative() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_white_balance_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);
    let d = develop(&data, e::WB_RELATIVE_PHOTO);

    assert_eq!(d.white_balance.mode.as_deref(), Some("custom"));
    assert_eq!(d.white_balance.hint, WhiteBalanceHint::Relative);
    assert_eq!(
        d.white_balance.temperature,
        Some(e::WB_INCREMENTAL_TEMPERATURE)
    );
    assert_eq!(d.white_balance.tint, Some(e::WB_INCREMENTAL_TINT));
    assert!(d.controls.is_empty() && d.partial.is_empty());
    assert!(d.unmapped.is_empty(), "{:?}", d.unmapped);
}

#[test]
fn a_named_mode_drops_its_stored_numbers() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_white_balance_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);
    for id in [e::WB_NAMED_ABSOLUTE_PHOTO, e::WB_NAMED_RELATIVE_PHOTO] {
        let d = develop(&data, id);
        assert_eq!(d.white_balance.hint, WhiteBalanceHint::None, "photo {id}");
        assert_eq!(d.white_balance.temperature, None, "photo {id}");
        assert!(d.unmapped.is_empty(), "photo {id}: {:?}", d.unmapped);
    }
}

#[test]
fn a_row_without_white_balance_fields_has_no_mode_and_no_hint() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_white_balance_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);
    let d = develop(&data, e::WB_NONE_PHOTO);

    assert!(d.white_balance.mode.is_none());
    assert_eq!(d.white_balance.hint, WhiteBalanceHint::None);
    assert!(d.unmapped.is_empty());
    // The row is still read: an unrelated slider is unaffected.
    assert_eq!(d.controls.get("color.vibrance").copied(), Some(0.5));
}

#[test]
fn a_second_read_returns_identical_white_balance_data() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_white_balance_catalog_with_files(dir.path()).unwrap();
    let (first, first_report) = read(&cat);
    let (second, second_report) = read(&cat);

    assert_eq!(first, second, "a second read must be identical");
    assert_eq!(first_report, second_report);
    assert_eq!(
        develop(&second, e::WB_ABSOLUTE_PHOTO).white_balance.hint,
        WhiteBalanceHint::Undecidable
    );
    assert_eq!(
        develop(&second, e::WB_RELATIVE_PHOTO).white_balance.hint,
        WhiteBalanceHint::Relative
    );
}

/// The probe is not implemented: reading a catalog must never open a photo file.
/// The placeholder originals are made unreadable; a reader that opened them
/// would have to fail (or at least lose the photo). The read still succeeds.
#[cfg(unix)]
#[test]
fn reading_never_opens_an_original_photo() {
    use std::fs::File;
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let cat = write_white_balance_catalog_with_files(dir.path()).unwrap();

    let mut blocked = Vec::new();
    make_originals_unreadable(dir.path(), &mut blocked);
    assert!(!blocked.is_empty(), "the fixture must create originals");
    for path in &blocked {
        assert!(
            File::open(path).is_err(),
            "the canary at {} is still readable",
            path.display()
        );
    }

    let (data, _) = read(&cat);
    assert_eq!(
        develop(&data, e::WB_ABSOLUTE_PHOTO).white_balance.hint,
        WhiteBalanceHint::Undecidable
    );
    assert_eq!(data.photos.len(), lrcat_fixtures::small::PHOTOS as usize);

    for path in &blocked {
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o600);
        let _ = std::fs::set_permissions(path, permissions);
    }
}

/// Takes read permission from every file below `root` except the catalog.
#[cfg(unix)]
fn make_originals_unreadable(root: &Path, blocked: &mut Vec<std::path::PathBuf>) {
    use std::os::unix::fs::PermissionsExt;

    for entry in std::fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            make_originals_unreadable(&path, blocked);
            continue;
        }
        if path.extension().is_some_and(|ext| ext == "lrcat") {
            continue;
        }
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o000);
        std::fs::set_permissions(&path, permissions).unwrap();
        blocked.push(path);
    }
}

/// Keeps the fixture honest: the report counter (see the CLI tests) counts
/// photos whose hint is not `None`.
#[test]
fn the_fixture_has_exactly_two_photos_with_an_unmapped_temperature_pair() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_white_balance_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);

    let counted = data
        .photos
        .iter()
        .filter(|p| {
            p.develop
                .as_ref()
                .is_some_and(|d| d.white_balance.hint != WhiteBalanceHint::None)
        })
        .count();
    assert_eq!(counted, 2, "one absolute and one incremental pair");
}
