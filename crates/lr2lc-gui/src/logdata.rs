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
    /// Item and message in lower case, for the search.
    pub search: String,
}

impl Row {
    /// Fills [`Row::search`] from the item and the message.
    pub fn indexed(mut self) -> Row {
        self.search = format!("{}\n{}", self.item, self.message).to_lowercase();
        self
    }

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
            search: String::new(),
        }
        .indexed()
    }

    /// A live entry of a running transfer.
    pub fn from_entry(e: &lr2lc_core::log::LogEntry) -> Row {
        Row {
            status: crate::widgets::status_of(e.status),
            area: e.area.to_owned(),
            stage: e.stage.to_owned(),
            item: e.item.clone(),
            message: e.message.clone(),
            code: e.code.map(str::to_owned),
            reason: e.reason.clone(),
            recommendation: e.recommendation.map(str::to_owned),
            source: e.source.clone(),
            result: e.result.clone(),
            photo: e.photo,
            search: String::new(),
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

/// Photos counted once each (the report's `photo_counts`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhotoCounts {
    pub total: u64,
    pub transferred: u64,
    pub unchanged: u64,
    pub skipped: u64,
    pub conflict: u64,
    pub error: u64,
}

impl PhotoCounts {
    /// `59 photos: 56 transferred, 3 skipped` (zero parts left out).
    pub fn line(&self, dry_run: bool) -> String {
        let done = if dry_run {
            "to transfer"
        } else {
            "transferred"
        };
        let parts: Vec<String> = [
            (self.transferred, done),
            (self.unchanged, "unchanged"),
            (self.skipped, "skipped"),
            (self.conflict, "with conflicts"),
            (self.error, "with errors"),
        ]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}"))
        .collect();
        let photos = if self.total == 1 { "photo" } else { "photos" };
        if parts.is_empty() {
            format!("{} {photos}", self.total)
        } else {
            format!("{} {photos}: {}", self.total, parts.join(", "))
        }
    }
}

/// One photo with all its entries, for the view grouped by photo.
#[derive(Debug, Clone)]
pub struct PhotoGroup {
    pub photo: i64,
    /// A row that stands for the photo: its worst status, its name, how
    /// many entries it has and what they say.
    pub row: Row,
    /// Indexes of its entries in [`LogData::rows`].
    pub entries: Vec<usize>,
}

/// The log of one run.
#[derive(Debug, Clone, Default)]
pub struct LogData {
    pub rows: Vec<Row>,
    pub anonymized: bool,
    pub photo_counts: Option<PhotoCounts>,
    pub dry_run: bool,
    /// Entries per status, in [`Status::ALL`] order.
    counts: [usize; 5],
    /// The areas that have entries, in display order.
    areas: Vec<&'static str>,
}

/// What the Transfer screen needs from a scan of the catalog (a dry run):
/// a handful of numbers instead of the whole report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanSummary {
    pub catalog_version: String,
    pub photos_read: u64,
    pub photos: u64,
    pub folders: u64,
    pub missing: u64,
    pub keywords: u64,
    pub rated: u64,
    pub labelled: u64,
    pub flagged: u64,
    pub captions: u64,
    pub copyrights: u64,
    pub albums: u64,
    pub smart: u64,
    pub stacks: u64,
    pub virtual_copies: u64,
    pub develop: u64,
}

impl ScanSummary {
    pub fn from_report(r: &Value) -> ScanSummary {
        let n = |path: &[&str]| {
            path.iter()
                .try_fold(r, |v, k| v.get(k))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        };
        let sum = |path: &[&str], keys: &[&str]| -> u64 {
            keys.iter()
                .map(|k| {
                    let mut full = path.to_vec();
                    full.push(k);
                    n(&full)
                })
                .sum()
        };
        ScanSummary {
            catalog_version: r["catalog_version"].as_str().unwrap_or("").to_owned(),
            photos_read: n(&["photos", "read"]),
            photos: n(&["photos", "converted"]),
            folders: n(&["folders", "converted"]),
            missing: n(&["photos", "by_code", "missing"]),
            keywords: n(&["keywords", "converted"]),
            rated: sum(&["ratings"], &["1", "2", "3", "4", "5"]),
            labelled: sum(
                &["photos", "labels"],
                &["red", "yellow", "green", "blue", "purple"],
            ),
            flagged: sum(&["photos", "flags"], &["pick", "reject"]),
            captions: n(&["photos", "captions"]),
            copyrights: n(&["photos", "copyrights"]),
            albums: n(&["collections", "albums"]),
            smart: n(&["collections", "smart"]),
            stacks: n(&["stacks", "converted"]),
            virtual_copies: r["entries"].as_array().map_or(0, |a| {
                a.iter()
                    .filter(|e| e["area"] == "virtual-copies" && e["status"] == "transferred")
                    .count() as u64
            }),
            develop: n(&["develop", "photos"]),
        }
    }
}

impl LogData {
    /// Reads the entries of a report; none when it has no log.
    pub fn from_report(report: &Value) -> LogData {
        let mut data = LogData {
            rows: report["entries"]
                .as_array()
                .map(|a| a.iter().map(Row::from_json).collect())
                .unwrap_or_default(),
            anonymized: report["anonymized"].as_bool().unwrap_or(false),
            photo_counts: report
                .get("photo_counts")
                .filter(|v| v.is_object())
                .map(|c| {
                    let n = |key: &str| c[key].as_u64().unwrap_or(0);
                    PhotoCounts {
                        total: n("total"),
                        transferred: n("transferred"),
                        unchanged: n("unchanged"),
                        skipped: n("skipped"),
                        conflict: n("conflict"),
                        error: n("error"),
                    }
                }),
            dry_run: report["dry_run"].as_bool().unwrap_or(false),
            counts: [0; 5],
            areas: Vec::new(),
        };
        data.recount();
        data
    }

    /// Recomputes the cached counts and areas after `rows` changed.
    pub fn recount(&mut self) {
        self.counts = [0; 5];
        let mut seen = std::collections::HashSet::new();
        for row in &self.rows {
            if let Some(i) = Status::ALL.iter().position(|s| *s == row.status) {
                self.counts[i] += 1;
            }
            seen.insert(row.area.as_str());
        }
        self.areas = AREAS.into_iter().filter(|a| seen.contains(a)).collect();
    }

    /// Entries per status.
    pub fn count(&self, status: Status) -> usize {
        Status::ALL
            .iter()
            .position(|s| *s == status)
            .map_or(0, |i| self.counts[i])
    }

    /// The areas that have entries, in display order.
    pub fn areas(&self) -> &[&'static str] {
        &self.areas
    }

    /// The `visible` entries grouped by photo, in order of each photo's
    /// first entry; entries about no single photo are left out.
    pub fn by_photo(&self, visible: &[usize]) -> Vec<PhotoGroup> {
        let mut order: Vec<i64> = Vec::new();
        let mut groups: std::collections::HashMap<i64, Vec<usize>> = Default::default();
        for &index in visible {
            if let Some(photo) = self.rows[index].photo {
                groups
                    .entry(photo)
                    .or_insert_with(|| {
                        order.push(photo);
                        Vec::new()
                    })
                    .push(index);
            }
        }
        order
            .into_iter()
            .filter_map(|photo| {
                let entries = groups.remove(&photo)?;
                let rows: Vec<&Row> = entries.iter().map(|i| &self.rows[*i]).collect();
                // A photo is "skipped" only when it was skipped as a whole.
                let rank = |r: &Row| match r.status {
                    Status::Error => 4,
                    Status::Conflict => 3,
                    Status::Skipped if matches!(r.area.as_str(), "photos" | "virtual-copies") => 2,
                    Status::Skipped => 0,
                    Status::Adjusted | Status::Transferred => 1,
                };
                let worst = rows.iter().max_by_key(|r| rank(r))?;
                let status = if rank(worst) == 0 {
                    Status::Transferred
                } else {
                    worst.status
                };
                let mut messages: Vec<&str> = Vec::new();
                for r in &rows {
                    if !messages.contains(&r.message.as_str()) {
                        messages.push(&r.message);
                    }
                }
                let n = entries.len();
                Some(PhotoGroup {
                    photo,
                    row: Row {
                        status,
                        area: if n == 1 {
                            "1 entry".into()
                        } else {
                            format!("{n} entries")
                        },
                        stage: String::new(),
                        item: rows.first().map(|r| r.item.clone()).unwrap_or_default(),
                        message: messages.join(" · "),
                        code: None,
                        reason: None,
                        recommendation: None,
                        source: None,
                        result: None,
                        photo: Some(photo),
                        search: String::new(),
                    },
                    entries,
                })
            })
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
            .filter(|(_, r)| needle.is_empty() || r.search.contains(&needle))
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
    fn entries_group_by_photo_with_the_worst_status() {
        let d = LogData::from_report(&json!({
            "dry_run": false,
            "photo_counts": {"total": 3, "transferred": 1, "unchanged": 0, "skipped": 1, "conflict": 0, "error": 1},
            "entries": [
                {"status": "transferred", "area": "photos", "item": "a.jpg", "message": "Imported as a reference", "photo": 1},
                {"status": "transferred", "area": "ratings", "item": "a.jpg", "message": "Star rating set", "photo": 1},
                {"status": "skipped", "area": "develop", "item": "a.jpg", "message": "2 development fields not carried over", "photo": 1},
                {"status": "error", "area": "photos", "item": "b.jpg", "message": "LightCraft did not accept the import", "photo": 2},
                {"status": "transferred", "area": "keywords", "item": "Reise", "message": "Keyword created"},
            ]
        }));
        let groups = d.by_photo(&d.filtered(&Filter::default()));
        assert_eq!(groups.len(), 2, "the keyword entry belongs to no photo");
        assert_eq!(
            groups[0].row.status,
            Status::Transferred,
            "a field left out is no skipped photo"
        );
        assert_eq!(groups[0].row.area, "3 entries");
        assert!(
            groups[0]
                .row
                .message
                .starts_with("Imported as a reference · Star rating set")
        );
        assert_eq!(groups[1].row.status, Status::Error);
        assert_eq!(
            d.photo_counts.unwrap().line(false),
            "3 photos: 1 transferred, 1 skipped, 1 with errors"
        );
    }

    #[test]
    fn details_read_as_text() {
        let text = data().rows[1].details();
        assert!(text.starts_with("Error · Photos · IMG_2.CR3\n"));
        assert!(text.contains("Code: import_failed"));
    }
}
