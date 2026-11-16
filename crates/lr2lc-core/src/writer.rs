//! The writer part of a run: the plan in a dry run, writing through the
//! LightCraft control port with `--apply`.

use crate::dry_run::Converted;
use crate::output::group_reasons;
use anyhow::{Context, Result};
use lc_writer::apply::{Aborted, ApplySummary, apply_partial_with};
use lc_writer::control::{CommandSink, ControlClient, TimedSink, Timing, WriteError, preflight};
use lc_writer::plan::{Conflict, MovedPhoto, Plan, RemovedPhoto, RenamedKeyword, Step, WriteInput};
use lc_writer::progress::Progress;
use lc_writer::state::{State, state_path};
use model::Skipped;
use model::skip::count_by_code;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

/// The `writer` section of `report.json`.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum WriterJson {
    /// Dry run: what would be sent.
    Planned(Box<PlannedSectionJson>),
    /// `--apply`: what was sent.
    Applied(Box<AppliedJson>),
}

/// The top-level `develop` section of `report.json`.
#[derive(Debug, Default, Serialize)]
pub struct DevelopJson {
    /// Converted photos that carry development settings.
    pub photos: usize,
    /// Photos the plan sends develop commands for.
    pub planned: usize,
    /// Photos whose develop commands all succeeded (0 in a dry run).
    pub written: usize,
    /// Distinct field names that could not be carried over, sorted, at most
    /// [`UNMAPPED_LIMIT`] entries. Names only, never values.
    pub unmapped: Vec<String>,
    /// Number of distinct unmapped field names, also above the listing cap.
    pub unmapped_count: usize,
    /// Every unmapped field name with the number of converted photos that
    /// carry it, sorted by name, not capped. Names and counts only.
    pub unmapped_photos: BTreeMap<String, usize>,
    /// Known photos whose settings changed in Lightroom and were left alone
    /// because `--overwrite-develop` was not given.
    pub not_applied: usize,
    /// Photos written because `--overwrite-develop` was given.
    pub overwritten: usize,
    /// Converted photos per camera profile name, sorted, at most
    /// [`CAMERA_PROFILES_LIMIT`] entries. Names and counts only.
    pub camera_profiles: BTreeMap<String, usize>,
    /// Number of distinct camera profile names, also above the listing cap.
    pub camera_profiles_count: usize,
    /// Converted photos that carry a non-empty camera profile name.
    pub photos_with_profile: usize,
    /// Converted photos with at least one mask.
    pub masks: usize,
    /// Converted photos with at least one spot.
    pub spots: usize,
    /// Converted photos with a custom white balance (temperature or tint
    /// with mode `custom`). A named mode such as As Shot is carried by the
    /// mode alone and not counted. Only the count, never a value or a photo
    /// id.
    pub white_balance_custom: usize,
    /// Dry run: planned Kelvin white balances whose transfer depends on
    /// LightCraft's scale for the photo, which only `--apply` can ask.
    /// Always 0 in an applied run.
    pub white_balance_to_check: usize,
    /// Applied run: photos whose custom white balance was written.
    pub white_balance_written: usize,
    /// Applied run: photos whose Kelvin white balance was not written
    /// because LightCraft edits them on a relative scale.
    pub white_balance_not_carried: usize,
}

/// The white-balance counts of the `develop` section.
#[derive(Debug, Clone, Copy, Default)]
struct WhiteBalanceCounts {
    custom: usize,
    to_check: usize,
    written: usize,
    not_carried: usize,
}

/// At most this many unmapped field names are listed in the report.
pub const UNMAPPED_LIMIT: usize = 20;

/// At most this many camera profile names are listed in the report.
pub const CAMERA_PROFILES_LIMIT: usize = 20;

/// The `develop` section of a dry run.
pub fn planned_develop(plan: &Plan) -> DevelopJson {
    develop_json(
        plan.develop_photos(),
        plan.develop_planned(),
        0,
        plan.develop_unmapped(),
        plan.develop_not_applied(),
        plan.develop_overwritten(),
        plan.develop_camera_profiles(),
        plan.develop_photos_with_profile(),
        plan.develop_masks(),
        plan.develop_spots(),
        WhiteBalanceCounts {
            custom: plan.develop_white_balance_custom(),
            to_check: plan.develop_white_balance_to_check(),
            ..WhiteBalanceCounts::default()
        },
    )
}

/// The `develop` section of an applied run.
pub fn applied_develop(s: &ApplySummary) -> DevelopJson {
    develop_json(
        s.develop_photos,
        s.develop_planned,
        s.develop_written,
        &s.develop_unmapped,
        s.develop_not_applied,
        s.develop_overwritten,
        &s.develop_camera_profiles,
        s.develop_photos_with_profile,
        s.develop_masks,
        s.develop_spots,
        WhiteBalanceCounts {
            custom: s.develop_white_balance_custom,
            to_check: 0,
            written: s.develop_white_balance_written,
            not_carried: s.develop_white_balance_not_carried,
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn develop_json(
    photos: usize,
    planned: usize,
    written: usize,
    unmapped: &BTreeMap<String, usize>,
    not_applied: usize,
    overwritten: usize,
    camera_profiles: &BTreeMap<String, usize>,
    photos_with_profile: usize,
    masks: usize,
    spots: usize,
    wb: WhiteBalanceCounts,
) -> DevelopJson {
    DevelopJson {
        photos,
        planned,
        written,
        unmapped: unmapped.keys().take(UNMAPPED_LIMIT).cloned().collect(),
        unmapped_count: unmapped.len(),
        unmapped_photos: unmapped.clone(),
        not_applied,
        overwritten,
        camera_profiles: camera_profiles
            .iter()
            .take(CAMERA_PROFILES_LIMIT)
            .map(|(name, count)| (name.clone(), *count))
            .collect(),
        camera_profiles_count: camera_profiles.len(),
        photos_with_profile,
        masks,
        spots,
        white_balance_custom: wb.custom,
        white_balance_to_check: wb.to_check,
        white_balance_written: wb.written,
        white_balance_not_carried: wb.not_carried,
    }
}

/// Formats the `develop` counts as one summary line. Names only, never
/// values.
fn develop_line(out: &mut String, d: &DevelopJson) {
    let _ = writeln!(
        out,
        "develop: photos with settings {}, commands planned {}, written {}, \
         unmapped fields {} ({} listed), changed and not applied {}, overwritten {}, \
         photos with camera profile {}, photos with masks {}, photos with spots {}, \
         custom white balance {} (to check {}, written {}, not carried {})",
        d.photos,
        d.planned,
        d.written,
        d.unmapped_count,
        d.unmapped.len(),
        d.not_applied,
        d.overwritten,
        d.photos_with_profile,
        d.masks,
        d.spots,
        d.white_balance_custom,
        d.white_balance_to_check,
        d.white_balance_written,
        d.white_balance_not_carried
    );
}

/// The `writer` section of a dry run.
#[derive(Debug, Serialize)]
pub struct PlannedSectionJson {
    planned: PlannedJson,
    photos_skipped: Vec<SkipJson>,
    photos_skipped_by_code: ByCode,
    keywords_not_created: Vec<SkipJson>,
    keywords_not_created_by_code: ByCode,
    members_skipped: Vec<SkipJson>,
    members_skipped_by_code: ByCode,
    stacks_skipped: Vec<SkipJson>,
    stacks_skipped_by_code: ByCode,
    quick_skipped: Vec<SkipJson>,
    quick_skipped_by_code: ByCode,
    smart_skipped: Vec<SkipJson>,
    smart_skipped_by_code: ByCode,
}

/// The `writer` section of an applied run.
#[derive(Debug, Serialize)]
pub struct AppliedJson {
    keywords_created: usize,
    photos_imported: usize,
    photos_existing: usize,
    /// Imported photos whose file has the same bytes as another photo; each
    /// became its own record (a copy linked to its own file).
    photos_same_content: usize,
    photos_updated: usize,
    photos_unchanged: usize,
    updates: usize,
    not_saved: usize,
    /// Fields changed differently in Lightroom and LightCraft since the last
    /// import; nothing was written for them. Only the Lightroom id and the
    /// field name, never a value.
    conflicts: Vec<Conflict>,
    conflicts_count: usize,
    /// Keyword memberships that left the photo in Lightroom; kept in
    /// LightCraft and only counted.
    keywords_removed: usize,
    /// Recorded photos no longer in the Lightroom model; only their ids.
    /// Nothing was deleted in LightCraft.
    photos_removed: usize,
    removed_photos: Vec<RemovedPhoto>,
    /// Recorded photos whose original moved; only their ids. Not imported
    /// again.
    photos_moved: usize,
    moved_photos: Vec<MovedPhoto>,
    /// Recorded keywords renamed in Lightroom; only their ids. Not created
    /// or renamed in LightCraft.
    keywords_renamed: usize,
    renamed_keywords: Vec<RenamedKeyword>,
    photos_skipped: Vec<SkipJson>,
    photos_skipped_by_code: ByCode,
    photos_incomplete: Vec<SkipJson>,
    photos_incomplete_by_code: ByCode,
    keywords_not_created: Vec<SkipJson>,
    keywords_not_created_by_code: ByCode,
    folders_created: usize,
    albums_created: usize,
    collections_known: usize,
    collections_existing: usize,
    members_added: usize,
    members_removed: usize,
    collections_renamed: usize,
    collections_moved: usize,
    collections_removed: usize,
    collections_skipped: Vec<SkipJson>,
    collections_skipped_by_code: ByCode,
    collections_incomplete: Vec<SkipJson>,
    collections_incomplete_by_code: ByCode,
    members_skipped: Vec<SkipJson>,
    members_skipped_by_code: ByCode,
    stacks_grouped: usize,
    stacks_known: usize,
    stack_members_removed: usize,
    stacks_skipped: Vec<SkipJson>,
    stacks_skipped_by_code: ByCode,
    stacks_incomplete: Vec<SkipJson>,
    stacks_incomplete_by_code: ByCode,
    quick_added: usize,
    quick_already: usize,
    quick_removed: usize,
    quick_skipped: Vec<SkipJson>,
    quick_skipped_by_code: ByCode,
    quick_incomplete: Vec<SkipJson>,
    quick_incomplete_by_code: ByCode,
    smart_created: usize,
    smart_known: usize,
    smart_existing: usize,
    smart_rules_changed: usize,
    smart_no_longer_translatable: usize,
    smart_skipped: Vec<SkipJson>,
    smart_skipped_by_code: ByCode,
    /// Why the run stopped early; absent after a complete run.
    #[serde(skip_serializing_if = "Option::is_none")]
    aborted: Option<String>,
}

/// Number of planned commands by kind, and the known photos with and
/// without updates. Every converted photo counts once in `photos`,
/// `photos_updated`, `photos_unchanged` or the writer's skipped photos;
/// every converted collection once in `folders`, `albums` or
/// `collections_known`; every converted smart collection once in
/// `smart_albums`, `smart_known` or the writer's skipped smart collections;
/// every converted stack once in `stacks`,
/// `stacks_known` or the writer's skipped stacks. `members_added` and
/// `members_removed` count photos.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PlannedJson {
    pub keywords: usize,
    pub photos: usize,
    pub updates: usize,
    pub photos_updated: usize,
    pub photos_unchanged: usize,
    pub folders: usize,
    pub albums: usize,
    pub members_added: usize,
    pub members_removed: usize,
    pub collections_known: usize,
    /// Recorded collections that no longer exist in Lightroom; only
    /// counted. Recorded smart collections that still exist are never
    /// counted here.
    pub collections_removed: usize,
    /// Stacks to group (new or changed).
    pub stacks: usize,
    pub stacks_known: usize,
    /// Recorded stack members that left their stack in Lightroom; only
    /// counted.
    pub stack_members_removed: usize,
    /// Quick Collection members to add (LightCraft may already hold some;
    /// they are not sent).
    pub quick_add: usize,
    /// Quick Collection members already recorded in the mapping file.
    pub quick_already: usize,
    /// Recorded Quick Collection members that left it in Lightroom; only
    /// counted.
    pub quick_removed: usize,
    /// Smart albums to create.
    pub smart_albums: usize,
    /// Smart collections already in the mapping file.
    pub smart_known: usize,
    /// Known smart collections whose rules changed in Lightroom; only
    /// counted.
    pub smart_rules_changed: usize,
    /// Recorded smart albums whose smart collection still exists in
    /// Lightroom but no longer translates; only counted.
    pub smart_no_longer_translatable: usize,
}

/// The writer input from the converted elements of the catalog.
///
/// Takes the elements by value: they are not needed again after the plan is
/// built, and copying them would duplicate every photo, keyword and keyword
/// assignment on a large catalog.
pub fn input(converted: Converted) -> WriteInput {
    WriteInput {
        photos: converted.photos,
        keywords: converted.keywords,
        photo_keywords: converted.photo_keywords,
        collections: converted.collections,
        stacks: converted.stacks,
        quick: converted.quick,
        smart: converted.smart,
        smart_untranslatable_ids: converted.smart_untranslatable_ids,
        // Set from the command line before the plan is built.
        overwrite_develop: false,
        // A dry run never connects; an apply run reads the live state itself.
        live: None,
        // Set from the job before the plan is built.
        areas: lc_writer::plan::Areas::ALL,
    }
}

/// The mapping file of `library`, or the empty state without a library.
/// Only reads; a missing file is the empty state.
pub fn load_state(library: Option<&Path>) -> Result<State> {
    match library {
        Some(library) => State::load(&state_path(library)).context("cannot read the mapping file"),
        None => Ok(State::default()),
    }
}

/// Counts the steps of a plan by kind.
pub fn planned(plan: &Plan) -> PlannedJson {
    let mut out = PlannedJson {
        photos_updated: plan.photos_updated(),
        photos_unchanged: plan.photos_unchanged(),
        collections_known: plan.collections_known(),
        collections_removed: plan.collections_removed(),
        stacks_known: plan.stacks_known(),
        stack_members_removed: plan.stack_members_removed(),
        quick_already: plan.quick_already(),
        quick_removed: plan.quick_removed(),
        smart_known: plan.smart_known(),
        smart_rules_changed: plan.smart_rules_changed(),
        smart_no_longer_translatable: plan.smart_no_longer_translatable(),
        ..PlannedJson::default()
    };
    for step in &plan.steps {
        match step {
            Step::CreateKeyword { .. } => out.keywords += 1,
            Step::Import { .. } => out.photos += 1,
            Step::VirtualCopy { .. } => out.photos += 1,
            Step::Update { .. } => out.updates += 1,
            Step::CreateAlbum { params, .. } => {
                if params["folder"] == true {
                    out.folders += 1;
                } else {
                    out.albums += 1;
                }
            }
            Step::AddMembers { gids, .. } => out.members_added += gids.len(),
            Step::RemoveMembers { gids, .. } => out.members_removed += gids.len(),
            Step::GroupStack { .. } => out.stacks += 1,
            Step::QuickAdd { gids } => out.quick_add += gids.len(),
            Step::CreateSmart { .. } => out.smart_albums += 1,
            // Counted in the develop section, not in `planned`.
            Step::DevelopSet { .. }
            | Step::DevelopMerge { .. }
            | Step::DevelopWhiteBalance { .. } => {}
        }
    }
    out
}

/// The report section of a dry run.
pub fn planned_json(plan: &Plan) -> WriterJson {
    WriterJson::Planned(Box::new(PlannedSectionJson {
        planned: planned(plan),
        photos_skipped: reasons(plan.photo_skips()),
        photos_skipped_by_code: count_by_code(plan.photo_skips()),
        keywords_not_created: reasons(plan.keyword_skips()),
        keywords_not_created_by_code: count_by_code(plan.keyword_skips()),
        members_skipped: reasons(plan.member_skips()),
        members_skipped_by_code: count_by_code(plan.member_skips()),
        stacks_skipped: reasons(plan.stack_skips()),
        stacks_skipped_by_code: count_by_code(plan.stack_skips()),
        quick_skipped: reasons(plan.quick_skips()),
        quick_skipped_by_code: count_by_code(plan.quick_skips()),
        smart_skipped: reasons(plan.smart_skips()),
        smart_skipped_by_code: count_by_code(plan.smart_skips()),
    }))
}

/// The report section of an applied run; `aborted` is the error that
/// stopped it early.
pub fn applied_json(s: &ApplySummary, aborted: Option<&WriteError>) -> WriterJson {
    WriterJson::Applied(Box::new(AppliedJson {
        keywords_created: s.keywords_created,
        photos_imported: s.photos_imported,
        photos_existing: s.photos_existing,
        photos_same_content: s.photos_same_content,
        photos_updated: s.photos_updated,
        photos_unchanged: s.photos_unchanged,
        updates: s.updates,
        not_saved: s.not_saved,
        conflicts: s.conflicts.clone(),
        conflicts_count: s.conflicts.len(),
        keywords_removed: s.keywords_removed,
        photos_removed: s.photos_removed,
        removed_photos: s.removed_photos.clone(),
        photos_moved: s.photos_moved,
        moved_photos: s.moved_photos.clone(),
        keywords_renamed: s.keywords_renamed,
        renamed_keywords: s.renamed_keywords.clone(),
        photos_skipped: reasons(&s.photos_skipped),
        photos_skipped_by_code: count_by_code(&s.photos_skipped),
        photos_incomplete: reasons(&s.photos_incomplete),
        photos_incomplete_by_code: count_by_code(&s.photos_incomplete),
        keywords_not_created: reasons(&s.keywords_not_created),
        keywords_not_created_by_code: count_by_code(&s.keywords_not_created),
        folders_created: s.folders_created,
        albums_created: s.albums_created,
        collections_known: s.collections_known,
        collections_existing: s.collections_existing,
        members_added: s.members_added,
        members_removed: s.members_removed,
        collections_renamed: s.collections_renamed,
        collections_moved: s.collections_moved,
        collections_removed: s.collections_removed,
        collections_skipped: reasons(&s.collections_skipped),
        collections_skipped_by_code: count_by_code(&s.collections_skipped),
        collections_incomplete: reasons(&s.collections_incomplete),
        collections_incomplete_by_code: count_by_code(&s.collections_incomplete),
        members_skipped: reasons(&s.members_skipped),
        members_skipped_by_code: count_by_code(&s.members_skipped),
        stacks_grouped: s.stacks_grouped,
        stacks_known: s.stacks_known,
        stack_members_removed: s.stack_members_removed,
        stacks_skipped: reasons(&s.stacks_skipped),
        stacks_skipped_by_code: count_by_code(&s.stacks_skipped),
        stacks_incomplete: reasons(&s.stacks_incomplete),
        stacks_incomplete_by_code: count_by_code(&s.stacks_incomplete),
        quick_added: s.quick_added,
        quick_already: s.quick_already,
        quick_removed: s.quick_removed,
        quick_skipped: reasons(&s.quick_skipped),
        quick_skipped_by_code: count_by_code(&s.quick_skipped),
        quick_incomplete: reasons(&s.quick_incomplete),
        quick_incomplete_by_code: count_by_code(&s.quick_incomplete),
        smart_created: s.smart_created,
        smart_known: s.smart_known,
        smart_existing: s.smart_existing,
        smart_rules_changed: s.smart_rules_changed,
        smart_no_longer_translatable: s.smart_no_longer_translatable,
        smart_skipped: reasons(&s.smart_skipped),
        smart_skipped_by_code: count_by_code(&s.smart_skipped),
        aborted: aborted.map(WriteError::short_reason),
    }))
}

/// One element the writer skipped or left incomplete.
#[derive(Debug, Serialize)]
pub struct SkipJson {
    pub reason: String,
    pub code: &'static str,
}

/// Entries of a writer list per code.
pub type ByCode = BTreeMap<&'static str, usize>;

fn reasons(items: &[Skipped]) -> Vec<SkipJson> {
    items
        .iter()
        .map(|s| SkipJson {
            reason: s.reason.clone(),
            code: s.code.as_str(),
        })
        .collect()
}

/// The readable summary of a dry run's plan. `albums to create` counts
/// albums only, not album folders.
pub fn planned_summary(plan: &Plan) -> String {
    let p = planned(plan);
    let mut out = format!(
        "writer: keywords to create {}, photos to import {}, updates {}, \
         albums to create {}, album members to add {}\n",
        p.keywords, p.photos, p.updates, p.albums, p.members_added
    );
    skipped_lines(&mut out, "writer would skip photos", plan.photo_skips());
    skipped_lines(
        &mut out,
        "writer would not create keywords",
        plan.keyword_skips(),
    );
    skipped_lines(
        &mut out,
        "writer would skip album members",
        plan.member_skips(),
    );
    let _ = writeln!(
        out,
        "writer stacks: to group {}, already in the library {}, members left in LightCraft {}",
        p.stacks, p.stacks_known, p.stack_members_removed
    );
    skipped_lines(&mut out, "writer would skip stacks", plan.stack_skips());
    let _ = writeln!(
        out,
        "writer quick collection: to add {}, already in it {}, members left in LightCraft {}",
        p.quick_add, p.quick_already, p.quick_removed
    );
    skipped_lines(
        &mut out,
        "writer would skip quick collection members",
        plan.quick_skips(),
    );
    let _ = writeln!(
        out,
        "writer smart albums: smart albums to create {}, already in the library {}, \
         rules changed in Lightroom {}",
        p.smart_albums, p.smart_known, p.smart_rules_changed
    );
    let _ = writeln!(
        out,
        "writer smart albums no longer translatable (left as is in LightCraft): {}",
        p.smart_no_longer_translatable
    );
    skipped_lines(
        &mut out,
        "writer would skip smart collections",
        plan.smart_skips(),
    );
    develop_line(&mut out, &planned_develop(plan));
    out
}

/// The readable summary of an applied run.
pub fn applied_summary(s: &ApplySummary) -> String {
    let mut out = format!(
        "written: keywords created {}, photos imported {} (same file content as another photo {}), \
         photos already in the library {}, photos updated {}, photos unchanged {}, updates {}, \
         not yet saved by LightCraft {}\n",
        s.keywords_created,
        s.photos_imported,
        s.photos_same_content,
        s.photos_existing,
        s.photos_updated,
        s.photos_unchanged,
        s.updates,
        s.not_saved
    );
    let _ = writeln!(
        out,
        "written collections: folders created {}, albums created {}, already in the library {}, \
         found in the library and adopted {}, album members added {}, album members removed {}",
        s.folders_created,
        s.albums_created,
        s.collections_known,
        s.collections_existing,
        s.members_added,
        s.members_removed
    );
    let _ = writeln!(
        out,
        "not written: photos skipped {}, photos incomplete {}, keywords not created {}, \
         collections skipped {}, collections incomplete {}, album members skipped {}",
        s.photos_skipped.len(),
        s.photos_incomplete.len(),
        s.keywords_not_created.len(),
        s.collections_skipped.len(),
        s.collections_incomplete.len(),
        s.members_skipped.len()
    );
    let _ = writeln!(
        out,
        "keywords removed in Lightroom and kept in LightCraft: {}",
        s.keywords_removed
    );
    let _ = writeln!(
        out,
        "changed in Lightroom, kept in LightCraft: photos removed {}, \
         keywords renamed {}, files moved {}",
        s.photos_removed, s.keywords_renamed, s.photos_moved
    );
    if !s.conflicts.is_empty() {
        let _ = writeln!(
            out,
            "changed differently in Lightroom and LightCraft, nothing written:"
        );
        for conflict in &s.conflicts {
            let _ = writeln!(out, "  photo {}: {}", conflict.photo, conflict.field);
        }
    }
    let _ = writeln!(
        out,
        "written stacks: grouped {}, already in the library {}, skipped {}, incomplete {}, \
         members left in LightCraft {}",
        s.stacks_grouped,
        s.stacks_known,
        s.stacks_skipped.len(),
        s.stacks_incomplete.len(),
        s.stack_members_removed
    );
    let _ = writeln!(
        out,
        "written quick collection: added {}, already in it {}, skipped {}, incomplete {}, \
         members left in LightCraft {}",
        s.quick_added,
        s.quick_already,
        s.quick_skipped.len(),
        s.quick_incomplete.len(),
        s.quick_removed
    );
    let _ = writeln!(
        out,
        "written smart albums: created {}, already in the library {}, \
         found in the library and adopted {}, skipped {}, \
         rules changed in Lightroom (left as is) {}",
        s.smart_created,
        s.smart_known,
        s.smart_existing,
        s.smart_skipped.len(),
        s.smart_rules_changed
    );
    let _ = writeln!(
        out,
        "smart albums no longer translatable (left as is in LightCraft): {}",
        s.smart_no_longer_translatable
    );
    if s.collections_renamed + s.collections_moved + s.collections_removed > 0 {
        let _ = writeln!(
            out,
            "changed in Lightroom, left as is in LightCraft: collections renamed {}, moved {}, \
             removed {}",
            s.collections_renamed, s.collections_moved, s.collections_removed
        );
    }
    skipped_lines(&mut out, "skipped photos", &s.photos_skipped);
    skipped_lines(&mut out, "incomplete photos", &s.photos_incomplete);
    skipped_lines(&mut out, "keywords not created", &s.keywords_not_created);
    skipped_lines(&mut out, "skipped collections", &s.collections_skipped);
    skipped_lines(
        &mut out,
        "incomplete collections",
        &s.collections_incomplete,
    );
    skipped_lines(&mut out, "skipped album members", &s.members_skipped);
    skipped_lines(&mut out, "skipped stacks", &s.stacks_skipped);
    skipped_lines(&mut out, "incomplete stacks", &s.stacks_incomplete);
    skipped_lines(&mut out, "skipped quick collection", &s.quick_skipped);
    skipped_lines(&mut out, "incomplete quick collection", &s.quick_incomplete);
    skipped_lines(&mut out, "skipped smart collections", &s.smart_skipped);
    develop_line(&mut out, &applied_develop(s));
    out
}

fn skipped_lines(out: &mut String, title: &str, items: &[Skipped]) {
    let groups = group_reasons(items);
    if groups.is_empty() {
        return;
    }
    let _ = writeln!(out, "{title}:");
    for (reason, count) in groups {
        let _ = writeln!(out, "  {count:>6} x {reason}");
    }
}

/// Connection settings of `--apply`.
pub struct Target<'a> {
    pub library: &'a Path,
    pub port: u16,
    pub allow_xmp: bool,
}

/// Connects, runs the pre-flight checks and writes `input`. A run that
/// stops hands back the numbers up to the stop; they are all 0 when the
/// connection or the pre-flight checks fail.
pub fn write(
    target: &Target<'_>,
    input: &WriteInput,
    state: State,
    progress: &mut dyn Progress,
    timing: &mut Timing,
) -> Result<ApplySummary, Aborted> {
    let mut client = ControlClient::connect(target.port).map_err(before_writing)?;
    write_to(&mut client, target, input, state, progress, timing)
}

/// Like [`write`], through `sink` instead of a new control-port
/// connection (`target.port` is not used).
pub fn write_to(
    sink: &mut dyn CommandSink,
    target: &Target<'_>,
    input: &WriteInput,
    mut state: State,
    progress: &mut dyn Progress,
    timing: &mut Timing,
) -> Result<ApplySummary, Aborted> {
    let state_file = state_path(target.library);
    let mut sink = TimedSink::new(sink);
    let result = preflight(&mut sink, target.library, target.allow_xmp)
        .map_err(before_writing)
        .and_then(|()| apply_partial_with(&mut sink, input, &mut state, &state_file, progress));
    *timing = sink.timing;
    result
}

/// A stop before anything was written: all numbers are 0.
fn before_writing(error: WriteError) -> Aborted {
    Aborted {
        error,
        summary: Box::default(),
    }
}

/// The error message of a run that stopped, with a hint what to do.
pub fn stop_error(e: &WriteError) -> anyhow::Error {
    let hint = match e {
        WriteError::ConnectionLost { .. } => {
            "; outcome unknown, run again; lr2lc re-checks the state"
        }
        WriteError::Connect { .. } => "; is LightCraft running with the control port enabled?",
        _ => "",
    };
    anyhow::anyhow!("writing to LightCraft stopped: {e}{hint}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{Develop, Photo, Rating, SkipCode, WhiteBalanceHint};

    fn plan_of(photos: Vec<Photo>) -> Plan {
        let input = WriteInput {
            photos,
            ..WriteInput::default()
        };
        lc_writer::plan::plan(&input, &State::default(), &Default::default())
    }

    #[test]
    fn planned_counts_steps_by_kind() {
        let p = plan_of(vec![
            Photo::new("a.jpg", 1, Rating::new(2).unwrap()).with_global_id("a"),
            Photo::new("b.jpg", 2, Rating::default()).with_global_id("b"),
            Photo::new("c.jpg", 3, Rating::default()),
        ]);
        assert_eq!(
            planned(&p),
            PlannedJson {
                keywords: 0,
                photos: 2,
                updates: 1,
                ..PlannedJson::default()
            }
        );
        let text = planned_summary(&p);
        assert!(text.starts_with(
            "writer: keywords to create 0, photos to import 2, updates 1, \
             albums to create 0, album members to add 0\n"
        ));
        assert!(text.contains("writer would skip photos:"));
    }

    #[test]
    fn json_sections_have_the_documented_keys() {
        let v = serde_json::to_value(planned_json(&plan_of(vec![]))).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "planned": {"keywords": 0, "photos": 0, "updates": 0,
                        "photos_updated": 0, "photos_unchanged": 0,
                        "folders": 0, "albums": 0, "members_added": 0,
                        "members_removed": 0, "collections_known": 0,
                        "collections_removed": 0,
                        "stacks": 0, "stacks_known": 0, "stack_members_removed": 0,
                        "quick_add": 0, "quick_already": 0, "quick_removed": 0,
                        "smart_albums": 0, "smart_known": 0, "smart_rules_changed": 0,
                        "smart_no_longer_translatable": 0},
                "photos_skipped": [],
                "photos_skipped_by_code": {},
                "keywords_not_created": [],
                "keywords_not_created_by_code": {},
                "members_skipped": [],
                "members_skipped_by_code": {},
                "stacks_skipped": [],
                "stacks_skipped_by_code": {},
                "quick_skipped": [],
                "quick_skipped_by_code": {},
                "smart_skipped": [],
                "smart_skipped_by_code": {},
            })
        );
        let s = ApplySummary {
            photos_incomplete: vec![
                Skipped::new(SkipCode::Rejected, "photo 3: x"),
                Skipped::new(SkipCode::Rejected, "photo 4: x"),
            ],
            ..ApplySummary::default()
        };
        let v = serde_json::to_value(applied_json(&s, None)).unwrap();
        assert_eq!(v["photos_incomplete"][0]["reason"], "photo 3: x");
        assert_eq!(v["photos_incomplete"][0]["code"], "rejected");
        assert_eq!(
            v["photos_incomplete_by_code"],
            serde_json::json!({"rejected": 2})
        );
        assert_eq!(v["photos_skipped_by_code"], serde_json::json!({}));
        assert_eq!(v["not_saved"], 0);
        assert_eq!(v["photos_updated"], 0);
        assert_eq!(v["photos_unchanged"], 0);
        assert!(v["keywords_not_created"].is_array());
        for key in [
            "folders_created",
            "albums_created",
            "collections_known",
            "collections_existing",
            "members_added",
            "members_removed",
            "collections_renamed",
            "collections_moved",
            "collections_removed",
            "stacks_grouped",
            "stacks_known",
            "stack_members_removed",
            "quick_added",
            "quick_already",
            "quick_removed",
            "smart_created",
            "smart_known",
            "smart_existing",
            "smart_rules_changed",
            "smart_no_longer_translatable",
        ] {
            assert_eq!(v[key], 0, "{key}");
        }
        for key in [
            "collections_skipped",
            "collections_incomplete",
            "members_skipped",
            "stacks_skipped",
            "stacks_incomplete",
            "quick_skipped",
            "quick_incomplete",
            "smart_skipped",
        ] {
            assert!(v[key].is_array(), "{key}");
        }
        assert!(v.get("aborted").is_none());
    }

    #[test]
    fn white_balance_counts_are_reported_and_end_the_summary_line() {
        let mut absolute = Develop::default();
        absolute.white_balance.hint = WhiteBalanceHint::Undecidable;
        absolute.white_balance.temperature = Some(5000.0);
        let mut relative = Develop::default();
        relative.white_balance.hint = WhiteBalanceHint::Relative;
        relative.white_balance.temperature = Some(10.0);
        let plan = plan_of(vec![
            Photo::new("a.dng", 1, Rating::default())
                .with_global_id("a")
                .with_develop(absolute),
            Photo::new("b.jpg", 2, Rating::default())
                .with_global_id("b")
                .with_develop(relative),
            Photo::new("c.jpg", 3, Rating::default()).with_global_id("c"),
        ]);

        let d = planned_develop(&plan);
        assert_eq!(d.white_balance_custom, 2);
        assert_eq!(d.white_balance_to_check, 1);
        assert_eq!(d.white_balance_written, 0);
        assert_eq!(d.white_balance_not_carried, 0);
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["white_balance_custom"], 2);
        assert_eq!(v["white_balance_to_check"], 1);

        let text = planned_summary(&plan);
        let line = text
            .lines()
            .find(|line| line.starts_with("develop:"))
            .expect("one develop line");
        assert!(
            line.ends_with(", custom white balance 2 (to check 1, written 0, not carried 0)"),
            "{line}"
        );
    }

    #[test]
    fn aborted_run_has_a_short_reason() {
        let lost = WriteError::ConnectionLost {
            command: "library.import".into(),
        };
        let v = serde_json::to_value(applied_json(&ApplySummary::default(), Some(&lost))).unwrap();
        assert_eq!(
            v["aborted"],
            "connection lost during library.import; outcome unknown"
        );
    }

    #[test]
    fn dry_run_counts_known_photos_once() {
        let photos = vec![
            Photo::new("a.jpg", 1, Rating::new(2).unwrap()).with_global_id("a"),
            Photo::new("b.jpg", 2, Rating::default()).with_global_id("b"),
            Photo::new("c.jpg", 3, Rating::default()).with_global_id("c"),
            Photo::new("d.jpg", 4, Rating::default()),
        ];
        let mut state = State::default();
        let fresh = lc_writer::state::PhotoFields::imported_default();
        state.record("a", 1, serde_json::json!(1), fresh.clone());
        state.record("b", 2, serde_json::json!(2), fresh);
        let input = WriteInput {
            photos,
            ..WriteInput::default()
        };
        let plan = lc_writer::plan::plan(&input, &state, &Default::default());
        let p = planned(&plan);
        assert_eq!((p.photos, p.photos_updated, p.photos_unchanged), (1, 1, 1));
        let skipped = plan.photo_skips().len();
        assert_eq!(
            p.photos + p.photos_updated + p.photos_unchanged + skipped,
            input.photos.len()
        );
        assert!(planned_summary(&plan).starts_with(
            "writer: keywords to create 0, photos to import 1, updates 1, \
             albums to create 0, album members to add 0\n"
        ));
    }

    #[test]
    fn planned_counts_folders_and_albums_apart() {
        use model::{Collection, CollectionKind};
        let collection = |id, kind, parent, members: Vec<i64>| Collection {
            lightroom_id: id,
            name: format!("c{id}"),
            kind,
            parent,
            members,
        };
        let input = WriteInput {
            photos: vec![Photo::new("a.jpg", 1, Rating::default()).with_global_id("a")],
            collections: vec![
                collection(1, CollectionKind::Folder, None, vec![]),
                collection(2, CollectionKind::Album, Some(1), vec![1, 9]),
            ],
            ..WriteInput::default()
        };
        let plan = lc_writer::plan::plan(&input, &State::default(), &Default::default());
        let p = planned(&plan);
        assert_eq!((p.folders, p.albums, p.members_added), (1, 1, 1));
        let text = planned_summary(&plan);
        assert!(text.starts_with(
            "writer: keywords to create 0, photos to import 1, updates 0, \
             albums to create 1, album members to add 1\n"
        ));
        assert!(text.contains("writer would skip album members:"));
    }

    #[test]
    fn applied_summary_names_collection_numbers_but_no_names() {
        let s = ApplySummary {
            folders_created: 2,
            albums_created: 3,
            collections_renamed: 1,
            collections_skipped: vec![Skipped::new(
                SkipCode::Rejected,
                "collection 4: LightCraft rejected `album.create`",
            )],
            ..ApplySummary::default()
        };
        let text = applied_summary(&s);
        assert!(text.contains("folders created 2, albums created 3"));
        assert!(text.contains("collections renamed 1, moved 0, removed 0"));
        assert!(text.contains("skipped collections:"));
        assert!(text.contains("LightCraft rejected `album.create`"));
    }

    fn photo_with_profile(gid: &str, lr_id: i64, profile: &str) -> Photo {
        let develop = Develop {
            camera_profile: Some(profile.to_owned()),
            ..Develop::default()
        };
        Photo::new("a.jpg", lr_id, Rating::default())
            .with_global_id(gid)
            .with_develop(develop)
    }

    #[test]
    fn camera_profiles_are_sorted_capped_and_counted_by_name() {
        let photos: Vec<Photo> = (0..25)
            .map(|i| photo_with_profile(&format!("g{i}"), i + 1, &format!("Profile {:02}", 24 - i)))
            .collect();
        let plan = plan_of(photos);
        let d = planned_develop(&plan);
        assert_eq!(d.camera_profiles.len(), CAMERA_PROFILES_LIMIT);
        assert_eq!(d.camera_profiles_count, 25);
        assert_eq!(d.photos_with_profile, 25);
        let listed: Vec<&String> = d.camera_profiles.keys().collect();
        let mut sorted = listed.clone();
        sorted.sort();
        assert_eq!(listed, sorted, "the listing is sorted by name");
        assert_eq!(listed.first().map(|s| s.as_str()), Some("Profile 00"));
        assert_eq!(listed.last().map(|s| s.as_str()), Some("Profile 19"));

        let v = serde_json::to_value(&d).unwrap();
        let json_keys: Vec<&String> = v["camera_profiles"]
            .as_object()
            .expect("camera_profiles object")
            .keys()
            .collect();
        let mut sorted_json = json_keys.clone();
        sorted_json.sort();
        assert_eq!(json_keys, sorted_json);
        assert_eq!(v["camera_profiles_count"], 25);
        assert_eq!(v["photos_with_profile"], 25);
        assert!(planned_summary(&plan).contains("photos with camera profile 25"));
    }

    #[test]
    fn a_camera_profile_alone_plans_no_develop_command() {
        let plan = plan_of(vec![photo_with_profile("g", 1, "Solo Profile")]);
        let d = planned_develop(&plan);
        assert_eq!(d.photos, 1);
        assert_eq!(d.planned, 0);
        assert_eq!(
            d.camera_profiles.get("Solo Profile").copied(),
            Some(1),
            "{:?}",
            d.camera_profiles
        );
        assert!(planned_summary(&plan).contains("commands planned 0"));
    }

    #[test]
    fn no_library_means_an_empty_state() {
        assert_eq!(load_state(None).unwrap(), State::default());
    }

    #[test]
    fn missing_mapping_file_is_an_empty_state_and_is_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("L.lightcraft");
        assert_eq!(load_state(Some(&library)).unwrap(), State::default());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
