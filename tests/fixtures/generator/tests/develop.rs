//! Checks of the development-settings fixtures themselves.

use lrcat_fixtures::develop::{self as dev, edge as e, settings};
use rusqlite::{Connection, OpenFlags, types::Value};

fn open(cat: &std::path::Path) -> Connection {
    Connection::open_with_flags(cat, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

#[test]
fn the_builder_writes_a_lua_table() {
    let text = settings()
        .number("Exposure2012", 1.25)
        .integer("HDREditMode", 1)
        .boolean("AutoLateralCA", true)
        .text("WhiteBalance", "As Shot")
        .raw("ToneCurve", "{ 0, 0, 255, 255 }")
        .build();
    assert_eq!(
        text,
        "s = {\n\
         \tExposure2012 = 1.25,\n\
         \tHDREditMode = 1,\n\
         \tAutoLateralCA = true,\n\
         \tWhiteBalance = \"As Shot\",\n\
         \tToneCurve = { 0, 0, 255, 255 },\n\
         }"
    );
}

#[test]
fn dotted_and_foreign_keys_are_bracketed() {
    let text = settings()
        .number("enhance.denoise", 40.0)
        .text("Mystery Key", "x")
        .build();
    assert!(text.contains("[\"enhance.denoise\"] = 40.0,"), "{text}");
    assert!(text.contains("[\"Mystery Key\"] = \"x\","), "{text}");
}

#[test]
fn all_sliders_are_unique_and_cover_the_bands() {
    let controls = dev::all_controls();
    assert_eq!(controls.len(), 113);

    let mut keys: Vec<&str> = controls.iter().map(|(k, _, _)| k.as_str()).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), controls.len(), "duplicate Lightroom key");

    let mut ids: Vec<&str> = controls.iter().map(|(_, c, _)| c.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), controls.len(), "duplicate control id");

    assert_eq!(
        controls
            .iter()
            .filter(|(_, c, _)| c.starts_with("mixer."))
            .count(),
        24
    );
    assert_eq!(
        controls
            .iter()
            .filter(|(_, c, _)| c.starts_with("bw."))
            .count(),
        8
    );
    for band in [
        "red", "orange", "yellow", "green", "aqua", "blue", "purple", "magenta",
    ] {
        for channel in ["hue", "sat", "lum"] {
            assert!(ids.contains(&format!("mixer.{band}.{channel}").as_str()));
        }
        assert!(ids.contains(&format!("bw.{band}").as_str()));
    }
    // No zero values: a later "drop defaults" filter cannot hide a slot.
    assert!(controls.iter().all(|(_, _, v)| *v != 0.0));
}

#[test]
fn rows_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let text = settings().number("Exposure2012", 1.25).build();
    let cat = dev::write_develop_catalog_with_files(
        dir.path(),
        "develop-round-trip.lrcat",
        &[(e::ALL_SLIDERS_PHOTO, text.clone())],
    )
    .unwrap();

    let c = open(&cat);
    let (image, stored): (i64, Value) = c
        .query_row(
            &format!("SELECT image, text FROM {}", dev::TABLE),
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(image, e::ALL_SLIDERS_PHOTO);
    assert_eq!(stored, Value::Text(text));
}

#[test]
fn other_fixtures_have_no_settings_table() {
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_small_catalog_with_files(dir.path()).unwrap();
    let c = open(&cat);
    let n: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [dev::TABLE],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0, "the settings table must stay optional");
}

#[test]
fn the_empty_table_fixture_has_the_table_but_no_rows() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_empty_table_catalog_with_files(dir.path()).unwrap();
    let c = open(&cat);
    let n: i64 = c
        .query_row(&format!("SELECT COUNT(*) FROM {}", dev::TABLE), [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn the_empty_text_fixture_stores_an_empty_string() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_empty_text_catalog_with_files(dir.path()).unwrap();
    let c = open(&cat);
    let text: Value = c
        .query_row(
            &format!("SELECT text FROM {} WHERE image = ?1", dev::TABLE),
            [e::EMPTY_TEXT_PHOTO],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(text, Value::Text(String::new()));
}

#[test]
fn the_invalid_fixture_holds_broken_and_oversized_text() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_invalid_catalog_with_files(dir.path()).unwrap();
    let c = open(&cat);
    let text = |image: i64| -> String {
        c.query_row(
            &format!("SELECT text FROM {} WHERE image = ?1", dev::TABLE),
            [image],
            |r| r.get(0),
        )
        .unwrap()
    };

    assert!(!text(e::INVALID_UNBALANCED_PHOTO).ends_with('}'));
    assert!(text(e::INVALID_MISSING_VALUE_PHOTO).contains("= }"));
    assert!(text(e::OVERSIZED_PHOTO).len() > 16 * 1024 * 1024);
    assert!(text(e::ALL_SLIDERS_PHOTO).starts_with("s = {"));
    assert!(text(e::VALID_AFTER_INVALID_PHOTO).contains("Contrast2012"));
}

#[test]
fn the_balance_fixture_has_one_row_per_photo() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_balance_catalog_with_files(dir.path()).unwrap();
    let c = open(&cat);
    let n: i64 = c
        .query_row(&format!("SELECT COUNT(*) FROM {}", dev::TABLE), [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(n, e::BALANCE_PHOTOS);
}

#[test]
fn the_deferred_fixture_contains_the_sentinel() {
    let dir = tempfile::tempdir().unwrap();
    let cat = dev::write_deferred_catalog_with_files(dir.path()).unwrap();
    let c = open(&cat);
    let text: String = c
        .query_row(
            &format!("SELECT text FROM {} WHERE image = ?1", dev::TABLE),
            [e::DEFERRED_PHOTO],
            |r| r.get(0),
        )
        .unwrap();
    assert!(text.contains("-999999"), "{text}");
}
