//! Acceptance tests for keyword attributes (task P5-7).
//!
//! Expected API:
//! - `model::Keyword` gets public `bool` fields `include_on_export`,
//!   `export_containing`, `export_synonyms`, `person`; `Keyword::new` sets
//!   `true, true, true, false`.
//! - `lr_reader::KeywordData` gets `pub unknown_type: usize`: the number of
//!   *converted* keywords whose `keywordType` is neither NULL nor `person`.
//! - `Catalog::keywords` maps `includeOnExport` -> `include_on_export`,
//!   `includeParents` -> `export_containing`, `includeSynonyms` ->
//!   `export_synonyms` (non-zero or NULL = true, 0 = false) and
//!   `keywordType = 'person'` (exact, case-sensitive) -> `person` (NULL or
//!   any other value = false). Keywords with an unknown type are converted.
//! - `Catalog::open` requires the columns `includeOnExport`,
//!   `includeParents`, `includeSynonyms` and `keywordType` of
//!   `AgLibraryKeyword` (`ReadError::MissingSchema`).

use lr_reader::{Catalog, KeywordData, ReadError, checksum};
use lrcat_fixtures::{keyword_attr, write_keyword_attr_catalog, write_small_catalog};
use model::{Keyword, Report};
use std::path::Path;

fn read(path: &Path) -> KeywordData {
    let catalog = Catalog::open(path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    catalog
        .keywords(&photos, &mut Report::new(), &mut Report::new())
        .unwrap()
}

fn attr() -> (tempfile::TempDir, KeywordData) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("attrs.lrcat");
    write_keyword_attr_catalog(&path).unwrap();
    let data = read(&path);
    (dir, data)
}

fn by_id(data: &KeywordData, id: i64) -> &Keyword {
    let name = format!("Zyx-Attr-{id}");
    data.keywords
        .iter()
        .find(|k| k.path() == [name.clone()])
        .unwrap_or_else(|| panic!("keyword {id} missing"))
}

fn flags(k: &Keyword) -> (bool, bool, bool, bool) {
    (
        k.include_on_export,
        k.export_containing,
        k.export_synonyms,
        k.person,
    )
}

#[test]
fn new_keyword_has_lightroom_defaults() {
    let k = Keyword::new(vec!["A".into()]).unwrap();
    assert_eq!(flags(&k), (true, true, true, false));
}

#[test]
fn all_eight_flag_combinations_are_read() {
    let (_d, data) = attr();
    for n in 0..8_i64 {
        let k = by_id(&data, 14 + n);
        let want = (n & 4 != 0, n & 2 != 0, n & 1 != 0, false);
        assert_eq!(flags(k), want, "keyword {}", 14 + n);
    }
}

#[test]
fn person_keywords_are_marked() {
    let (_d, data) = attr();
    assert_eq!(flags(by_id(&data, 22)), (true, true, true, true));
    assert_eq!(flags(by_id(&data, 23)), (false, false, false, true));
}

#[test]
fn unknown_types_are_converted_without_person_flag() {
    let (_d, data) = attr();
    // Wrong case is not a person.
    assert_eq!(flags(by_id(&data, 24)), (true, true, true, false));
    assert_eq!(flags(by_id(&data, 25)), (false, true, false, false));
}

#[test]
fn unknown_type_count_covers_converted_keywords_only() {
    let (_d, data) = attr();
    // 24 and 25 count; the skipped keyword 28 (type `weird`) does not.
    assert_eq!(data.unknown_type as i64, keyword_attr::UNKNOWN_TYPE);
}

#[test]
fn non_zero_flag_values_mean_true() {
    let (_d, data) = attr();
    assert_eq!(flags(by_id(&data, 26)), (true, true, true, false));
}

#[test]
fn skipped_keyword_is_not_in_the_result() {
    let (_d, data) = attr();
    assert_eq!(data.keywords.len() as i64, keyword_attr::KEYWORDS_CONVERTED);
    assert!(
        !data
            .keywords
            .iter()
            .any(|k| k.path().last().is_some_and(|n| n.is_empty()))
    );
}

#[test]
fn counts_over_the_result_match_the_fixture_constants() {
    let (_d, data) = attr();
    let n = |f: &dyn Fn(&Keyword) -> bool| data.keywords.iter().filter(|k| f(k)).count() as i64;
    assert_eq!(n(&|k| k.person), keyword_attr::PERSONS);
    assert_eq!(n(&|k| !k.include_on_export), keyword_attr::NOT_EXPORTED);
    assert_eq!(n(&|k| !k.export_containing), keyword_attr::NO_PARENTS);
    assert_eq!(n(&|k| !k.export_synonyms), keyword_attr::NO_SYNONYMS);
}

#[test]
fn small_fixture_keywords_have_defaults() {
    // The small fixture has NULL types and all flags 1.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.lrcat");
    write_small_catalog(&path).unwrap();
    let data = read(&path);
    assert_eq!(data.unknown_type, 0);
    assert!(
        data.keywords
            .iter()
            .all(|k| flags(k) == (true, true, true, false))
    );
}

#[test]
fn reading_leaves_the_catalog_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("attrs.lrcat");
    write_keyword_attr_catalog(&path).unwrap();
    let before = checksum(&path).unwrap();
    let first = read(&path);
    let second = read(&path);
    assert_eq!(checksum(&path).unwrap(), before);
    assert_eq!(first.keywords.len(), second.keywords.len());
}

#[test]
fn missing_attribute_columns_are_reported() {
    for column in [
        "includeOnExport",
        "includeParents",
        "includeSynonyms",
        "keywordType",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("damaged.lrcat");
        lrcat_fixtures::write_small_catalog_without(&path, "AgLibraryKeyword", Some(column))
            .unwrap();
        match Catalog::open(&path) {
            Err(ReadError::MissingSchema { table, column: c }) => {
                assert_eq!(table, "AgLibraryKeyword");
                assert_eq!(c.as_deref(), Some(column));
            }
            Err(other) => panic!("expected MissingSchema for {column}, got {other:?}"),
            Ok(_) => panic!("open must fail without {column}"),
        }
    }
}
