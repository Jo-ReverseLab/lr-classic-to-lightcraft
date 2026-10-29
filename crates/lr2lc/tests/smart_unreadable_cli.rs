//! CLI acceptance test: a smart collection that was written
//! earlier and whose rules are now UNREADABLE (not merely untranslatable)
//! counts as `smart_no_longer_translatable`, not as removed. It still
//! exists in Lightroom.
//!
//! Setup: first run with `--apply` writes the smart albums of the
//! `smart_translate` fixture; then the rules of collection 101 are
//! destroyed (cut off text, deleted row, NULL content, dropped table).
//! Second run: `writer.smart_no_longer_translatable == 1` (a dropped table
//! hits every written smart album: 4), `writer.collections_removed == 0`,
//! no text line `removed`, no album command sent, catalog unchanged
//! (checked inside `go`), a third run gives the same numbers.

use lrcat_fixtures::{RuleDamage, smart_translate as t};
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::process::Command;

use fake_lightcraft::{Config, FakeLightCraft};
use lc_test_support::fake_lightcraft;

fn n(v: &Value) -> usize {
    v.as_u64().unwrap_or_else(|| panic!("not a number: {v}")) as usize
}

fn go(
    catalog: &std::path::Path,
    work: &std::path::Path,
    port: u16,
    apply: bool,
) -> (Value, String) {
    let library = work.join("Test.lightcraft");
    let report = work.join("report.json");
    let port_text = port.to_string();
    let mut args: Vec<&OsStr> = vec![
        catalog.as_os_str(),
        OsStr::new("--library"),
        library.as_os_str(),
        OsStr::new("--port"),
        OsStr::new(&port_text),
        OsStr::new("--report"),
        report.as_os_str(),
    ];
    if apply {
        args.push(OsStr::new("--apply"));
    }
    let before = fs::read(catalog).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(work)
        .args(args)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(before, fs::read(catalog).unwrap(), "catalog changed");
    (
        serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn check(damage: RuleDamage, expected: usize) {
    let photos = tempfile::tempdir().unwrap();
    let catalog = lrcat_fixtures::write_smart_translate_catalog_with_files(photos.path()).unwrap();
    let work = tempfile::tempdir().unwrap();
    let fake = FakeLightCraft::start(Config::new(&work.path().join("Test.lightcraft")));
    let (first, _) = go(&catalog, work.path(), fake.port(), true);
    assert_eq!(
        n(&first["writer"]["smart_created"]),
        t::TRANSLATABLE.len(),
        "{damage:?}"
    );
    let commands = fake.commands().len();
    let albums = fake.albums();

    lrcat_fixtures::break_smart_rules(&catalog, 101, damage).unwrap();
    let (r, text) = go(&catalog, work.path(), fake.port(), true);
    let w = &r["writer"];
    assert_eq!(
        n(&w["smart_no_longer_translatable"]),
        expected,
        "{damage:?} {w}"
    );
    assert_eq!(n(&w["collections_removed"]), 0, "{damage:?} {w}");
    assert_eq!(n(&w["smart_created"]), 0, "{damage:?}");
    assert!(
        text.lines()
            .any(|l| l.contains("no longer translatable") && l.contains(&expected.to_string())),
        "{damage:?}: {text}"
    );
    assert!(
        !text
            .lines()
            .any(|l| l.contains("removed") && !l.contains('0')),
        "{damage:?}: must not be shown as removed: {text}"
    );
    let new_commands = &fake.commands()[commands..];
    assert!(
        new_commands
            .iter()
            .all(|(c, _)| !c.starts_with("album.") && c != "library.select"),
        "{damage:?}: {new_commands:?}"
    );
    assert_eq!(fake.albums(), albums, "{damage:?}");

    let (again, _) = go(&catalog, work.path(), fake.port(), true);
    assert_eq!(
        n(&again["writer"]["smart_no_longer_translatable"]),
        expected,
        "{damage:?}"
    );
    assert_eq!(n(&again["writer"]["collections_removed"]), 0, "{damage:?}");
    assert_eq!(fake.albums(), albums, "{damage:?}");
}

#[test]
fn truncated_rules_count_as_no_longer_translatable() {
    check(RuleDamage::Truncated, 1);
}

#[test]
fn a_missing_content_row_counts_as_no_longer_translatable() {
    check(RuleDamage::RowMissing, 1);
}

#[test]
fn null_content_counts_as_no_longer_translatable() {
    check(RuleDamage::NullContent, 1);
}

#[test]
fn a_dropped_content_table_hits_every_written_smart_album() {
    check(RuleDamage::TableMissing, t::TRANSLATABLE.len());
}

#[test]
fn the_dry_run_plans_the_same_counter() {
    let photos = tempfile::tempdir().unwrap();
    let catalog = lrcat_fixtures::write_smart_translate_catalog_with_files(photos.path()).unwrap();
    let work = tempfile::tempdir().unwrap();
    let fake = FakeLightCraft::start(Config::new(&work.path().join("Test.lightcraft")));
    go(&catalog, work.path(), fake.port(), true);
    lrcat_fixtures::break_smart_rules(&catalog, 101, RuleDamage::Truncated).unwrap();
    let (r, _) = go(&catalog, work.path(), fake.port(), false);
    let planned = &r["writer"]["planned"];
    assert_eq!(n(&planned["smart_no_longer_translatable"]), 1, "{planned}");
    assert_eq!(n(&planned["collections_removed"]), 0, "{planned}");
}
