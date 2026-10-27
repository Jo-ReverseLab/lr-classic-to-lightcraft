#![cfg(any())]
//! Acceptance tests for the report of an aborted `--apply` (task P5-4d),
//! against the stateful fake LightCraft in `tests/support`.
//!
//! Expected behaviour: when `--apply` stops after the run has started to talk
//! to LightCraft (connection lost, protocol error, pre-flight failure, no
//! server), lr2lc still writes `report.json` if `--report` is given, and
//! still exits with 1 (Principle 4: nothing is lost silently). Without
//! `--report` nothing is written, as before.
//!
//! The report of an aborted run has
//! - `dry_run: false` and all catalog sections as in a normal run;
//! - a `writer` section with the usual applied fields (`keywords_created`,
//!   `photos_imported`, `photos_existing`, `updates`, `not_saved`, ...) holding
//!   the numbers up to the abort: only commands that were answered count; the
//!   command whose reply never arrived does not;
//! - `writer.aborted`: a short, non-empty text without any path or name, for
//!   example `connection lost during library.import; outcome unknown`.
//!   If the connection or the pre-flight check fails, all counters are 0.
//!
//! DECISION: without an abort the field `aborted` is ABSENT from `writer`
//! (not `null`), also in dry runs.
//!
//! STATUS: waiting for P5-4d. The whole file is disabled by the crate-level
//! `cfg(any())` on the first line. The developer may ONLY remove that line.
//! Any other change to this file needs the test author.

use lrcat_fixtures::write_keyword_edge_catalog_with_files;
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[path = "support/fake_lightcraft.rs"]
mod fake_lightcraft;
use fake_lightcraft::{Config, FakeLightCraft};

/// Counters of the `writer` section that must be 0 when nothing was sent.
const COUNTERS: [&str; 5] = [
    "keywords_created",
    "photos_imported",
    "photos_existing",
    "updates",
    "not_saved",
];

struct Setup {
    photos: tempfile::TempDir,
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
        photos,
        work,
        catalog,
        library,
    }
}

impl Setup {
    fn report(&self) -> PathBuf {
        self.work.path().join("report.json")
    }

    /// `lr2lc <catalog> --library <lib> --port <port> --apply [--report f]`
    fn run(&self, port: u16, with_report: bool) -> Output {
        let port_text = port.to_string();
        let report = self.report();
        let mut args: Vec<&OsStr> = vec![
            self.catalog.as_os_str(),
            OsStr::new("--library"),
            self.library.as_os_str(),
            OsStr::new("--port"),
            OsStr::new(&port_text),
            OsStr::new("--apply"),
        ];
        if with_report {
            args.extend([OsStr::new("--report"), report.as_os_str()]);
        }
        Command::new(env!("CARGO_BIN_EXE_lr2lc"))
            .current_dir(self.work.path())
            .args(args)
            .output()
            .unwrap()
    }

    fn report_json(&self) -> Value {
        let text = fs::read_to_string(self.report()).expect("report.json must exist");
        serde_json::from_str(&text).unwrap()
    }

    /// Every directory name of the test that must not leak into the report.
    fn private_paths(&self) -> Vec<String> {
        [self.photos.path(), self.work.path(), &self.library]
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect()
    }
}

fn closed_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

fn code(o: &Output) -> Option<i32> {
    o.status.code()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// The text of `writer.aborted`; fails when it is missing or empty.
fn aborted(report: &Value) -> String {
    let text = report["writer"]["aborted"]
        .as_str()
        .unwrap_or_else(|| panic!("writer.aborted must be a string: {}", report["writer"]));
    assert!(!text.trim().is_empty(), "writer.aborted must not be empty");
    assert!(text.len() < 200, "writer.aborted must be short: {text}");
    text.to_string()
}

fn no_private_paths(s: &Setup, text: &str) {
    for p in s.private_paths() {
        assert!(!text.contains(&p), "path {p} leaks into: {text}");
    }
    let tmp = std::env::temp_dir().to_string_lossy().into_owned();
    assert!(!text.contains(&tmp), "temp dir leaks into: {text}");
}

/// What every aborted run must satisfy on top of its specific checks.
fn common_checks(s: &Setup, before: &[u8], o: &Output) -> Value {
    assert_eq!(code(o), Some(1), "{}", stderr(o));
    assert_eq!(before, fs::read(&s.catalog).unwrap(), "catalog changed");
    let report = s.report_json();
    assert_eq!(report["dry_run"], Value::Bool(false));
    assert_eq!(report["catalog_unchanged"], Value::Bool(true));
    // The catalog part of the report still balances: read = converted + skipped.
    for area in ["folders", "photos", "keywords"] {
        let a = &report[area];
        let read = a["read"].as_u64().unwrap();
        let converted = a["converted"].as_u64().unwrap();
        let skipped = a["skipped"].as_array().map(|v| v.len() as u64);
        let skipped = skipped.or_else(|| a["skipped"].as_u64()).unwrap();
        assert_eq!(read, converted + skipped, "{area} does not add up");
    }
    assert!(report["writer"].is_object(), "writer section is missing");
    no_private_paths(s, &aborted(&report));
    no_private_paths(s, &report.to_string());
    report
}

fn count(fake: &FakeLightCraft, command: &str, first: usize) -> usize {
    fake.commands()
        .iter()
        .take(first)
        .filter(|(c, _)| c == command)
        .count()
}

/// 1-based position of the n-th (1-based) `command` in a complete run.
fn position_in_full_run(command: &str, n: usize) -> usize {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let o = s.run(fake.port(), true);
    assert_eq!(code(&o), Some(0), "baseline run failed: {}", stderr(&o));
    fake.commands()
        .iter()
        .enumerate()
        .filter(|(_, (c, _))| c == command)
        .nth(n - 1)
        .unwrap_or_else(|| panic!("baseline run has fewer than {n} `{command}` commands"))
        .0
        + 1
}

// ------------------------------------------------------------ abort mid-run

/// Aborts at the `at`-th command line and checks the partial numbers.
fn abort_at(at: usize, applies: bool) {
    let s = setup();
    let before = fs::read(&s.catalog).unwrap();
    let mut config = Config::new(&s.library);
    config.close_at = Some(at);
    config.close_applies = applies;
    let fake = FakeLightCraft::start(config);
    let o = s.run(fake.port(), true);
    let report = common_checks(&s, &before, &o);
    assert_eq!(fake.commands().len(), at, "nothing is sent after the loss");
    let text = aborted(&report);
    assert!(
        text.contains("connection lost") && text.contains("outcome unknown"),
        "{text}"
    );
    // Only answered imports count; the line `at` got no reply.
    let answered = count(&fake, "library.import", at - 1);
    assert_eq!(
        report["writer"]["photos_imported"].as_u64(),
        Some(answered as u64),
        "{}",
        report["writer"]
    );
    let created = count(&fake, "keyword.create", at - 1);
    assert!(
        report["writer"]["keywords_created"].as_u64().unwrap() <= created as u64,
        "{}",
        report["writer"]
    );
    for key in COUNTERS {
        assert!(report["writer"][key].is_u64(), "{key} must be a number");
    }
}

#[test]
fn abort_among_the_first_commands_reports_a_partial_run() {
    // Line 5 is after the pre-flight and the keyword list, before photos.
    abort_at(5, false);
}

#[test]
fn abort_after_two_answered_imports_reports_exactly_two() {
    let third = position_in_full_run("library.import", 3);
    let s_at = third; // the third import itself gets no reply
    abort_at(s_at, false);
    // The same, with the unanswered import taking effect in the app: it must
    // still not be counted as imported.
    abort_at(s_at, true);
}

#[test]
fn abort_exactly_two_imports_is_two_in_the_report() {
    let third = position_in_full_run("library.import", 3);
    let s = setup();
    let mut config = Config::new(&s.library);
    config.close_at = Some(third);
    let fake = FakeLightCraft::start(config);
    let o = s.run(fake.port(), true);
    assert_eq!(code(&o), Some(1));
    let report = s.report_json();
    assert_eq!(report["writer"]["photos_imported"].as_u64(), Some(2));
    assert!(aborted(&report).contains("library.import"));
}

#[test]
fn abort_right_after_the_first_import_still_reports() {
    let next = position_in_full_run("library.import", 1) + 1;
    abort_at(next, false);
}

#[test]
fn abort_without_report_writes_no_report() {
    let s = setup();
    let before = fs::read(&s.catalog).unwrap();
    let mut config = Config::new(&s.library);
    config.close_at = Some(10);
    let fake = FakeLightCraft::start(config);
    let o = s.run(fake.port(), false);
    assert_eq!(code(&o), Some(1));
    assert!(!s.report().exists());
    assert_eq!(before, fs::read(&s.catalog).unwrap());
}

#[test]
fn a_rerun_after_an_abort_finishes_without_duplicates() {
    let s = setup();
    let mut config = Config::new(&s.library);
    config.close_at = Some(position_in_full_run("library.import", 2));
    let fake = FakeLightCraft::start(config);
    assert_eq!(code(&s.run(fake.port(), true)), Some(1));
    assert!(aborted(&s.report_json()).contains("outcome unknown"));

    assert_eq!(code(&s.run(fake.port(), true)), Some(0));
    let report = s.report_json();
    assert!(
        report["writer"].get("aborted").is_none(),
        "{}",
        report["writer"]
    );
    let photos = fake.photos();
    let mut ids: Vec<i64> = photos.values().copied().collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), photos.len(), "each photo exactly once");
}

// ------------------------------------------------- failure before any write

#[test]
fn failed_preflight_reports_the_abort_with_zero_counters() {
    let s = setup();
    let before = fs::read(&s.catalog).unwrap();
    let mut config = Config::new(&s.library);
    config.auto_write = true;
    let fake = FakeLightCraft::start(config);
    let o = s.run(fake.port(), true);
    let report = common_checks(&s, &before, &o);
    aborted(&report);
    for key in COUNTERS {
        assert_eq!(report["writer"][key].as_u64(), Some(0), "{key}");
    }
    assert!(fake.write_commands().is_empty());
    assert!(fake.photos().is_empty());
}

#[test]
fn wrong_library_reports_the_abort_with_zero_counters() {
    let s = setup();
    let before = fs::read(&s.catalog).unwrap();
    let other = s.work.path().join("Other.lightcraft");
    let fake = FakeLightCraft::start(Config::new(&other));
    let o = s.run(fake.port(), true);
    let report = common_checks(&s, &before, &o);
    for key in COUNTERS {
        assert_eq!(report["writer"][key].as_u64(), Some(0), "{key}");
    }
}

#[test]
fn no_server_reports_the_abort_with_zero_counters() {
    let s = setup();
    let before = fs::read(&s.catalog).unwrap();
    let o = s.run(closed_port(), true);
    let report = common_checks(&s, &before, &o);
    for key in COUNTERS {
        assert_eq!(report["writer"][key].as_u64(), Some(0), "{key}");
    }
    assert!(
        !s.library.exists(),
        "nothing is created next to the library"
    );
}

// --------------------------------------------------------- no abort, no field

#[test]
fn successful_run_has_no_aborted_field() {
    let s = setup();
    let before = fs::read(&s.catalog).unwrap();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let o = s.run(fake.port(), true);
    assert_eq!(code(&o), Some(0), "{}", stderr(&o));
    assert_eq!(before, fs::read(&s.catalog).unwrap());
    let report = s.report_json();
    assert_eq!(report["dry_run"], Value::Bool(false));
    assert!(report["writer"].is_object());
    assert!(
        report["writer"].get("aborted").is_none(),
        "{}",
        report["writer"]
    );
}

#[test]
fn dry_run_has_no_aborted_field() {
    let s = setup();
    let report = s.report();
    let o = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(s.work.path())
        .args([s.catalog.as_path(), Path::new("--report"), report.as_path()])
        .output()
        .unwrap();
    assert_eq!(code(&o), Some(0), "{}", stderr(&o));
    let json = s.report_json();
    assert_eq!(json["dry_run"], Value::Bool(true));
    assert!(json["writer"].get("aborted").is_none());
}
