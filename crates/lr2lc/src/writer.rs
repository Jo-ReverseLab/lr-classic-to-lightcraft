//! The writer part of a run: the plan in a dry run, writing through the
//! LightCraft control port with `--apply`.

use crate::dry_run::Converted;
use crate::output::group_reasons;
use anyhow::{Context, Result};
use lc_writer::apply::{Aborted, ApplySummary, apply_partial};
use lc_writer::control::{ControlClient, WriteError, preflight};
use lc_writer::plan::{Plan, Step, WriteInput};
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
    photos_updated: usize,
    photos_unchanged: usize,
    updates: usize,
    not_saved: usize,
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
pub fn input(converted: &Converted) -> WriteInput {
    WriteInput {
        photos: converted.photos.clone(),
        keywords: converted.keywords.clone(),
        photo_keywords: converted.photo_keywords.clone(),
        collections: converted.collections.clone(),
        stacks: converted.stacks.clone(),
        quick: converted.quick.clone(),
        smart: converted.smart.clone(),
        smart_untranslatable_ids: converted.smart_untranslatable_ids.clone(),
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
        photos_updated: s.photos_updated,
        photos_unchanged: s.photos_unchanged,
        updates: s.updates,
        not_saved: s.not_saved,
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
    out
}

/// The readable summary of an applied run.
pub fn applied_summary(s: &ApplySummary) -> String {
    let mut out = format!(
        "written: keywords created {}, photos imported {}, photos already in the library {}, \
         photos updated {}, photos unchanged {}, updates {}, not yet saved by LightCraft {}\n",
        s.keywords_created,
        s.photos_imported,
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
    mut state: State,
) -> Result<ApplySummary, Aborted> {
    let before_writing = |error| Aborted {
        error,
        summary: Box::default(),
    };
    let state_file = state_path(target.library);
    let mut client = ControlClient::connect(target.port).map_err(before_writing)?;
    preflight(&mut client, target.library, target.allow_xmp).map_err(before_writing)?;
    apply_partial(&mut client, input, &mut state, &state_file)
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
    use model::{Photo, Rating, SkipCode};

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
