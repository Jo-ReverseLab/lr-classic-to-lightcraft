#![cfg(any())]
//! CLI acceptance tests for the quick collection (task P5b-6), against the
//! stateful fake LightCraft.
//!
//! STATUS: waiting for P5b-6. Disabled by the crate-level `cfg(any())`; the
//! developer may ONLY remove that line.
//!
//! Fixtures: `quick_edge` (one quick collection with three converted
//! members, see the generator) and `collection_edge` (two quick collections,
//! the first has a member whose original is missing, so it is empty here).
//!
//! Contract (assumptions in brackets):
//! - `report.json`: `collections.quick` is 1 when a quick collection was
//!   read, else 0; the quick collection counts as converted;
//!   `collections.converted` = folders + albums + quick;
//! - writer summary: integers `quick_added`, `quick_already`,
//!   `quick_removed`; list of `{reason}` `quick_skipped`, `quick_incomplete`
//!   [both lists exist in every applied run];
//! - dry run: `writer.planned.quick_add` = members to add, nothing written,
//!   no command except reads;
//! - second run: no `album.setTarget`, `album.toggleTarget`, `catalog.query`.

use lrcat_fixtures::{collection_edge as e, quick_edge as q};
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[path = "support/fake_lightcraft.rs"]
mod fake_lightcraft;
use fake_lightcraft::{Config, FakeLightCraft};

struct Setup {
    _photos: tempfile::TempDir,
    work: tempfile::TempDir,
    catalog: PathBuf,
    library: PathBuf,
}

fn setup(quick_fixture: bool) -> Setup {
    let photos = tempfile::tempdir().unwrap();
    let catalog = if quick_fixture {
        lrcat_fixtures::write_quick_catalog_with_files(photos.path()).unwrap()
    } else {
        lrcat_fixtures::write_collection_catalog_with_files(photos.path()).unwrap()
    };
    let work = tempfile::tempdir().unwrap();
    let library = work.path().join("Test.lightcraft");
    Setup {
        _photos: photos,
        work,
        catalog,
        library,
    }
}

impl Setup {
    /// A run that must succeed; the catalog stays byte for byte the same.
    fn ok(&self, port: u16, apply: bool) -> Value {
        let before = fs::read(&self.catalog).unwrap();
        let port_text = port.to_string();
        let report = self.work.path().join("report.json");
        let mut args: Vec<&OsStr> = vec![
            self.catalog.as_os_str(),
            OsStr::new("--library"),
            self.library.as_os_str(),
            OsStr::new("--port"),
            OsStr::new(&port_text),
            OsStr::new("--report"),
            report.as_os_str(),
        ];
        if apply {
            args.push(OsStr::new("--apply"));
        }
        let o = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
            .current_dir(self.work.path())
            .args(args)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
        assert_eq!(o.status.code(), Some(0), "{stderr}");
        assert_eq!(before, fs::read(&self.catalog).unwrap(), "catalog changed");
        let raw = fs::read_to_string(&report).unwrap();
        let all = format!("{}\n{stderr}\n{raw}", String::from_utf8_lossy(&o.stdout));
        for name in e::SKIPPED_NAMES {
            assert!(!all.contains(name), "name {name:?} leaked");
        }
        serde_json::from_str(&raw).unwrap()
    }
}

fn n(v: &Value) -> usize {
    v.as_u64().unwrap_or_else(|| panic!("not a number: {v}")) as usize
}

fn quick_commands(fake: &FakeLightCraft, from: usize) -> usize {
    fake.commands()[from..]
        .iter()
        .filter(|(c, _)| {
            ["album.setTarget", "album.toggleTarget", "catalog.query"].contains(&c.as_str())
        })
        .count()
}

#[test]
fn apply_adds_the_quick_collection_and_the_numbers_add_up() {
    let s = setup(true);
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let r = s.ok(fake.port(), true);
    let w = &r["writer"];
    assert_eq!(n(&w["quick_added"]), q::MEMBERS.len());
    assert_eq!((n(&w["quick_already"]), n(&w["quick_removed"])), (0, 0));
    assert!(w["quick_skipped"].as_array().unwrap().is_empty());
    assert!(w["quick_incomplete"].as_array().unwrap().is_empty());
    assert_eq!(n(&r["collections"]["quick"]), 1);
    // Balance: converted collections = folders + albums + quick.
    assert_eq!(
        n(&r["collections"]["converted"]),
        n(&w["folders_created"]) + n(&w["albums_created"]) + n(&r["collections"]["quick"])
    );
    // The fake's Quick Collection has exactly the members; no clearQuick.
    let quick = fake.quick_collection().expect("Quick Collection");
    assert_eq!(quick.photos.len(), q::MEMBERS.len());
    assert!(fake.commands().iter().all(|(c, _)| c != "album.clearQuick"));
    // The report names the quick members as converted.
    let m = &r["collection_members"];
    assert_eq!(
        n(&m["read"]),
        n(&m["converted"]) + m["skipped"].as_array().unwrap().len()
    );
}

#[test]
fn second_run_sends_no_quick_command_and_creates_no_duplicates() {
    let s = setup(true);
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);
    let before = fake.commands().len();
    let members = fake.quick_collection().unwrap().photos;
    let r = s.ok(fake.port(), true);
    assert_eq!(quick_commands(&fake, before), 0);
    assert_eq!(n(&r["writer"]["quick_added"]), 0);
    assert_eq!(n(&r["writer"]["quick_already"]), q::MEMBERS.len());
    assert_eq!(fake.quick_collection().unwrap().photos, members);
    assert_eq!(
        fake.albums()
            .iter()
            .filter(|a| a.name == "Quick Collection")
            .count(),
        1
    );
}

#[test]
fn dry_run_reports_the_plan_and_writes_nothing() {
    let s = setup(true);
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let r = s.ok(fake.port(), false);
    assert_eq!(n(&r["writer"]["planned"]["quick_add"]), q::MEMBERS.len());
    assert!(fake.quick_collection().is_none());
    assert!(fake.albums().is_empty());
    assert!(
        fake.write_commands().is_empty(),
        "{:?}",
        fake.write_commands()
    );
    assert!(!s.work.path().join("Test.lightcraft.lr2lc.json").exists());
    assert_eq!(n(&r["collections"]["quick"]), 1);
}

#[test]
fn edge_fixture_reports_one_quick_collection_and_one_skipped() {
    let s = setup(false);
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let r = s.ok(fake.port(), true);
    assert_eq!(n(&r["collections"]["quick"]), 1);
    assert_eq!(n(&r["collections"]["read"]), e::COLLECTIONS as usize);
    assert_eq!(
        r["collections"]["skipped"].as_array().unwrap().len(),
        e::SKIPPED as usize - 1
    );
    let reasons: Vec<String> = r["collections"]["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["reason"].as_str().unwrap().to_lowercase())
        .collect();
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("more than one quick collection"))
    );
    assert!(
        !reasons
            .iter()
            .any(|r| r.contains("14") && r.contains("quick"))
    );
    // Its only member has no original here: nothing to add, nothing sent.
    assert_eq!(n(&r["writer"]["quick_added"]), 0);
    assert!(
        fake.commands()
            .iter()
            .all(|(c, _)| c != "album.toggleTarget")
    );
    // Albums are written as before.
    assert_eq!(n(&r["writer"]["albums_created"]), e::ALBUMS as usize);
}
