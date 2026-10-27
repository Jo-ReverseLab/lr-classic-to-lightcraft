#![cfg(any())]
//! CLI acceptance tests for the photo balance in EVERY run (task P5-4d,
//! extension), against the stateful fake LightCraft.
//!
//! `writer` of an applied run has `photos_imported`, `photos_existing`,
//! `photos_updated`, `photos_unchanged` and `photos_skipped` (a list):
//! `photos.converted == imported + existing + updated + unchanged +
//! photos_skipped.len()` in the first run, in later runs and after a run that
//! was aborted before. A dry run has `writer.planned.photos_unchanged`: the
//! known photos that need no command; the stdout line
//! `writer: keywords to create K, photos to import P, updates U` is
//! unchanged. In a dry run, `planned.photos + known photos ==
//! photos.converted - photos_skipped.len()`, where known photos are those in
//! the mapping file, and `planned.photos_unchanged <= known`.
//!
//! The tests change one field of a known photo by editing the rating in the
//! mapping file (`photos.<global id>.fields.rating`), so the next run finds a
//! difference.
//!
//! STATUS: waiting for P5-4d. The whole file is disabled by the crate-level
//! `cfg(any())` on the first line. The developer may ONLY remove that line.
//! Any other change to this file needs the test author.

use lrcat_fixtures::write_keyword_edge_catalog_with_files;
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

#[path = "support/fake_lightcraft.rs"]
mod fake_lightcraft;
use fake_lightcraft::{Config, FakeLightCraft};

struct Setup {
    _photos: tempfile::TempDir,
    work: tempfile::TempDir,
    catalog: PathBuf,
    library: PathBuf,
}

fn setup() -> Setup {
    let photos = tempfile::tempdir().unwrap();
    let catalog = write_keyword_edge_catalog_with_files(photos.path()).unwrap();
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
    fn report(&self) -> PathBuf {
        self.work.path().join("report.json")
    }

    fn state_file(&self) -> PathBuf {
        let mut name = self.library.file_name().unwrap().to_os_string();
        name.push(".lr2lc.json");
        self.work.path().join(name)
    }

    fn run(&self, port: u16, apply: bool) -> Output {
        let port_text = port.to_string();
        let report = self.report();
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
        Command::new(env!("CARGO_BIN_EXE_lr2lc"))
            .current_dir(self.work.path())
            .args(args)
            .output()
            .unwrap()
    }

    /// A run that must succeed; returns the parsed report.
    fn ok(&self, port: u16, apply: bool) -> (Value, String) {
        let before = fs::read(&self.catalog).unwrap();
        let o = self.run(port, apply);
        assert_eq!(
            o.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert_eq!(before, fs::read(&self.catalog).unwrap(), "catalog changed");
        let text = fs::read_to_string(self.report()).unwrap();
        (
            serde_json::from_str(&text).unwrap(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
        )
    }

    fn state(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.state_file()).unwrap()).unwrap()
    }

    /// Number of photos recorded in the mapping file.
    fn known(&self) -> u64 {
        self.state()["photos"].as_object().unwrap().len() as u64
    }

    /// Changes the rating of one recorded photo (a difference to the catalog).
    fn change_one_rating(&self) {
        let mut state = self.state();
        let photos = state["photos"].as_object_mut().unwrap();
        let entry = photos.values_mut().next().expect("a recorded photo");
        let rating = entry["fields"]["rating"].as_u64().unwrap();
        entry["fields"]["rating"] = Value::from((rating + 1) % 6);
        fs::write(self.state_file(), serde_json::to_string(&state).unwrap()).unwrap();
    }
}

fn n(v: &Value) -> u64 {
    v.as_u64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn skipped(report: &Value) -> u64 {
    report["writer"]["photos_skipped"].as_array().unwrap().len() as u64
}

fn converted(report: &Value) -> u64 {
    n(&report["photos"]["converted"])
}

/// The five counters of an applied run: imported, existing, updated,
/// unchanged, skipped.
fn counters(report: &Value) -> [u64; 5] {
    let w = &report["writer"];
    [
        n(&w["photos_imported"]),
        n(&w["photos_existing"]),
        n(&w["photos_updated"]),
        n(&w["photos_unchanged"]),
        skipped(report),
    ]
}

fn assert_balance(report: &Value) {
    let c = counters(report);
    assert_eq!(
        c.iter().sum::<u64>(),
        converted(report),
        "{}",
        report["writer"]
    );
    let incomplete = report["writer"]["photos_incomplete"]
        .as_array()
        .unwrap()
        .len() as u64;
    assert!(incomplete <= c[0] + c[1] + c[2], "{}", report["writer"]);
}

/// `(keywords, photos, updates)` of the stdout line of a dry run.
fn planned_line(text: &str) -> (u64, u64, u64) {
    let line = text
        .lines()
        .find(|l| l.starts_with("writer: keywords to create "))
        .unwrap_or_else(|| panic!("no writer line in {text}"));
    let numbers: Vec<u64> = line
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| p.parse().unwrap())
        .collect();
    assert_eq!(numbers.len(), 3, "line must stay as it was: {line}");
    (numbers[0], numbers[1], numbers[2])
}

#[test]
fn first_run_balances_with_everything_imported() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let (report, _) = s.ok(fake.port(), true);
    assert_balance(&report);
    let c = counters(&report);
    assert_eq!([c[1], c[2], c[3]], [0, 0, 0], "{}", report["writer"]);
    assert!(c[0] > 0);
}

#[test]
fn second_run_counts_every_written_photo_as_unchanged() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let (first, _) = s.ok(fake.port(), true);
    let (second, _) = s.ok(fake.port(), true);
    assert_balance(&second);
    let c = counters(&second);
    assert_eq!([c[0], c[1], c[2]], [0, 0, 0], "{}", second["writer"]);
    assert_eq!(c[3], n(&first["writer"]["photos_imported"]));
    assert_eq!(c[4], skipped(&first), "the same photos are skipped");
    assert_eq!(n(&second["writer"]["updates"]), 0);
}

#[test]
fn one_changed_field_counts_one_photo_as_updated() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let (first, _) = s.ok(fake.port(), true);
    s.change_one_rating();
    let (second, _) = s.ok(fake.port(), true);
    assert_balance(&second);
    let c = counters(&second);
    assert_eq!([c[0], c[1], c[2]], [0, 0, 1], "{}", second["writer"]);
    assert_eq!(c[3], n(&first["writer"]["photos_imported"]) - 1);
    assert_eq!(n(&second["writer"]["updates"]), 1);
}

#[test]
fn dry_run_with_an_empty_state_plans_every_photo_and_no_unchanged() {
    let s = setup();
    let (report, out) = s.ok(closed(), false);
    let planned = &report["writer"]["planned"];
    assert_eq!(n(&planned["photos_unchanged"]), 0);
    assert_eq!(n(&planned["photos"]), converted(&report) - skipped(&report));
    let (_, photos, _) = planned_line(&out);
    assert_eq!(photos, n(&planned["photos"]));
    assert!(report["writer"].get("photos_unchanged").is_none());
}

#[test]
fn dry_run_after_a_complete_apply_has_everything_unchanged() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);
    let (report, out) = s.ok(closed(), false);
    let planned = &report["writer"]["planned"];
    assert_eq!(n(&planned["photos"]), 0);
    assert_eq!(n(&planned["updates"]), 0);
    assert_eq!(
        n(&planned["photos_unchanged"]),
        converted(&report) - skipped(&report)
    );
    assert_eq!(planned_line(&out).1, 0);
    assert_eq!(planned_line(&out).2, 0);
}

#[test]
fn dry_run_with_a_changed_field_plans_one_update() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);
    s.change_one_rating();
    let (report, out) = s.ok(closed(), false);
    let planned = &report["writer"]["planned"];
    assert_eq!(n(&planned["photos"]), 0);
    assert_eq!(n(&planned["updates"]), 1);
    assert_eq!(
        n(&planned["photos_unchanged"]),
        converted(&report) - skipped(&report) - 1
    );
    assert_eq!(planned_line(&out).2, 1);
}

#[test]
fn dry_run_after_an_abort_balances_with_the_known_photos() {
    let s = setup();
    // Abort at the third import, so two photos are known, the rest not.
    let mut config = Config::new(&s.library);
    config.close_at = Some(third_import());
    let fake = FakeLightCraft::start(config);
    let o = s.run(fake.port(), true);
    assert_eq!(o.status.code(), Some(1));
    let known = s.known();
    assert_eq!(known, 2, "the abort comes at the third import");
    let (report, _) = s.ok(closed(), false);
    let planned = &report["writer"]["planned"];
    assert_eq!(
        n(&planned["photos"]) + known,
        converted(&report) - skipped(&report)
    );
    assert!(n(&planned["photos_unchanged"]) <= known);
}

#[test]
fn run_after_an_abort_balances() {
    let s = setup();
    let mut config = Config::new(&s.library);
    config.close_at = Some(third_import());
    let fake = FakeLightCraft::start(config);
    assert_eq!(s.run(fake.port(), true).status.code(), Some(1));
    let known = s.known();
    let (report, _) = s.ok(fake.port(), true);
    assert_balance(&report);
    let c = counters(&report);
    // Photos recorded before the abort are known now: updated or unchanged;
    // the others are imported.
    assert_eq!(c[2] + c[3], known, "{}", report["writer"]);
    assert_eq!(c[0] + c[1], converted(&report) - skipped(&report) - known);
}

/// 1-based position of the third `library.import` in a complete run: an abort
/// there leaves two photos recorded in the mapping file.
fn third_import() -> usize {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);
    fake.commands()
        .iter()
        .enumerate()
        .filter(|(_, (c, _))| c == "library.import")
        .nth(2)
        .expect("at least three imports")
        .0
        + 1
}

fn closed() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}
