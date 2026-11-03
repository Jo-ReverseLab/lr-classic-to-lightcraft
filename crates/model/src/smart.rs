//! Rules of a smart collection, as read from the catalog.
//!
//! Names (criteria, operations, units, combine) are kept verbatim; the
//! LightCraft rule JSON is not built here (that lives in the writer). What
//! does live here is the **verdict**: whether a collection's rules have an
//! exact target equivalent, with the skip code and reason set where the
//! reason arises (see [`SmartRules::verdict`]). The reader uses it so the
//! report is final at read time and the caller never rewrites it by reason
//! text. Values may hold user data and must never be written to a report.

use crate::SkipCode;

/// The highest star rating.
const MAX_STARS: f64 = 5.0;

/// How the rules of a smart collection are combined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmartCombine {
    /// All rules must match (`intersect`).
    Intersect,
    /// Any rule must match (`union`).
    Union,
    /// Any other combine text, verbatim.
    Other(String),
}

impl SmartRule {
    /// Whether this rule has an exact equivalent in the target. Today that is
    /// a star rating with `is` (`==`) or `at least` (`>=`) and a whole number
    /// of stars from 0 to 5, with no second value and no units. The target
    /// shape (the LightCraft JSON) is built by the writer; this predicate is
    /// the shared gate so the reader's verdict and the writer's translation
    /// agree.
    pub fn translatable(&self) -> bool {
        self.criterion == "rating"
            && self.value2.is_none()
            && self.units.is_none()
            && matches!(self.operation.as_str(), "==" | ">=")
            && matches!(
                self.value,
                Some(SmartValue::Number(n)) if n.fract() == 0.0 && (0.0..=MAX_STARS).contains(&n)
            )
    }
}

impl SmartRules {
    /// The verdict for this collection's rules: `Ok(())` if every rule has an
    /// exact target equivalent and the combine mode is known, or the skip code
    /// and reason why the whole collection is not converted (a collection with
    /// a single rule that has no equivalent is not converted at all, never
    /// half). The reason names the first criterion without an equivalent, or
    /// the combine mode, or "no criteria"; it never contains a value, an
    /// operation or combine text.
    pub fn verdict(&self) -> Result<(), (SkipCode, String)> {
        if self.rules.is_empty() {
            return Err((SkipCode::SmartNoCriteria, "no criteria".to_string()));
        }
        for r in &self.rules {
            if !r.translatable() {
                return Err((
                    SkipCode::SmartUntranslatable,
                    format!(
                        "criterion \"{}\" has no exact LightCraft equivalent",
                        r.criterion
                    ),
                ));
            }
        }
        if matches!(self.combine, SmartCombine::Other(_)) {
            return Err((
                SkipCode::SmartUntranslatable,
                "the combine mode has no exact LightCraft equivalent".to_string(),
            ));
        }
        Ok(())
    }
}

impl SmartCombine {
    /// Maps the stored combine text: `intersect` and `union` are known,
    /// everything else is kept as [`SmartCombine::Other`].
    pub fn from_text(text: &str) -> SmartCombine {
        match text {
            "intersect" => SmartCombine::Intersect,
            "union" => SmartCombine::Union,
            other => SmartCombine::Other(other.to_owned()),
        }
    }
}

/// A rule value: a number, a text or a boolean. A number written as text
/// stays text.
#[derive(Debug, Clone, PartialEq)]
pub enum SmartValue {
    /// A number as stored: whole, negative or fractional.
    Number(f64),
    /// A text as stored, with escapes decoded. May hold user data.
    Text(String),
    /// `true` or `false` as stored.
    Bool(bool),
}

/// One criterion of a smart collection.
#[derive(Debug, Clone, PartialEq)]
pub struct SmartRule {
    /// The criterion name (key `criteria`), verbatim.
    pub criterion: String,
    /// The operation (key `operation`), verbatim.
    pub operation: String,
    /// Key `value`; `None` if absent.
    pub value: Option<SmartValue>,
    /// Key `value2`; `None` if absent.
    pub value2: Option<SmartValue>,
    /// Key `value_units`; `None` if absent.
    pub units: Option<String>,
}

/// All rules of one smart collection, in stored order.
#[derive(Debug, Clone, PartialEq)]
pub struct SmartRules {
    /// How the rules are combined (key `combine`).
    pub combine: SmartCombine,
    /// The criteria, in stored order; empty if the collection has none.
    pub rules: Vec<SmartRule>,
}

/// A smart collection with readable rules.
#[derive(Debug, Clone, PartialEq)]
pub struct SmartCollection {
    /// The collection's id in the catalog.
    pub lightroom_id: i64,
    /// The collection's name. User data: never written to a report.
    pub name: String,
    /// The id of the parent collection set; `None` on top level.
    pub parent: Option<i64>,
    /// The rules, verbatim.
    pub rules: SmartRules,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rating(op: &str, value: Option<SmartValue>) -> SmartRule {
        SmartRule {
            criterion: "rating".into(),
            operation: op.into(),
            value,
            value2: None,
            units: None,
        }
    }

    #[test]
    fn combine_texts() {
        assert_eq!(
            SmartCombine::from_text("intersect"),
            SmartCombine::Intersect
        );
        assert_eq!(SmartCombine::from_text("union"), SmartCombine::Union);
        assert_eq!(
            SmartCombine::from_text("Union"),
            SmartCombine::Other("Union".into())
        );
        assert_eq!(
            SmartCombine::from_text(""),
            SmartCombine::Other(String::new())
        );
    }

    #[test]
    fn translatable_rating_rules() {
        assert!(rating(">=", Some(SmartValue::Number(3.0))).translatable());
        assert!(rating("==", Some(SmartValue::Number(0.0))).translatable());
        assert!(rating("==", Some(SmartValue::Number(5.0))).translatable());
    }

    #[test]
    fn non_translatable_rules() {
        // wrong criterion
        let mut r = rating(">=", Some(SmartValue::Number(3.0)));
        r.criterion = "pick".into();
        assert!(!r.translatable());
        // second value or units spoil it
        let mut r = rating(">=", Some(SmartValue::Number(3.0)));
        r.value2 = Some(SmartValue::Number(1.0));
        assert!(!r.translatable());
        let mut r = rating(">=", Some(SmartValue::Number(3.0)));
        r.units = Some("days".into());
        assert!(!r.translatable());
        // wrong operation
        assert!(!rating(">", Some(SmartValue::Number(3.0))).translatable());
        // fractional or out-of-range stars
        assert!(!rating(">=", Some(SmartValue::Number(2.5))).translatable());
        assert!(!rating(">=", Some(SmartValue::Number(6.0))).translatable());
        // a number written as text stays text
        assert!(!rating(">=", Some(SmartValue::Text("3".into()))).translatable());
    }

    #[test]
    fn verdict_names_the_first_untranslatable_criterion() {
        let r = SmartRules {
            combine: SmartCombine::Union,
            rules: vec![rating("==", Some(SmartValue::Number(5.0))), {
                let mut r = rating("==", Some(SmartValue::Number(4.0)));
                r.criterion = "labelColor".into();
                r
            }],
        };
        let (code, why) = r.verdict().unwrap_err();
        assert_eq!(code, SkipCode::SmartUntranslatable);
        assert_eq!(
            why,
            "criterion \"labelColor\" has no exact LightCraft equivalent"
        );
    }

    #[test]
    fn verdict_for_no_criteria_and_unknown_combine() {
        let empty = SmartRules {
            combine: SmartCombine::Intersect,
            rules: vec![],
        };
        assert_eq!(empty.verdict().unwrap_err().0, SkipCode::SmartNoCriteria);
        let other = SmartRules {
            combine: SmartCombine::Other("zyx".into()),
            rules: vec![rating(">=", Some(SmartValue::Number(3.0)))],
        };
        let (code, why) = other.verdict().unwrap_err();
        assert_eq!(code, SkipCode::SmartUntranslatable);
        assert_eq!(why, "the combine mode has no exact LightCraft equivalent");
    }

    #[test]
    fn verdict_passes_translatable_rules() {
        let r = SmartRules {
            combine: SmartCombine::Union,
            rules: vec![
                rating(">=", Some(SmartValue::Number(2.0))),
                rating("==", Some(SmartValue::Number(0.0))),
            ],
        };
        assert!(r.verdict().is_ok());
    }
}
