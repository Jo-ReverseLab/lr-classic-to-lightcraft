//! CLI acceptance test for a written smart album whose Lightroom collection
//! still exists but can no longer be translated. Writer
//! API: see `crates/lc-writer/tests/smart_untranslatable.rs`.
//!
//! Setup: run once with `--apply` (writes the smart albums of the
//! `smart_translate` fixture), then change the rules of collection 101 in
//! the catalog to a rule that cannot be translated, run again.
//!
//! Rules:
//! - `writer.smart_no_longer_translatable` (applied run) and
//!   `writer.planned.smart_no_longer_translatable` (dry run) are integers
//!   and equal 1; `writer.collections_removed` stays 0;
//! - `writer.smart_known` is one less than before;
//! - the text summary has a line containing `no longer translatable` and
//!   the count, apart from the `changed in Lightroom ... removed` line
//!   (which is absent when nothing else changed);
//! - nothing is sent to LightCraft that changes albums, and the catalog is
//!   unchanged by the run.
//! - the key is `0` in every other run.

use lrcat_fixtures::smart_translate as t;
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::process::Command;

use fake_lightcraft::{Config, FakeLightCraft};
use lc_test_support::fake_lightcraft;

struct Run {
    report: Value,
    stdout: String,
}

fn n(v: &Value) -> usize {
    v.as_u64().unwrap_or_else(|| panic!("not a number: {v}")) as usize
}

fn go(catalog: &std::path::Path, work: &std::path::Path, port: u16, apply: bool) -> Run {
    let library = work.join("Test.lightcraft");
    let report = work.join("report.json");
    let port_text = port.to_string();
    let mut args: Vec<&OsStr> = vec![
        catalog.as_os_str(),
        OsStr::new("--library"),
        library.as_os_str(),
        OsStr::new("--port"),
        OsStr::new(&port_text),
        OsStr::new("--anonymize"),
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
    Run {
        report: serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
    }
}

struct Setup {
    _photos: tempfile::TempDir,
    work: tempfile::TempDir,
    catalog: std::path::PathBuf,
    fake: FakeLightCraft,
    first: Run,
}

fn setup() -> Setup {
    let photos = tempfile::tempdir().unwrap();
    let catalog = lrcat_fixtures::write_smart_translate_catalog_with_files(photos.path()).unwrap();
    let work = tempfile::tempdir().unwrap();
    let fake = FakeLightCraft::start(Config::new(&work.path().join("Test.lightcraft")));
    let first = go(&catalog, work.path(), fake.port(), true);
    Setup {
        _photos: photos,
        work,
        catalog,
        fake,
        first,
    }
}

#[test]
fn a_smart_collection_that_stopped_translating_has_its_own_counter() {
    let s = setup();
    assert_eq!(
        n(&s.first.report["writer"]["smart_created"]),
        t::TRANSLATABLE.len()
    );
    assert_eq!(
        n(&s.first.report["writer"]["smart_no_longer_translatable"]),
        0
    );
    let commands = s.fake.commands().len();
    let albums = s.fake.albums();
    lrcat_fixtures::make_smart_untranslatable(&s.catalog, 101).unwrap();

    let r = go(&s.catalog, s.work.path(), s.fake.port(), true);
    let w = &r.report["writer"];
    assert_eq!(n(&w["smart_no_longer_translatable"]), 1, "{w}");
    assert_eq!(n(&w["collections_removed"]), 0, "{w}");
    assert_eq!(n(&w["smart_known"]), t::TRANSLATABLE.len() - 1);
    assert_eq!(n(&w["smart_created"]), 0);
    assert!(
        r.stdout
            .lines()
            .any(|l| l.contains("no longer translatable") && l.contains('1')),
        "{}",
        r.stdout
    );
    assert!(
        !r.stdout.contains("removed 1"),
        "must not be shown as removed: {}",
        r.stdout
    );
    let new_commands = &s.fake.commands()[commands..];
    assert!(
        new_commands
            .iter()
            .all(|(c, _)| !c.starts_with("album.") && c != "library.select"),
        "{new_commands:?}"
    );
    assert_eq!(s.fake.albums(), albums);

    // A third run: same numbers, no duplicates.
    let again = go(&s.catalog, s.work.path(), s.fake.port(), true);
    assert_eq!(
        n(&again.report["writer"]["smart_no_longer_translatable"]),
        1
    );
    assert_eq!(s.fake.albums(), albums);
}

#[test]
fn the_dry_run_plans_the_same_counter() {
    let s = setup();
    lrcat_fixtures::make_smart_untranslatable(&s.catalog, 101).unwrap();
    // Dry run: no --apply, connection to the fake is not needed for the plan.
    let r = go(&s.catalog, s.work.path(), s.fake.port(), false);
    let planned = &r.report["writer"]["planned"];
    assert_eq!(n(&planned["smart_no_longer_translatable"]), 1, "{planned}");
}
