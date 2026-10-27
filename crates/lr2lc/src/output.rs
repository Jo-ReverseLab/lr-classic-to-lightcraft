//! Readable summary and `report.json`.

use crate::dry_run::{FlagCounts, KeywordAttributes, Outcome};
use crate::labels::LabelCounts;
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
    pub photos: PhotosJson<'a>,
    pub collections: AreaJson<'a>,
    pub keywords: KeywordsJson<'a>,
    pub keyword_assignments: AreaJson<'a>,
    pub keyword_synonyms: AreaJson<'a>,
    /// Converted photos per star rating, keys "0" to "5".
    pub ratings: BTreeMap<String, usize>,
}

/// Counts of one kind of element.
#[derive(Debug, Serialize)]
pub struct AreaJson<'a> {
    pub read: usize,
    pub converted: usize,
    pub skipped: Vec<SkippedJson<'a>>,
}

/// Counts of photos, plus how many of the skipped photos are videos.
#[derive(Debug, Serialize)]
pub struct PhotosJson<'a> {
    #[serde(flatten)]
    pub area: AreaJson<'a>,
    /// Skipped videos; they are also listed in `skipped`.
    pub videos: usize,
    /// Converted photos per flag.
    pub flags: FlagsJson,
    /// Converted photos per colour label.
    pub labels: LabelsJson,
    /// Converted photos with a caption. Caption texts are never listed.
    pub captions: usize,
}

/// Converted photos per flag; the fields add up to the converted photos.
#[derive(Debug, Serialize)]
pub struct FlagsJson {
    pub pick: usize,
    pub reject: usize,
    pub none: usize,
    pub unknown: usize,
}

impl From<FlagCounts> for FlagsJson {
    fn from(f: FlagCounts) -> Self {
        FlagsJson {
            pick: f.pick,
            reject: f.reject,
            none: f.none,
            unknown: f.unknown,
        }
    }
}

/// Converted photos per colour label; the seven counters add up to the
/// converted photos. Label names are never listed.
#[derive(Debug, Serialize)]
pub struct LabelsJson {
    pub red: usize,
    pub yellow: usize,
    pub green: usize,
    pub blue: usize,
    pub purple: usize,
    pub none: usize,
    pub custom: usize,
    /// `en`, `de`, `file` or `none`.
    pub label_set: &'static str,
    /// One entry per custom photo, ascending by id.
    pub custom_reasons: Vec<ReasonJson>,
}

/// A reason without a borrowed source.
#[derive(Debug, Serialize)]
pub struct ReasonJson {
    pub reason: String,
}

impl From<&LabelCounts> for LabelsJson {
    fn from(l: &LabelCounts) -> Self {
        LabelsJson {
            red: l.red,
            yellow: l.yellow,
            green: l.green,
            blue: l.blue,
            purple: l.purple,
            none: l.none,
            custom: l.custom,
            label_set: l.set,
            custom_reasons: l
                .custom_ids
                .iter()
                .map(|id| ReasonJson {
                    reason: format!("photo {id}: {CUSTOM_LABEL_REASON}"),
                })
                .collect(),
        }
    }
}

/// Reason of a photo whose label is not in the label set, after the
/// `photo {id}: ` prefix.
const CUSTOM_LABEL_REASON: &str = "custom label not in label set";

/// Counts of keywords, plus attribute counts over the converted keywords.
#[derive(Debug, Serialize)]
pub struct KeywordsJson<'a> {
    #[serde(flatten)]
    pub area: AreaJson<'a>,
    pub persons: usize,
    pub not_exported: usize,
    pub no_parents_on_export: usize,
    pub no_synonyms_on_export: usize,
    pub unknown_type: usize,
    pub unused: usize,
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
        photos: PhotosJson {
            area: AreaJson::new(&outcome.photos),
            videos: outcome.videos,
            flags: outcome.flags.into(),
            labels: (&outcome.labels).into(),
            captions: outcome.captions,
        },
        collections: AreaJson::new(&outcome.collections),
        keywords: KeywordsJson {
            area: AreaJson::new(&outcome.keywords),
            persons: outcome.keyword_attributes.persons,
            not_exported: outcome.keyword_attributes.not_exported,
            no_parents_on_export: outcome.keyword_attributes.no_parents_on_export,
            no_synonyms_on_export: outcome.keyword_attributes.no_synonyms_on_export,
            unknown_type: outcome.keyword_attributes.unknown_type,
            unused: outcome.keyword_attributes.unused,
        },
        keyword_assignments: AreaJson::new(&outcome.keyword_assignments),
        keyword_synonyms: AreaJson::new(&outcome.keyword_synonyms),
        ratings: outcome
            .ratings
            .iter()
            .enumerate()
            .map(|(stars, count)| (stars.to_string(), *count))
            .collect(),
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
    let _ = writeln!(out, "videos: skipped {}", outcome.videos);
    let _ = writeln!(out, "{}", attributes_line(&outcome.keyword_attributes));
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
    let _ = writeln!(out, "{}", ratings_line(&outcome.ratings));
    let _ = writeln!(out, "{}", flags_line(&outcome.flags));
    let _ = writeln!(out, "{}", labels_line(&outcome.labels));
    let _ = writeln!(out, "captions: {}", outcome.captions);
    if outcome.labels.recommend {
        let _ = writeln!(
            out,
            "hint: the colour label names match neither the English nor the German set; \
             {} photos keep no label. Choose a set with --label-set en, de or FILE",
            outcome.labels.custom
        );
    }
    if outcome.catalog_unchanged {
        let _ = writeln!(out, "catalog unchanged (checksum verified)");
    }
    out
}

/// The report areas in output order.
fn areas(outcome: &Outcome) -> [(&'static str, &Report); 6] {
    [
        ("folders", &outcome.folders),
        ("photos", &outcome.photos),
        ("collections", &outcome.collections),
        ("keywords", &outcome.keywords),
        ("keyword assignments", &outcome.keyword_assignments),
        ("keyword synonyms", &outcome.keyword_synonyms),
    ]
}

/// Formats the keyword attribute counts as one line.
fn attributes_line(a: &KeywordAttributes) -> String {
    format!(
        "keyword attributes: persons {}, not exported {}, no parents on export {}, \
         no synonyms on export {}, unknown type {}, unused {}",
        a.persons,
        a.not_exported,
        a.no_parents_on_export,
        a.no_synonyms_on_export,
        a.unknown_type,
        a.unused
    )
}

/// Formats the rating distribution as `ratings: 0: a, 1: b, ..., 5: f`.
fn ratings_line(ratings: &[usize; 6]) -> String {
    let parts: Vec<String> = ratings
        .iter()
        .enumerate()
        .map(|(stars, count)| format!("{stars}: {count}"))
        .collect();
    format!("ratings: {}", parts.join(", "))
}

/// Formats the label counts as
/// `labels: red R, yellow Y, green G, blue B, purple P, none N, custom C, set S`.
fn labels_line(l: &LabelCounts) -> String {
    format!(
        "labels: red {}, yellow {}, green {}, blue {}, purple {}, none {}, custom {}, set {}",
        l.red, l.yellow, l.green, l.blue, l.purple, l.none, l.custom, l.set
    )
}

/// Formats the flag counts as `flags: pick P, reject R, none N, unknown U`.
fn flags_line(f: &FlagCounts) -> String {
    format!(
        "flags: pick {}, reject {}, none {}, unknown {}",
        f.pick, f.reject, f.none, f.unknown
    )
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
        let mut keywords = Report::new();
        for _ in 0..3 {
            keywords.read();
        }
        keywords.converted();
        keywords.converted();
        keywords.skipped("keyword 7: name is empty");
        let mut keyword_assignments = Report::new();
        keyword_assignments.read();
        keyword_assignments.skipped("assignment 5: photo 2 is skipped or unknown");
        let mut keyword_synonyms = Report::new();
        for _ in 0..2 {
            keyword_synonyms.read();
        }
        keyword_synonyms.converted();
        keyword_synonyms.skipped("synonym 4: keyword 9 is unknown");
        Outcome {
            catalog_version: "1500000".into(),
            catalog_unchanged: true,
            folders,
            photos,
            videos: 0,
            collections,
            keywords,
            keyword_attributes: KeywordAttributes {
                persons: 1,
                not_exported: 2,
                no_parents_on_export: 3,
                no_synonyms_on_export: 4,
                unknown_type: 5,
                unused: 6,
            },
            keyword_assignments,
            keyword_synonyms,
            ratings: [0, 0, 0, 1, 0, 0],
            flags: FlagCounts {
                pick: 1,
                reject: 0,
                none: 0,
                unknown: 0,
            },
            labels: LabelCounts {
                set: "none",
                none: 1,
                ..LabelCounts::default()
            },
            captions: 0,
        }
    }

    #[test]
    fn captions_are_in_summary_and_json() {
        let mut o = outcome();
        o.captions = 4;
        assert_eq!(
            summary(&o).lines().filter(|l| *l == "captions: 4").count(),
            1
        );
        let v: serde_json::Value = serde_json::from_str(&report_json(&o)).unwrap();
        assert_eq!(v["photos"]["captions"], 4);
    }

    #[test]
    fn labels_are_in_summary_and_json() {
        let mut o = outcome();
        o.labels = LabelCounts {
            red: 1,
            custom: 2,
            set: "none",
            custom_ids: vec![3, 8],
            recommend: true,
            ..LabelCounts::default()
        };
        let text = summary(&o);
        assert!(text.lines().any(|l| l
            == "labels: red 1, yellow 0, green 0, blue 0, purple 0, none 0, custom 2, set none"));
        assert!(
            text.lines()
                .any(|l| l.starts_with("hint:") && l.contains("--label-set"))
        );
        let v: serde_json::Value = serde_json::from_str(&report_json(&o)).unwrap();
        let l = &v["photos"]["labels"];
        assert_eq!(l["custom"], 2);
        assert_eq!(l["label_set"], "none");
        assert_eq!(
            l["custom_reasons"][1]["reason"],
            "photo 8: custom label not in label set"
        );
        o.labels.recommend = false;
        assert!(!summary(&o).contains("--label-set"));
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
        assert!(lines.contains(&"keywords: read 3, would convert 2, skipped 1"));
        assert!(lines.contains(&"keyword assignments: read 1, would convert 0, skipped 1"));
        assert!(lines.contains(&"skipped keywords:"));
        assert!(lines.contains(&"1 x name is empty"));
        assert!(lines.contains(&"skipped keyword assignments:"));
        assert!(lines.contains(&"1 x photo 2 is skipped or unknown"));
        assert!(lines.contains(&"keyword synonyms: read 2, would convert 1, skipped 1"));
        assert!(lines.contains(&"skipped keyword synonyms:"));
        assert!(lines.contains(&"1 x keyword 9 is unknown"));
        assert!(lines.contains(&"ratings: 0: 0, 1: 0, 2: 0, 3: 1, 4: 0, 5: 0"));
        assert!(lines.contains(&"videos: skipped 0"));
    }

    #[test]
    fn videos_are_counted_in_summary_and_json() {
        let mut o = outcome();
        o.photos.read();
        o.photos
            .skipped("photo 5: video files are not supported by LightCraft");
        o.videos = 1;
        let lines: Vec<String> = summary(&o).lines().map(|l| l.trim().to_owned()).collect();
        assert!(lines.contains(&"videos: skipped 1".to_owned()));
        assert!(lines.contains(&"1 x video files are not supported by LightCraft".to_owned()));
        let v: serde_json::Value = serde_json::from_str(&report_json(&o)).unwrap();
        assert_eq!(v["photos"]["videos"], 1);
        assert_eq!(v["photos"]["read"], 5);
        assert_eq!(v["photos"]["skipped"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn keyword_attributes_are_in_summary_and_json() {
        let o = outcome();
        let text = summary(&o);
        assert!(
            text.lines().any(|l| l
                == "keyword attributes: persons 1, not exported 2, no parents on export 3, \
                    no synonyms on export 4, unknown type 5, unused 6"),
            "{text}"
        );
        let v: serde_json::Value = serde_json::from_str(&report_json(&o)).unwrap();
        let k = &v["keywords"];
        assert_eq!(k["read"], 3);
        assert_eq!(k["persons"], 1);
        assert_eq!(k["not_exported"], 2);
        assert_eq!(k["no_parents_on_export"], 3);
        assert_eq!(k["no_synonyms_on_export"], 4);
        assert_eq!(k["unknown_type"], 5);
        assert_eq!(k["unused"], 6);
    }

    #[test]
    fn flags_are_in_summary_and_json() {
        let mut o = outcome();
        o.flags = FlagCounts {
            pick: 1,
            reject: 2,
            none: 3,
            unknown: 4,
        };
        let text = summary(&o);
        assert!(
            text.lines()
                .any(|l| l == "flags: pick 1, reject 2, none 3, unknown 4"),
            "{text}"
        );
        let v: serde_json::Value = serde_json::from_str(&report_json(&o)).unwrap();
        let f = &v["photos"]["flags"];
        assert_eq!(f["pick"], 1);
        assert_eq!(f["reject"], 2);
        assert_eq!(f["none"], 3);
        assert_eq!(f["unknown"], 4);
    }

    #[test]
    fn ratings_line_lists_all_six_stars() {
        assert_eq!(
            ratings_line(&[6, 3, 2, 3, 2, 1]),
            "ratings: 0: 6, 1: 3, 2: 2, 3: 3, 4: 2, 5: 1"
        );
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
                "keyword_assignments",
                "keyword_synonyms",
                "keywords",
                "photos",
                "ratings",
                "tool_version"
            ]
        );
        assert_eq!(v["dry_run"], true);
        assert_eq!(v["photos"]["read"], 4);
        assert_eq!(v["photos"]["converted"], 1);
        assert_eq!(v["photos"]["videos"], 0);
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
        assert_eq!(v["keywords"]["read"], 3);
        assert_eq!(v["keywords"]["converted"], 2);
        assert_eq!(
            v["keywords"]["skipped"][0]["reason"],
            "keyword 7: name is empty"
        );
        assert_eq!(v["keyword_assignments"]["read"], 1);
        assert_eq!(v["keyword_synonyms"]["read"], 2);
        assert_eq!(v["keyword_synonyms"]["converted"], 1);
        assert_eq!(
            v["keyword_synonyms"]["skipped"][0]["reason"],
            "synonym 4: keyword 9 is unknown"
        );
        let ratings = v["ratings"].as_object().unwrap();
        let keys: Vec<&str> = ratings.keys().map(String::as_str).collect();
        assert_eq!(keys, ["0", "1", "2", "3", "4", "5"]);
        assert_eq!(v["ratings"]["3"], 1);
    }
}
