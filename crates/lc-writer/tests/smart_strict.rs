//! Acceptance tests for two strictness rules of the smart translation
//! module `lc_writer::smart`.
//!
//! Rules:
//! - Rating bounds: a `rating` rule with `value2` set (any value) or `units` set
//!   is not translated (strict reading), so the whole collection is not
//!   translated. `translate_coded` answers `Err((SkipCode::SmartUntranslatable,
//!   reason))` and the reason names the criterion `rating`, in the form of
//!   the other untranslatable rules: `criterion "rating" has no exact
//!   LightCraft equivalent`. It never contains a value or unit text. A
//!   rating rule with `value2: None` and `units: None` still translates;
//! - Parent check: `convertible` answers a smart collection whose parent is not
//!   a collection set in `collections` (a plain album, a missing id or a
//!   set that was not converted) with `SkipCode::ParentNotSet`; the free
//!   text stays `its parent collection <id> is not a converted collection
//!   set`. Untranslatable rules still win with their own code, and a
//!   parent that is a set keeps working.

use lc_writer::smart::{convertible, translate_coded};
use model::{
    Collection, CollectionKind, SkipCode, SmartCollection, SmartCombine, SmartRule, SmartRules,
    SmartValue,
};

fn rating(value2: Option<SmartValue>, units: Option<&str>) -> SmartRules {
    SmartRules {
        combine: SmartCombine::Intersect,
        rules: vec![SmartRule {
            criterion: "rating".into(),
            operation: ">=".into(),
            value: Some(SmartValue::Number(3.0)),
            value2,
            units: units.map(String::from),
        }],
    }
}

#[test]
fn plain_rating_rule_still_translates() {
    assert!(translate_coded(&rating(None, None)).is_ok());
}

#[test]
fn rating_with_value2_is_not_translated() {
    for v in [
        SmartValue::Number(0.0),
        SmartValue::Number(5.0),
        SmartValue::Text("ZyxSecret".into()),
    ] {
        let (code, why) = translate_coded(&rating(Some(v), None)).unwrap_err();
        assert_eq!(code, SkipCode::SmartUntranslatable);
        assert_eq!(
            why,
            "criterion \"rating\" has no exact LightCraft equivalent"
        );
    }
}

#[test]
fn rating_with_units_is_not_translated() {
    let (code, why) = translate_coded(&rating(None, Some("ZyxUnits"))).unwrap_err();
    assert_eq!(code, SkipCode::SmartUntranslatable);
    assert!(why.contains("\"rating\""), "{why}");
    assert!(!why.contains("ZyxUnits"), "{why}");
}

#[test]
fn one_strict_rule_spoils_the_whole_collection() {
    let mut r = rating(None, None);
    r.rules
        .push(rating(Some(SmartValue::Number(1.0)), None).rules.remove(0));
    assert!(translate_coded(&r).is_err());
}

fn collection(id: i64, kind: CollectionKind) -> Collection {
    Collection {
        lightroom_id: id,
        name: String::new(),
        kind,
        parent: None,
        members: Vec::new(),
    }
}

fn smart(parent: Option<i64>, rules: SmartRules) -> SmartCollection {
    SmartCollection {
        lightroom_id: 9,
        name: "S".into(),
        parent,
        rules,
    }
}

#[test]
fn parent_that_is_no_set_gets_parent_not_set() {
    let cols = [
        collection(1, CollectionKind::Folder),
        collection(2, CollectionKind::Album),
    ];
    // A plain album and an unknown id.
    for parent in [2, 77] {
        let (code, why) = convertible(&smart(Some(parent), rating(None, None)), &cols).unwrap_err();
        assert_eq!(code, SkipCode::ParentNotSet, "{parent}");
        assert!(
            why.contains(&format!(
                "its parent collection {parent} is not a converted collection set"
            )),
            "{why}"
        );
    }
    assert!(convertible(&smart(Some(1), rating(None, None)), &cols).is_ok());
    assert!(convertible(&smart(None, rating(None, None)), &cols).is_ok());
}

#[test]
fn untranslatable_rules_keep_their_own_code_before_the_parent_check() {
    let cols = [collection(2, CollectionKind::Album)];
    let bad = rating(Some(SmartValue::Number(1.0)), None);
    let (code, _) = convertible(&smart(Some(2), bad), &cols).unwrap_err();
    assert_eq!(code, SkipCode::SmartUntranslatable);
}
