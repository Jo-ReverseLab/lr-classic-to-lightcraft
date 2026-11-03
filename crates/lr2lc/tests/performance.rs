//! Large-catalog performance measurements (task T20).
//!
//! These tests generate proportional catalogs with 10 000, 100 000 and
//! 500 000 photos (keywords and collections in proportion, deterministic
//! seed) and run the whole dry run. They are ignored by default because they
//! write hundreds of thousands of placeholder files and take minutes.
//!
//! Run all of them, one after another, with:
//!
//! ```text
//! cargo test --release -p lr2lc --test performance -- --ignored --nocapture
//! ```
//!
//! Run one size only by adding the test name, e.g.
//! `cargo test --release -p lr2lc --test performance -- --ignored --nocapture dry_run_10_000_photos`.
//!
//! Each test prints one line: the catalog shape, the catalog size, the
//! generation time and the dry-run time (the 10 000 and 100 000 tests run a
//! second dry run to prove the report is repeatable; the 500 000 test runs
//! once so a baseline measurement stays feasible). The numbers are
//! observations for the task report, not pass/fail criteria; there is
//! deliberately no time limit here. The 2 000-photo guard in
//! `performance_guard.rs` runs in the normal suite and does have a generous
//! limit.
//!
//! The generated schema has no indexes (like every generated fixture), so a
//! timing includes the reader's own query shape. Do not "fix" a slow
//! measurement by adding indexes to the fixture without changing the reader:
//! a catalog without indexes is valid input, and the tool must not become
//! quadratic on it.

#[path = "perf_support/mod.rs"]
mod perf_support;

use lrcat_fixtures::{CatalogShape, write_performance_catalog_with_files};
use perf_support::{
    assert_catalog_unchanged, assert_no_state_file, assert_photo_subcounts,
    assert_report_invariants, dry_run, exit_ok, read_report,
};
use std::fs;
use std::time::{Duration, Instant};

#[test]
#[ignore = "performance measurement (10 000 photos): run with cargo test --release -p lr2lc --test performance -- --ignored --nocapture"]
fn dry_run_10_000_photos() {
    measure(10_000, true);
}

#[test]
#[ignore = "performance measurement (100 000 photos): run with cargo test --release -p lr2lc --test performance -- --ignored --nocapture"]
fn dry_run_100_000_photos() {
    measure(100_000, true);
}

#[test]
#[ignore = "performance measurement (500 000 photos): run with cargo test --release -p lr2lc --test performance -- --ignored --nocapture"]
fn dry_run_500_000_photos() {
    measure(500_000, false);
}

/// Generates a catalog of `photos` photos (with files) and runs the dry run.
/// With `second_run` it runs a second time and compares the two reports.
fn measure(photos: usize, second_run: bool) {
    let shape = CatalogShape::proportional(photos);
    let dir = tempfile::tempdir().unwrap();

    let started = Instant::now();
    let catalog = write_performance_catalog_with_files(dir.path(), shape).unwrap();
    let generated = started.elapsed();
    let catalog_bytes = fs::metadata(&catalog).unwrap().len();
    let before = fs::read(&catalog).unwrap();

    let first_path = dir.path().join("report-first.json");
    let started = Instant::now();
    let first = dry_run(&catalog, &first_path);
    let first_time = started.elapsed();
    exit_ok(&first);

    let second_path = dir.path().join("report-second.json");
    let second_time = if second_run {
        let started = Instant::now();
        let second = dry_run(&catalog, &second_path);
        let elapsed = started.elapsed();
        exit_ok(&second);
        assert_eq!(
            fs::read(&second_path).unwrap(),
            fs::read(&first_path).unwrap(),
            "the second dry run produced a different report"
        );
        Some(elapsed)
    } else {
        None
    };

    let report = read_report(&first_path);
    assert_report_invariants(&report);
    assert_photo_subcounts(&report);
    assert_eq!(report["photos"]["read"], photos);
    assert_eq!(
        report["photos"]["converted"], photos,
        "every generated original exists, so every photo is converted"
    );
    assert_eq!(report["photos"]["skipped"].as_array().unwrap().len(), 0);
    assert_eq!(report["folders"]["read"], shape.folders);
    assert_eq!(report["keywords"]["read"], shape.keywords);
    assert_eq!(report["keyword_assignments"]["read"], photos);
    assert_catalog_unchanged(&catalog, &before);
    assert_no_state_file(dir.path());

    let second =
        second_time.map_or_else(|| "not run".to_string(), |d| format!("{:.1} s", seconds(d)));
    println!(
        "performance: {} photos, {} folders, {} keywords, {} collections, \
         catalog {:.1} MiB, generate {:.1} s, dry run {:.1} s, second dry run {}",
        photos,
        shape.folders,
        shape.keywords,
        shape.collections,
        catalog_bytes as f64 / (1024.0 * 1024.0),
        seconds(generated),
        seconds(first_time),
        second,
    );
}

fn seconds(d: Duration) -> f64 {
    d.as_secs_f64()
}
