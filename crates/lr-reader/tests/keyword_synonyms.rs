//! Acceptance tests for reading keyword synonyms (task P5-3; user decision:
//! synonyms are converted together with their keyword).
//!
//! Expected API changes (no new `Catalog` method):
//! - `model::Keyword` gets a public field `pub synonyms: Vec<String>`
//!   (empty by default, set by `Keyword::new`). The tests read the field; how
//!   `PartialEq` treats it is up to the developer (existing keyword tests
//!   compare against `Keyword::new(path)`, so those expectations may need to
//!   compare `path()` instead - the test author will adapt them on request).
//! - `lr_reader::KeywordData` gets `pub synonym_report: model::Report`.
//!   `Catalog::keywords` fills `KeywordData::keywords[i].synonyms` with the
//!   texts of that keyword's `AgLibraryKeywordSynonym` rows, ordered by
//!   `id_local`.
//! - Every synonym row calls `synonym_report.read()`; skipped rows call
//!   `skipped(reason)`; the reader never calls `converted()` (like the other
//!   reports; converted = total synonyms on the returned keywords). Reasons
//!   (exact, ids only, never the text), checked in this order:
//!   1. keyword not in `AgLibraryKeyword`: `synonym {id}: keyword {k} is unknown`
//!   2. keyword not converted (empty name, `|`, skipped parent, hidden root):
//!      `synonym {id}: keyword {k} is skipped`
//!   3. text NULL or empty: `synonym {id}: name is empty`

use lr_reader::Catalog;
use lrcat_fixtures::{keyword_edge, write_keyword_edge_catalog, write_small_catalog};
use model::{Photo, Report};
use std::path::Path;

fn run(path: &Path) -> lr_reader::KeywordData {
    let catalog = Catalog::open(path).unwrap();
    let mut photo_report = Report::new();
    let photos: Vec<Photo> = catalog.photos(&mut photo_report).expect("read photos");
    catalog
        .keywords(&photos, &mut Report::new(), &mut Report::new())
        .expect("read keywords")
}

fn reasons(report: &Report) -> Vec<String> {
    report
        .skipped_items()
        .iter()
        .map(|s| s.reason.clone())
        .collect()
}

fn edge() -> (tempfile::TempDir, lr_reader::KeywordData) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edge.lrcat");
    write_keyword_edge_catalog(&path).unwrap();
    let data = run(&path);
    (dir, data)
}

#[test]
fn small_fixture_has_no_synonyms() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.lrcat");
    write_small_catalog(&path).unwrap();
    let data = run(&path);
    assert_eq!(data.synonym_report.read_count(), 0);
    assert_eq!(data.synonym_report.skipped_count(), 0);
    assert!(data.keywords.iter().all(|k| k.synonyms.is_empty()));
}

#[test]
fn synonyms_are_attached_in_id_order() {
    let (_d, data) = edge();
    let italy = data
        .keywords
        .iter()
        .find(|k| k.path() == ["Travel", "Italy"])
        .expect("Italy keyword");
    assert_eq!(italy.synonyms, keyword_edge::KEYWORD_3_SYNONYMS);
}

#[test]
fn only_keyword_3_has_synonyms() {
    let (_d, data) = edge();
    let total: usize = data.keywords.iter().map(|k| k.synonyms.len()).sum();
    assert_eq!(total as i64, keyword_edge::SYNONYMS_CONVERTED);
    // Keyword 2 only had empty/NULL synonyms.
    let travel = data
        .keywords
        .iter()
        .find(|k| k.path() == ["Travel"])
        .expect("Travel keyword");
    assert!(travel.synonyms.is_empty());
}

#[test]
fn counts_add_up() {
    let (_d, data) = edge();
    let r = &data.synonym_report;
    assert_eq!(r.read_count() as i64, keyword_edge::SYNONYMS);
    assert_eq!(r.converted_count(), 0, "reader never marks converted");
    assert_eq!(
        r.skipped_count() as i64,
        keyword_edge::SYNONYMS - keyword_edge::SYNONYMS_CONVERTED
    );
    let mut rep = r.clone();
    for k in &data.keywords {
        for _ in &k.synonyms {
            rep.converted();
        }
    }
    assert!(rep.is_balanced());
}

#[test]
fn skip_reasons_are_exact_and_in_id_order() {
    let (_d, data) = edge();
    assert_eq!(
        reasons(&data.synonym_report),
        vec![
            "synonym 3: keyword 16 is skipped",
            "synonym 4: keyword 999 is unknown",
            "synonym 5: name is empty",
            "synonym 6: name is empty",
        ]
    );
}

#[test]
fn reasons_never_contain_synonym_text() {
    let (_d, data) = edge();
    for reason in reasons(&data.synonym_report) {
        assert!(!reason.contains("Zyx"), "{reason}");
    }
}
