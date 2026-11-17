//! The run log: one entry per thing that was done, adjusted, skipped, in
//! conflict or failed, gathered from the reader, the plan and the run, with
//! names looked up. An anonymized log keeps ids, codes and texts but drops
//! every name, path and value.

use crate::dry_run::{Names, Outcome};
use crate::guidance::guidance;
use lc_writer::apply::ApplySummary;
use lc_writer::entries::{StepOutcome, step_entries};
use lc_writer::plan::{Conflict, Plan, WriteInput};
use model::{Area, Entry, Report, SkipCode, Skipped, Stage, Status, Subject};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// One entry of `report.json`'s `entries`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LogEntry {
    pub status: &'static str,
    pub area: &'static str,
    pub stage: &'static str,
    /// What the entry is about: a file, keyword or album name, or an id
    /// such as `photo 7` when anonymized or unknown.
    pub item: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<&'static str>,
    /// The value in Lightroom; never in an anonymized log.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The value in LightCraft; never in an anonymized log.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// The photo's Lightroom id, when the entry is about one photo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub photo: Option<i64>,
}

/// A log entry before names are looked up, with its recommendation.
#[derive(Debug, Clone)]
pub struct RawEntry {
    pub entry: Entry,
    pub recommendation: Option<&'static str>,
}

/// The run log before it is written: entries by id, and the names to show.
#[derive(Debug, Clone, Default)]
pub struct Log {
    pub entries: Vec<RawEntry>,
    names: Names,
    by_gid: BTreeMap<String, (i64, PathBuf)>,
}

impl Log {
    /// Adds what taking the run back did: one entry per kind of change.
    pub fn add_rollback(&mut self, r: &lc_writer::rollback::RollbackSummary) {
        use lc_writer::entries::counted;
        let mut push = |status, area, message: String, code, recommendation| {
            let entry =
                Entry::new(status, area, Stage::Rollback, Subject::None, message).code(code);
            self.entries.push(raw(entry, recommendation));
        };
        for (n, area, what) in [
            (r.photos_removed, Area::Photos, "photo"),
            (r.albums_removed, Area::Collections, "album"),
            (r.keywords_removed, Area::Keywords, "keyword"),
            (r.stacks_ungrouped, Area::Stacks, "stack"),
        ] {
            if n > 0 {
                push(
                    Status::Adjusted,
                    area,
                    format!("Run cancelled: {} removed again", counted(n, what)),
                    "rolled_back",
                    None,
                );
            }
        }
        for (n, area, what) in [
            (r.fields_restored, Area::Metadata, "field"),
            (r.members_restored, Area::Collections, "album member"),
            (r.quick_removed, Area::Stacks, "Quick Collection photo"),
        ] {
            if n > 0 {
                push(
                    Status::Adjusted,
                    area,
                    format!("Run cancelled: {} set back", counted(n, what)),
                    "rolled_back",
                    None,
                );
            }
        }
        if r.develop_not_reset > 0 {
            push(
                Status::Error,
                Area::Develop,
                format!(
                    "Run cancelled: development settings of {} stay as written",
                    counted(r.develop_not_reset, "photo")
                ),
                "develop_not_reset",
                Some(
                    "LightCraft cannot set them back over the control port. Use the photo's History in LightCraft if needed.",
                ),
            );
        }
        for command in &r.failed {
            push(
                Status::Error,
                Area::Photos,
                format!("Run cancelled: LightCraft refused `{command}` while taking the run back"),
                "rollback_failed",
                Some("Check the library in LightCraft; the rest was taken back."),
            );
        }
    }

    /// A log without entries that names what `input` and `names` hold, for
    /// rendering entries as they arrive during a run.
    pub fn live(names: Names, input: &WriteInput) -> Log {
        Log {
            entries: Vec::new(),
            by_gid: by_gid(input),
            names,
        }
    }

    /// One entry with names looked up (or left out when `anonymize`).
    pub fn render_entry(&self, entry: &Entry, anonymize: bool) -> LogEntry {
        let namer = Namer {
            names: &self.names,
            by_gid: &self.by_gid,
            anonymize,
        };
        namer.entry(entry.clone(), None)
    }

    /// The entries with names looked up, or with every name, path and value
    /// left out when `anonymize` is set.
    pub fn render(&self, anonymize: bool) -> Vec<LogEntry> {
        let namer = Namer {
            names: &self.names,
            by_gid: &self.by_gid,
            anonymize,
        };
        self.entries
            .iter()
            .map(|raw| namer.entry(raw.entry.clone(), raw.recommendation))
            .collect()
    }
}

/// Looks names up for entries.
struct Namer<'a> {
    names: &'a Names,
    by_gid: &'a BTreeMap<String, (i64, PathBuf)>,
    anonymize: bool,
}

impl Namer<'_> {
    fn photo_name(&self, id: i64, path: Option<&PathBuf>) -> String {
        if self.anonymize {
            return format!("photo {id}");
        }
        path.or_else(|| self.names.photos.get(&id))
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("photo {id}"))
    }

    /// The finished log entry for `entry`.
    fn entry(&self, entry: Entry, recommendation: Option<&'static str>) -> LogEntry {
        let (item, photo) = match &entry.subject {
            Subject::Photo(id) => (self.photo_name(*id, None), Some(*id)),
            Subject::PhotoGlobal(gid) => match self.by_gid.get(gid.as_str()) {
                Some((id, path)) => (self.photo_name(*id, Some(path)), Some(*id)),
                None => ("photo".to_owned(), None),
            },
            Subject::Keyword(path) if !self.anonymize => (path.replace('|', " › "), None),
            Subject::Keyword(_) => ("keyword".to_owned(), None),
            Subject::Collection(id) if !self.anonymize => (
                entry
                    .item
                    .clone()
                    .or_else(|| self.names.collections.get(id).cloned())
                    .unwrap_or_else(|| format!("collection {id}")),
                None,
            ),
            Subject::Collection(id) => (format!("collection {id}"), None),
            Subject::Stack(id) => (format!("stack {id}"), None),
            Subject::Folder(path) if !self.anonymize => (path.clone(), None),
            Subject::Folder(_) => ("folder".to_owned(), None),
            // An area left out is named by its area name, which is no
            // catalog data.
            Subject::None => (
                entry
                    .item
                    .clone()
                    .filter(|_| !self.anonymize || entry.code == Some("not_selected"))
                    .unwrap_or_default(),
                None,
            ),
        };
        let private = |value: Option<String>| value.filter(|_| !self.anonymize);
        LogEntry {
            status: entry.status.as_str(),
            area: entry.area.as_str(),
            stage: entry.stage.as_str(),
            item,
            message: entry.message,
            code: entry.code,
            reason: entry.reason,
            recommendation,
            source: private(entry.source),
            result: private(entry.result),
            photo,
        }
    }
}

/// The entry for one skip, with the guidance of its code. `failed` marks a
/// writer failure (an incomplete element) as an error.
fn skip(skipped: &Skipped, area: Area, stage: Stage, failed: bool) -> RawEntry {
    let g = guidance(skipped.code);
    let status = if failed { Status::Error } else { g.status };
    let area = if skipped.code == SkipCode::VirtualCopy {
        Area::VirtualCopies
    } else {
        area
    };
    let entry = Entry::new(status, area, stage, skipped.subject.clone(), g.message)
        .code(skipped.code.as_str())
        .reason(skipped.reason.clone());
    raw(entry, g.recommendation)
}

fn raw(entry: Entry, recommendation: Option<&'static str>) -> RawEntry {
    RawEntry {
        entry,
        recommendation,
    }
}

/// What the log is built from.
pub struct Sources<'a> {
    pub outcome: &'a Outcome,
    pub input: &'a WriteInput,
    pub plan: &'a Plan,
    /// The run's summary, when it wrote; `None` in a dry run.
    pub applied: Option<&'a ApplySummary>,
    pub not_selected: &'a [&'static str],
    /// Converted photos whose development settings could not be read.
    pub develop_unreadable: &'a [i64],
}

/// The whole log, in run order: reading, areas left out, the steps, then
/// what the writer skipped, failed or found in conflict.
pub fn build(sources: &Sources<'_>, mut names: Names) -> Log {
    // Only needed to build the entries.
    names.develop_unreadable = Vec::new();
    Log {
        entries: entries(sources),
        by_gid: by_gid(sources.input),
        names,
    }
}

fn by_gid(input: &WriteInput) -> BTreeMap<String, (i64, PathBuf)> {
    input
        .photos
        .iter()
        .filter_map(|p| Some((p.global_id.clone()?, (p.lightroom_id, p.path.clone()))))
        .collect()
}

/// Says which photos use each keyword that is created: the count in the
/// message, the file names in `source`.
fn name_keyword_users(out: &mut [RawEntry], input: &WriteInput) {
    let mut users: BTreeMap<String, Vec<&std::path::Path>> = BTreeMap::new();
    for photo in &input.photos {
        for keyword in input
            .photo_keywords
            .get(&photo.lightroom_id)
            .into_iter()
            .flatten()
        {
            users
                .entry(lc_writer::state::keyword_text(keyword))
                .or_default()
                .push(&photo.path);
        }
    }
    for raw in out {
        let e = &mut raw.entry;
        let Subject::Keyword(path) = &e.subject else {
            continue;
        };
        if e.status != Status::Transferred {
            continue;
        }
        let photos = users.get(path).map_or(&[][..], Vec::as_slice);
        e.message = format!(
            "{} · used by {}",
            e.message,
            lc_writer::entries::counted(photos.len(), "photo")
        );
        if !photos.is_empty() {
            let mut names: Vec<String> = photos
                .iter()
                .take(8)
                .filter_map(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .collect();
            if photos.len() > 8 {
                names.push(format!("… ({} more)", photos.len() - 8));
            }
            e.source = Some(names.join(", "));
        }
    }
}

fn entries(sources: &Sources<'_>) -> Vec<RawEntry> {
    let mut out = entries_unnamed(sources);
    name_keyword_users(&mut out, sources.input);
    out
}

fn entries_unnamed(sources: &Sources<'_>) -> Vec<RawEntry> {
    let mut out = Vec::new();
    let o = sources.outcome;
    let read: [(&Report, Area); 9] = [
        (&o.folders, Area::Folders),
        (&o.photos, Area::Photos),
        (&o.keywords, Area::Keywords),
        (&o.keyword_assignments, Area::Keywords),
        (&o.keyword_synonyms, Area::Keywords),
        (&o.collections, Area::Collections),
        (&o.collection_members, Area::Collections),
        (&o.stacks, Area::Stacks),
        (&o.stack_members, Area::Stacks),
    ];
    for (report, area) in read {
        for skipped in report.skipped_items() {
            out.push(skip(skipped, area, Stage::Read, false));
        }
    }
    for name in sources.not_selected {
        let entry = Entry::new(
            Status::Skipped,
            area_of(name),
            Stage::Read,
            Subject::None,
            "Not selected for this run",
        )
        .code("not_selected")
        .item(*name);
        out.push(raw(entry, None));
    }

    let plan = sources.plan;
    match sources.applied {
        None => {
            for step in &plan.steps {
                for entry in step_entries(step, StepOutcome::Planned) {
                    out.push(raw(entry, None));
                }
            }
            let lists: [(&[Skipped], Area, Stage); 6] = [
                (plan.keyword_skips(), Area::Keywords, Stage::Keywords),
                (plan.photo_skips(), Area::Photos, Stage::Photos),
                (plan.member_skips(), Area::Collections, Stage::Organize),
                (plan.stack_skips(), Area::Stacks, Stage::Organize),
                (plan.quick_skips(), Area::Stacks, Stage::Organize),
                (plan.smart_skips(), Area::Collections, Stage::Organize),
            ];
            for (list, area, stage) in lists {
                for skipped in list {
                    out.push(skip(skipped, area, stage, false));
                }
            }
            conflicts(&mut out, plan.conflicts());
        }
        Some(s) => {
            for entry in &s.entries {
                out.push(raw(entry.clone(), None));
            }
            let lists: [(&[Skipped], Area, Stage, bool); 11] = [
                (
                    &s.keywords_not_created,
                    Area::Keywords,
                    Stage::Keywords,
                    false,
                ),
                (&s.photos_skipped, Area::Photos, Stage::Photos, false),
                (&s.photos_incomplete, Area::Photos, Stage::Photos, true),
                (
                    &s.collections_skipped,
                    Area::Collections,
                    Stage::Organize,
                    false,
                ),
                (
                    &s.collections_incomplete,
                    Area::Collections,
                    Stage::Organize,
                    true,
                ),
                (
                    &s.members_skipped,
                    Area::Collections,
                    Stage::Organize,
                    false,
                ),
                (&s.stacks_skipped, Area::Stacks, Stage::Organize, false),
                (&s.stacks_incomplete, Area::Stacks, Stage::Organize, true),
                (&s.quick_skipped, Area::Stacks, Stage::Organize, false),
                (&s.quick_incomplete, Area::Stacks, Stage::Organize, true),
                (&s.smart_skipped, Area::Collections, Stage::Organize, false),
            ];
            for (list, area, stage, failed) in lists {
                for skipped in list {
                    out.push(skip(skipped, area, stage, failed));
                }
            }
            conflicts(&mut out, &s.conflicts);
        }
    }

    for gid in plan.develop_not_applied_ids() {
        let entry = Entry::new(
            Status::Conflict,
            Area::Develop,
            Stage::Photos,
            Subject::PhotoGlobal(gid.clone()),
            "Development settings changed in Lightroom since the last import",
        )
        .code("develop_changed")
        .reason("The development settings in LightCraft are kept; lr2lc never overwrites them silently.");
        out.push(raw(
            entry,
            Some("Turn on “Overwrite photos changed in Lightroom” (--overwrite-develop) to write the Lightroom settings."),
        ));
    }
    if !sources.not_selected.contains(&"develop") {
        for photo in &sources.input.photos {
            let Some(develop) = &photo.develop else {
                continue;
            };
            if develop.unmapped.is_empty() {
                continue;
            }
            let mut fields = develop.unmapped.clone();
            fields.sort();
            fields.dedup();
            let entry = Entry::new(
                Status::Skipped,
                Area::Develop,
                Stage::Photos,
                Subject::Photo(photo.lightroom_id),
                format!(
                    "{} not carried over",
                    lc_writer::entries::counted(fields.len(), "development field")
                ),
            )
            .code("develop_unmapped")
            .reason(format!(
                "LightCraft has no exact equivalent for: {}.",
                fields.join(", ")
            ));
            out.push(raw(entry, None));
        }
    }
    for id in sources.develop_unreadable {
        let entry = Entry::new(
            Status::Error,
            Area::Develop,
            Stage::Read,
            Subject::Photo(*id),
            "Development settings cannot be read; the photo is transferred without them",
        )
        .code("develop_unreadable")
        .reason("The settings text in the catalog is damaged, of an unexpected kind or too large.");
        out.push(raw(
            entry,
            Some("Open the photo in Lightroom and save its settings again, then run again."),
        ));
    }
    for removed in plan.removed_photos() {
        let entry = Entry::new(
            Status::Skipped,
            Area::Photos,
            Stage::Photos,
            Subject::Photo(removed.photo),
            "No longer in Lightroom; kept in LightCraft",
        )
        .code("photo_removed");
        out.push(raw(entry, None));
    }
    for moved in plan.moved_photos() {
        let entry = Entry::new(
            Status::Skipped,
            Area::Photos,
            Stage::Photos,
            Subject::Photo(moved.photo),
            "File moved in Lightroom; not imported again",
        )
        .code("photo_moved");
        out.push(raw(
            entry,
            Some("Relink the photo in LightCraft (Find Missing Photos) if its file moved."),
        ));
    }
    for renamed in plan.renamed_keywords() {
        let entry = Entry::new(
            Status::Skipped,
            Area::Keywords,
            Stage::Keywords,
            Subject::None,
            format!(
                "Keyword {} renamed in Lightroom; not renamed in LightCraft",
                renamed.keyword
            ),
        )
        .code("keyword_renamed");
        out.push(raw(
            entry,
            Some("Rename the keyword in LightCraft if needed."),
        ));
    }
    out
}

fn conflicts(out: &mut Vec<RawEntry>, list: &[Conflict]) {
    for c in list {
        let area = match c.field.as_str() {
            "rating" => Area::Ratings,
            "flag" => Area::Flags,
            "label" => Area::Labels,
            _ => Area::Metadata,
        };
        let entry = Entry::new(
            Status::Conflict,
            area,
            Stage::Photos,
            Subject::Photo(c.photo),
            format!(
                "{} changed in Lightroom and in LightCraft",
                field_name(&c.field)
            ),
        )
        .code("field_conflict")
        .reason("Both sides changed since the last import. lr2lc keeps the value in LightCraft.")
        .source(c.lightroom.clone())
        .result(c.lightcraft.clone());
        out.push(raw(
            entry,
            Some("Change it in LightCraft by hand if the Lightroom value should win."),
        ));
    }
}

fn field_name(field: &str) -> &'static str {
    match field {
        "rating" => "Star rating",
        "flag" => "Flag",
        "label" => "Color label",
        "title" => "Title",
        "caption" => "Caption",
        "copyright" => "Copyright",
        "creator" => "Creator",
        _ => "Field",
    }
}

fn area_of(name: &str) -> Area {
    match name {
        "keywords" => Area::Keywords,
        "ratings" => Area::Ratings,
        "labels" => Area::Labels,
        "flags" => Area::Flags,
        "texts" => Area::Metadata,
        "collections" => Area::Collections,
        "stacks" => Area::Stacks,
        "virtual-copies" => Area::VirtualCopies,
        _ => Area::Develop,
    }
}

/// Photos counted once each, however many entries they have.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PhotoCounts {
    /// Photos read from the catalog.
    pub total: usize,
    /// Something was (or in a dry run would be) written for the photo and
    /// nothing went wrong.
    pub transferred: usize,
    /// Already in the library with nothing to change.
    pub unchanged: usize,
    /// Not transferred at all (video, missing original, ...).
    pub skipped: usize,
    /// At least one field in conflict, nothing failed.
    pub conflict: usize,
    /// At least one step failed.
    pub error: usize,
}

impl Log {
    /// Counts the catalog's `total` photos by what happened to each.
    pub fn photo_counts(&self, total: usize) -> PhotoCounts {
        // 1 written, 2 skipped as a whole, 3 conflict, 4 error; the worst wins.
        let mut worst: BTreeMap<i64, u8> = BTreeMap::new();
        for raw in &self.entries {
            let e = &raw.entry;
            let id = match &e.subject {
                Subject::Photo(id) => *id,
                Subject::PhotoGlobal(gid) => match self.by_gid.get(gid) {
                    Some((id, _)) => *id,
                    None => continue,
                },
                _ => continue,
            };
            let whole_photo = matches!(e.area, Area::Photos | Area::VirtualCopies)
                && !matches!(e.code, Some("photo_removed" | "photo_moved"));
            let rank = match e.status {
                Status::Error => 4,
                Status::Conflict => 3,
                Status::Skipped if whole_photo => 2,
                Status::Skipped => 0,
                Status::Transferred | Status::Adjusted => 1,
            };
            let slot = worst.entry(id).or_insert(0);
            *slot = (*slot).max(rank);
        }
        let count = |rank: u8| worst.values().filter(|r| **r == rank).count();
        let (transferred, skipped, conflict, error) = (count(1), count(2), count(3), count(4));
        PhotoCounts {
            total,
            transferred,
            unchanged: total.saturating_sub(transferred + skipped + conflict + error),
            skipped,
            conflict,
            error,
        }
    }
}

/// Counts per status, for the report and the summary.
pub fn counts(entries: &[RawEntry]) -> BTreeMap<&'static str, usize> {
    let mut out: BTreeMap<&'static str, usize> =
        Status::ALL.iter().map(|s| (s.as_str(), 0)).collect();
    for e in entries {
        *out.entry(e.entry.status.as_str()).or_insert(0) += 1;
    }
    out
}
