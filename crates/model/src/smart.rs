//! Rules of a smart collection, as read from the catalog.
//!
//! Names (criteria, operations, units, combine) are kept verbatim; nothing
//! is translated here. Values may hold user data and must never be
//! written to a report.

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

/// A rule value: a number or a text. A number written as text stays text.
#[derive(Debug, Clone, PartialEq)]
pub enum SmartValue {
    /// A number as stored: whole, negative or fractional.
    Number(f64),
    /// A text as stored, with escapes decoded. May hold user data.
    Text(String),
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
