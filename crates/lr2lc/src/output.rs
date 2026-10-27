//! Readable summary and `report.json`.

use crate::dry_run::Outcome;
use model::Report;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write;

/// Structure of `report.json`.
#[derive(Debug, Serialize)]
pub struct ReportJson<'a> {
    pub tool_version: &'a str,
    pub catalog_version: &'a str,
    pub dry_run: bool,
    pub catalog_unchanged: bool,
    pub folders: AreaJson<'a>,
    pub photos: AreaJson<'a>,
    pub collections: AreaJson<'a>,
}

/// Counts of one kind of element.
#[derive(Debug, Serialize)]
pub struct AreaJson<'a> {
    pub read: usize,
    pub converted: usize,
    pub skipped: Vec<SkippedJson<'a>>,
}

/// One skipped element.
#[derive(Debug, Serialize)]
pub struct SkippedJson<'a> {
    pub reason: &'a str,
}

impl<'a> AreaJson<'a> {
    fn new(report: &'a Report) -> Self {
        AreaJson {
            read: report.read_count(),
            converted: report.converted_count(),
            skipped: report
                .skipped_items()
                .iter()
                .map(|s| SkippedJson { reason: &s.reason })
                .collect(),
        }
    }
}

/// Builds the JSON report of a dry run.
pub fn report_json(outcome: &Outcome) -> String {
    let json = ReportJson {
        tool_version: env!("CARGO_PKG_VERSION"),
        catalog_version: &outcome.catalog_version,
        dry_run: true,
        catalog_unchanged: outcome.catalog_unchanged,
        folders: AreaJson::new(&outcome.folders),
        photos: AreaJson::new(&outcome.photos),
        collections: AreaJson::new(&outcome.collections),
    };
    let mut text = serde_json::to_string_pretty(&json).expect("report serializes");
    text.push('\n');
    text
}

/// Builds the readable summary of a dry run.
pub fn summary(outcome: &Outcome) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "lr2lc {} dry run: nothing is written (use --apply to convert)",
        env!("CARGO_PKG_VERSION")
    );
    let _ = writeln!(out, "catalog version {}", outcome.catalog_version);
    for (area, report) in areas(outcome) {
        let _ = writeln!(
            out,
            "{area}: read {}, would convert {}, skipped {}",
            report.read_count(),
            report.converted_count(),
            report.skipped_count()
        );
    }
    for (area, report) in areas(outcome) {
        let groups = group_reasons(report);
        if groups.is_empty() {
            continue;
        }
        let _ = writeln!(out, "skipped {area}:");
        for (reason, count) in groups {
            let _ = writeln!(out, "  {count:>6} x {reason}");
        }
    }
    if outcome.catalog_unchanged {
        let _ = writeln!(out, "catalog unchanged (checksum verified)");
    }
    out
}

/// The report areas in output order.
fn areas(outcome: &Outcome) -> [(&'static str, &Report); 3] {
    [
        ("folders", &outcome.folders),
        ("photos", &outcome.photos),
        ("collections", &outcome.collections),
    ]
}

/// Groups skip reasons without the leading `<kind> <id>: ` prefix, sorted
/// by count (descending), then by text.
fn group_reasons(report: &Report) -> Vec<(&str, usize)> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for s in report.skipped_items() {
        *counts.entry(strip_id_prefix(&s.reason)).or_default() += 1;
    }
    let mut groups: Vec<(&str, usize)> = counts.into_iter().collect();
    groups.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    groups
}

/// Removes a leading `word <digits>: ` prefix such as `photo 12: `.
fn strip_id_prefix(reason: &str) -> &str {
    let Some((head, rest)) = reason.split_once(": ") else {
        return reason;
    };
    match head.split_once(' ') {
        Some((word, id))
            if !word.is_empty()
                && !id.is_empty()
                && id.bytes().all(|b| b.is_ascii_digit())
                && !rest.is_empty() =>
        {
            rest
        }
        _ => reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome() -> Outcome {
        let mut folders = Report::new();
        folders.read();
        folders.converted();
        let mut photos = Report::new();
        for _ in 0..4 {
            photos.read();
        }
        photos.converted();
        photos.skipped("photo 2: original file is missing");
        photos.skipped("photo 3: original file is missing");
        photos.skipped("photo 9: virtual copy of photo 1 is not converted");
        let mut collections = Report::new();
        for id in [1, 2] {
            collections.read();
            collections.skipped(format!(
                "collection {id}: smart collection is not converted"
            ));
        }
        Outcome {
            catalog_version: "1500000".into(),
            catalog_unchanged: true,
            folders,
            photos,
            collections,
        }
    }

    #[test]
    fn strips_only_id_prefixes() {
        assert_eq!(strip_id_prefix("photo 12: gone"), "gone");
        assert_eq!(
            strip_id_prefix("folder 3: root folder 9 x"),
            "root folder 9 x"
        );
        assert_eq!(strip_id_prefix("unspecified"), "unspecified");
        assert_eq!(strip_id_prefix("note: x"), "note: x");
        assert_eq!(strip_id_prefix("photo x: y"), "photo x: y");
    }

    #[test]
    fn summary_has_count_lines_and_grouped_reasons() {
        let text = summary(&outcome());
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        assert!(lines.contains(&"folders: read 1, would convert 1, skipped 0"));
        assert!(lines.contains(&"photos: read 4, would convert 1, skipped 3"));
        assert!(lines.contains(&"2 x original file is missing"), "{text}");
        assert!(lines.contains(&"collections: read 2, would convert 0, skipped 2"));
        assert!(lines.contains(&"skipped collections:"));
        assert!(lines.contains(&"2 x smart collection is not converted"));
        assert!(text.contains("dry run"));
        assert!(!text.contains("skipped folders:"));
    }

    #[test]
    fn json_has_documented_fields() {
        let v: serde_json::Value = serde_json::from_str(&report_json(&outcome())).unwrap();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        let mut keys = keys;
        keys.sort();
        assert_eq!(
            keys,
            [
                "catalog_unchanged",
                "catalog_version",
                "collections",
                "dry_run",
                "folders",
                "photos",
                "tool_version"
            ]
        );
        assert_eq!(v["dry_run"], true);
        assert_eq!(v["photos"]["read"], 4);
        assert_eq!(v["photos"]["converted"], 1);
        assert_eq!(
            v["photos"]["skipped"][0]["reason"],
            "photo 2: original file is missing"
        );
        assert_eq!(v["folders"]["skipped"].as_array().unwrap().len(), 0);
        assert_eq!(v["collections"]["read"], 2);
        assert_eq!(v["collections"]["converted"], 0);
        assert_eq!(
            v["collections"]["skipped"][1]["reason"],
            "collection 2: smart collection is not converted"
        );
    }
}
