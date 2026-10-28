#![cfg(any())]
//! Acceptance tests for the translation of smart collection rules (task
//! P5b-4b), module `lc_writer::smart`.
//!
//! STATUS: waiting for P5b-4b. The whole file is disabled by the crate-level
//! `cfg(any())` on the first line. The developer may ONLY remove that line.
//! Any other change to this file needs the test author.
//!
//! API under test:
//! `pub fn translate(rules: &model::SmartRules) -> Result<serde_json::Value, String>`
//! - a pure function: no I/O, same input, same output;
//! - `Ok` is the `rules` parameter of `album.createSmart`, a partial filter
//!   `{"ruleSet": {"match": "all" | "any", "rules": [{"field", "op",
//!   "value"}]}}` (docs/research/lightcraft-albums.md A2);
//! - `Err` is the skip reason of the whole collection.
//!
//! Whitelist (F-017 point 9, strict; F-021 open): only criterion `rating`
//! with operation `==` (op `is`) or `>=` (op `gte`), value a number that is
//! a whole number 0..=5 (a text, a fraction, a negative number, 6 or more,
//! or no value is not translated). `value2` is ignored (Lightroom writes 0
//! there). `combine` `intersect` -> `all`, `union` -> `any`; the rules keep
//! their order. Everything else skips the whole collection.
//!
//! Reasons: name the first untranslatable criterion as `criterion "<name>"`
//! plus "no exact LightCraft equivalent" (for an unsupported operation or
//! value of `rating`, the criterion is `rating`); an unknown combine says
//! "combine"; an empty rule list says "no criteria". A reason never contains
//! a value, an operation text or a combine text.
//!
//! Decision on an empty rule list: skipped. Lightroom's meaning of a smart
//! collection without criteria is unproven (all photos or none), and
//! LightCraft's empty rule set is "all" for `all` but "none" for `any`.
//! "Unsure" means not translated (F-017 point 9).

use lc_writer::smart::translate;
use model::{SmartCombine, SmartRule, SmartRules, SmartValue};
use serde_json::json;

const SECRET: &str = "ZyxSecret";

fn rule(criterion: &str, operation: &str, value: Option<SmartValue>) -> SmartRule {
    SmartRule {
        criterion: criterion.to_string(),
        operation: operation.to_string(),
        value,
        value2: None,
        units: None,
    }
}

fn rating(op: &str, stars: f64) -> SmartRule {
    rule("rating", op, Some(SmartValue::Number(stars)))
}

fn rules(combine: SmartCombine, rules: Vec<SmartRule>) -> SmartRules {
    SmartRules { combine, rules }
}

fn all(r: Vec<SmartRule>) -> SmartRules {
    rules(SmartCombine::Intersect, r)
}

fn reason(r: &SmartRules) -> String {
    translate(r).expect_err("must be skipped")
}

#[test]
fn rating_at_least_is_translated() {
    assert_eq!(
        translate(&all(vec![rating(">=", 3.0)])).unwrap(),
        json!({"ruleSet": {"match": "all", "rules": [
            {"field": "rating", "op": "gte", "value": 3}
        ]}})
    );
}

#[test]
fn rating_equals_is_translated_and_union_means_any() {
    assert_eq!(
        translate(&rules(SmartCombine::Union, vec![rating("==", 5.0)])).unwrap(),
        json!({"ruleSet": {"match": "any", "rules": [
            {"field": "rating", "op": "is", "value": 5}
        ]}})
    );
}

#[test]
fn every_star_count_from_zero_to_five_works_and_value_is_an_integer() {
    for stars in 0..=5 {
        for (op, lc) in [("==", "is"), (">=", "gte")] {
            let v = translate(&all(vec![rating(op, stars as f64)])).unwrap();
            let sent = &v["ruleSet"]["rules"][0];
            assert_eq!(sent["op"], lc);
            assert_eq!(sent["value"], json!(stars), "{v}");
            assert!(sent["value"].is_u64(), "{v}");
        }
    }
}

#[test]
fn value2_is_ignored() {
    let mut r = rating(">=", 3.0);
    r.value2 = Some(SmartValue::Number(0.0));
    assert!(translate(&all(vec![r])).is_ok());
}

#[test]
fn several_rating_rules_keep_their_order() {
    let v = translate(&all(vec![
        rating(">=", 2.0),
        rating("==", 5.0),
        rating(">=", 1.0),
    ]))
    .unwrap();
    let ops: Vec<(&str, i64)> = v["ruleSet"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["op"].as_str().unwrap(), r["value"].as_i64().unwrap()))
        .collect();
    assert_eq!(ops, [("gte", 2), ("is", 5), ("gte", 1)]);
}

#[test]
fn translation_is_pure() {
    let r = all(vec![rating(">=", 3.0)]);
    assert_eq!(translate(&r).unwrap(), translate(&r).unwrap());
}

#[test]
fn other_criteria_are_skipped_with_their_name() {
    for (name, op, value) in [
        ("pick", "==", Some(SmartValue::Number(1.0))),
        (
            "labelColor",
            "==",
            Some(SmartValue::Text(format!("red {SECRET}"))),
        ),
        (
            "keywords",
            "any",
            Some(SmartValue::Text(format!("x {SECRET}"))),
        ),
        ("keywords", "empty", None),
        ("captureTime", "inLast", Some(SmartValue::Number(30.0))),
        ("touchTime", "inLast", Some(SmartValue::Number(2.0))),
        (
            "fileFormat",
            "==",
            Some(SmartValue::Text(format!("DNG {SECRET}"))),
        ),
        (
            "zyxCriterion",
            "zyxOp",
            Some(SmartValue::Text(SECRET.to_string())),
        ),
    ] {
        let why = reason(&all(vec![rule(name, op, value)]));
        assert!(
            why.contains(&format!("criterion \"{name}\"")),
            "{name}: {why}"
        );
        assert!(why.contains("no exact LightCraft equivalent"), "{why}");
        assert!(!why.contains(SECRET) && !why.contains(op), "{name}: {why}");
    }
}

#[test]
fn unsupported_rating_operations_and_values_are_skipped() {
    let texts = [
        rating(">", 3.0),
        rating("<", 3.0),
        rating("<=", 3.0),
        rating("!=", 3.0),
        rating("zyxOp", 3.0),
        rating(">=", 6.0),
        rating("==", -1.0),
        rating(">=", 2.5),
        rule("rating", ">=", Some(SmartValue::Text("3".into()))),
        rule("rating", ">=", None),
    ];
    for r in texts {
        let why = reason(&all(vec![r.clone()]));
        assert!(why.contains("rating"), "{r:?}: {why}");
        assert!(!why.contains("zyxOp") && !why.contains("2.5"), "{why}");
    }
}

#[test]
fn mixed_rules_name_the_first_untranslatable_criterion() {
    let r = all(vec![
        rating(">=", 3.0),
        rule("pick", "==", Some(SmartValue::Number(1.0))),
        rule("zyxCriterion", "zyxOp", None),
    ]);
    let why = reason(&r);
    assert!(
        why.contains("pick") && !why.contains("zyxCriterion"),
        "{why}"
    );
    let r = all(vec![
        rule("keywords", "empty", None),
        rule("pick", "==", Some(SmartValue::Number(1.0))),
    ]);
    assert!(reason(&r).contains("keywords"));
    // Not partial: the translatable rule alone never comes out.
    assert!(translate(&all(vec![rating(">=", 3.0), rule("pick", "==", None)])).is_err());
    assert!(
        translate(&rules(
            SmartCombine::Union,
            vec![rating(">=", 3.0), rule("pick", "==", None)]
        ))
        .is_err()
    );
}

#[test]
fn unknown_combine_is_skipped_without_the_text() {
    for text in ["zyxCombine", "exclude", "Union", ""] {
        let r = rules(
            SmartCombine::Other(text.to_string()),
            vec![rating(">=", 3.0)],
        );
        let why = reason(&r);
        assert!(why.contains("combine"), "{why}");
        if !text.is_empty() {
            assert!(!why.contains(text), "{why}");
        }
    }
}

#[test]
fn empty_rule_list_is_skipped() {
    for combine in [
        SmartCombine::Intersect,
        SmartCombine::Union,
        SmartCombine::Other("zyxCombine".into()),
    ] {
        let why = reason(&rules(combine, vec![]));
        assert!(why.contains("no criteria"), "{why}");
        assert!(!why.contains("zyxCombine"));
    }
}
