//! Translation of smart collection rules into the rules of a LightCraft
//! smart album (`album.createSmart`).
//!
//! The translation is strict: only rules with an exact LightCraft
//! equivalent are translated, and a collection with a single rule that
//! has none is not translated at all (never half). Today that is the star
//! rating with "is" (`==`) or "at least" (`>=`) and a whole number of
//! stars from 0 to 5.
//!
//! Reasons name a criterion, never a value, an operation or a combine
//! text: those may hold user data.

use model::{Collection, CollectionKind, SmartCollection, SmartCombine, SmartRule, SmartValue};
use serde_json::{Value, json};

/// The highest star rating.
const MAX_STARS: f64 = 5.0;

/// Translates the rules of a smart collection into the `rules` parameter of
/// `album.createSmart`: `{"ruleSet": {"match": "all" | "any", "rules":
/// [{"field", "op", "value"}]}}`, rules in stored order.
///
/// `Err` is the reason why the whole collection is not translated. `value2`
/// and units are ignored.
pub fn translate(rules: &model::SmartRules) -> Result<Value, String> {
    if rules.rules.is_empty() {
        // Lightroom's meaning of an empty rule list is unproven, and
        // LightCraft reads it as "all" or "none" depending on the match.
        return Err("no criteria".to_string());
    }
    // Criteria first: a criterion name says more than the combine mode.
    let translated = rules
        .rules
        .iter()
        .map(|r| {
            translate_rule(r).ok_or_else(|| {
                format!(
                    "criterion \"{}\" has no exact LightCraft equivalent",
                    r.criterion
                )
            })
        })
        .collect::<Result<Vec<Value>, String>>()?;
    let mode = match &rules.combine {
        SmartCombine::Intersect => "all",
        SmartCombine::Union => "any",
        SmartCombine::Other(_) => {
            return Err("the combine mode has no exact LightCraft equivalent".to_string());
        }
    };
    Ok(json!({"ruleSet": {"match": mode, "rules": translated}}))
}

/// One rule, or `None` if it has no exact equivalent.
fn translate_rule(rule: &SmartRule) -> Option<Value> {
    if rule.criterion != "rating" {
        return None;
    }
    let op = match rule.operation.as_str() {
        "==" => "is",
        ">=" => "gte",
        _ => return None,
    };
    let stars = match rule.value {
        Some(SmartValue::Number(n)) if n.fract() == 0.0 && (0.0..=MAX_STARS).contains(&n) => {
            n as u64
        }
        _ => return None,
    };
    Some(json!({"field": "rating", "op": op, "value": stars}))
}

/// The translated rules of a smart collection that can be written, or the
/// reason why it cannot: its rules have no exact equivalent, or its parent
/// is not one of the album folders in `collections` (LightCraft nests smart
/// albums only in folders).
pub fn convertible(smart: &SmartCollection, collections: &[Collection]) -> Result<Value, String> {
    let rules = translate(&smart.rules)?;
    if let Some(parent) = smart.parent
        && !collections
            .iter()
            .any(|c| c.lightroom_id == parent && c.kind == CollectionKind::Folder)
    {
        return Err(format!(
            "its parent collection {parent} is not a converted collection set"
        ));
    }
    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::SmartRules;

    fn rule(criterion: &str, operation: &str, value: Option<SmartValue>) -> SmartRule {
        SmartRule {
            criterion: criterion.into(),
            operation: operation.into(),
            value,
            value2: None,
            units: None,
        }
    }

    fn stars(op: &str, n: f64) -> SmartRule {
        rule("rating", op, Some(SmartValue::Number(n)))
    }

    fn all(rules: Vec<SmartRule>) -> SmartRules {
        SmartRules {
            combine: SmartCombine::Intersect,
            rules,
        }
    }

    #[test]
    fn rating_rules_translate_in_order() {
        let r = SmartRules {
            combine: SmartCombine::Union,
            rules: vec![stars(">=", 2.0), stars("==", 0.0)],
        };
        assert_eq!(
            translate(&r).unwrap(),
            json!({"ruleSet": {"match": "any", "rules": [
                {"field": "rating", "op": "gte", "value": 2},
                {"field": "rating", "op": "is", "value": 0},
            ]}})
        );
    }

    #[test]
    fn stars_must_be_whole_and_in_range() {
        for n in [-1.0, 5.5, 6.0, 0.5, f64::NAN, f64::INFINITY] {
            assert!(translate_rule(&stars(">=", n)).is_none(), "{n}");
        }
        assert!(translate_rule(&stars(">=", 5.0)).is_some());
        assert!(translate_rule(&rule("rating", ">=", None)).is_none());
        assert!(
            translate_rule(&rule("rating", ">=", Some(SmartValue::Text("3".into())))).is_none()
        );
    }

    #[test]
    fn other_operations_and_criteria_have_no_equivalent() {
        for op in [">", "<", "<=", "!=", "x"] {
            assert!(translate_rule(&stars(op, 3.0)).is_none(), "{op}");
        }
        assert!(translate_rule(&rule("pick", "==", Some(SmartValue::Number(1.0)))).is_none());
    }

    #[test]
    fn reasons_name_the_first_bad_criterion_only() {
        let r = all(vec![
            stars(">=", 1.0),
            rule("pick", "==", None),
            rule("labelColor", "==", None),
        ]);
        assert_eq!(
            translate(&r).unwrap_err(),
            "criterion \"pick\" has no exact LightCraft equivalent"
        );
        assert_eq!(translate(&all(vec![])).unwrap_err(), "no criteria");
        let other = SmartRules {
            combine: SmartCombine::Other("x".into()),
            rules: vec![stars(">=", 1.0)],
        };
        assert!(translate(&other).unwrap_err().contains("combine"));
        let both = SmartRules {
            combine: SmartCombine::Other("x".into()),
            rules: vec![rule("zyx", "==", None)],
        };
        assert!(translate(&both).unwrap_err().contains("\"zyx\""));
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

    #[test]
    fn convertible_needs_a_converted_folder_as_parent() {
        let cols = [
            collection(1, CollectionKind::Folder),
            collection(2, CollectionKind::Album),
        ];
        let smart = |parent| SmartCollection {
            lightroom_id: 9,
            name: "n".into(),
            parent,
            rules: all(vec![stars(">=", 3.0)]),
        };
        assert!(convertible(&smart(None), &cols).is_ok());
        assert!(convertible(&smart(Some(1)), &cols).is_ok());
        for parent in [2, 3] {
            let why = convertible(&smart(Some(parent)), &cols).unwrap_err();
            assert!(why.contains("parent") && why.contains(&parent.to_string()));
        }
        let bad = SmartCollection {
            rules: all(vec![]),
            ..smart(Some(2))
        };
        assert_eq!(convertible(&bad, &cols).unwrap_err(), "no criteria");
    }
}
