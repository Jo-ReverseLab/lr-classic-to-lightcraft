//! Readable summary and `report.json`.

use crate::dry_run::{CollectionCounts, FlagCounts, KeywordAttributes, Outcome, StackCounts};
use crate::labels::LabelCounts;
use crate::writer::{DevelopJson, WriterJson};
use model::{Report, Skipped};
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
    pub collections: CollectionsJson<'a>,
    pub collection_members: AreaJson<'a>,
    pub stacks: StacksJson<'a>,
    pub stack_members: AreaJson<'a>,
    pub keywords: KeywordsJson<'a>,
    pub keyword_assignments: AreaJson<'a>,
    pub keyword_synonyms: AreaJson<'a>,
    /// Skipped elements per code, summed over the nine areas above (not
    /// over `writer`).
    pub skipped_by_code: BTreeMap<&'static str, usize>,
    /// Converted photos per star rating, keys "0" to "5".
    pub ratings: BTreeMap<String, usize>,
    /// Development settings: what the writer plans or wrote. Counts and
    /// unmapped field names only, never values.
    pub develop: &'a DevelopJson,
    /// What the writer plans (dry run) or did (`--apply`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub writer: Option<&'a WriterJson>,
    /// Areas left out of this run (`--skip`), in a fixed order; absent when
    /// everything was selected.
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub not_selected: &'a [&'static str],
    /// Time spent in LightCraft per command (`--apply` only): count and
    /// total milliseconds, most expensive first is up to the reader.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub timing: BTreeMap<&'a str, TimingJson>,
    /// The run log.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub log: Option<LogJson<'a>>,
}

/// Time spent in one LightCraft command.
#[derive(Debug, Serialize)]
pub struct TimingJson {
    pub count: usize,
    pub ms: u64,
}

/// The run log in `report.json`.
#[derive(Debug, Serialize)]
pub struct LogJson<'a> {
    /// Whether names, paths and values were left out (`--anonymize`).
    pub anonymized: bool,
    /// Photos counted once each: `total`, `transferred`, `unchanged`,
    /// `skipped`, `conflict`, `error`.
    pub photo_counts: crate::log::PhotoCounts,
    /// Entries per status: `transferred`, `adjusted`, `skipped`,
    /// `conflict`, `error`.
    pub entry_counts: BTreeMap<&'static str, usize>,
    /// One entry per thing done, adjusted, skipped, in conflict or failed.
    pub entries: &'a [crate::log::LogEntry],
    /// What taking the run back did (Cancel in the window); absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollback: Option<RollbackJson>,
}

/// The `rollback` section: counts only.
#[derive(Debug, Clone, Serialize)]
pub struct RollbackJson {
    pub photos_removed: usize,
    pub albums_removed: usize,
    pub keywords_removed: usize,
    pub stacks_ungrouped: usize,
    pub members_restored: usize,
    pub quick_removed: usize,
    pub fields_restored: usize,
    pub develop_not_reset: usize,
    /// Photos the run had restored from LightCraft's trash, moved back.
    pub photos_trashed_again: usize,
    pub failed: usize,
    pub mapping_restored: bool,
}

/// Counts of one kind of element.
#[derive(Debug, Serialize)]
pub struct AreaJson<'a> {
    pub read: usize,
    pub converted: usize,
    pub skipped: Vec<SkippedJson<'a>>,
    /// Skipped elements per code; the counts add up to `skipped`.
    pub by_code: BTreeMap<&'static str, usize>,
}

/// Counts of photos, plus how many of the skipped photos are videos.
#[derive(Debug, Serialize)]
pub struct PhotosJson<'a> {
    #[serde(flatten)]
    pub area: AreaJson<'a>,
    /// Skipped videos; they are also listed in `skipped` and equal
    /// `by_code.video`.
    pub videos: usize,
    /// Converted photos per flag.
    pub flags: FlagsJson,
    /// Converted photos per colour label.
    pub labels: LabelsJson,
    /// Converted photos with a caption. Caption texts are never listed.
    pub captions: usize,
    /// Converted photos with a copyright notice. Texts are never listed.
    pub copyrights: usize,
    /// Converted photos with a creator. Names are never listed.
    pub creators: usize,
    /// Converted photos with a title. Texts are never listed.
    pub titles: usize,
    /// Converted photos whose title blob cannot be read.
    pub titles_unreadable: usize,
    /// Converted photos whose original is only in the cloud (macOS
    /// "dataless", e.g. iCloud Drive), and their size together in bytes.
    pub cloud_only: usize,
    pub cloud_only_bytes: u64,
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
    /// `auto`, `en`, `de` or `file`.
    pub label_set: &'static str,
    /// Under `auto`, the standard sets whose names matched at least one
    /// converted photo, in the order `en`, `de`; empty for a fixed set.
    pub matched_sets: Vec<&'static str>,
    /// Under `auto`, the standard set used (`en`, `de`), else `null`.
    pub detected_set: Option<&'static str>,
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
            matched_sets: l.matched_sets.clone(),
            detected_set: l.detected_set,
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

/// Counts of collections, plus kind and name counts over the converted
/// ones. Names are never listed.
#[derive(Debug, Serialize)]
pub struct CollectionsJson<'a> {
    #[serde(flatten)]
    pub area: AreaJson<'a>,
    pub albums: usize,
    pub folders: usize,
    pub duplicate_names: usize,
    /// 1 if a Quick Collection was read and converted, else 0.
    pub quick: usize,
    /// Smart collections converted as smart albums.
    pub smart: usize,
}

/// Counts of stacks, plus counts over the converted ones.
#[derive(Debug, Serialize)]
pub struct StacksJson<'a> {
    #[serde(flatten)]
    pub area: AreaJson<'a>,
    /// Stacks collapsed in Lightroom; they are written expanded.
    pub collapsed: usize,
    /// Stacks without a member at position 1.
    pub no_top: usize,
}

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
    pub trimmed: usize,
}

/// One skipped element: the free text and its machine-readable code.
#[derive(Debug, Serialize)]
pub struct SkippedJson<'a> {
    pub reason: &'a str,
    pub code: &'static str,
}

impl<'a> From<&'a Skipped> for SkippedJson<'a> {
    fn from(s: &'a Skipped) -> Self {
        SkippedJson {
            reason: &s.reason,
            code: s.code.as_str(),
        }
    }
}

impl<'a> AreaJson<'a> {
    fn new(report: &'a Report) -> Self {
        AreaJson {
            read: report.read_count(),
            converted: report.converted_count(),
            skipped: report
                .skipped_items()
                .iter()
                .map(SkippedJson::from)
                .collect(),
            by_code: report.skipped_by_code(),
        }
    }
}

/// Builds the JSON report of a dry run without writer section.
#[cfg(test)]
pub fn report_json(outcome: &Outcome) -> String {
    report_json_with(
        outcome,
        true,
        None,
        &DevelopJson::default(),
        &[],
        &Default::default(),
        None,
    )
}

/// Builds the JSON report of a run.
pub fn report_json_with(
    outcome: &Outcome,
    dry_run: bool,
    writer: Option<&WriterJson>,
    develop: &DevelopJson,
    not_selected: &[&'static str],
    timing: &lc_writer::control::Timing,
    log: Option<LogJson<'_>>,
) -> String {
    let json = ReportJson {
        tool_version: env!("CARGO_PKG_VERSION"),
        catalog_version: &outcome.catalog_version,
        dry_run,
        catalog_unchanged: outcome.catalog_unchanged,
        folders: AreaJson::new(&outcome.folders),
        photos: PhotosJson {
            area: AreaJson::new(&outcome.photos),
            videos: outcome.videos,
            flags: outcome.flags.into(),
            labels: (&outcome.labels).into(),
            captions: outcome.captions,
            copyrights: outcome.copyrights,
            creators: outcome.creators,
            titles: outcome.titles,
            titles_unreadable: outcome.titles_unreadable,
            cloud_only: outcome.cloud_only.count(),
            cloud_only_bytes: outcome.cloud_only.bytes(),
        },
        collections: collections_json(&outcome.collections, &outcome.collection_counts),
        collection_members: AreaJson::new(&outcome.collection_members),
        stacks: stacks_json(&outcome.stacks, &outcome.stack_counts),
        stack_members: AreaJson::new(&outcome.stack_members),
        keywords: KeywordsJson {
            area: AreaJson::new(&outcome.keywords),
            persons: outcome.keyword_attributes.persons,
            not_exported: outcome.keyword_attributes.not_exported,
            no_parents_on_export: outcome.keyword_attributes.no_parents_on_export,
            no_synonyms_on_export: outcome.keyword_attributes.no_synonyms_on_export,
            unknown_type: outcome.keyword_attributes.unknown_type,
            unused: outcome.keyword_attributes.unused,
            trimmed: outcome.keyword_attributes.trimmed,
        },
        keyword_assignments: AreaJson::new(&outcome.keyword_assignments),
        keyword_synonyms: AreaJson::new(&outcome.keyword_synonyms),
        skipped_by_code: skipped_by_code(outcome),
        ratings: outcome
            .ratings
            .iter()
            .enumerate()
            .map(|(stars, count)| (stars.to_string(), *count))
            .collect(),
        develop,
        writer,
        not_selected,
        timing: timing
            .iter()
            .map(|(name, (count, ms))| {
                (
                    name.as_str(),
                    TimingJson {
                        count: *count,
                        ms: *ms,
                    },
                )
            })
            .collect(),
        log,
    };
    // Provably infallible: every map in the report has string keys and every
    // value is a plain number, string, bool or list, which serde_json always
    // serializes.
    #[allow(clippy::expect_used)]
    let mut text = serde_json::to_string_pretty(&json).expect("report serializes");
    text.push('\n');
    text
}

/// Skipped elements per code over all catalog areas.
fn skipped_by_code(outcome: &Outcome) -> BTreeMap<&'static str, usize> {
    let mut out = BTreeMap::new();
    for (_, report) in areas(outcome) {
        for (code, n) in report.skipped_by_code() {
            *out.entry(code).or_insert(0) += n;
        }
    }
    out
}

fn collections_json<'a>(report: &'a Report, counts: &CollectionCounts) -> CollectionsJson<'a> {
    CollectionsJson {
        area: AreaJson::new(report),
        albums: counts.albums,
        folders: counts.folders,
        duplicate_names: counts.duplicate_names,
        quick: counts.quick,
        smart: counts.smart,
    }
}

fn stacks_json<'a>(report: &'a Report, counts: &StackCounts) -> StacksJson<'a> {
    StacksJson {
        area: AreaJson::new(report),
        collapsed: counts.collapsed,
        no_top: counts.no_top,
    }
}

/// Builds the readable summary of a dry run.
#[cfg(test)]
pub fn summary(outcome: &Outcome) -> String {
    summary_with(outcome, true)
}

/// `12.3 MB`.
pub fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

/// Builds the readable summary of the catalog part of a run.
pub fn summary_with(outcome: &Outcome, dry_run: bool) -> String {
    let mut out = String::new();
    let (header, convert) = if dry_run {
        (
            "dry run: nothing is written (use --apply to convert)",
            "would convert",
        )
    } else {
        ("apply: writing to the LightCraft library", "converted")
    };
    let _ = writeln!(out, "lr2lc {} {header}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(out, "catalog version {}", outcome.catalog_version);
    for (area, report) in areas(outcome) {
        let _ = writeln!(
            out,
            "{area}: read {}, {convert} {}, skipped {}",
            report.read_count(),
            report.converted_count(),
            report.skipped_count()
        );
    }
    let _ = writeln!(out, "videos: skipped {}", outcome.videos);
    let _ = writeln!(
        out,
        "originals only in the cloud (not downloaded): {} ({})",
        outcome.cloud_only.count(),
        megabytes(outcome.cloud_only.bytes())
    );
    let _ = writeln!(out, "{}", stack_counts_line(&outcome.stack_counts));
    let _ = writeln!(out, "{}", attributes_line(&outcome.keyword_attributes));
    for (area, report) in areas(outcome) {
        let groups = group_reasons(report.skipped_items());
        if groups.is_empty() {
            continue;
        }
        let _ = writeln!(out, "skipped {area}:");
        for (reason, count) in groups {
            let _ = writeln!(out, "  {count:>6} x {reason}");
        }
    }
    for (area, report) in areas(outcome) {
        let groups = group_codes(report.skipped_items());
        if groups.is_empty() {
            continue;
        }
        let _ = writeln!(out, "skip codes {area}:");
        for (code, count) in groups {
            let _ = writeln!(out, "  {count:>6} x {code}");
        }
    }
    let _ = writeln!(out, "{}", ratings_line(&outcome.ratings));
    let _ = writeln!(out, "{}", flags_line(&outcome.flags));
    let _ = writeln!(out, "{}", labels_line(&outcome.labels));
    let _ = writeln!(out, "captions: {}", outcome.captions);
    let _ = writeln!(out, "copyrights: {}", outcome.copyrights);
    let _ = writeln!(out, "creators: {}", outcome.creators);
    let _ = writeln!(out, "titles: {}", outcome.titles);
    let _ = writeln!(out, "titles unreadable: {}", outcome.titles_unreadable);
    if outcome.catalog_unchanged {
        let _ = writeln!(out, "catalog unchanged (checksum verified)");
    }
    out
}

/// The report areas in output order.
fn areas(outcome: &Outcome) -> [(&'static str, &Report); 9] {
    [
        ("folders", &outcome.folders),
        ("photos", &outcome.photos),
        ("collections", &outcome.collections),
        ("collection members", &outcome.collection_members),
        ("stacks", &outcome.stacks),
        ("stack members", &outcome.stack_members),
        ("keywords", &outcome.keywords),
        ("keyword assignments", &outcome.keyword_assignments),
        ("keyword synonyms", &outcome.keyword_synonyms),
    ]
}

/// Formats the counts over the converted stacks as one line.
fn stack_counts_line(c: &StackCounts) -> String {
    format!(
        "stack details: collapsed {} (written expanded), no top photo {}",
        c.collapsed, c.no_top
    )
}

/// Formats the keyword attribute counts as one line.
fn attributes_line(a: &KeywordAttributes) -> String {
    format!(
        "keyword attributes: persons {}, not exported {}, no parents on export {}, \
         no synonyms on export {}, unknown type {}, unused {}, \
         trimmed by LightCraft {}",
        a.persons,
        a.not_exported,
        a.no_parents_on_export,
        a.no_synonyms_on_export,
        a.unknown_type,
        a.unused,
        a.trimmed
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
    let detected = l
        .detected_set
        .map(|set| format!(" (detected {set})"))
        .unwrap_or_default();
    format!(
        "labels: red {}, yellow {}, green {}, blue {}, purple {}, none {}, custom {}, set {}{detected}",
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
pub fn group_reasons(skipped: &[Skipped]) -> Vec<(&str, usize)> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for s in skipped {
        *counts.entry(strip_id_prefix(&s.reason)).or_default() += 1;
    }
    let mut groups: Vec<(&str, usize)> = counts.into_iter().collect();
    groups.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    groups
}

/// Counts skipped elements per code, sorted by count (descending), then
/// by code.
pub fn group_codes(skipped: &[Skipped]) -> Vec<(&'static str, usize)> {
    let mut groups: Vec<(&'static str, usize)> =
        model::skip::count_by_code(skipped).into_iter().collect();
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
    use model::SkipCode;

    fn outcome() -> Outcome {
        let mut folders = Report::new();
        folders.read();
        folders.converted();
        let mut photos = Report::new();
        for _ in 0..4 {
            photos.read();
        }
        photos.converted();
        photos.skip(SkipCode::Missing, "photo 2: original file is missing");
        photos.skip(SkipCode::Missing, "photo 3: original file is missing");
        photos.skip(
            SkipCode::VirtualCopy,
            "photo 9: virtual copy of photo 1 is not converted",
        );
        let mut collections = Report::new();
        for id in [1, 2] {
            collections.read();
            collections.skip(
                SkipCode::SmartUnreadable,
                format!("collection {id}: smart collection is not converted"),
            );
        }
        collections.read();
        collections.converted();
        let mut collection_members = Report::new();
        for _ in 0..3 {
            collection_members.read();
        }
        collection_members.converted();
        collection_members.skip(
            SkipCode::CollectionSkipped,
            "photo 1: collection skipped (collection 2)",
        );
        collection_members.skip(
            SkipCode::CollectionSkipped,
            "photo 4: collection skipped (collection 2)",
        );
        let mut keywords = Report::new();
        for _ in 0..3 {
            keywords.read();
        }
        keywords.converted();
        keywords.converted();
        keywords.skip(SkipCode::NameEmpty, "keyword 7: name is empty");
        let mut keyword_assignments = Report::new();
        keyword_assignments.read();
        keyword_assignments.skip(
            SkipCode::AssignmentPhotoSkipped,
            "assignment 5: photo 2 is skipped or unknown",
        );
        let mut keyword_synonyms = Report::new();
        for _ in 0..2 {
            keyword_synonyms.read();
        }
        keyword_synonyms.converted();
        keyword_synonyms.skip(SkipCode::KeywordUnknown, "synonym 4: keyword 9 is unknown");
        Outcome {
            catalog_version: "1500000".into(),
            catalog_unchanged: true,
            folders,
            photos,
            videos: 0,
            collections,
            collection_counts: CollectionCounts {
                albums: 1,
                folders: 0,
                duplicate_names: 0,
                quick: 0,
                smart: 0,
            },
            collection_members,
            stacks: Report::new(),
            stack_counts: StackCounts::default(),
            stack_members: Report::new(),
            keywords,
            keyword_attributes: KeywordAttributes {
                persons: 1,
                not_exported: 2,
                no_parents_on_export: 3,
                no_synonyms_on_export: 4,
                unknown_type: 5,
                unused: 6,
                trimmed: 7,
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
                set: "auto",
                none: 1,
                ..LabelCounts::default()
            },
            captions: 0,
            copyrights: 0,
            creators: 0,
            titles: 0,
            titles_unreadable: 0,
            catalog_checksum: [0; 32],
            cloud_only: Default::default(),
            converted: Default::default(),
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
    fn titles_are_in_summary_and_json() {
        let mut o = outcome();
        o.titles = 4;
        o.titles_unreadable = 1;
        let s = summary(&o);
        assert_eq!(s.lines().filter(|l| *l == "titles: 4").count(), 1);
        assert_eq!(
            s.lines().filter(|l| *l == "titles unreadable: 1").count(),
            1
        );
        let v: serde_json::Value = serde_json::from_str(&report_json(&o)).unwrap();
        assert_eq!(v["photos"]["titles"], 4);
        assert_eq!(v["photos"]["titles_unreadable"], 1);
    }

    #[test]
    fn copyrights_and_creators_are_in_summary_and_json() {
        let mut o = outcome();
        o.copyrights = 3;
        o.creators = 2;
        let s = summary(&o);
        assert_eq!(s.lines().filter(|l| *l == "copyrights: 3").count(), 1);
        assert_eq!(s.lines().filter(|l| *l == "creators: 2").count(), 1);
        let v: serde_json::Value = serde_json::from_str(&report_json(&o)).unwrap();
        assert_eq!(v["photos"]["copyrights"], 3);
        assert_eq!(v["photos"]["creators"], 2);
    }

    #[test]
    fn labels_are_in_summary_and_json() {
        let mut o = outcome();
        o.labels = LabelCounts {
            red: 1,
            custom: 2,
            set: "auto",
            matched_sets: vec!["en"],
            custom_ids: vec![3, 8],
            ..LabelCounts::default()
        };
        let text = summary(&o);
        assert!(text.lines().any(|l| l
            == "labels: red 1, yellow 0, green 0, blue 0, purple 0, none 0, custom 2, set auto"));
        // The recommendation line is gone in every case.
        assert!(!text.lines().any(|l| l.contains("--label-set")));
        let v: serde_json::Value = serde_json::from_str(&report_json(&o)).unwrap();
        let l = &v["photos"]["labels"];
        assert_eq!(l["custom"], 2);
        assert_eq!(l["label_set"], "auto");
        assert_eq!(l["matched_sets"], serde_json::json!(["en"]));
        assert_eq!(
            l["custom_reasons"][1]["reason"],
            "photo 8: custom label not in label set"
        );
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
        assert!(lines.contains(&"collections: read 3, would convert 1, skipped 2"));
        assert!(lines.contains(&"skipped collections:"));
        assert!(lines.contains(&"2 x smart collection is not converted"));
        assert!(lines.contains(&"collection members: read 3, would convert 1, skipped 2"));
        assert!(lines.contains(&"skipped collection members:"));
        assert!(
            lines.contains(&"2 x collection skipped (collection 2)"),
            "{text}"
        );
        assert!(!text.contains("albums"));
        assert!(!text.contains("duplicate"));
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
        o.photos.skip(
            SkipCode::Video,
            "photo 5: video files are not supported by LightCraft",
        );
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
                    no synonyms on export 4, unknown type 5, unused 6, \
                    trimmed by LightCraft 7"),
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
        assert_eq!(k["trimmed"], 7);
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
    fn stacks_are_reported_with_their_counts() {
        let mut o = outcome();
        o.stacks.read();
        o.stacks.read();
        o.stacks.converted();
        o.stacks.skip(
            SkipCode::TooFewPhotos,
            "stack 3: fewer than two converted photos",
        );
        o.stack_counts = StackCounts {
            collapsed: 1,
            no_top: 0,
        };
        let text = summary(&o);
        assert!(
            text.lines()
                .any(|l| l == "stacks: read 2, would convert 1, skipped 1")
        );
        assert!(
            text.lines()
                .any(|l| l == "stack members: read 0, would convert 0, skipped 0")
        );
        assert!(
            text.lines()
                .any(|l| l == "stack details: collapsed 1 (written expanded), no top photo 0")
        );
        let v: serde_json::Value = serde_json::from_str(&report_json(&o)).unwrap();
        assert_eq!(v["stacks"]["read"], 2);
        assert_eq!(v["stacks"]["collapsed"], 1);
        assert_eq!(v["stacks"]["no_top"], 0);
        assert_eq!(
            v["stacks"]["skipped"][0]["reason"],
            "stack 3: fewer than two converted photos"
        );
        assert_eq!(v["stack_members"]["read"], 0);
    }

    #[test]
    fn json_has_codes_per_entry_area_and_run() {
        let v: serde_json::Value = serde_json::from_str(&report_json(&outcome())).unwrap();
        assert_eq!(v["photos"]["skipped"][0]["code"], "missing");
        assert_eq!(v["photos"]["skipped"][2]["code"], "virtual_copy");
        assert_eq!(
            v["photos"]["by_code"],
            serde_json::json!({"missing": 2, "virtual_copy": 1})
        );
        assert_eq!(v["folders"]["by_code"], serde_json::json!({}));
        assert_eq!(
            v["skipped_by_code"],
            serde_json::json!({
                "missing": 2,
                "virtual_copy": 1,
                "smart_unreadable": 2,
                "collection_skipped": 2,
                "name_empty": 1,
                "assignment_photo_skipped": 1,
                "keyword_unknown": 1,
            })
        );
    }

    #[test]
    fn summary_counts_codes_after_the_reasons() {
        let text = summary(&outcome());
        let lines: Vec<&str> = text.lines().collect();
        let at = |l: &str| lines.iter().position(|x| *x == l).unwrap();
        let photos = at("skip codes photos:");
        assert_eq!(lines[photos + 1], "       2 x missing");
        assert_eq!(lines[photos + 2], "       1 x virtual_copy");
        assert!(at("skipped keyword synonyms:") < photos);
        assert!(!lines.contains(&"skip codes folders:"));
        assert_eq!(
            lines[at("skip codes collection members:") + 1],
            "       2 x collection_skipped"
        );
    }

    #[test]
    fn codes_are_grouped_by_count_then_code() {
        let items = [
            Skipped::new(SkipCode::Video, "a"),
            Skipped::new(SkipCode::Missing, "b"),
            Skipped::new(SkipCode::VirtualCopy, "c"),
            Skipped::new(SkipCode::VirtualCopy, "d"),
        ];
        assert_eq!(
            group_codes(&items),
            [("virtual_copy", 2), ("missing", 1), ("video", 1)]
        );
        assert!(group_codes(&[]).is_empty());
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
                "collection_members",
                "collections",
                "develop",
                "dry_run",
                "folders",
                "keyword_assignments",
                "keyword_synonyms",
                "keywords",
                "photos",
                "ratings",
                "skipped_by_code",
                "stack_members",
                "stacks",
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
        assert_eq!(v["collections"]["read"], 3);
        assert_eq!(v["collections"]["converted"], 1);
        assert_eq!(v["collections"]["albums"], 1);
        assert_eq!(v["collections"]["folders"], 0);
        assert_eq!(v["collections"]["duplicate_names"], 0);
        assert_eq!(v["collections"]["quick"], 0);
        assert_eq!(v["collections"]["smart"], 0);
        assert_eq!(v["collection_members"]["read"], 3);
        assert_eq!(v["collection_members"]["converted"], 1);
        assert_eq!(
            v["collection_members"]["skipped"][1]["reason"],
            "photo 4: collection skipped (collection 2)"
        );
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
