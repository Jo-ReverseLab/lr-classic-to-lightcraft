//! Performance guard (task T20): one full dry run over a generated catalog of
//! 2 000 photos with keywords and collections in proportion.
//!
//! This test runs in the normal `cargo test` and is intentionally generous:
//! the time limit only catches gross accidental quadratic behaviour on a slow
//! CI runner, it is not a benchmark. The measurement runs (10 000, 100 000 and
//! 500 000 photos) are `#[ignore]`d in `performance.rs`.
//!
//! Besides the runtime the test pins that the guard catalog behaves like any
//! other run: the catalog is unchanged (checksum), `read == converted +
//! skipped` for every area, the photo sub-counts add up, a second dry run gives
//! the identical report, and no mapping file is written without `--apply`.

#[path = "perf_support/mod.rs"]
mod perf_support;

use lrcat_fixtures::{CatalogShape, write_proportional_catalog_with_files};
use perf_support::{
    assert_catalog_unchanged, assert_no_state_file, assert_photo_subcounts,
    assert_report_invariants, dry_run, exit_ok, read_report,
};
use std::fs;
use std::time::{Duration, Instant};

/// About 2 000 photos: cheap enough for every `cargo test`, big enough that a
/// quadratic reader or planner becomes visible.
const PHOTOS: usize = 2_000;

/// Very generous for a slow CI runner. A linear run needs a few seconds; a
/// quadratic one at this size already takes minutes.
const LIMIT: Duration = Duration::from_secs(180);

#[test]
fn dry_run_of_2000_photos_stays_within_the_guard_limit() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = write_proportional_catalog_with_files(dir.path(), PHOTOS).unwrap();
    let before = fs::read(&catalog).unwrap();
    let report_path = dir.path().join("report.json");

    let started = Instant::now();
    let output = dry_run(&catalog, &report_path);
    let elapsed = started.elapsed();
    let stdout = exit_ok(&output);
    assert!(stdout.contains("dry run"), "{stdout}");
    assert!(
        elapsed < LIMIT,
        "the dry run of {PHOTOS} photos took {elapsed:?}, over the guard limit of {LIMIT:?}; \
         check for accidental quadratic work in the reader or planner"
    );

    let report = read_report(&report_path);
    assert_report_invariants(&report);
    assert_photo_subcounts(&report);
    assert_catalog_unchanged(&catalog, &before);

    // The generated shape is known, so the counts are pinned exactly.
    let shape = CatalogShape::proportional(PHOTOS);
    assert_eq!(shape.folders, 100);
    assert_eq!(shape.keywords, 200);
    assert_eq!(shape.collections, 4);

    assert_eq!(report["photos"]["read"], PHOTOS);
    assert_eq!(report["photos"]["converted"], PHOTOS);
    assert_eq!(report["photos"]["skipped"].as_array().unwrap().len(), 0);
    assert_eq!(report["photos"]["videos"], 0);

    assert_eq!(report["folders"]["read"], shape.folders);
    assert_eq!(report["folders"]["converted"], shape.folders);

    assert_eq!(report["keywords"]["read"], shape.keywords);
    assert_eq!(report["keywords"]["converted"], shape.keywords);
    assert_eq!(report["keywords"]["skipped"].as_array().unwrap().len(), 0);

    // One assignment per photo, all converted.
    assert_eq!(report["keyword_assignments"]["read"], PHOTOS);
    assert_eq!(report["keyword_assignments"]["converted"], PHOTOS);

    // Collection 1 is a set, 2 an album inside it, 3 an unreadable smart
    // collection (skipped), 4 the Quick Collection.
    assert_eq!(report["collections"]["read"], shape.collections);
    assert_eq!(report["collections"]["converted"], shape.collections - 1);
    assert_eq!(report["collections"]["folders"], 1);
    assert_eq!(report["collections"]["albums"], 1);
    assert_eq!(report["collections"]["smart"], 0);
    assert_eq!(report["collections"]["quick"], 1);
    assert_eq!(
        report["collections"]["by_code"]["smart_unreadable"], 1,
        "the generated smart collection has no readable rules"
    );

    // Every tenth photo is in the album: 2 000 / 10 = 200 members.
    assert_eq!(report["collection_members"]["read"], PHOTOS / 10);
    assert_eq!(report["collection_members"]["converted"], PHOTOS / 10);
    assert_eq!(
        report["collection_members"]["skipped"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    // The writer plan counts the whole catalog once.
    assert_eq!(report["writer"]["planned"]["photos"], PHOTOS);
    assert_eq!(report["writer"]["planned"]["keywords"], shape.keywords);

    // A second dry run must not change anything and must give the same report.
    let second_path = dir.path().join("report-second.json");
    let second = dry_run(&catalog, &second_path);
    exit_ok(&second);
    assert_eq!(
        fs::read(&second_path).unwrap(),
        fs::read(&report_path).unwrap(),
        "two dry runs over the same catalog produced different reports"
    );
    assert_catalog_unchanged(&catalog, &before);
    assert_no_state_file(dir.path());
}
