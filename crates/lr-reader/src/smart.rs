//! Parsing the rules of a smart collection.
//!
//! The catalog stores them as Lua-table-like text, for example
//!
//! ```text
//! s = {
//!     { criteria = "rating", operation = ">=", value = 3, },
//!     combine = "intersect",
//! }
//! ```
//!
//! The parser reads a small subset of Lua table constructors: tables,
//! double- or single-quoted strings, numbers, `true`, `false` and `nil`.
//! Nesting is limited to [`MAX_DEPTH`] levels, so absurd input gives an
//! error, never a stack overflow. Errors carry a kind and a byte offset,
//! never a part of the input.

use model::{SmartCombine, SmartRule, SmartRules, SmartValue};
use std::fmt;

/// Deepest table nesting accepted. Rules need two levels; unknown extra
/// keys may hold a few more.
pub const MAX_DEPTH: usize = 32;

/// Why a rule text could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmartParseError {
    /// The text ends before the value is complete.
    UnexpectedEnd,
    /// An unexpected character at the given byte offset.
    UnexpectedChar(usize),
    /// Tables are nested deeper than [`MAX_DEPTH`].
    TooDeep,
    /// A string holds an escape that is not supported (byte offset).
    BadEscape(usize),
    /// A number cannot be read (byte offset).
    BadNumber(usize),
    /// More text follows the outer table (byte offset).
    TrailingText(usize),
    /// The assigned value is not a table.
    NotATable,
    /// The same key appears twice in one table.
    DuplicateKey,
    /// A rule entry is not a table.
    RuleNotATable,
    /// A rule lacks `criteria` or `operation`.
    MissingKey(&'static str),
    /// A known key holds a value of the wrong type.
    WrongType(&'static str),
    /// The outer table has no `combine`.
    MissingCombine,
}

impl fmt::Display for SmartParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEnd => write!(f, "rule text ends early"),
            Self::UnexpectedChar(at) => write!(f, "unexpected character at byte {at}"),
            Self::TooDeep => write!(f, "tables nested deeper than {MAX_DEPTH} levels"),
            Self::BadEscape(at) => write!(f, "unsupported string escape at byte {at}"),
            Self::BadNumber(at) => write!(f, "number not readable at byte {at}"),
            Self::TrailingText(at) => write!(f, "unexpected text after the rules at byte {at}"),
            Self::NotATable => write!(f, "rules are not a table"),
            Self::DuplicateKey => write!(f, "a key appears twice in one table"),
            Self::RuleNotATable => write!(f, "a rule is not a table"),
            Self::MissingKey(key) => write!(f, "a rule has no {key:?}"),
            Self::WrongType(key) => write!(f, "key {key:?} has a value of the wrong type"),
            Self::MissingCombine => write!(f, "rules have no \"combine\""),
        }
    }
}

impl std::error::Error for SmartParseError {}

/// A parsed Lua value.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Nil,
    Bool,
    Number(f64),
    Text(String),
    Table(Table),
}

/// A table: named entries and unnamed (positional) entries, each in
/// order of appearance.
#[derive(Debug, Clone, Default, PartialEq)]
struct Table {
    named: Vec<(String, Value)>,
    items: Vec<Value>,
}

impl Table {
    fn get(&self, key: &str) -> Option<&Value> {
        self.named.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

/// Parses the rule text of one smart collection.
///
/// The rules are the unnamed entries of the outer table, in order;
/// `combine` may stand anywhere. Unknown keys are ignored.
pub fn parse_smart_rules(text: &str) -> Result<SmartRules, SmartParseError> {
    let outer = match Parser::new(text).document()? {
        Value::Table(t) => t,
        _ => return Err(SmartParseError::NotATable),
    };
    let combine = match outer.get("combine") {
        Some(Value::Text(t)) => SmartCombine::from_text(t),
        Some(_) => return Err(SmartParseError::WrongType("combine")),
        None => return Err(SmartParseError::MissingCombine),
    };
    let rules = outer
        .items
        .iter()
        .map(|item| match item {
            Value::Table(t) => rule(t),
            _ => Err(SmartParseError::RuleNotATable),
        })
        .collect::<Result<_, _>>()?;
    Ok(SmartRules { combine, rules })
}

/// One rule from its table.
fn rule(t: &Table) -> Result<SmartRule, SmartParseError> {
    let required = |key: &'static str| match t.get(key) {
        Some(Value::Text(s)) => Ok(s.clone()),
        None | Some(Value::Nil) => Err(SmartParseError::MissingKey(key)),
        Some(_) => Err(SmartParseError::WrongType(key)),
    };
    let value = |key: &'static str| match t.get(key) {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Number(n)) => Ok(Some(SmartValue::Number(*n))),
        Some(Value::Text(s)) => Ok(Some(SmartValue::Text(s.clone()))),
        Some(_) => Err(SmartParseError::WrongType(key)),
    };
    let units = match t.get("value_units") {
        None | Some(Value::Nil) => None,
        Some(Value::Text(s)) => Some(s.clone()),
        Some(_) => return Err(SmartParseError::WrongType("value_units")),
    };
    Ok(SmartRule {
        criterion: required("criteria")?,
        operation: required("operation")?,
        value: value("value")?,
        value2: value("value2")?,
        units,
    })
}

/// A recursive-descent parser over the bytes of the text. Recursion is
/// bounded by [`MAX_DEPTH`].
struct Parser<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Parser { text, pos: 0 }
    }

    /// `name = value`, then only whitespace.
    fn document(&mut self) -> Result<Value, SmartParseError> {
        self.skip_space();
        self.name()?;
        self.skip_space();
        self.expect(b'=')?;
        let value = self.value(0)?;
        self.skip_space();
        if self.pos < self.text.len() {
            return Err(SmartParseError::TrailingText(self.pos));
        }
        Ok(value)
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), SmartParseError> {
        match self.peek() {
            Some(b) if b == byte => {
                self.pos += 1;
                Ok(())
            }
            Some(_) => Err(SmartParseError::UnexpectedChar(self.pos)),
            None => Err(SmartParseError::UnexpectedEnd),
        }
    }

    /// A Lua identifier.
    fn name(&mut self) -> Result<&'a str, SmartParseError> {
        let start = self.pos;
        match self.peek() {
            Some(b) if b.is_ascii_alphabetic() || b == b'_' => {}
            Some(_) => return Err(SmartParseError::UnexpectedChar(self.pos)),
            None => return Err(SmartParseError::UnexpectedEnd),
        }
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            self.pos += 1;
        }
        Ok(&self.text[start..self.pos])
    }

    /// Any value; `depth` is the number of enclosing tables.
    fn value(&mut self, depth: usize) -> Result<Value, SmartParseError> {
        self.skip_space();
        match self.peek() {
            None => Err(SmartParseError::UnexpectedEnd),
            Some(b'{') => self.table(depth + 1).map(Value::Table),
            Some(q @ (b'"' | b'\'')) => self.string(q).map(Value::Text),
            Some(b'-' | b'.' | b'0'..=b'9') => self.number().map(Value::Number),
            Some(b) if b.is_ascii_alphabetic() || b == b'_' => {
                let at = self.pos;
                match self.name()? {
                    "true" | "false" => Ok(Value::Bool),
                    "nil" => Ok(Value::Nil),
                    _ => Err(SmartParseError::UnexpectedChar(at)),
                }
            }
            Some(_) => Err(SmartParseError::UnexpectedChar(self.pos)),
        }
    }

    /// `{ entry, entry, ... }` with `,` or `;` between entries and an
    /// optional separator at the end. An entry is `name = value`,
    /// `["key"] = value` or a bare value.
    fn table(&mut self, depth: usize) -> Result<Table, SmartParseError> {
        if depth > MAX_DEPTH {
            return Err(SmartParseError::TooDeep);
        }
        self.expect(b'{')?;
        let mut table = Table::default();
        loop {
            self.skip_space();
            match self.peek() {
                None => return Err(SmartParseError::UnexpectedEnd),
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(table);
                }
                _ => {}
            }
            match self.key()? {
                Some(key) => {
                    let value = self.value(depth)?;
                    if table.get(&key).is_some() {
                        return Err(SmartParseError::DuplicateKey);
                    }
                    table.named.push((key, value));
                }
                None => {
                    let value = self.value(depth)?;
                    table.items.push(value);
                }
            }
            self.skip_space();
            match self.peek() {
                Some(b',' | b';') => self.pos += 1,
                Some(b'}') => {}
                Some(_) => return Err(SmartParseError::UnexpectedChar(self.pos)),
                None => return Err(SmartParseError::UnexpectedEnd),
            }
        }
    }

    /// Reads `name =` or `["key"] =` and returns the key; returns `None`
    /// and leaves the position unchanged if the entry is a bare value.
    fn key(&mut self) -> Result<Option<String>, SmartParseError> {
        let start = self.pos;
        let key = match self.peek() {
            Some(b'[') => {
                self.pos += 1;
                self.skip_space();
                let key = match self.peek() {
                    Some(q @ (b'"' | b'\'')) => self.string(q)?,
                    Some(_) => return Err(SmartParseError::UnexpectedChar(self.pos)),
                    None => return Err(SmartParseError::UnexpectedEnd),
                };
                self.skip_space();
                self.expect(b']')?;
                key
            }
            Some(b) if b.is_ascii_alphabetic() || b == b'_' => self.name()?.to_owned(),
            _ => return Ok(None),
        };
        self.skip_space();
        if self.peek() == Some(b'=') {
            self.pos += 1;
            Ok(Some(key))
        } else if self.text.as_bytes()[start] == b'[' {
            match self.peek() {
                Some(_) => Err(SmartParseError::UnexpectedChar(self.pos)),
                None => Err(SmartParseError::UnexpectedEnd),
            }
        } else {
            // A bare word such as `true`: read it again as a value.
            self.pos = start;
            Ok(None)
        }
    }

    /// A quoted string with the escapes `\"`, `\'`, `\\`, `\n`, `\t`,
    /// `\r` and backslash-newline (a newline).
    fn string(&mut self, quote: u8) -> Result<String, SmartParseError> {
        self.pos += 1; // opening quote
        let mut out = String::new();
        loop {
            let rest = &self.text[self.pos..];
            let Some(c) = rest.chars().next() else {
                return Err(SmartParseError::UnexpectedEnd);
            };
            let at = self.pos;
            self.pos += c.len_utf8();
            match c {
                c if c as u32 == quote as u32 => return Ok(out),
                '\n' => return Err(SmartParseError::UnexpectedChar(at)),
                '\\' => {
                    let Some(e) = self.text[self.pos..].chars().next() else {
                        return Err(SmartParseError::UnexpectedEnd);
                    };
                    self.pos += e.len_utf8();
                    out.push(match e {
                        '"' | '\'' | '\\' => e,
                        'n' | '\n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        _ => return Err(SmartParseError::BadEscape(at)),
                    });
                }
                c => out.push(c),
            }
        }
    }

    /// A decimal number with optional sign, fraction and exponent.
    fn number(&mut self) -> Result<f64, SmartParseError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while let Some(b) = self.peek() {
            let sign_after_exp = matches!(b, b'+' | b'-')
                && matches!(self.text.as_bytes()[self.pos - 1], b'e' | b'E');
            if b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E') || sign_after_exp {
                self.pos += 1;
            } else {
                break;
            }
        }
        let token = &self.text[start..self.pos];
        match token.parse::<f64>() {
            Ok(n) if n.is_finite() && token.bytes().any(|b| b.is_ascii_digit()) => Ok(n),
            _ if self.pos == self.text.len() => Err(SmartParseError::UnexpectedEnd),
            _ => Err(SmartParseError::BadNumber(start)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule_of(text: &str) -> SmartRule {
        let mut rules = parse_smart_rules(text).unwrap().rules;
        assert_eq!(rules.len(), 1);
        rules.remove(0)
    }

    fn wrap(entries: &str) -> String {
        format!(
            "s = {{ {{ criteria = \"c\", operation = \"o\", {entries} }}, combine = \"union\" }}"
        )
    }

    #[test]
    fn reads_a_minimal_rule_set() {
        let r = parse_smart_rules("s = { { criteria = \"rating\", operation = \"==\", value = 5 }, combine = \"intersect\" }").unwrap();
        assert_eq!(r.combine, SmartCombine::Intersect);
        assert_eq!(
            r.rules,
            vec![SmartRule {
                criterion: "rating".into(),
                operation: "==".into(),
                value: Some(SmartValue::Number(5.0)),
                value2: None,
                units: None,
            }]
        );
    }

    #[test]
    fn combine_may_come_first_and_separators_vary() {
        let r = parse_smart_rules(
            "s={combine='union';{criteria='a';operation='b'};{criteria='c',operation='d',};}",
        )
        .unwrap();
        assert_eq!(r.combine, SmartCombine::Union);
        let names: Vec<_> = r.rules.iter().map(|r| r.criterion.as_str()).collect();
        assert_eq!(names, ["a", "c"]);
    }

    #[test]
    fn numbers() {
        for (text, want) in [
            ("3", 3.0),
            ("-1", -1.0),
            ("2.5", 2.5),
            ("-0.25", -0.25),
            (".5", 0.5),
            ("1e3", 1000.0),
            ("1E-2", 0.01),
        ] {
            let r = rule_of(&wrap(&format!("value = {text}")));
            assert_eq!(r.value, Some(SmartValue::Number(want)), "{text}");
        }
        for bad in ["-", "1.2.3", "1e", "--1", "-x"] {
            assert!(
                parse_smart_rules(&wrap(&format!("value = {bad}"))).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn strings_and_escapes() {
        let r = rule_of(&wrap(r#"value = "a\"b\\c\'d\ne\tf", value2 = 'x"y'"#));
        assert_eq!(r.value, Some(SmartValue::Text("a\"b\\c'd\ne\tf".into())));
        assert_eq!(r.value2, Some(SmartValue::Text("x\"y".into())));
        let r = rule_of(&wrap("value = \"a\\\nb\""));
        assert_eq!(r.value, Some(SmartValue::Text("a\nb".into())));
        let r = rule_of(&wrap("value = \"3\""));
        assert_eq!(r.value, Some(SmartValue::Text("3".into())));
        assert_eq!(
            parse_smart_rules(&wrap("value = \"a\\qb\"")).unwrap_err(),
            SmartParseError::BadEscape(51)
        );
        // An unescaped newline ends nothing: it is an error.
        assert!(parse_smart_rules(&wrap("value = \"a\nb\"")).is_err());
    }

    #[test]
    fn units_and_absent_values() {
        let r = rule_of(&wrap("value = 30, value_units = \"days\""));
        assert_eq!(r.units.as_deref(), Some("days"));
        let r = rule_of(&wrap("value = nil"));
        assert_eq!(r.value, None);
        assert_eq!(r.value2, None);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let r = rule_of(&wrap(
            "flag = true, other = false, nested = { a = { 1, 2 } }, [\"x y\"] = 1, value = 1",
        ));
        assert_eq!(r.value, Some(SmartValue::Number(1.0)));
        let r = parse_smart_rules("s = { extra = { 1 }, combine = \"union\" }").unwrap();
        assert!(r.rules.is_empty());
    }

    #[test]
    fn empty_rule_list_is_ok() {
        let r = parse_smart_rules("s = { combine = \"zyx\" }").unwrap();
        assert_eq!(r.combine, SmartCombine::Other("zyx".into()));
        assert!(r.rules.is_empty());
    }

    #[test]
    fn structural_errors() {
        use SmartParseError::*;
        let cases: [(&str, SmartParseError); 12] = [
            ("", UnexpectedEnd),
            ("s", UnexpectedEnd),
            ("s =", UnexpectedEnd),
            ("s = {", UnexpectedEnd),
            ("s = 1", NotATable),
            ("s = { combine = \"union\" } x", TrailingText(26)),
            ("= {}", UnexpectedChar(0)),
            ("s = { combine = \"union\" combine }", UnexpectedChar(24)),
            ("s = {}", MissingCombine),
            ("s = { combine = 1 }", WrongType("combine")),
            ("s = { combine = \"a\", combine = \"b\" }", DuplicateKey),
            ("s = { 1, combine = \"a\" }", RuleNotATable),
        ];
        for (text, want) in cases {
            assert_eq!(parse_smart_rules(text).unwrap_err(), want, "{text:?}");
        }
    }

    #[test]
    fn rule_errors() {
        use SmartParseError::*;
        let r = |e: &str| format!("s = {{ {{ {e} }}, combine = \"union\" }}");
        assert_eq!(
            parse_smart_rules(&r("operation = \"o\"")).unwrap_err(),
            MissingKey("criteria")
        );
        assert_eq!(
            parse_smart_rules(&r("criteria = \"c\"")).unwrap_err(),
            MissingKey("operation")
        );
        assert_eq!(
            parse_smart_rules(&r("criteria = 1, operation = \"o\"")).unwrap_err(),
            WrongType("criteria")
        );
        assert_eq!(
            parse_smart_rules(&wrap("value = true")).unwrap_err(),
            WrongType("value")
        );
        assert_eq!(
            parse_smart_rules(&wrap("value2 = {}")).unwrap_err(),
            WrongType("value2")
        );
        assert_eq!(
            parse_smart_rules(&wrap("value_units = 1")).unwrap_err(),
            WrongType("value_units")
        );
    }

    #[test]
    fn nesting_is_limited() {
        let ok = format!(
            "s = {{ x = {}{}, combine = \"u\" }}",
            "{".repeat(MAX_DEPTH - 1),
            "}".repeat(MAX_DEPTH - 1)
        );
        assert!(parse_smart_rules(&ok).is_ok());
        let deep = format!(
            "s = {{ x = {}{}, combine = \"u\" }}",
            "{".repeat(MAX_DEPTH),
            "}".repeat(MAX_DEPTH)
        );
        assert_eq!(
            parse_smart_rules(&deep).unwrap_err(),
            SmartParseError::TooDeep
        );
        let absurd = format!("s = {}", "{".repeat(1_000_000));
        assert_eq!(
            parse_smart_rules(&absurd).unwrap_err(),
            SmartParseError::TooDeep
        );
    }

    #[test]
    fn non_ascii_outside_strings_is_rejected() {
        assert!(parse_smart_rules("s = { \u{fc} }").is_err());
        assert!(parse_smart_rules("s = \u{0}{}").is_err());
    }

    #[test]
    fn errors_name_no_input() {
        let text = "s = { { criteria = \"Secret\", operation = \"o\", value = \"Secret\\q\" } }";
        let err = parse_smart_rules(text).unwrap_err();
        let shown = format!("{err} {err:?}");
        assert!(!shown.contains("Secret"), "{shown}");
    }
}
