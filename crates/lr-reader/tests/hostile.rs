//! Hostile-input tests: deliberately damaged catalogs never panic or hang.
//!
//! For each [`lrcat_fixtures::hostile::HostileCase`] the generator writes a
//! damaged catalog; the reader then runs the full pipeline (photos, folders,
//! keywords, collections, stacks) under `catch_unwind`. The acceptance rule is
//! that the run either succeeds (with damaged rows skipped and a reason code,
//! and `read = converted + skipped`) or fails with a clear [`ReadError`],
//! never a panic.
//!
//! See `tasks/reports/t16-hostile-catalogs.md` for the per-case table.

use lr_reader::{Catalog, ReadError};
use lrcat_fixtures::hostile::{HostileCase, write_hostile_catalog};
use model::Report;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;

/// Runs the whole read pipeline on the catalog for `case`. Returns the opened
/// catalog's outcome: `Ok(())` if every read returned `Ok`, or the first
/// `ReadError`. A panic is caught by the caller.
fn read_pipeline(case: HostileCase, dir: &std::path::Path) -> Result<PathBuf, ReadError> {
    let path = write_hostile_catalog(dir, case)
        .map_err(|e| ReadError::Open(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))?;
    let catalog = Catalog::open(&path)?;

    let mut photos_report = Report::new();
    let photo_data = catalog.photo_data(&mut photos_report)?;
    // read = converted + skipped: mark one converted per returned photo.
    for _ in &photo_data.photos {
        photos_report.converted();
    }
    assert!(
        photos_report.is_balanced(),
        "{}: photos report not balanced",
        case.slug()
    );

    let mut folders_report = Report::new();
    let folders = catalog.folders(&mut folders_report)?;
    for _ in &folders {
        folders_report.converted();
    }
    assert!(
        folders_report.is_balanced(),
        "{}: folders report not balanced",
        case.slug()
    );

    let mut keyword_report = Report::new();
    let mut assignment_report = Report::new();
    let data = catalog.keywords(
        &photo_data.photos,
        &mut keyword_report,
        &mut assignment_report,
    )?;
    for _ in &data.keywords {
        keyword_report.converted();
    }
    for _ in data.assignments.values().flatten() {
        assignment_report.converted();
    }
    assert!(
        keyword_report.is_balanced(),
        "{}: keyword report not balanced",
        case.slug()
    );
    assert!(
        assignment_report.is_balanced(),
        "{}: assignment report not balanced",
        case.slug()
    );

    // Collections and stacks read their own reports internally; they must not
    // panic and must return Ok (skips live inside their data).
    catalog.collection_data(&photo_data.photos)?;
    catalog.stack_data(&photo_data.photos)?;
    Ok(path)
}

/// Cases that are expected to fail already at `Catalog::open` with a missing
/// schema, rather than going through the reads.
fn open_fails(case: HostileCase) -> bool {
    matches!(
        case,
        HostileCase::MissingRequiredTable | HostileCase::MissingRequiredColumn
    )
}

#[test]
fn no_hostile_case_panics_or_hangs() {
    for &case in HostileCase::all() {
        let dir = tempfile::tempdir().unwrap();
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| read_pipeline(case, dir.path())));
        match result {
            Ok(Ok(_)) => {
                assert!(
                    !open_fails(case),
                    "{}: expected Catalog::open to fail, but the pipeline succeeded",
                    case.slug()
                );
            }
            Ok(Err(e)) => {
                // A clear, typed error. For the missing-schema cases this is
                // expected; for any other case it is a defect to note.
                if !open_fails(case) {
                    eprintln!(
                        "{}: pipeline returned a ReadError (defect to review): {e}",
                        case.slug()
                    );
                }
            }
            Err(payload) => {
                // A panic is always a defect.
                let msg = payload
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| payload.downcast_ref::<&str>().copied())
                    .unwrap_or("<non-string panic>");
                panic!("{}: reader panicked: {msg}", case.slug());
            }
        }
    }
}
