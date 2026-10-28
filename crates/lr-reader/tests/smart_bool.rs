//! Acceptance tests for boolean values in smart collection rules (task
//! K-5 rest), on `lrcat_fixtures::write_smart_bool_catalog`.
//!
//! STATUS: RED ON PURPOSE. The file does not compile until
//! `model::SmartValue::Bool` exists.
//!
//! API under test:
//!
//! ```ignore
//! pub enum SmartValue { Number(f64), Text(String), Bool(bool) }
//! ```
//!
//! Rules:
//! - `value = true|false` and `value2 = true|false` are read as
//!   `SmartValue::Bool`; the collection is then readable (it is in
//!   `CollectionData::smart`) and its skip reason names the criterion
//!   (`first criterion: <name>`), not "unreadable rules";
//! - `value_units = true|false` keeps the type `Option<String>` and holds
//!   the literal text `"true"` / `"false"`;
//! - a table as value, value2 or value_units stays an error (unreadable
//!   rules); `nil` is still "absent";
//! - an unknown extra key with a boolean (`flag = true`) is still ignored.

use lr_reader::{Catalog, CollectionData, parse_smart_rules};
use lrcat_fixtures::smart_bool as b;
use model::{Report, SmartCombine, SmartRule, SmartValue};

fn rule_of(id: i64) -> SmartRule {
    let text = b::ROWS.iter().find(|(i, _)| *i == id).unwrap().1;
    let r = parse_smart_rules(text).unwrap_or_else(|e| panic!("{id}: {e}"));
    assert_eq!(r.combine, SmartCombine::Intersect);
    assert_eq!(r.rules.len(), 1);
    r.rules.into_iter().next().unwrap()
}

#[test]
fn bool_value_is_read() {
    let r = rule_of(301);
    assert_eq!(r.criterion, "hasGps");
    assert_eq!(r.value, Some(SmartValue::Bool(true)));
    assert_eq!((r.value2, r.units), (None, None));
}

#[test]
fn bool_value2_is_read() {
    let r = rule_of(302);
    assert_eq!(r.value, Some(SmartValue::Number(3.0)));
    assert_eq!(r.value2, Some(SmartValue::Bool(false)));
}

#[test]
fn bool_units_are_kept_as_literal_text() {
    let r = rule_of(303);
    assert_eq!(r.units.as_deref(), Some("true"));
}

#[test]
fn false_and_nil_are_distinct() {
    let f = parse_smart_rules(
        "s = { { criteria = \"x\", operation = \"==\", value = false, }, combine = \"union\", }",
    )
    .unwrap();
    assert_eq!(f.rules[0].value, Some(SmartValue::Bool(false)));
    let n = parse_smart_rules(
        "s = { { criteria = \"x\", operation = \"==\", value = nil, }, combine = \"union\", }",
    )
    .unwrap();
    assert_eq!(n.rules[0].value, None);
}

#[test]
fn tables_as_values_stay_errors() {
    for key in ["value", "value2", "value_units"] {
        let text = format!(
            "s = {{ {{ criteria = \"x\", operation = \"==\", {key} = {{ 1 }}, }}, combine = \"union\", }}"
        );
        assert!(parse_smart_rules(&text).is_err(), "{key}");
    }
    assert!(parse_smart_rules(b::ROWS[3].1).is_err());
}

#[test]
fn unknown_boolean_keys_are_still_ignored() {
    let r = parse_smart_rules(
        "s = { { criteria = \"x\", operation = \"==\", flag = true, }, combine = \"union\", }",
    )
    .unwrap();
    assert_eq!(r.rules[0].value, None);
}

fn read() -> (tempfile::TempDir, CollectionData) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("smart.lrcat");
    lrcat_fixtures::write_smart_bool_catalog(&path).unwrap();
    let catalog = Catalog::open(&path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    let d = catalog.collection_data(&photos).unwrap();
    (dir, d)
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
    assert_eq!(hits.len(), 1, "{id}: {hits:?}");
    hits.into_iter().next().unwrap()
}

#[test]
fn bool_collections_are_readable_and_name_the_criterion() {
    let (_dir, d) = read();
    for (id, name) in [(301, "hasGps"), (302, "rating"), (303, "captureTime")] {
        assert!(d.smart.contains_key(&id), "{id}");
        let r = reason_for(&d, id);
        assert!(r.contains(name), "{id}: {r}");
        assert!(!r.to_lowercase().contains("unreadable"), "{id}: {r}");
    }
    assert_eq!(d.smart[&301].rules[0].value, Some(SmartValue::Bool(true)));
}

#[test]
fn table_value_collection_is_still_unreadable() {
    let (_dir, d) = read();
    assert!(!d.smart.contains_key(&304));
    assert!(reason_for(&d, 304).contains("unreadable rules"));
}

#[test]
fn balance_holds() {
    let (_dir, d) = read();
    assert!(d.report.is_balanced());
}
