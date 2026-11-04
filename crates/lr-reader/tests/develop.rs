//! Acceptance tests for reading development settings (P6-1: sliders).
//!
//! API under test:
//! * `Photo::develop: Option<model::Develop>`, filled by `Catalog::photo_data`
//!   from the optional table `Adobe_imageDevelopSettings` (`image`, `text`).
//! * `PhotoData::develop_invalid`: catalog ids of photos whose settings text
//!   cannot be read (ascending).
//! * `PhotoData::develop_deferred`: catalog ids of photos whose settings
//!   contain the `-999999` sentinel (ascending).
//! * `lr_reader::DEVELOP_MAX_TEXT_BYTES`: larger texts are invalid.
//!
//! The text is the Lua table literal Lightroom stores; it is parsed, never
//! executed. The fixture is `lrcat_fixtures::develop` with invented names and
//! values only.
//!
//! Every test keeps the mandatory invariants: the catalog checksum is the
//! same before and after reading, the report balances
//! (`read == converted + skipped`), and a second read returns identical data.

use lr_reader::{Catalog, DEVELOP_MAX_TEXT_BYTES, PhotoData, checksum};
use lrcat_fixtures::develop::{self as dev, edge as e, settings, write_develop_catalog_with_files};
use model::{Develop, MergeValue, Report, WhiteBalanceHint};
use std::collections::BTreeSet;
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
    // Every row read is either a returned photo or a skip. The develop table
    // must not add rows to the photo query and must not make rows disappear.
    assert_eq!(
        report.read_count(),
        data.photos.len() + report.skipped_count(),
        "read rows = returned photos + skipped rows"
    );
    // In the full run every returned photo is converted; do the same here so
    // the report balances.
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

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn photo(data: &PhotoData, id: i64) -> &model::Photo {
    data.photos
        .iter()
        .find(|p| p.lightroom_id == id)
        .unwrap_or_else(|| panic!("photo {id} was not returned"))
}

fn develop(data: &PhotoData, id: i64) -> &Develop {
    photo(data, id)
        .develop
        .as_ref()
        .unwrap_or_else(|| panic!("photo {id} has no development settings"))
}

#[test]
fn every_mapped_slider_lands_on_its_control_id() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_all_sliders_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);
    let d = develop(&data, e::ALL_SLIDERS_PHOTO);

    let expected = dev::all_controls();
    assert_eq!(
        d.controls.len(),
        expected.len(),
        "unexpected controls: {:?}",
        d.controls.keys().collect::<Vec<_>>()
    );
    for (key, control, value) in expected {
        assert_eq!(
            d.controls.get(&control).copied(),
            Some(value),
            "{key} must map to {control}"
        );
    }

    // Sliders alone: no structured parts, nothing lost, nothing deferred.
    assert!(d.partial.is_empty());
    assert!(d.unmapped.is_empty());
    assert!(!d.deferred);
    assert!(d.crop.is_none());
    assert!(d.curves.master.is_empty());
    assert!(d.curves.red.is_empty());
    assert!(d.curves.green.is_empty());
    assert!(d.curves.blue.is_empty());
    assert!(d.source_hash.is_some());
}

#[test]
fn the_eight_colour_bands_expand() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_all_sliders_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);
    let d = develop(&data, e::ALL_SLIDERS_PHOTO);

    for band in [
        "red", "orange", "yellow", "green", "aqua", "blue", "purple", "magenta",
    ] {
        for channel in ["hue", "sat", "lum"] {
            let id = format!("mixer.{band}.{channel}");
            assert!(d.controls.contains_key(&id), "missing {id}");
        }
        let id = format!("bw.{band}");
        assert!(d.controls.contains_key(&id), "missing {id}");
    }
    assert_eq!(
        d.controls
            .keys()
            .filter(|k| k.starts_with("mixer."))
            .count(),
        24
    );
    assert_eq!(
        d.controls.keys().filter(|k| k.starts_with("bw.")).count(),
        8
    );
}

#[test]
fn old_process_fields_use_the_documented_formulas() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_old_process_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);
    let d = develop(&data, e::OLD_PROCESS_PHOTO);

    let expected = dev::old_process_expected();
    assert_eq!(d.controls.len(), expected.len());
    for (control, value) in expected {
        assert_eq!(
            d.controls.get(control).copied(),
            Some(value),
            "old process must map to {control}"
        );
    }

    // The old point curve is phase 6c; until then its name is reported.
    assert!(
        d.unmapped.iter().any(|name| name == "ToneCurve"),
        "ToneCurve must be reported as unmapped, got {:?}",
        d.unmapped
    );
}

#[test]
fn a_2012_key_suppresses_the_whole_old_block() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_old_process_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);
    let d = develop(&data, e::OLD_SUPPRESSED_PHOTO);

    assert_eq!(
        d.controls.len(),
        1,
        "only the 2012 value may map, got {:?}",
        d.controls
    );
    assert_eq!(
        d.controls.get("light.exposure").copied(),
        Some(e::OLD_SUPPRESSED_EXPOSURE2012)
    );
}

#[test]
fn merge_only_scalars_go_to_the_partial_settings() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_merge_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);

    let on = develop(&data, e::MERGE_ON_PHOTO);
    assert!(on.controls.is_empty());
    assert_eq!(on.partial.len(), 4);
    assert_eq!(
        on.partial.get("treatment"),
        Some(&MergeValue::Text("bw".to_owned()))
    );
    assert_eq!(on.partial.get("hdr.enabled"), Some(&MergeValue::Bool(true)));
    assert_eq!(
        on.partial.get("optics.lens_profile"),
        Some(&MergeValue::Bool(true))
    );
    assert_eq!(
        on.partial.get("optics.remove_ca"),
        Some(&MergeValue::Bool(true))
    );
    assert!(on.unmapped.is_empty());

    let off = develop(&data, e::MERGE_OFF_PHOTO);
    assert_eq!(
        off.partial.get("treatment"),
        Some(&MergeValue::Text("color".to_owned()))
    );
    assert_eq!(
        off.partial.get("hdr.enabled"),
        Some(&MergeValue::Bool(false))
    );
    assert_eq!(
        off.partial.get("optics.lens_profile"),
        Some(&MergeValue::Bool(true))
    );
    assert_eq!(
        off.partial.get("optics.remove_ca"),
        Some(&MergeValue::Bool(false))
    );

    // A lens profile switched off is not carried: LightCraft only stores the
    // enable flag when it is true. The CA switch is carried either way.
    let disabled = develop(&data, e::MERGE_DISABLED_PHOTO);
    assert_eq!(disabled.partial.len(), 1);
    assert_eq!(
        disabled.partial.get("optics.remove_ca"),
        Some(&MergeValue::Bool(true))
    );
    assert!(!disabled.partial.contains_key("optics.lens_profile"));
    assert!(!disabled.partial.contains_key("treatment"));
    assert!(!disabled.partial.contains_key("hdr.enabled"));
}

#[test]
fn white_balance_modes_are_normalised() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_white_balance_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);

    for (id, stored, expected) in e::WB_MODES {
        let d = develop(&data, id);
        assert_eq!(
            d.white_balance.mode.as_deref(),
            Some(expected),
            "photo {id} stores {stored:?}"
        );
        assert!(
            d.controls.is_empty(),
            "photo {id}: a white-balance mode is not a slider"
        );
    }

    // Names outside the list are custom; the stored spelling is not kept.
    for (id, stored) in [
        (e::WB_CUSTOM_PHOTO, "Custom"),
        (e::WB_UNKNOWN_PHOTO, "Zyx-Odd"),
    ] {
        let d = develop(&data, id);
        assert_eq!(
            d.white_balance.mode.as_deref(),
            Some("custom"),
            "photo {id} stores {stored:?}"
        );
        assert_eq!(d.white_balance.hint, WhiteBalanceHint::None);
    }
}

#[test]
fn temperature_and_tint_are_reported_not_mapped() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_white_balance_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);

    let d = develop(&data, e::WB_ABSOLUTE_PHOTO);
    assert_eq!(d.white_balance.mode.as_deref(), Some("cloudy"));
    assert_eq!(
        d.white_balance.hint,
        WhiteBalanceHint::Undecidable,
        "a Kelvin value without an as-shot reference cannot be converted"
    );
    assert!(
        d.controls.is_empty(),
        "no probe for temperature is released"
    );
    assert_eq!(d.unmapped, names(&e::WB_UNMAPPED_ABSOLUTE));
}

#[test]
fn incremental_temperature_and_tint_are_relative() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_white_balance_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);

    let d = develop(&data, e::WB_RELATIVE_PHOTO);
    assert_eq!(d.white_balance.mode.as_deref(), Some("shade"));
    assert_eq!(d.white_balance.hint, WhiteBalanceHint::Relative);
    assert!(d.controls.is_empty());
    assert_eq!(d.unmapped, names(&e::WB_UNMAPPED_RELATIVE));
}

#[test]
fn a_row_without_white_balance_fields_has_no_hint() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_white_balance_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);

    let d = develop(&data, e::WB_NONE_PHOTO);
    assert!(d.white_balance.mode.is_none());
    assert_eq!(d.white_balance.hint, WhiteBalanceHint::None);
    assert!(d.unmapped.is_empty());
    assert_eq!(d.controls.get("color.vibrance").copied(), Some(0.5));
}

#[test]
fn the_sentinel_is_dropped_and_marked_deferred() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_deferred_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);

    assert_eq!(
        data.develop_deferred,
        vec![e::DEFERRED_PHOTO, e::NESTED_SENTINEL_PHOTO]
    );
    assert!(data.develop_invalid.is_empty());

    let d = develop(&data, e::DEFERRED_PHOTO);
    assert!(d.deferred);
    assert_eq!(d.controls.len(), 1);
    assert_eq!(
        d.controls.get("light.contrast").copied(),
        Some(e::DEFERRED_CONTRAST)
    );
    for value in d.controls.values() {
        assert_ne!(*value, e::SENTINEL, "sentinel reached the controls");
    }
    assert!(d.unmapped.is_empty());
}

#[test]
fn the_sentinel_is_dropped_inside_nested_tables_too() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_deferred_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);

    let d = develop(&data, e::NESTED_SENTINEL_PHOTO);
    assert!(
        d.deferred,
        "a sentinel inside a nested table must mark the photo deferred"
    );
    assert!(d.controls.is_empty());
    assert_eq!(d.unmapped, vec!["PointColors".to_owned()]);
    assert!(
        data.develop_deferred.contains(&e::NESTED_SENTINEL_PHOTO),
        "photo id must be listed in develop_deferred"
    );
}

#[test]
fn unknown_keys_are_reported_once_without_values() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_unmapped_catalog_with_files(dir.path()).unwrap();
    let (data, report) = read(&cat);
    let d = develop(&data, e::UNMAPPED_PHOTO);

    assert_eq!(
        d.controls.get("light.exposure").copied(),
        Some(1.0),
        "a known key next to unknown ones must still map"
    );

    let mut got = d.unmapped.clone();
    got.sort();
    let mut want = names(&e::UNMAPPED_NAMES);
    want.sort();
    assert_eq!(got, want, "every unknown key exactly once");
    assert_eq!(d.unmapped.len(), e::UNMAPPED_NAMES.len());

    let number = e::UNMAPPED_NUMBER.to_string();
    for name in &d.unmapped {
        assert!(!name.contains(&number), "{name} leaks a value");
        assert!(!name.contains(e::UNMAPPED_TEXT), "{name} leaks a value");
        assert!(!name.contains('='), "{name} is not a plain name");
        assert!(!name.contains('{'), "{name} is not a plain name");
    }
    for item in report.skipped_items() {
        assert!(!item.reason.contains(e::UNMAPPED_TEXT));
        assert!(!item.reason.contains(&number));
    }
}

#[test]
fn unknown_keys_stay_stable_across_runs() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_unmapped_catalog_with_files(dir.path()).unwrap();
    let (first, first_report) = read(&cat);
    let (second, second_report) = read(&cat);

    assert_eq!(first, second, "a second read must be identical");
    assert_eq!(first_report, second_report);
    let d = develop(&first, e::UNMAPPED_PHOTO);
    assert_eq!(d.unmapped.len(), e::UNMAPPED_NAMES.len());
}

#[test]
fn broken_and_oversized_text_keep_the_photo() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_invalid_catalog_with_files(dir.path()).unwrap();
    let (data, report) = read(&cat);

    assert!(
        dev::oversized_text().len() > DEVELOP_MAX_TEXT_BYTES,
        "the fixture must exceed DEVELOP_MAX_TEXT_BYTES"
    );
    assert_eq!(
        data.develop_invalid,
        vec![
            e::INVALID_UNBALANCED_PHOTO,
            e::INVALID_MISSING_VALUE_PHOTO,
            e::OVERSIZED_PHOTO
        ]
    );
    for &id in &data.develop_invalid {
        assert!(
            photo(&data, id).develop.is_none(),
            "photo {id} must keep no settings"
        );
    }

    // The run continues: valid rows before and after the broken ones are read.
    assert_eq!(
        develop(&data, e::ALL_SLIDERS_PHOTO)
            .controls
            .get("light.exposure")
            .copied(),
        Some(1.0)
    );
    assert_eq!(
        develop(&data, e::VALID_AFTER_INVALID_PHOTO)
            .controls
            .get("light.contrast")
            .copied(),
        Some(2.0)
    );
    assert_eq!(data.photos.len(), lrcat_fixtures::small::PHOTOS as usize);
    assert!(data.develop_deferred.is_empty());
    assert!(report.is_balanced());
}

#[test]
fn a_missing_settings_table_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_small_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);

    assert_eq!(data.photos.len(), lrcat_fixtures::small::PHOTOS as usize);
    assert!(data.photos.iter().all(|p| p.develop.is_none()));
    assert!(data.develop_invalid.is_empty());
    assert!(data.develop_deferred.is_empty());
}

#[test]
fn an_empty_settings_table_gives_no_develop() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_empty_table_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);

    assert_eq!(data.photos.len(), lrcat_fixtures::small::PHOTOS as usize);
    assert!(data.photos.iter().all(|p| p.develop.is_none()));
    assert!(data.develop_invalid.is_empty());
}

#[test]
fn an_empty_settings_text_is_an_empty_develop() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_empty_text_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);

    let d = develop(&data, e::EMPTY_TEXT_PHOTO);
    assert!(d.controls.is_empty());
    assert!(d.partial.is_empty());
    assert!(d.curves.master.is_empty());
    assert!(d.curves.red.is_empty());
    assert!(d.curves.green.is_empty());
    assert!(d.curves.blue.is_empty());
    assert!(d.crop.is_none());
    assert!(d.white_balance.mode.is_none());
    assert_eq!(d.white_balance.hint, WhiteBalanceHint::None);
    assert!(d.unmapped.is_empty());
    assert!(!d.deferred);
}

#[test]
fn a_virtual_copy_has_its_own_settings() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_virtual_copy_catalog_with_files(dir.path()).unwrap();
    let (data, _) = read(&cat);

    let master = develop(&data, e::VIRTUAL_MASTER_PHOTO);
    assert_eq!(master.controls.len(), 2);
    assert_eq!(
        master.controls.get("light.exposure").copied(),
        Some(e::VIRTUAL_MASTER_EXPOSURE)
    );
    assert_eq!(
        master.controls.get("light.contrast").copied(),
        Some(e::VIRTUAL_MASTER_CONTRAST)
    );

    let copy = develop(&data, e::VIRTUAL_COPY_PHOTO);
    assert_eq!(copy.controls.len(), 2);
    assert_eq!(
        copy.controls.get("light.exposure").copied(),
        Some(e::VIRTUAL_COPY_EXPOSURE)
    );
    assert_eq!(
        copy.controls.get("color.vibrance").copied(),
        Some(e::VIRTUAL_COPY_VIBRANCE)
    );
    assert!(
        !copy.controls.contains_key("light.contrast"),
        "a virtual copy must not inherit the master's settings"
    );
}

#[test]
fn the_parser_does_not_execute_embedded_code() {
    let dir = tempfile::tempdir().unwrap();
    let canary = dir.path().join("canary-must-survive.txt");
    std::fs::write(&canary, b"keep me").unwrap();
    let victim = dir.path().join("must-not-be-created.txt");

    let create = format!("os.execute('touch {}')", victim.display());
    let delete = format!("os.remove('{}')", canary.display());
    let text = settings()
        .text("ZyxPayloadCreate", &create)
        .text("ZyxPayloadDelete", &delete)
        .number("Exposure2012", 1.0)
        .build();
    let cat = write_develop_catalog_with_files(
        dir.path(),
        "develop-exec.lrcat",
        &[(e::ALL_SLIDERS_PHOTO, text)],
    )
    .unwrap();
    let (data, _) = read(&cat);

    assert_eq!(
        develop(&data, e::ALL_SLIDERS_PHOTO)
            .controls
            .get("light.exposure")
            .copied(),
        Some(1.0)
    );
    assert!(canary.exists(), "embedded code removed a file");
    assert!(!victim.exists(), "embedded code created a file");
    let d = develop(&data, e::ALL_SLIDERS_PHOTO);
    assert_eq!(
        d.unmapped,
        vec!["ZyxPayloadCreate".to_owned(), "ZyxPayloadDelete".to_owned()],
        "the payload is only a string value of an unknown key"
    );
}

#[test]
fn reading_leaves_the_catalog_and_its_directory_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_all_sliders_catalog_with_files(dir.path()).unwrap();
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

#[test]
fn a_second_read_is_identical_and_creates_no_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_balance_catalog_with_files(dir.path()).unwrap();
    let (first, first_report) = read(&cat);
    let (second, second_report) = read(&cat);

    assert_eq!(first, second, "a second read must be identical");
    assert_eq!(first_report, second_report);

    let ids: BTreeSet<i64> = first.photos.iter().map(|p| p.lightroom_id).collect();
    assert_eq!(ids.len(), first.photos.len(), "duplicate photo ids");
}

#[test]
fn develop_counts_add_up() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_balance_catalog_with_files(dir.path()).unwrap();
    let (data, report) = read(&cat);

    let total = data.photos.len();
    let mapped = data.photos.iter().filter(|p| p.develop.is_some()).count();
    assert_eq!(total, e::BALANCE_PHOTOS as usize);
    assert_eq!(data.develop_invalid, e::BALANCE_INVALID.to_vec());
    assert_eq!(
        mapped + data.develop_invalid.len(),
        total,
        "every settings row is either mapped or invalid"
    );
    assert_eq!(mapped, total - e::BALANCE_INVALID.len());
    assert_eq!(data.develop_deferred, vec![e::BALANCE_DEFERRED]);
    assert!(
        develop(&data, e::BALANCE_DEFERRED).deferred,
        "the deferred photo keeps its settings"
    );
    assert!(
        develop(&data, e::BALANCE_EMPTY_TEXT).controls.is_empty(),
        "an empty text is an empty develop, not an invalid row"
    );
    assert_eq!(report.read_count(), total);
    assert!(report.is_balanced());
}

#[test]
fn source_hash_follows_the_settings_text() {
    let dir = tempfile::tempdir().unwrap();
    let same = settings().number("Exposure2012", 1.0).build();
    let other = settings().number("Exposure2012", 2.0).build();
    let cat = write_develop_catalog_with_files(
        dir.path(),
        "develop-source-hash.lrcat",
        &[(4, same.clone()), (5, same), (6, other)],
    )
    .unwrap();
    let (data, _) = read(&cat);

    let a = develop(&data, 4)
        .source_hash
        .expect("a settings row must carry a source hash");
    let b = develop(&data, 5)
        .source_hash
        .expect("a settings row must carry a source hash");
    let c = develop(&data, 6)
        .source_hash
        .expect("a settings row must carry a source hash");
    assert_eq!(a, b, "identical text must hash identically");
    assert_ne!(a, c, "different text must hash differently");
    assert!(
        photo(&data, 7).develop.is_none(),
        "a photo without a row has no settings and no hash"
    );
}

#[test]
fn develop_settings_do_not_change_which_photos_are_returned() {
    let plain_dir = tempfile::tempdir().unwrap();
    let plain = lrcat_fixtures::write_small_catalog_with_files(plain_dir.path()).unwrap();
    let (plain_data, plain_report) = read(&plain);

    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_balance_catalog_with_files(dir.path()).unwrap();
    let (data, report) = read(&cat);

    // The settings table must not add, drop or reorder photos: both catalogs
    // describe the same photo set, and the report reads the same number of
    // rows. Which develop rows are invalid or deferred is not part of this
    // comparison; `develop_counts_add_up` covers that for the settings
    // catalog.
    let ids = |d: &PhotoData| -> Vec<i64> { d.photos.iter().map(|p| p.lightroom_id).collect() };
    assert_eq!(
        ids(&data),
        ids(&plain_data),
        "settings must not change which photos are returned"
    );
    assert_eq!(data.photos.len(), plain_data.photos.len());
    assert_eq!(report.read_count(), plain_report.read_count());

    // The flat catalog has no settings table at all, so nothing can be
    // invalid or deferred there.
    assert!(plain_data.develop_invalid.is_empty());
    assert!(plain_data.develop_deferred.is_empty());
}
