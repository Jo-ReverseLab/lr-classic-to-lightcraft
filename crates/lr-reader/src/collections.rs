//! Reading collections (`AgLibraryCollection`).
//!
//! Collections are not converted yet: every row is counted as read and
//! skipped with a reason that names its kind. Collection names are never
//! read, so they cannot leak into the report.

use crate::catalog::{Catalog, ReadError};
use model::Report;
use rusqlite::types::Value;

const PLAIN: &str = "com.adobe.ag.library.collection";
const SET: &str = "com.adobe.ag.library.group";
const SMART: &str = "com.adobe.ag.library.smart_collection";

impl Catalog {
    /// Reads all collections, ordered by their catalog id.
    ///
    /// Every row is counted with [`Report::read`] and recorded with
    /// [`Report::skipped`]; nothing is marked converted. The reason names the
    /// collection id and its kind, never its name.
    pub fn collections(&self, report: &mut Report) -> Result<(), ReadError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id_local, creationId, systemOnly FROM AgLibraryCollection \
                 ORDER BY id_local",
            )
            .map_err(ReadError::Query)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Value>(1)?,
                    row.get::<_, Value>(2)?,
                ))
            })
            .map_err(ReadError::Query)?;
        for row in rows {
            let (id, creation, system) = row.map_err(ReadError::Query)?;
            report.read();
            let creation = match &creation {
                Value::Text(t) => Some(t.as_str()),
                _ => None,
            };
            report.skipped(skip_reason(id, creation, is_nonzero(&system)));
        }
        Ok(())
    }
}

/// The skip reason for one collection. System collections win over the
/// `creationId`.
fn skip_reason(id: i64, creation_id: Option<&str>, system_only: bool) -> String {
    let kind = if system_only {
        "system collection"
    } else {
        match creation_id {
            Some(PLAIN) => "collection",
            Some(SET) => "collection set",
            Some(SMART) => "smart collection",
            _ => "unknown collection type",
        }
    };
    format!("collection {id}: {kind} is not converted")
}

/// True if a `systemOnly` value is anything other than zero or NULL.
/// Numeric text such as `"0"` or `"0.0"` counts as zero.
fn is_nonzero(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Integer(n) => *n != 0,
        Value::Real(f) => *f != 0.0,
        Value::Text(t) => t.trim().parse::<f64>() != Ok(0.0),
        Value::Blob(b) => !b.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_per_kind() {
        assert_eq!(
            skip_reason(2, Some(PLAIN), false),
            "collection 2: collection is not converted"
        );
        assert_eq!(
            skip_reason(1, Some(SET), false),
            "collection 1: collection set is not converted"
        );
        assert_eq!(
            skip_reason(5, Some(SMART), false),
            "collection 5: smart collection is not converted"
        );
        assert_eq!(
            skip_reason(9, Some("com.example.x"), false),
            "collection 9: unknown collection type is not converted"
        );
        assert_eq!(
            skip_reason(10, None, false),
            "collection 10: unknown collection type is not converted"
        );
    }

    #[test]
    fn system_wins_over_creation_id() {
        for creation in [Some(PLAIN), Some(SMART), None] {
            assert_eq!(
                skip_reason(8, creation, true),
                "collection 8: system collection is not converted"
            );
        }
    }

    #[test]
    fn system_only_values() {
        assert!(!is_nonzero(&Value::Null));
        assert!(!is_nonzero(&Value::Integer(0)));
        assert!(is_nonzero(&Value::Integer(1)));
        assert!(!is_nonzero(&Value::Real(0.0)));
        assert!(is_nonzero(&Value::Real(1.0)));
        assert!(!is_nonzero(&Value::Text("0".into())));
        assert!(!is_nonzero(&Value::Text("0.0".into())));
        assert!(is_nonzero(&Value::Text("1".into())));
        assert!(is_nonzero(&Value::Text("yes".into())));
    }
}
