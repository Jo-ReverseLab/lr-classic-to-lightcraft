//! Acceptance tests for reading keyword synonyms (task P5-3, mapping F-009:
//! synonyms are counted, not converted).
//!
//! Expected API (new file `crates/lr-reader/src/keyword_synonyms.rs`, same
//! pattern as `collections`):
//!
//! ```ignore
//! impl Catalog {
//!     pub fn keyword_synonyms(&self, report: &mut model::Report)
//!         -> Result<(), ReadError>;
//! }
//! ```
//!
//! Every `AgLibraryKeywordSynonym` row (ordered by `id_local`) calls
//! `report.read()` and `report.skipped(reason)`; the reader never calls
//! `converted()`. Reasons (exact, ids only, never the synonym text):
//! - `synonym {id} of keyword {k}: synonyms are not converted`
//! - keyword `k` not in `AgLibraryKeyword`:
//!   `synonym {id}: keyword {k} is unknown`
//! A synonym of a skipped keyword (empty name, `|`) gets the first reason.

use lr_reader::{Catalog, checksum};
use lrcat_fixtures::{keyword_edge, small, write_keyword_edge_catalog, write_small_catalog};
use model::Report;
use std::path::Path;

fn read_synonyms(path: &Path) -> Report {
    let catalog = Catalog::open(path).unwrap();
    let mut report = Report::new();
    catalog
        .keyword_synonyms(&mut report)
        .expect("read synonyms");
    report
}

fn reasons(report: &Report) -> Vec<String> {
    report
        .skipped_items()
        .iter()
        .map(|s| s.reason.clone())
        .collect()
}

#[test]
fn small_fixture_has_no_synonyms() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.lrcat");
    write_small_catalog(&path).unwrap();
    let report = read_synonyms(&path);
    assert_eq!(report.read_count(), 0);
    assert_eq!(report.converted_count(), 0);
    assert_eq!(report.skipped_count(), 0);
    assert!(report.is_balanced());
    let _ = small::PHOTOS;
}

#[test]
fn all_synonyms_are_read_and_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edge.lrcat");
    write_keyword_edge_catalog(&path).unwrap();
    let report = read_synonyms(&path);
    assert_eq!(report.read_count() as i64, keyword_edge::SYNONYMS);
    assert_eq!(report.converted_count(), 0);
    assert_eq!(report.skipped_count() as i64, keyword_edge::SYNONYMS);
    assert!(report.is_balanced());
}

#[test]
fn skip_reasons_are_exact_and_in_id_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edge.lrcat");
    write_keyword_edge_catalog(&path).unwrap();
    // Ids 1, 2: keyword 3. Id 3: skipped keyword 16 (first reason applies).
    // Id 4: keyword 999 does not exist.
    assert_eq!(
        reasons(&read_synonyms(&path)),
        vec![
            "synonym 1 of keyword 3: synonyms are not converted",
            "synonym 2 of keyword 3: synonyms are not converted",
            "synonym 3 of keyword 16: synonyms are not converted",
            "synonym 4: keyword 999 is unknown",
        ]
    );
}

#[test]
fn reasons_never_contain_synonym_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edge.lrcat");
    write_keyword_edge_catalog(&path).unwrap();
    for reason in reasons(&read_synonyms(&path)) {
        for text in keyword_edge::SYNONYM_TEXTS {
            assert!(!reason.contains(text), "{reason}");
        }
        assert!(!reason.contains("Zyx"), "{reason}");
    }
}

#[test]
fn reading_leaves_catalog_unchanged_and_repeats_identically() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edge.lrcat");
    write_keyword_edge_catalog(&path).unwrap();
    let before = checksum(&path).unwrap();
    let first = reasons(&read_synonyms(&path));
    let second = reasons(&read_synonyms(&path));
    assert_eq!(first, second);
    assert_eq!(checksum(&path).unwrap(), before);
}
