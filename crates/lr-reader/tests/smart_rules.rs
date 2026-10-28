//! Acceptance tests for reading smart collection rules, on the
//! smart fixture (`lrcat_fixtures::smart_edge`, `write_smart_catalog`).
//!
//! Expected API:
//!
//! ```ignore
//! // model (re-exported at the crate root)
//! #[derive(Debug, Clone, PartialEq)]
//! pub struct SmartRules { pub combine: SmartCombine, pub rules: Vec<SmartRule> }
//! #[derive(Debug, Clone, PartialEq, Eq)]
//! pub enum SmartCombine { Intersect, Union, Other(String) }
//! #[derive(Debug, Clone, PartialEq)]
//! pub struct SmartRule {
//!     pub criterion: String,          // key `criteria`, verbatim
//!     pub operation: String,          // key `operation`, verbatim
//!     pub value: Option<SmartValue>,  // key `value`; None if absent
//!     pub value2: Option<SmartValue>, // key `value2`; None if absent
//!     pub units: Option<String>,      // key `value_units`; None if absent
//! }
//! #[derive(Debug, Clone, PartialEq)]
//! pub enum SmartValue { Number(f64), Text(String) }
//!
//! // lr-reader (re-exported from lib.rs)
//! /// Parses the Lua/lron text of one smart collection. Never panics, also
//! /// not on truncated, binary or absurdly nested input. The error type
//! /// implements `Debug + Display` and carries no part of the input.
//! pub fn parse_smart_rules(text: &str) -> Result<model::SmartRules, SmartParseError>;
//! // CollectionData gets one more field:
//! pub smart: BTreeMap<i64, model::SmartRules>, // by collection id, readable only
//! ```
//!
//! Rules under test:
//! - Text form: `s = { {criteria = "..", operation = "..", value = ..,
//!   value2 = .., value_units = ".."}, ..., combine = ".." }`. The rules are
//!   the unnamed table entries in order; `combine` may stand before or
//!   after them; whitespace is free. `intersect` / `union` map to
//!   `Intersect` / `Union`, any other string to `Other(string)` verbatim.
//!   Unknown criteria and operations stay verbatim. Unknown extra keys
//!   (strings with escapes, booleans) are ignored. String escapes `\"`,
//!   `\\` and backslash-newline are decoded. Numbers (integer, negative,
//!   fractional) become `Number`, strings `Text` (a string "3" stays text).
//!   No criteria: `Ok` with an empty `rules`.
//! - Broken text (truncated, not a table, empty, trailing garbage, missing
//!   value after `=`, binary): `Err`, never a panic. A rule without
//!   `criteria` or `operation` is `Err` as well.
//! - `Catalog::collection_data`: only content rows with `owningModule`
//!   `ag.library.smart_collection` of collections whose `creationId` is
//!   smart count. A missing `AgLibraryCollectionContent` table, a missing
//!   row or a NULL content is "unreadable rules", not an error.
//! - Nothing is translated yet: every smart collection stays skipped. The
//!   reason keeps the old phrase and id: `collection <id>: smart collection
//!   is not converted (first criterion: <name>)` for readable rules,
//!   `... (no criteria)` for an empty rule list, `... (unreadable rules)`
//!   for the rest. The exact wording beyond the quoted phrases is not
//!   tested (case-insensitive). Reasons never contain a rule value, an
//!   operation or a collection name; criterion names (also unknown ones)
//!   are allowed.
//! - Balance: read = converted + skipped, as before.

use lr_reader::{Catalog, CollectionData, parse_smart_rules};
use lrcat_fixtures::smart_edge as e;
use model::{Report, SmartCombine, SmartRule, SmartRules, SmartValue};
use std::path::Path;

fn num(x: f64) -> Option<SmartValue> {
    Some(SmartValue::Number(x))
}

fn txt(s: &str) -> Option<SmartValue> {
    Some(SmartValue::Text(s.to_string()))
}

fn rule(
    criterion: &str,
    operation: &str,
    value: Option<SmartValue>,
    value2: Option<SmartValue>,
    units: Option<&str>,
) -> SmartRule {
    SmartRule {
        criterion: criterion.into(),
        operation: operation.into(),
        value,
        value2,
        units: units.map(String::from),
    }
}

fn rules(combine: SmartCombine, rules: Vec<SmartRule>) -> SmartRules {
    SmartRules { combine, rules }
}

/// The exact expected result for each readable fixture text.
fn expected(id: i64) -> SmartRules {
    use SmartCombine::{Intersect, Other, Union};
    match id {
        101 => rules(Intersect, vec![rule("rating", ">=", num(3.0), None, None)]),
        102 => rules(Union, vec![rule("pick", "==", num(-1.0), None, None)]),
        103 => rules(
            Intersect,
            vec![rule("labelColor", "==", txt("red ZyxSecret"), None, None)],
        ),
        104 => rules(
            Intersect,
            vec![rule(
                "keywords",
                "any",
                txt("Z\u{fc}\u{df}e \u{1f305} ZyxSecret"),
                None,
                None,
            )],
        ),
        105 => rules(Intersect, vec![rule("keywords", "empty", None, None, None)]),
        106 => rules(
            Intersect,
            vec![rule("captureTime", "inLast", num(30.0), None, Some("days"))],
        ),
        107 => rules(
            Intersect,
            vec![rule("touchTime", "inLast", num(2.0), None, Some("months"))],
        ),
        108 => rules(
            Intersect,
            vec![rule("fileFormat", "==", txt("DNG ZyxSecret"), None, None)],
        ),
        109 => rules(
            Union,
            vec![
                rule("rating", "==", num(5.0), None, None),
                rule("labelColor", "==", txt("blue ZyxSecret"), None, None),
                rule("rating", "==", num(4.0), None, None),
            ],
        ),
        110 => rules(
            Other("zyxCombine".into()),
            vec![rule("zyxCriterion", "zyxOp", txt("ZyxSecret"), None, None)],
        ),
        111 => rules(
            Intersect,
            vec![
                rule("rating", "<", num(2.5), num(-1.0), None),
                rule("rating", ">", txt("3"), None, None),
            ],
        ),
        112 => rules(
            Intersect,
            vec![rule(
                "keywords",
                "any",
                txt("say \"ZyxSecret\" \\ ok"),
                None,
                None,
            )],
        ),
        113 => rules(Intersect, vec![]),
        114 => rules(Union, vec![rule("pick", "==", num(1.0), None, None)]),
        _ => panic!("no expectation for {id}"),
    }
}

fn fixture(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("smart.lrcat");
    lrcat_fixtures::write_smart_catalog(&path).expect("generate fixture");
    path
}

fn read(path: &Path) -> CollectionData {
    let catalog = Catalog::open(path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    catalog.collection_data(&photos).expect("read collections")
}

// ---- the parser alone ----

#[test]
fn parser_returns_the_exact_model_for_every_readable_text() {
    for (id, text) in e::READABLE {
        let got = parse_smart_rules(text).unwrap_or_else(|err| panic!("{id}: {err}"));
        assert_eq!(got, expected(id), "collection {id}");
    }
}

#[test]
fn parser_rejects_broken_texts_without_panic() {
    for (id, text) in e::UNREADABLE {
        if let Some(text) = text {
            assert!(parse_smart_rules(text).is_err(), "collection {id}");
        }
    }
}

#[test]
fn parser_errors_do_not_echo_the_input() {
    for (_, text) in e::UNREADABLE {
        if let Some(text) = text {
            let err = parse_smart_rules(text).unwrap_err();
            let shown = format!("{err} {err:?}");
            assert!(!shown.contains(e::SECRET), "{shown}");
        }
    }
}

#[test]
fn parser_survives_every_prefix_of_every_text() {
    for (_, text) in e::READABLE {
        for end in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
            let _ = parse_smart_rules(&text[..end]);
        }
    }
}

#[test]
fn parser_rejects_a_truncated_text_it_cannot_complete() {
    // Cutting the closing brace of the outer table must not be accepted.
    let (_, text) = e::READABLE[0];
    let cut = text.trim_end().strip_suffix('}').unwrap();
    assert!(parse_smart_rules(cut).is_err());
}

#[test]
fn parser_handles_absurd_nesting_without_overflow() {
    let deep = format!("s = {}", "{".repeat(200_000));
    assert!(parse_smart_rules(&deep).is_err());
    let closed = format!("s = {}{}", "{".repeat(200_000), "}".repeat(200_000));
    let _ = parse_smart_rules(&closed); // Ok or Err, but no crash
}

#[test]
fn parser_rejects_a_rule_without_criteria_or_operation() {
    let no_criteria = "s = { { operation = \"==\", value = 1, }, combine = \"intersect\", }";
    let no_operation = "s = { { criteria = \"rating\", value = 1, }, combine = \"intersect\", }";
    assert!(parse_smart_rules(no_criteria).is_err());
    assert!(parse_smart_rules(no_operation).is_err());
}

#[test]
fn parser_keeps_unicode_and_empty_strings() {
    let text =
        "s = { { criteria = \"keywords\", operation = \"any\", value = \"\", }, combine = \"\", }";
    let got = parse_smart_rules(text).unwrap();
    assert_eq!(got.combine, SmartCombine::Other(String::new()));
    assert_eq!(got.rules[0].value, txt(""));
}

// ---- through the catalog ----

#[test]
fn catalog_returns_rules_of_readable_smart_collections_only() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    let ids: Vec<i64> = d.smart.keys().copied().collect();
    let want: Vec<i64> = e::READABLE.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, want);
    assert_eq!(d.smart.len() as i64, e::READABLE_COUNT);
    for (id, rules) in &d.smart {
        assert_eq!(rules, &expected(*id), "collection {id}");
    }
}

#[test]
fn other_modules_and_plain_collections_do_not_count() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    // 101 has a garbage view-settings row next to its rules.
    assert_eq!(d.smart[&101], expected(101));
    assert!(!d.smart.contains_key(&e::OTHER_MODULE_ONLY));
    assert!(!d.smart.contains_key(&e::PLAIN_WITH_RULES), "plain album");
    // The plain album is still converted as before.
    assert!(
        d.collections
            .iter()
            .any(|c| c.lightroom_id == e::PLAIN_WITH_RULES)
    );
}

fn reason_for(d: &CollectionData, id: i64) -> String {
    let needle = id.to_string();
    let hits: Vec<String> = d
        .report
        .skipped_items()
        .iter()
        .map(|s| s.reason.clone())
        .filter(|r| r.split(|c: char| !c.is_ascii_digit()).any(|w| w == needle))
        .collect();
    assert_eq!(hits.len(), 1, "one reason for collection {id}: {hits:?}");
    hits.into_iter().next().unwrap()
}

#[test]
fn every_smart_collection_stays_skipped_with_the_old_phrase() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    let all = e::READABLE
        .iter()
        .map(|(id, _)| *id)
        .chain(e::UNREADABLE.iter().map(|(id, _)| *id))
        .chain([e::OTHER_MODULE_ONLY, 5, 6]);
    let mut n = 0;
    for id in all {
        let r = reason_for(&d, id).to_lowercase();
        assert!(r.contains("smart collection"), "{id}: {r}");
        assert!(!d.collections.iter().any(|c| c.lightroom_id == id), "{id}");
        n += 1;
    }
    assert_eq!(n, e::SMART_TOTAL);
}

#[test]
fn reason_names_the_first_criterion() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    let first = [
        (101, "rating"),
        (102, "pick"),
        (103, "labelColor"),
        (104, "keywords"),
        (105, "keywords"),
        (106, "captureTime"),
        (107, "touchTime"),
        (108, "fileFormat"),
        (109, "rating"),
        (110, "zyxCriterion"),
        (111, "rating"),
        (112, "keywords"),
        (114, "pick"),
    ];
    for (id, name) in first {
        let r = reason_for(&d, id);
        assert!(r.contains(name), "{id}: {r}");
        assert!(!r.to_lowercase().contains("unreadable"), "{id}: {r}");
    }
    // 109 starts with rating, not with labelColor.
    assert!(!reason_for(&d, 109).contains("labelColor"));
    let r = reason_for(&d, 113).to_lowercase();
    assert!(r.contains("no criteria"), "{r}");
}

#[test]
fn broken_rules_give_unreadable_rules() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    let broken = e::UNREADABLE
        .iter()
        .map(|(id, _)| *id)
        .chain([e::OTHER_MODULE_ONLY, 5, 6]);
    let mut n = 0;
    for id in broken {
        let r = reason_for(&d, id).to_lowercase();
        assert!(r.contains("unreadable rules"), "{id}: {r}");
        n += 1;
    }
    assert_eq!(n, e::UNREADABLE_COUNT);
}

#[test]
fn reasons_never_contain_values_operations_or_names() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    let all: String = d
        .report
        .skipped_items()
        .iter()
        .map(|s| s.reason.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!all.contains(e::SECRET), "{all}");
    for word in [
        "zyxOp",
        "zyxCombine",
        "inLast",
        "intersect",
        "union",
        "days",
    ] {
        assert!(!all.contains(word), "{word} in reasons");
    }
}

#[test]
fn collections_balance_and_are_not_otherwise_affected() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    // Small fixture: 9 rows, 2 smart; plus the 23 added.
    let total = lrcat_fixtures::small::COLLECTIONS + e::ADDED;
    assert_eq!(d.report.read_count(), total as usize);
    assert!(d.report.is_balanced());
    // Album 4 and the others that were converted before still are.
    let plain: Vec<i64> = d.collections.iter().map(|c| c.lightroom_id).collect();
    assert_eq!(plain, vec![1, 2, 3, 4]);
}

#[test]
fn catalog_without_content_table_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.lrcat");
    lrcat_fixtures::write_small_catalog(&path).unwrap();
    let d = read(&path);
    assert!(d.smart.is_empty());
    for id in [5, 6] {
        let r = reason_for(&d, id).to_lowercase();
        assert!(r.contains("smart collection"), "{r}");
        assert!(r.contains("unreadable rules"), "{r}");
    }
}

#[test]
fn reading_leaves_the_catalog_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture(dir.path());
    let before = lr_reader::checksum(&path).unwrap();
    let first = read(&path);
    let second = read(&path);
    assert_eq!(lr_reader::checksum(&path).unwrap(), before);
    assert_eq!(first.smart, second.smart);
    let a: Vec<_> = first
        .report
        .skipped_items()
        .iter()
        .map(|s| s.reason.clone())
        .collect();
    let b: Vec<_> = second
        .report
        .skipped_items()
        .iter()
        .map(|s| s.reason.clone())
        .collect();
    assert_eq!(a, b);
}
