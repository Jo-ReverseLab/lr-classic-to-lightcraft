//! Acceptance tests for reading keywords.
//!
//! Expected API (new file `crates/lr-reader/src/keywords.rs`, re-exported
//! from `lib.rs`):
//!
//! ```ignore
//! pub struct KeywordData {
//!     /// Converted keywords, ordered by `AgLibraryKeyword.id_local`.
//!     pub keywords: Vec<model::Keyword>,
//!     /// Keywords per Lightroom photo id (`Photo::lightroom_id`), in order of
//!     /// the assignment rows (`AgLibraryKeywordImage.id_local`). Photos
//!     /// without keywords have no entry.
//!     pub assignments: std::collections::BTreeMap<i64, Vec<model::Keyword>>,
//! }
//!
//! impl Catalog {
//!     pub fn keywords(
//!         &self,
//!         photos: &[model::Photo],            // the photos that were converted
//!         keyword_report: &mut model::Report, // one entry per keyword row
//!         assignment_report: &mut model::Report, // one entry per assignment row
//!     ) -> Result<KeywordData, ReadError>;
//! }
//! ```
//!
//! Rules:
//! - Tree via `AgLibraryKeyword.parent`; `Keyword::path()` is the full path,
//!   root first. The hidden root row (`Adobe_variablesTable`
//!   `AgLibraryKeyword_rootTagID`) is neither a keyword nor counted as read.
//! - Every other keyword row calls `keyword_report.read()`; the reader never
//!   calls `converted()` (the writer does). Skipped with a reason (exact
//!   texts, ids only, never names):
//!   - name contains `|`: "keyword {id}: name contains a vertical bar"
//!   - name empty: "keyword {id}: name is empty"
//!   - parent skipped: "keyword {id}: parent keyword {parent} is skipped"
//!     (this covers the whole subtree of an empty keyword and the children
//!     of a `|` keyword; a missing parent or a cycle gets the same reason)
//! - Every `AgLibraryKeywordImage` row calls `assignment_report.read()`.
//!   Skipped with a reason:
//!   - photo not in `photos` (skipped by the photo reader, or unknown):
//!     "assignment {id}: photo {image} is skipped or unknown"
//!   - keyword skipped, unknown or the root:
//!     "assignment {id}: keyword {tag} is skipped"
//!   - photo and keyword both skipped: one entry, with the photo reason
//! - Person and "do not export" keywords are converted like any other.

use lr_reader::{Catalog, checksum};
use lrcat_fixtures::{keyword_edge, small, write_keyword_edge_catalog};
use model::{Keyword, Photo, Report};
use std::path::{Path, PathBuf};

struct Run {
    data: lr_reader::KeywordData,
    keyword_report: Report,
    assignment_report: Report,
}

fn edge_fixture(dir: &Path) -> PathBuf {
    let path = dir.join("keywords.lrcat");
    write_keyword_edge_catalog(&path).expect("generate fixture");
    path
}

fn run(path: &Path) -> Run {
    let catalog = Catalog::open(path).unwrap();
    let mut photo_report = Report::new();
    let photos: Vec<Photo> = catalog.photos(&mut photo_report).expect("read photos");
    let mut keyword_report = Report::new();
    let mut assignment_report = Report::new();
    let data = catalog
        .keywords(&photos, &mut keyword_report, &mut assignment_report)
        .expect("read keywords");
    Run {
        data,
        keyword_report,
        assignment_report,
    }
}

fn kw(parts: &[&str]) -> Keyword {
    Keyword::new(parts.iter().map(|s| s.to_string()).collect()).unwrap()
}

fn paths(keywords: &[Keyword]) -> Vec<Vec<String>> {
    keywords.iter().map(|k| k.path().to_vec()).collect()
}

fn reasons(report: &Report) -> Vec<String> {
    report
        .skipped_items()
        .iter()
        .map(|s| s.reason.clone())
        .collect()
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

#[test]
fn keyword_counts_add_up() {
    let dir = tempfile::tempdir().unwrap();
    let r = run(&edge_fixture(dir.path()));
    // The hidden root row is not counted.
    assert_eq!(r.keyword_report.read_count() as i64, keyword_edge::KEYWORDS);
    assert_eq!(
        r.data.keywords.len() as i64,
        keyword_edge::KEYWORDS_CONVERTED
    );
    assert_eq!(
        r.keyword_report.skipped_count() as i64,
        keyword_edge::KEYWORDS - keyword_edge::KEYWORDS_CONVERTED
    );
    // Reader does not mark converted; the writer does. Simulate it.
    let mut rep = r.keyword_report.clone();
    for _ in &r.data.keywords {
        rep.converted();
    }
    assert!(rep.is_balanced());
}

#[test]
fn reader_never_marks_converted() {
    let dir = tempfile::tempdir().unwrap();
    let r = run(&edge_fixture(dir.path()));
    assert_eq!(r.keyword_report.converted_count(), 0);
    assert_eq!(r.assignment_report.converted_count(), 0);
}

#[test]
fn small_fixture_keyword_counts_match_constants() {
    // The small fixture has one empty keyword (id 12) and no `|` keyword.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.lrcat");
    lrcat_fixtures::write_small_catalog(&path).unwrap();
    let r = run(&path);
    assert_eq!(r.keyword_report.read_count() as i64, small::KEYWORDS);
    assert_eq!(r.keyword_report.skipped_count(), 1);
    assert_eq!(r.data.keywords.len() as i64, small::KEYWORDS - 1);
    assert_eq!(r.assignment_report.read_count() as i64, small::ASSIGNMENTS);
}

#[test]
fn keywords_have_full_paths_root_first() {
    let dir = tempfile::tempdir().unwrap();
    let r = run(&edge_fixture(dir.path()));
    let expected = vec![
        kw(&["Travel"]),                               // 2
        kw(&["Travel", "Italy"]),                      // 3
        kw(&["Travel", "Italy", "Rome"]),              // 4
        kw(&["Travel", "Italy", "Rome", "Colosseum"]), // 5, depth 4
        kw(&["Travel", "Köln"]),                       // 6 (umlaut)
        kw(&["Süßwasser"]),                            // 7
        kw(&["🌅 Sunrise"]),                           // 8 (emoji)
        kw(&["Sea"]),                                  // 9
        kw(&["Ocean"]),                                // 10
        kw(&["Unused"]),                               // 11
        kw(&["Family"]),                               // 13
        kw(&["Alice Example"]),                        // 19 (person)
    ];
    // Compare paths only: synonyms are tested in keyword_synonyms.rs.
    assert_eq!(paths(&r.data.keywords), paths(&expected));
    let max_depth = r.data.keywords.iter().map(|k| k.path().len()).max();
    assert_eq!(max_depth, Some(small::KEYWORD_MAX_DEPTH as usize));
}

#[test]
fn root_keyword_is_not_a_keyword() {
    let dir = tempfile::tempdir().unwrap();
    let r = run(&edge_fixture(dir.path()));
    // No keyword has an empty path or shows up as an extra top-level entry.
    assert!(r.data.keywords.iter().all(|k| !k.path().is_empty()));
    assert!(
        reasons(&r.keyword_report)
            .iter()
            .all(|s| !s.starts_with("keyword 1:"))
    );
}

#[test]
fn person_and_do_not_export_keyword_is_converted() {
    let dir = tempfile::tempdir().unwrap();
    let r = run(&edge_fixture(dir.path()));
    // Compare paths only: the person and export attributes are tested in
    // keyword_attributes.rs.
    let alice = kw(&["Alice Example"]).path().to_vec();
    assert!(paths(&r.data.keywords).contains(&alice));
    // Photo 5 carries it.
    assert!(paths(&r.data.assignments[&5]).contains(&alice));
}

#[test]
fn skipped_keywords_have_exact_reasons_without_names() {
    let dir = tempfile::tempdir().unwrap();
    let r = run(&edge_fixture(dir.path()));
    let expected = sorted(vec![
        "keyword 12: name is empty".to_string(),
        "keyword 14: name contains a vertical bar".to_string(),
        "keyword 16: name is empty".to_string(),
        "keyword 17: parent keyword 16 is skipped".to_string(),
        "keyword 18: parent keyword 17 is skipped".to_string(),
    ]);
    assert_eq!(sorted(reasons(&r.keyword_report)), expected);
    // Names never leak.
    for reason in reasons(&r.keyword_report) {
        assert!(!reason.contains("Bad"), "{reason}");
        assert!(!reason.contains("Under Empty"), "{reason}");
    }
}

#[test]
fn subtree_of_empty_keyword_is_not_returned() {
    let dir = tempfile::tempdir().unwrap();
    let r = run(&edge_fixture(dir.path()));
    for k in &r.data.keywords {
        assert!(!k.path().iter().any(|p| p.contains("Under Empty")));
        assert!(!k.path().iter().any(|p| p.contains('|')));
    }
}

#[test]
fn assignment_counts_add_up() {
    let dir = tempfile::tempdir().unwrap();
    let r = run(&edge_fixture(dir.path()));
    let converted: usize = r.data.assignments.values().map(Vec::len).sum();
    assert_eq!(
        r.assignment_report.read_count() as i64,
        keyword_edge::ASSIGNMENTS
    );
    assert_eq!(converted as i64, keyword_edge::ASSIGNMENTS_CONVERTED);
    assert_eq!(
        r.assignment_report.skipped_count(),
        keyword_edge::ASSIGNMENTS_SKIPPED.len()
    );
    let mut rep = r.assignment_report.clone();
    for _ in 0..converted {
        rep.converted();
    }
    assert!(rep.is_balanced());
}

#[test]
fn skipped_assignments_have_exact_reasons() {
    let dir = tempfile::tempdir().unwrap();
    let r = run(&edge_fixture(dir.path()));
    let expected = sorted(vec![
        // Missing photo.
        "assignment 34: photo 999 is skipped or unknown".to_string(),
        // Skipped keywords: `|`, empty, below empty, other empty.
        "assignment 31: keyword 14 is skipped".to_string(),
        "assignment 32: keyword 16 is skipped".to_string(),
        "assignment 33: keyword 18 is skipped".to_string(),
        "assignment 36: keyword 12 is skipped".to_string(),
    ]);
    assert_eq!(sorted(reasons(&r.assignment_report)), expected);
}

#[test]
fn skipped_photo_has_no_keyword_entry() {
    let dir = tempfile::tempdir().unwrap();
    let r = run(&edge_fixture(dir.path()));
    // The virtual copy 20 converts with its master (T03) and keeps its own
    // assignments; the unknown photo 999 has none.
    assert!(r.data.assignments.contains_key(&20));
    assert!(!r.data.assignments.contains_key(&999));
}

#[test]
fn photos_get_their_keyword_lists() {
    let dir = tempfile::tempdir().unwrap();
    let r = run(&edge_fixture(dir.path()));
    // Compare paths only: keyword 3 (Italy) carries synonyms in this fixture.
    let a: std::collections::BTreeMap<i64, Vec<Vec<String>>> = r
        .data
        .assignments
        .iter()
        .map(|(photo, list)| (*photo, paths(list)))
        .collect();
    let p = |parts: &[&str]| kw(parts).path().to_vec();
    assert_eq!(a[&1], vec![p(&["Travel"]), p(&["Travel", "Italy"])]);
    assert_eq!(
        a[&5],
        vec![
            p(&["Travel", "Italy", "Rome"]),
            p(&["Travel", "Italy", "Rome", "Colosseum"]),
            p(&["Alice Example"]),
        ]
    );
    assert_eq!(
        a[&2],
        vec![
            p(&["Travel", "Italy", "Rome", "Colosseum"]),
            p(&["Travel", "Köln"])
        ]
    );
    // Photo 4: Family (kept), the empty keyword 12 is dropped.
    assert_eq!(a[&4], vec![p(&["Family"]), p(&["Travel"])]);
}

#[test]
fn only_converted_photos_are_given_keywords() {
    // Passing a reduced photo list makes the other photos "skipped or unknown".
    let dir = tempfile::tempdir().unwrap();
    let path = edge_fixture(dir.path());
    let catalog = Catalog::open(&path).unwrap();
    let mut pr = Report::new();
    let mut photos = catalog.photos(&mut pr).unwrap();
    photos.retain(|p| p.lightroom_id == 1);
    let (mut kr, mut ar) = (Report::new(), Report::new());
    let data = catalog.keywords(&photos, &mut kr, &mut ar).unwrap();
    assert_eq!(
        data.assignments.keys().copied().collect::<Vec<_>>(),
        vec![1]
    );
    assert_eq!(ar.read_count() as i64, keyword_edge::ASSIGNMENTS);
    let kept: usize = data.assignments.values().map(Vec::len).sum();
    assert_eq!(kept + ar.skipped_count(), ar.read_count());
}

#[test]
fn catalog_is_unchanged_and_second_run_is_identical() {
    let dir = tempfile::tempdir().unwrap();
    let path = edge_fixture(dir.path());
    let before = checksum(&path).unwrap();
    let first = run(&path);
    let second = run(&path);
    assert_eq!(checksum(&path).unwrap(), before);
    assert_eq!(first.data.keywords, second.data.keywords);
    assert_eq!(first.data.assignments, second.data.assignments);
    // No duplicate keywords.
    let mut seen = std::collections::HashSet::new();
    assert!(first.data.keywords.iter().all(|k| seen.insert(k.clone())));
    assert_eq!(
        reasons(&first.keyword_report),
        reasons(&second.keyword_report)
    );
}

#[test]
fn large_catalog_keywords_balance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.lrcat");
    lrcat_fixtures::write_large_catalog(&path, 2000).unwrap();
    let r = run(&path);
    assert_eq!(r.keyword_report.read_count(), 200);
    assert_eq!(r.keyword_report.skipped_count(), 0);
    assert_eq!(r.data.keywords.len(), 200);
    assert_eq!(r.assignment_report.read_count(), 2000);
    let converted: usize = r.data.assignments.values().map(Vec::len).sum();
    assert_eq!(converted + r.assignment_report.skipped_count(), 2000);
}
