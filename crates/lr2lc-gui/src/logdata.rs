//! The run log as the Log screen uses it: entries read from `report.json`,
//! counts per status and area, and filtering.

use model::Status;
use serde_json::Value;

/// One log entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub status: Status,
    pub area: String,
    pub stage: String,
    pub item: String,
    pub message: String,
    pub code: Option<String>,
    pub reason: Option<String>,
    pub recommendation: Option<String>,
    pub source: Option<String>,
    pub result: Option<String>,
    pub photo: Option<i64>,
}

impl Row {
    pub fn from_json(v: &Value) -> Row {
        let text = |key: &str| v[key].as_str().map(str::to_owned);
        Row {
            status: crate::widgets::status_of(v["status"].as_str().unwrap_or("")),
            area: text("area").unwrap_or_default(),
            stage: text("stage").unwrap_or_default(),
            item: text("item").unwrap_or_default(),
            message: text("message").unwrap_or_default(),
            code: text("code"),
            reason: text("reason"),
            recommendation: text("recommendation"),
            source: text("source"),
            result: text("result"),
            photo: v["photo"].as_i64(),
        }
    }

    /// The entry as readable text, for “Copy details”.
    pub fn details(&self) -> String {
        let mut out = format!(
            "{} · {} · {}\n{}\n",
            crate::widgets::status_label(self.status),
            area_label(&self.area),
            self.item,
            self.message
        );
        for (name, value) in [
            ("From Lightroom", &self.source),
            ("In LightCraft", &self.result),
            ("Reason", &self.reason),
            ("Recommendation", &self.recommendation),
            ("Code", &self.code),
        ] {
            if let Some(value) = value {
                out.push_str(&format!("{name}: {value}\n"));
            }
        }
        out
    }
}

/// The label of an area key.
pub fn area_label(area: &str) -> &str {
    match area {
        "folders" => "Folders",
        "photos" => "Photos",
        "keywords" => "Keywords",
        "ratings" => "Star ratings",
        "labels" => "Color labels",
        "flags" => "Flags",
        "metadata" => "Metadata",
        "collections" => "Collections",
        "stacks" => "Stacks",
        "virtual-copies" => "Virtual copies",
        "develop" => "Develop",
        other => other,
    }
}

/// Every area key in display order.
pub const AREAS: [&str; 11] = [
    "folders",
    "photos",
    "keywords",
    "ratings",
    "labels",
    "flags",
    "metadata",
    "collections",
    "stacks",
    "virtual-copies",
    "develop",
];

/// The log of one run.
#[derive(Debug, Clone, Default)]
pub struct LogData {
    pub rows: Vec<Row>,
    pub anonymized: bool,
}

impl LogData {
    /// Reads the entries of a report; none when it has no log.
    pub fn from_report(report: &Value) -> LogData {
        LogData {
            rows: report["entries"]
                .as_array()
                .map(|a| a.iter().map(Row::from_json).collect())
                .unwrap_or_default(),
            anonymized: report["anonymized"].as_bool().unwrap_or(false),
        }
    }

    /// Entries per status.
    pub fn count(&self, status: Status) -> usize {
        self.rows.iter().filter(|r| r.status == status).count()
    }

    /// The areas that have entries, in display order.
    pub fn areas(&self) -> Vec<&'static str> {
        AREAS
            .into_iter()
            .filter(|a| self.rows.iter().any(|r| r.area == *a))
            .collect()
    }

    /// The indexes of the rows that pass the filters.
    pub fn filtered(&self, filter: &Filter) -> Vec<usize> {
        let needle = filter.search.trim().to_lowercase();
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| filter.status.is_none_or(|s| r.status == s))
            .filter(|(_, r)| filter.area.as_deref().is_none_or(|a| r.area == a))
            .filter(|(_, r)| {
                needle.is_empty()
                    || r.item.to_lowercase().contains(&needle)
                    || r.message.to_lowercase().contains(&needle)
            })
            .map(|(i, _)| i)
            .collect()
    }
}

/// The Log screen's filters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub status: Option<Status>,
    pub area: Option<String>,
    pub search: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn data() -> LogData {
        LogData::from_report(&json!({
            "anonymized": false,
            "entries": [
                {"status": "transferred", "area": "photos", "stage": "photos", "item": "IMG_1.CR3", "message": "Imported as a reference"},
                {"status": "error", "area": "photos", "stage": "photos", "item": "IMG_2.CR3", "message": "LightCraft did not accept the import", "code": "import_failed"},
                {"status": "skipped", "area": "keywords", "stage": "read", "item": "Reise", "message": "Keyword name contains “|”"},
            ]
        }))
    }

    #[test]
    fn filters_combine_status_area_and_search() {
        let d = data();
        assert_eq!(d.count(Status::Error), 1);
        assert_eq!(d.areas(), ["photos", "keywords"]);
        let mut f = Filter::default();
        assert_eq!(d.filtered(&f), [0, 1, 2]);
        f.status = Some(Status::Error);
        assert_eq!(d.filtered(&f), [1]);
        f = Filter {
            area: Some("photos".into()),
            search: "img_1".into(),
            ..Filter::default()
        };
        assert_eq!(d.filtered(&f), [0]);
        f.search = "accept".into();
        assert_eq!(d.filtered(&f), [1]);
    }

    #[test]
    fn details_read_as_text() {
        let text = data().rows[1].details();
        assert!(text.starts_with("Error · Photos · IMG_2.CR3\n"));
        assert!(text.contains("Code: import_failed"));
    }
}
