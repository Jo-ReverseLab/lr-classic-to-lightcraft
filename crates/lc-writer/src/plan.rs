//! The writer plan: which LightCraft commands bring the library up to date
//! with the model, given what the mapping file says was written before.
//!
//! Planning is a pure function; nothing is sent here. Rules:
//! - missing keywords are created first, parents before children; missing
//!   ancestors are created implicitly with the default attributes;
//! - a new photo is imported on its own (`{paths: [p], mode: "add"}`, never
//!   anything else), then only fields that differ from a fresh import are
//!   set;
//! - a known photo only gets commands for fields that changed since the last
//!   run; keyword and text changes share one `photo.setMeta`;
//! - keywords whose attributes changed after creation are not updated, only
//!   reported;
//! - photos are processed in the order of their global ids;
//! - stacks come after all photo steps, ascending by Lightroom id: a stack
//!   is grouped (`stack.group`, always expanded) if it is new or its members
//!   or their order changed; members that left a stack in Lightroom are only
//!   counted, never removed from the LightCraft stack;
//! - collections come after the stack steps: new album folders first, then
//!   new albums, a parent always before its children; then the members to
//!   add and to remove, album by album;
//! - `album.create` gets only `name`, `folder` and `parent` (never
//!   `addSelected`); `parent` is the LightCraft id of a known parent or the
//!   placeholder `lr-collection:<Lightroom id>` of a parent created by the
//!   same plan;
//! - only members that lr2lc added are ever removed; a known collection that
//!   was renamed, moved or deleted in Lightroom gets no command, it is only
//!   counted;
//! - then the Quick Collection: one step with its members, in Lightroom
//!   order, that are not recorded in the mapping file; recorded members
//!   that left it in Lightroom are only counted;
//! - smart collections come last, in input order: one `album.createSmart`
//!   with `name`, `parent` (below top level, as for `album.create`) and the
//!   translated `rules` (see [`crate::smart`]) for each one that is not
//!   recorded yet; a recorded one gets no command, whatever changed, and a
//!   change of its rules is only counted;
//! - when the live state of the library is known ([`WriteInput::live`]), the
//!   fields of a known photo are compared three ways: last imported value
//!   (mapping file), Lightroom now (model) and LightCraft now. A field
//!   changed in Lightroom is written only if LightCraft still has the last
//!   imported value or already has the new one; a field changed differently
//!   on both sides is a conflict, never written and reported by photo and
//!   field only ([`Conflict`]). Keyword memberships that left a photo in
//!   Lightroom are only counted ([`Plan::keywords_removed`]);
//!   `removeKeywords` is never sent.

use crate::live::{LivePhoto, LiveState};
use crate::smart::{convertible, translate};
use crate::state::{EntryKind, PhotoFields, State, hex_hash, keyword_text, snapshot};
use model::{
    Collection, CollectionKind, CurvePoint, Curves, Develop, Keyword, Mask, MaskComponent, MaskOp,
    MaskShape, MergeValue, Photo, Point, QuickCollection, SkipCode, Skipped, SmartCollection, Spot,
    SpotMode, Stack, WhiteBalanceHint,
};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// A field that changed differently on both sides since the last import:
/// nothing is written for it. Only the photo's Lightroom id and the field
/// name are carried, never a value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Conflict {
    /// The photo's id in the Lightroom catalog.
    pub photo: i64,
    /// `rating`, `flag`, `label`, `title`, `caption`, `copyright` or
    /// `creator`.
    pub field: String,
}

/// A recorded photo that is no longer in the Lightroom model. Only the
/// photo's Lightroom id is carried, never a path or a file name; nothing is
/// deleted in LightCraft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RemovedPhoto {
    /// The photo's id in the Lightroom catalog.
    pub photo: i64,
}

/// A recorded photo whose original moved while its global id stayed. Only
/// the photo's Lightroom id is carried, never a path or a file name; the
/// photo is not imported again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MovedPhoto {
    /// The photo's id in the Lightroom catalog.
    pub photo: i64,
}

/// A recorded keyword whose path text changed while its global id stayed.
/// Only the keyword's Lightroom id is carried, never a name; nothing is
/// created or renamed in LightCraft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RenamedKeyword {
    /// The keyword's id in the Lightroom catalog.
    pub keyword: i64,
}

/// Everything the writer is asked to put into the library.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WriteInput {
    /// The photos to write.
    pub photos: Vec<Photo>,
    /// The keywords to create, with their attributes. Missing ancestors and
    /// keywords used only by photos are created as well.
    pub keywords: Vec<Keyword>,
    /// The keywords of each photo, by `Photo::lightroom_id`.
    pub photo_keywords: BTreeMap<i64, Vec<Keyword>>,
    /// The collections to write as albums and album folders.
    pub collections: Vec<Collection>,
    /// The stacks to group, members top first.
    pub stacks: Vec<Stack>,
    /// The Quick Collection, if the catalog has one.
    pub quick: Option<QuickCollection>,
    /// The smart collections to write as smart albums.
    pub smart: Vec<SmartCollection>,
    /// The Lightroom ids of the smart collections that were read but are
    /// not in `smart` (untranslatable, or their parent is not a collection
    /// set). Recorded smart albums with such an id are counted as
    /// [`Plan::smart_no_longer_translatable`], not as removed.
    pub smart_untranslatable_ids: Vec<i64>,
    /// The library as LightCraft has it now, read with
    /// [`crate::live::read_live`]. `None` in a dry run (no connection) and
    /// before an apply run has read the library; the plan then compares only
    /// Lightroom with the mapping file.
    pub live: Option<LiveState>,
    /// Write development settings of a known photo again although they
    /// changed in Lightroom since the last run. Without it such a photo is
    /// only counted (`develop_not_applied`), never silently overwritten.
    pub overwrite_develop: bool,
}

/// One command of the plan.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Create a keyword (`keyword.create`).
    CreateKeyword {
        /// The keyword's path text, `a|b|c`.
        path: String,
        /// The command parameters.
        params: Value,
    },
    /// Import one photo as a reference (`library.import`).
    Import {
        /// Lightroom global id of the photo.
        global_id: String,
        /// The photo's file.
        path: PathBuf,
    },
    /// Create one virtual copy from its master (`photo.virtualCopy`). The
    /// copy is created at most once: the step is planned only when the copy
    /// is not yet in the mapping file. The master's LightCraft id is read
    /// from the mapping file during the run, so the master is imported first.
    VirtualCopy {
        /// Lightroom global id of the copy.
        global_id: String,
        /// Lightroom global id of the master.
        master_global_id: String,
    },
    /// Change a field of one photo. `params` never contains `ids`; the
    /// photo is named by its global id.
    Update {
        /// Lightroom global id of the photo.
        global_id: String,
        /// `photo.rate`, `photo.flag`, `photo.label` or `photo.setMeta`.
        command: String,
        /// The command parameters without `ids`.
        params: Value,
    },
    /// Set slider values of one photo (`develop.set`) by its LightCraft id;
    /// no `library.select` precedes it.
    DevelopSet {
        /// Lightroom global id of the photo.
        global_id: String,
        /// LightCraft control id to value.
        values: BTreeMap<String, f64>,
    },
    /// Merge structured development settings of one photo (`library.select`,
    /// then `develop.merge` without `ids`, which acts on the selection).
    DevelopMerge {
        /// Lightroom global id of the photo.
        global_id: String,
        /// Nested settings object of `develop.merge`.
        settings: Value,
    },
    /// Write a custom white balance of one photo (`develop.set` with
    /// `wb.temp` and `wb.tint`), after its other develop steps. With
    /// `check_kelvin` the values are Kelvin and are written only when
    /// `photo.inspect` shows that LightCraft edits the photo on a Kelvin
    /// scale; otherwise they are already on LightCraft's scale.
    DevelopWhiteBalance {
        /// Lightroom global id of the photo.
        global_id: String,
        /// `wb.temp` and/or `wb.tint`.
        values: BTreeMap<String, f64>,
        /// Ask LightCraft for the photo's scale first.
        check_kelvin: bool,
    },
    /// Group photos into a stack (`library.select`, then `stack.group`
    /// with `collapsed: false`); the first photo is the top.
    GroupStack {
        /// Lightroom id of the stack.
        stack_id: i64,
        /// Lightroom global ids of the members, top first.
        gids: Vec<String>,
    },
    /// Create an album or album folder (`album.create`).
    CreateAlbum {
        /// Lightroom id of the collection.
        collection_id: i64,
        /// `name`, `folder` and, below top level, `parent`: a LightCraft id
        /// or the placeholder `lr-collection:<Lightroom id>`.
        params: Value,
    },
    /// Add photos to an album (`album.addPhotos`), in Lightroom order.
    AddMembers {
        /// Lightroom id of the collection.
        collection_id: i64,
        /// Lightroom global ids of the photos.
        gids: Vec<String>,
    },
    /// Add photos to the Quick Collection (`album.setTarget {id: null}`,
    /// then, for the photos it does not hold yet, `library.select` and
    /// `album.toggleTarget`). Only run by `--apply`; never part of a
    /// script, since the members must be read first.
    QuickAdd {
        /// Lightroom global ids of the photos, in Lightroom order.
        gids: Vec<String>,
    },
    /// Create a smart album (`album.createSmart`).
    CreateSmart {
        /// Lightroom id of the smart collection.
        collection_id: i64,
        /// `name`, `rules` and, below top level, `parent`: a LightCraft id
        /// or the placeholder `lr-collection:<Lightroom id>`.
        params: Value,
    },
    /// Remove photos that lr2lc added from an album (`album.removePhotos`).
    RemoveMembers {
        /// Lightroom id of the collection.
        collection_id: i64,
        /// Lightroom global ids of the photos.
        gids: Vec<String>,
    },
}

/// The commands to run, in order, and what is left out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    /// Commands in the order they must run.
    pub steps: Vec<Step>,
    /// Elements that are not written, each with a reason. Keyword entries
    /// come first, then photo entries; see [`Plan::keyword_skips`] and
    /// [`Plan::photo_skips`].
    pub skipped: Vec<Skipped>,
    /// How many of the leading entries of `skipped` concern keywords.
    keyword_skips: usize,
    /// Known photos (in the mapping file) with at least one update.
    photos_updated: usize,
    /// Known photos that need no command.
    photos_unchanged: usize,
    /// Input collections already in the mapping file.
    collections_known: usize,
    /// Known collections whose name differs from the recorded one.
    collections_renamed: usize,
    /// Known collections whose parent differs from the recorded one.
    collections_moved: usize,
    /// Recorded collections missing from the input.
    collections_removed: usize,
    /// Recorded smart albums whose collection still exists in Lightroom
    /// but can no longer be translated.
    smart_no_longer_translatable: usize,
    /// Album members that cannot be written.
    member_skips: Vec<Skipped>,
    /// Input stacks recorded with the same members in the same order.
    stacks_known: usize,
    /// Input stacks with fewer than two members that have a global id.
    stack_skips: Vec<Skipped>,
    /// Recorded members of input stacks that are no longer members.
    stack_members_removed: usize,
    /// Quick Collection members already recorded in the mapping file.
    quick_already: usize,
    /// Recorded Quick Collection members that left it in Lightroom.
    quick_removed: usize,
    /// Quick Collection members that cannot be written.
    quick_skips: Vec<Skipped>,
    /// Input smart collections already in the mapping file.
    smart_known: usize,
    /// Known smart collections whose translated rules differ from the
    /// recorded ones (or no longer translate).
    smart_rules_changed: usize,
    /// Smart collections that cannot be written.
    smart_skips: Vec<Skipped>,
    /// Fields changed differently on both sides, sorted by photo and field;
    /// nothing is written for them.
    conflicts: Vec<Conflict>,
    /// Keyword memberships that left a photo in Lightroom (`last` has them,
    /// `lr` does not). They stay in LightCraft and are only counted.
    keywords_removed: usize,
    /// The fields to record for known photos after the run, by global id, so
    /// a conflicting field keeps its last imported value and the next run
    /// reports the conflict again.
    recorded: BTreeMap<String, PhotoFields>,
    /// Recorded photos that are no longer in the Lightroom model, ascending
    /// by Lightroom id. Nothing is deleted in LightCraft.
    removed_photos: Vec<RemovedPhoto>,
    /// Recorded photos whose original moved, ascending by Lightroom id. They
    /// are not imported again.
    moved_photos: Vec<MovedPhoto>,
    /// Recorded keywords whose path text changed but whose global id stayed,
    /// ascending by Lightroom id. Nothing is created or renamed in
    /// LightCraft.
    renamed_keywords: Vec<RenamedKeyword>,
    /// Global ids of the recorded photos missing from the input; they are
    /// taken out of the albums the mapping file records.
    removed_gids: BTreeSet<String>,
    /// Converted photos that carry development settings (`Develop` is
    /// `Some`), whether or not a command was needed.
    develop_photos: usize,
    /// Photos the plan sends develop commands for.
    develop_planned: usize,
    /// Known photos whose development settings changed in Lightroom and were
    /// left alone because `overwrite_develop` was not given.
    develop_not_applied: usize,
    /// Photos written because `overwrite_develop` was given.
    develop_overwritten: usize,
    /// Names of catalog fields that could not be carried over, each with
    /// the number of converted photos that carry it. Only names and counts,
    /// never values.
    develop_unmapped: BTreeMap<String, usize>,
    /// Converted photos per camera profile name, ordered by name. Names and
    /// counts only, never rendering values.
    develop_camera_profiles: BTreeMap<String, usize>,
    /// Converted photos that carry a non-empty camera profile name.
    develop_photos_with_profile: usize,
    /// Converted photos with at least one mask.
    develop_masks: usize,
    /// Converted photos with at least one spot.
    develop_spots: usize,
    /// Converted photos with a custom white balance (temperature or tint
    /// stored with mode `custom`). Only counted, never named or valued.
    develop_white_balance_custom: usize,
    /// Planned white-balance writes of a Kelvin value: whether they are
    /// carried depends on LightCraft's scale for the photo, which only the
    /// applied run can ask.
    develop_white_balance_to_check: usize,
}

impl Plan {
    /// The fields that changed differently in Lightroom and in LightCraft
    /// since the last import, sorted ascending by photo, then by field in
    /// the fixed order rating, flag, label, title, caption, copyright,
    /// creator. Nothing is written for a conflicting field and no value is
    /// ever reported.
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    /// Keyword memberships that left a photo in Lightroom since the last
    /// import. They are kept in LightCraft and only counted; the plan never
    /// sends `removeKeywords`.
    pub fn keywords_removed(&self) -> usize {
        self.keywords_removed
    }

    /// Recorded photos that are no longer in the Lightroom model, ascending
    /// by Lightroom id. They are only reported (and taken out of the albums
    /// the mapping file records); nothing is deleted in LightCraft.
    pub fn removed_photos(&self) -> &[RemovedPhoto] {
        &self.removed_photos
    }

    /// Recorded photos whose original moved while their global id stayed,
    /// ascending by Lightroom id. They are only reported, never imported
    /// again.
    pub fn moved_photos(&self) -> &[MovedPhoto] {
        &self.moved_photos
    }

    /// Recorded keywords whose path text changed while their global id
    /// stayed, ascending by Lightroom id (a renamed ancestor also changes
    /// the path text of its descendants). They are only reported; nothing is
    /// created or renamed in LightCraft.
    pub fn renamed_keywords(&self) -> &[RenamedKeyword] {
        &self.renamed_keywords
    }

    /// The field values to record in the mapping file for the known photos
    /// the live state covered, by global id. A conflicting field keeps its
    /// last imported value.
    pub(crate) fn recorded_fields_map(&self) -> BTreeMap<String, PhotoFields> {
        self.recorded.clone()
    }

    /// The skipped keywords (attributes changed after creation).
    pub fn keyword_skips(&self) -> &[Skipped] {
        &self.skipped[..self.keyword_skips.min(self.skipped.len())]
    }

    /// The skipped photos (no or duplicate global id, path not UTF-8).
    pub fn photo_skips(&self) -> &[Skipped] {
        &self.skipped[self.keyword_skips.min(self.skipped.len())..]
    }

    /// The photos already in the mapping file that get at least one
    /// update. Together with the imports, [`Plan::photos_unchanged`] and
    /// [`Plan::photo_skips`] every input photo counts exactly once.
    pub fn photos_updated(&self) -> usize {
        self.photos_updated
    }

    /// The photos already in the mapping file that need no command.
    pub fn photos_unchanged(&self) -> usize {
        self.photos_unchanged
    }

    /// The input smart collections already in the mapping file; they get
    /// no `album.createSmart`.
    pub fn smart_known(&self) -> usize {
        self.smart_known
    }

    /// The known smart collections whose rules changed in Lightroom since
    /// their creation (the translation differs from the recorded one or is
    /// no longer possible); they are not changed in LightCraft.
    pub fn smart_rules_changed(&self) -> usize {
        self.smart_rules_changed
    }

    /// The smart collections that are not written: their rules have no
    /// exact LightCraft equivalent, or their parent is not a converted
    /// collection set. The reason names the Lightroom id and, for a rule,
    /// the criterion.
    pub fn smart_skips(&self) -> &[Skipped] {
        &self.smart_skips
    }

    /// The input collections already in the mapping file; they get no
    /// `album.create`.
    pub fn collections_known(&self) -> usize {
        self.collections_known
    }

    /// The known collections and smart collections renamed in Lightroom
    /// since their creation; they are not renamed in LightCraft.
    pub fn collections_renamed(&self) -> usize {
        self.collections_renamed
    }

    /// The known collections and smart collections moved in Lightroom
    /// since their creation; they are not moved in LightCraft.
    pub fn collections_moved(&self) -> usize {
        self.collections_moved
    }

    /// The recorded collections and smart collections that are no longer in
    /// the input; they are not deleted in LightCraft.
    pub fn collections_removed(&self) -> usize {
        self.collections_removed
    }

    /// The recorded smart albums whose Lightroom smart collection still
    /// exists but is not written any more (its rules no longer translate,
    /// or its parent is no longer a collection set). They are kept in
    /// LightCraft and in the mapping file and only counted; they are not
    /// part of [`Plan::collections_removed`].
    pub fn smart_no_longer_translatable(&self) -> usize {
        self.smart_no_longer_translatable
    }

    /// The album members whose photo is not in the input or has no global
    /// id. The reason names the photo's Lightroom id.
    pub fn member_skips(&self) -> &[Skipped] {
        &self.member_skips
    }

    /// The Quick Collection members already recorded in the mapping file.
    pub fn quick_already(&self) -> usize {
        self.quick_already
    }

    /// The recorded Quick Collection members that are no longer members in
    /// Lightroom. They stay in LightCraft and are only counted.
    pub fn quick_removed(&self) -> usize {
        self.quick_removed
    }

    /// The Quick Collection members without a global id; the reason names
    /// the photo's Lightroom id.
    pub fn quick_skips(&self) -> &[Skipped] {
        &self.quick_skips
    }

    /// The input stacks recorded in the mapping file with the same members
    /// in the same order (members that left the stack in Lightroom are not
    /// compared); they get no command.
    pub fn stacks_known(&self) -> usize {
        self.stacks_known
    }

    /// The input stacks with fewer than two members that have a global id;
    /// the reason names the stack's Lightroom id.
    pub fn stack_skips(&self) -> &[Skipped] {
        &self.stack_skips
    }

    /// The recorded members of input stacks that are no longer members in
    /// Lightroom. They stay in the LightCraft stack and are only counted.
    pub fn stack_members_removed(&self) -> usize {
        self.stack_members_removed
    }

    /// Converted photos that carry development settings (`Develop` is
    /// `Some`), whether or not a command was needed.
    pub fn develop_photos(&self) -> usize {
        self.develop_photos
    }

    /// Photos the plan sends develop commands for.
    pub fn develop_planned(&self) -> usize {
        self.develop_planned
    }

    /// Known photos whose development settings changed in Lightroom and were
    /// left alone because `overwrite_develop` was not given.
    pub fn develop_not_applied(&self) -> usize {
        self.develop_not_applied
    }

    /// Photos written because `overwrite_develop` was given.
    pub fn develop_overwritten(&self) -> usize {
        self.develop_overwritten
    }

    /// Names of catalog fields that could not be carried over, sorted, with
    /// the number of converted photos that carry each. Only names and
    /// counts, never values.
    pub fn develop_unmapped(&self) -> &BTreeMap<String, usize> {
        &self.develop_unmapped
    }

    /// Converted photos per camera profile name, ordered by name.
    pub fn develop_camera_profiles(&self) -> &BTreeMap<String, usize> {
        &self.develop_camera_profiles
    }

    /// Converted photos that carry a non-empty camera profile name.
    pub fn develop_photos_with_profile(&self) -> usize {
        self.develop_photos_with_profile
    }

    /// Converted photos with at least one mask.
    pub fn develop_masks(&self) -> usize {
        self.develop_masks
    }

    /// Converted photos with at least one spot.
    pub fn develop_spots(&self) -> usize {
        self.develop_spots
    }

    /// Converted photos with a custom white balance.
    pub fn develop_white_balance_custom(&self) -> usize {
        self.develop_white_balance_custom
    }

    /// Planned Kelvin white-balance writes that depend on LightCraft's scale
    /// for the photo.
    pub fn develop_white_balance_to_check(&self) -> usize {
        self.develop_white_balance_to_check
    }

    /// The plan as a script: one JSON line per step, each an
    /// `engine.execute` request, plus one `library.select` line before the
    /// updates of each photo (LightCraft runs photo commands only on a
    /// selection). Selects and updates carry `ids`: the known LightCraft
    /// id, or the placeholder `lr:<global id>` for a photo that is imported
    /// by this plan. Member changes are preceded by a `library.select` of
    /// exactly their photos and name the album by its LightCraft id or the
    /// placeholder `lr-collection:<Lightroom id>`. The Quick Collection
    /// step is not part of the script (see [`Step::QuickAdd`]).
    pub fn to_script(&self, state: &State) -> String {
        let mut out = String::new();
        let mut push = |command: &str, params: Value| {
            let line = json!({
                "method": "engine.execute",
                "params": {"command": command, "params": params},
            });
            out.push_str(&line.to_string());
            out.push('\n');
        };
        let mut selected: Option<&str> = None;
        for step in &self.steps {
            match step {
                Step::CreateKeyword { params, .. } => {
                    selected = None;
                    push("keyword.create", params.clone());
                }
                Step::Import { path, .. } => {
                    selected = None;
                    // The plan only creates imports for valid UTF-8 paths
                    // (others are skipped with `PathNotUtf8`), so this is
                    // always `Some`; dropping a line is safer than sending a
                    // path that names another file.
                    if let Some(params) = import_params(path) {
                        push("library.import", params);
                    }
                }
                Step::VirtualCopy {
                    master_global_id, ..
                } => {
                    selected = None;
                    let master = photo_ref(state, master_global_id);
                    push("photo.virtualCopy", json!({"ids": [master]}));
                }
                Step::CreateAlbum { params, .. } => {
                    selected = None;
                    push("album.create", params.clone());
                }
                Step::CreateSmart { params, .. } => {
                    selected = None;
                    push("album.createSmart", params.clone());
                }
                Step::GroupStack { gids, .. } => {
                    selected = None;
                    let ids: Vec<Value> = gids.iter().map(|g| photo_ref(state, g)).collect();
                    push("library.select", members_select_params(&ids));
                    push("stack.group", group_params(&ids));
                }
                Step::AddMembers {
                    collection_id,
                    gids,
                }
                | Step::RemoveMembers {
                    collection_id,
                    gids,
                } => {
                    selected = None;
                    let ids: Vec<Value> = gids.iter().map(|g| photo_ref(state, g)).collect();
                    push("library.select", members_select_params(&ids));
                    let command = match step {
                        Step::AddMembers { .. } => "album.addPhotos",
                        _ => "album.removePhotos",
                    };
                    let album = album_ref(state, *collection_id);
                    push(command, json!({"id": album, "ids": ids}));
                }
                Step::QuickAdd { .. } => {
                    // Left out: `album.toggleTarget` removes photos that are
                    // all in the Quick Collection already, and a script
                    // cannot read its members first. Only `--apply` writes
                    // it.
                }
                Step::Update {
                    global_id,
                    command,
                    params,
                } => {
                    let id = photo_ref(state, global_id);
                    if selected != Some(global_id.as_str()) {
                        selected = Some(global_id);
                        push("library.select", select_params(&id));
                    }
                    let mut params = params.clone();
                    if let Value::Object(map) = &mut params {
                        map.insert("ids".to_string(), json!([id]));
                    }
                    push(command, params);
                }
                Step::DevelopSet { global_id, values } => {
                    selected = None;
                    let id = photo_ref(state, global_id);
                    push("develop.set", json!({"values": values, "ids": [id]}));
                }
                Step::DevelopMerge {
                    global_id,
                    settings,
                } => {
                    let id = photo_ref(state, global_id);
                    selected = Some(global_id);
                    push("library.select", select_params(&id));
                    push("develop.merge", json!({"settings": settings}));
                }
                // A Kelvin value depends on LightCraft's scale for the photo,
                // which a script cannot ask; only `--apply` writes it.
                Step::DevelopWhiteBalance {
                    check_kelvin: true, ..
                } => {}
                Step::DevelopWhiteBalance {
                    global_id, values, ..
                } => {
                    selected = None;
                    let id = photo_ref(state, global_id);
                    push("develop.set", json!({"values": values, "ids": [id]}));
                }
            }
        }
        out
    }
}

/// The parameters of a reference-only import of one file. Nothing else is
/// ever sent: no copy, move, rename, presets or keywords.
///
/// Returns `None` for a path that is not valid UTF-8: the JSON protocol can
/// only carry text, and a lossily encoded path would name a different file.
/// The plan never creates such a step (it skips the photo with
/// [`SkipCode::PathNotUtf8`] first); the `None` is a defensive check for
/// callers outside the plan.
pub fn import_params(path: &std::path::Path) -> Option<Value> {
    Some(json!({"paths": [path.to_str()?], "mode": "add"}))
}

/// The LightCraft id of a photo, or the placeholder `lr:<global id>`.
fn photo_ref(state: &State, global_id: &str) -> Value {
    state.photo(global_id).map_or_else(
        || Value::String(format!("lr:{global_id}")),
        |e| e.lightcraft_id.clone(),
    )
}

/// The LightCraft id of a collection, or the placeholder
/// `lr-collection:<Lightroom id>`.
fn album_ref(state: &State, collection_id: i64) -> Value {
    state.collection(collection_id).map_or_else(
        || Value::String(format!("{COLLECTION_PLACEHOLDER}{collection_id}")),
        |e| e.lightcraft_id.clone(),
    )
}

/// Prefix of the placeholder for a collection created by the same plan.
pub const COLLECTION_PLACEHOLDER: &str = "lr-collection:";

/// The parameters that select exactly the given photos (at least one) and
/// make the first one active; LightCraft changes album members only on a
/// selection.
pub fn members_select_params(ids: &[Value]) -> Value {
    json!({"ids": ids, "active": ids.first()})
}

/// The parameters of `stack.group` for photos `ids` (at least two), the
/// first one on top. `collapsed` is always sent as `false`: LightCraft
/// would collapse a new stack, and the Lightroom state is not taken over.
pub fn group_params(ids: &[Value]) -> Value {
    json!({"ids": ids, "top": ids.first(), "collapsed": false})
}

/// The parameters that select exactly one photo and make it the active
/// one; LightCraft runs `photo.*` commands only on a selection.
pub fn select_params(id: &Value) -> Value {
    json!({"ids": [id], "active": id})
}

/// Computes the commands that bring the library up to date.
///
/// `library_keywords` holds the path texts (`a|b|c`) of the keywords the
/// library already has; they and keywords recorded in `state` are not
/// created again.
pub fn plan(input: &WriteInput, state: &State, library_keywords: &BTreeSet<String>) -> Plan {
    let mut out = Plan::default();
    let (renamed, renamed_report) = renamed_keywords(input, state);
    out.renamed_keywords = renamed_report;
    plan_keywords(input, state, library_keywords, &renamed, &mut out);
    out.keyword_skips = out.skipped.len();
    plan_photos(input, state, &renamed, &mut out);
    plan_removed_and_moved(input, state, &mut out);
    plan_stacks(input, state, &mut out);
    plan_collections(input, state, &mut out);
    plan_quick(input, state, &mut out);
    plan_smart(input, state, &mut out);
    out
}

/// Whether reconciling at least one known photo needs the live values from
/// LightCraft: a field or a keyword membership changed in Lightroom since the
/// last import. If nothing changed in Lightroom, the live value can never
/// change the outcome (rule: `lr == last` means nothing is written), so an
/// apply run can skip the read and a second run sends no extra commands.
pub(crate) fn needs_live(input: &WriteInput, state: &State) -> bool {
    let (renamed, _) = renamed_keywords(input, state);
    input.photos.iter().any(|photo| {
        let Some(global_id) = photo.global_id.as_deref() else {
            return false;
        };
        let Some(entry) = state.photo(global_id) else {
            return false;
        };
        let keywords = input
            .photo_keywords
            .get(&photo.lightroom_id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let new = snapshot_with_renames(photo, keywords, &renamed);
        let old = &entry.fields;
        new.rating != old.rating
            || new.flag != old.flag
            || new.label != old.label
            || new.title != old.title
            || new.caption != old.caption
            || new.copyright != old.copyright
            || new.creator != old.creator
            || new
                .keywords
                .iter()
                .any(|keyword| !old.keywords.iter().any(|last| last == keyword))
    })
}

fn plan_smart(input: &WriteInput, state: &State, out: &mut Plan) {
    for s in &input.smart {
        if let Some(entry) = state.collection(s.lightroom_id) {
            out.smart_known += 1;
            if entry.name != s.name {
                out.collections_renamed += 1;
            }
            if entry.parent != s.parent {
                out.collections_moved += 1;
            }
            if translate(&s.rules).ok() != entry.rules {
                out.smart_rules_changed += 1;
            }
            continue;
        }
        match convertible(s, &input.collections) {
            Ok(rules) => {
                let mut params = json!({"name": s.name, "rules": rules});
                if let Some(parent) = s.parent {
                    params["parent"] = album_ref(state, parent);
                }
                out.steps.push(Step::CreateSmart {
                    collection_id: s.lightroom_id,
                    params,
                });
            }
            Err((code, why)) => out
                .smart_skips
                .push(Skipped::new(code, smart_skip_reason(s.lightroom_id, &why))),
        }
    }
}

/// The reason of a smart collection that is not written; `why` must not
/// hold a name or a rule value.
pub(crate) fn smart_skip_reason(collection_id: i64, why: &str) -> String {
    format!("smart collection {collection_id}: {why}; not converted")
}

fn plan_quick(input: &WriteInput, state: &State, out: &mut Plan) {
    let Some(quick) = &input.quick else {
        return;
    };
    let global_ids = global_ids(input);
    let recorded = &state.quick().members;
    let mut wanted: Vec<String> = Vec::new();
    for member in &quick.members {
        match global_ids.get(member) {
            Some(gid) if !wanted.iter().any(|g| g == gid) => wanted.push((*gid).to_string()),
            Some(_) => {}
            None => out.quick_skips.push(Skipped::new(
                SkipCode::NoGlobalId,
                format!("photo {member}: no global id; not added to the quick collection"),
            )),
        }
    }
    out.quick_removed = recorded.iter().filter(|g| !wanted.contains(g)).count();
    let (known, new): (Vec<String>, Vec<String>) =
        wanted.into_iter().partition(|g| recorded.contains(g));
    out.quick_already = known.len();
    if !new.is_empty() {
        out.steps.push(Step::QuickAdd { gids: new });
    }
}

/// A keyword the input needs, and whether it was given explicitly (with
/// attributes) or only as an ancestor.
struct Wanted {
    keyword: Keyword,
    explicit: bool,
}

/// Every keyword the input needs, with its missing ancestors, ordered by
/// path parts so a parent always comes before its children.
fn wanted_keywords(input: &WriteInput) -> BTreeMap<Vec<String>, Wanted> {
    let mut wanted: BTreeMap<Vec<String>, Wanted> = BTreeMap::new();
    let explicit = input
        .keywords
        .iter()
        .chain(input.photo_keywords.values().flatten());
    for k in explicit {
        wanted.entry(k.path().to_vec()).or_insert_with(|| Wanted {
            keyword: k.clone(),
            explicit: true,
        });
    }
    let ancestors: Vec<Keyword> = wanted
        .values()
        .flat_map(|w| std::iter::successors(w.keyword.parent(), Keyword::parent))
        .collect();
    for a in ancestors {
        wanted.entry(a.path().to_vec()).or_insert(Wanted {
            keyword: a,
            explicit: false,
        });
    }
    wanted
}

/// Finds recorded keywords whose global id is the same but whose path text
/// changed (renamed in Lightroom). Returns the new path text → recorded path
/// text map and the report entries. A renamed ancestor also changes the
/// path text of its descendants, so those are reported too. Keyword names
/// never leave this function.
pub(crate) fn renamed_keywords(
    input: &WriteInput,
    state: &State,
) -> (BTreeMap<String, String>, Vec<RenamedKeyword>) {
    let recorded: BTreeMap<&str, (&str, Option<i64>)> = state
        .keywords()
        .filter_map(|(path, entry)| {
            entry
                .global_id
                .as_deref()
                .map(|gid| (gid, (path, entry.lightroom_id)))
        })
        .collect();
    let mut paths = BTreeMap::new();
    let mut report = Vec::new();
    for w in wanted_keywords(input).values() {
        let Some(gid) = w.keyword.global_id.as_deref() else {
            continue;
        };
        let text = keyword_text(&w.keyword);
        if state.keyword(&text).is_some() {
            continue;
        }
        let Some((old_path, id)) = recorded.get(gid) else {
            continue;
        };
        if *old_path != text {
            paths.insert(text, (*old_path).to_string());
            // An older mapping file may carry the global id but no
            // Lightroom id. The reader always sets the id on the model
            // keyword, so the rename can still be reported by id; the
            // mapping entry itself is left as it is.
            if let Some(id) = id.or(w.keyword.lightroom_id) {
                report.push(RenamedKeyword { keyword: id });
            }
        }
    }
    report.sort_by_key(|r| r.keyword);
    report.dedup_by_key(|r| r.keyword);
    (paths, report)
}

fn plan_keywords(
    input: &WriteInput,
    state: &State,
    library_keywords: &BTreeSet<String>,
    renamed: &BTreeMap<String, String>,
    out: &mut Plan,
) {
    for w in wanted_keywords(input).values() {
        let text = keyword_text(&w.keyword);
        if let Some(entry) = state.keyword(&text) {
            if w.explicit && !entry.same_attributes(&w.keyword) {
                out.skipped.push(Skipped::new(
                    SkipCode::KeywordChanged,
                    "keyword synonyms or attributes changed after the keyword was \
                             created in LightCraft; not updated"
                        .to_string(),
                ));
            }
            continue;
        }
        // Renamed in Lightroom: the new name is never created, and it is
        // never added to a photo (see `snapshot_with_renames`).
        if renamed.contains_key(&text) {
            continue;
        }
        if library_keywords.contains(&text) {
            continue;
        }
        out.steps.push(Step::CreateKeyword {
            path: text,
            params: create_keyword_params(&w.keyword),
        });
    }
}

fn create_keyword_params(k: &Keyword) -> Value {
    let parent = k.parent().map(|p| keyword_text(&p));
    json!({
        "name": k.name(),
        "parent": parent,
        "synonyms": k.synonyms,
        "includeOnExport": k.include_on_export,
        "exportContaining": k.export_containing,
        "exportSynonyms": k.export_synonyms,
        "person": k.person,
    })
}

fn plan_photos(
    input: &WriteInput,
    state: &State,
    renamed: &BTreeMap<String, String>,
    out: &mut Plan,
) {
    let mut by_gid: BTreeMap<&str, &Photo> = BTreeMap::new();
    for p in &input.photos {
        match p.global_id.as_deref() {
            None => out.skipped.push(Skipped::new(
                SkipCode::MissingGlobalId,
                format!("photo {} has no global id", p.lightroom_id),
            )),
            Some(gid) => {
                if by_gid.contains_key(gid) {
                    out.skipped.push(Skipped::new(
                        SkipCode::DuplicateGlobalId,
                        format!(
                            "photo {} has the same global id as another photo",
                            p.lightroom_id
                        ),
                    ));
                } else {
                    by_gid.insert(gid, p);
                }
            }
        }
    }

    // A copy names its master by Lightroom id; resolve that to the master's
    // global id so the writer can look the master up in the mapping file.
    let master_gid: BTreeMap<i64, &str> = by_gid
        .iter()
        .map(|(gid, p)| (p.lightroom_id, *gid))
        .collect();

    // Masters are planned before their copies: the copy step reads the
    // master's LightCraft id from the mapping file, which the master's import
    // step writes earlier in the same run. `by_gid` iterates sorted by global
    // id; the two passes keep that order within each group.
    let mut order: Vec<&str> = by_gid.keys().copied().collect();
    order.sort_by_key(|gid| by_gid[gid].master.is_some());

    for gid in order {
        let p = by_gid[gid];
        if let Some(develop) = &p.develop {
            out.develop_photos += 1;
            // An empty name is no profile (the reader already drops it).
            if let Some(profile) = develop
                .camera_profile
                .as_deref()
                .filter(|profile| !profile.is_empty())
            {
                *out.develop_camera_profiles
                    .entry(profile.to_owned())
                    .or_default() += 1;
                out.develop_photos_with_profile += 1;
            }
            if !develop.masks.is_empty() {
                out.develop_masks += 1;
            }
            if !develop.spots.is_empty() {
                out.develop_spots += 1;
            }
            let names: BTreeSet<&String> = develop.unmapped.iter().collect();
            for name in names {
                *out.develop_unmapped.entry(name.clone()).or_default() += 1;
            }
            if develop.white_balance.hint != WhiteBalanceHint::None {
                out.develop_white_balance_custom += 1;
            }
        }
        let keywords = input
            .photo_keywords
            .get(&p.lightroom_id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let new = snapshot_with_renames(p, keywords, renamed);
        let known = state.photo(gid).is_some();
        let old = match state.photo(gid) {
            Some(entry) => entry.fields.clone(),
            None => {
                if let Some(master) = p.master {
                    // A virtual copy is created from its master, never
                    // imported as a file. The master must be in the input
                    // (was converted by the reader) and, during the run, get
                    // a LightCraft id. If the master was skipped at read
                    // time it is absent here and the copy cannot be created.
                    let Some(master_global_id) = master_gid.get(&master).copied() else {
                        out.skipped.push(Skipped::new(
                            SkipCode::VirtualCopy,
                            format!(
                                "photo {}: virtual copy of photo {} cannot be created, \
                                 the master was not converted",
                                p.lightroom_id, master
                            ),
                        ));
                        continue;
                    };
                    out.steps.push(Step::VirtualCopy {
                        global_id: gid.to_string(),
                        master_global_id: master_global_id.to_string(),
                    });
                } else {
                    if p.path.to_str().is_none() {
                        out.skipped.push(Skipped::new(
                            SkipCode::PathNotUtf8,
                            format!(
                                "photo {} has a file path that is not valid UTF-8",
                                p.lightroom_id
                            ),
                        ));
                        continue;
                    }
                    out.steps.push(Step::Import {
                        global_id: gid.to_string(),
                        path: p.path.clone(),
                    });
                }
                PhotoFields::imported_default()
            }
        };
        let changes = if known {
            // Memberships that left the photo in Lightroom: they stay in
            // LightCraft, so they are only counted, never sent.
            out.keywords_removed += old
                .keywords
                .iter()
                .filter(|k| !new.keywords.contains(k))
                .count();
            match input.live.as_ref().and_then(|live| live.photos.get(gid)) {
                Some(live) => {
                    let (changes, recorded) =
                        reconcile(&old, &new, live, p.lightroom_id, &mut out.conflicts);
                    out.recorded.insert(gid.to_string(), recorded);
                    changes
                }
                None => {
                    // Without live state the only difference can be a
                    // keyword that left the photo in Lightroom: it is never
                    // removed in LightCraft, but it is no longer recorded, so
                    // a later run does not report it again.
                    out.recorded.insert(gid.to_string(), new.clone());
                    field_changes(&old, &new)
                }
            }
        } else {
            field_changes(&old, &new)
        };
        if known {
            if changes.is_empty() {
                out.photos_unchanged += 1;
            } else {
                out.photos_updated += 1;
            }
        }
        for (command, params) in changes {
            out.steps.push(Step::Update {
                global_id: gid.to_string(),
                command: command.to_string(),
                params,
            });
        }
        if let Some(develop) = &p.develop {
            plan_develop(gid, state.photo(gid), develop, input.overwrite_develop, out);
        }
    }
    // Deterministic report order: by photo, then by the fixed field order.
    out.conflicts
        .sort_by_key(|c| (c.photo, field_order(&c.field)));
}

/// The development commands of one photo, in the order they must run:
/// `develop.set` for slider values, then `develop.merge` for the structured
/// parts, after the photo's field updates.
///
/// A known photo whose recorded hash equals the source hash needs nothing.
/// A different hash is only reported (`develop_not_applied`) unless
/// `overwrite` is given, so a Lightroom change never overwrites silently. A
/// mapping file without a hash plans one write.
fn plan_develop(
    global_id: &str,
    known: Option<&crate::state::PhotoEntry>,
    develop: &Develop,
    overwrite: bool,
    out: &mut Plan,
) {
    if !has_develop_content(develop) {
        return;
    }
    if let Some(entry) = known {
        match entry.develop_hash.as_deref() {
            Some(recorded)
                if develop
                    .source_hash
                    .is_some_and(|source| hex_hash(&source) == recorded) =>
            {
                return;
            }
            None => {}
            Some(_) => {
                if !overwrite {
                    out.develop_not_applied += 1;
                    return;
                }
                out.develop_overwritten += 1;
            }
        }
    }
    if !develop.controls.is_empty() {
        out.steps.push(Step::DevelopSet {
            global_id: global_id.to_string(),
            values: develop.controls.clone(),
        });
    }
    if let Some(settings) = develop_settings(develop) {
        out.steps.push(Step::DevelopMerge {
            global_id: global_id.to_string(),
            settings,
        });
    }
    if let Some((values, check_kelvin)) = white_balance_values(develop) {
        if check_kelvin {
            out.develop_white_balance_to_check += 1;
        }
        out.steps.push(Step::DevelopWhiteBalance {
            global_id: global_id.to_string(),
            values,
            check_kelvin,
        });
    }
    out.develop_planned += 1;
}

/// LightCraft's reference temperature: on a relative scale, 6500 K means
/// "as shot".
const RELATIVE_REFERENCE_KELVIN: f64 = 6500.0;

/// Mired per step of Lightroom's `IncrementalTemperature`, as in
/// LightCraft's own mapper.
const MIRED_PER_STEP: f64 = 0.8;

/// The `develop.set` values of a custom white balance and whether they are
/// Kelvin values that need LightCraft's scale check. A relative temperature
/// (-100..100) becomes LightCraft's relative Kelvin: a mired shift of 0.8
/// per step around 6500 K. Tints are taken as stored.
fn white_balance_values(develop: &Develop) -> Option<(BTreeMap<String, f64>, bool)> {
    let wb = &develop.white_balance;
    let (temperature, check_kelvin) = match wb.hint {
        WhiteBalanceHint::None => return None,
        WhiteBalanceHint::Relative => (wb.temperature.map(relative_to_kelvin), false),
        WhiteBalanceHint::Kelvin => (wb.temperature, false),
        WhiteBalanceHint::Undecidable => (wb.temperature, true),
    };
    let mut values = BTreeMap::new();
    if let Some(temperature) = temperature {
        values.insert("wb.temp".to_owned(), temperature);
    }
    if let Some(tint) = wb.tint {
        values.insert("wb.tint".to_owned(), tint);
    }
    (!values.is_empty()).then_some((values, check_kelvin))
}

/// Lightroom's relative temperature (-100..100) as LightCraft's relative
/// Kelvin value.
fn relative_to_kelvin(steps: f64) -> f64 {
    let steps = steps.clamp(-100.0, 100.0);
    1e6 / (1e6 / RELATIVE_REFERENCE_KELVIN - MIRED_PER_STEP * steps)
}

/// Whether the settings hold anything the writer can send. The source hash,
/// the unmapped names and the deferred flag alone do not.
fn has_develop_content(develop: &Develop) -> bool {
    !develop.controls.is_empty()
        || !develop.partial.is_empty()
        || develop.white_balance.mode.is_some()
        || develop.white_balance.hint != WhiteBalanceHint::None
        || !curves_are_empty(&develop.curves)
        || develop.crop.is_some()
        || !develop.masks.is_empty()
        || !develop.spots.is_empty()
}

/// The nested `settings` object of `develop.merge`: dotted `partial` paths
/// are nested (`hdr.enabled` -> `{"hdr": {"enabled": ...}}`), the
/// white-balance mode becomes `wb.mode`, and a Kelvin hint adds
/// `wb.scale: "kelvin"`. The tone curves (delivered by P6-3) become
/// `curve.master` and the three channel lists, and the crop (also P6-3)
/// becomes `crop.geometry` with its rectangle and angle. Temperature and tint
/// never appear here: they travel in [`Step::DevelopWhiteBalance`].
fn develop_settings(develop: &Develop) -> Option<Value> {
    let mut settings = Map::new();
    for (path, value) in &develop.partial {
        let value = match value {
            MergeValue::Bool(on) => json!(on),
            MergeValue::Text(text) => json!(text),
        };
        insert_path(&mut settings, path, value);
    }
    if let Some(mode) = &develop.white_balance.mode {
        object_entry(&mut settings, "wb").insert("mode".to_string(), json!(mode));
    }
    if develop.white_balance.hint == WhiteBalanceHint::Kelvin {
        object_entry(&mut settings, "wb").insert("scale".to_string(), json!("kelvin"));
    }
    add_curves(&mut settings, &develop.curves);
    if let Some(crop) = develop.crop {
        object_entry(&mut settings, "crop").insert(
            "geometry".to_string(),
            json!({
                "rect": {
                    "x0": crop.x0,
                    "y0": crop.y0,
                    "x1": crop.x1,
                    "y1": crop.y1,
                },
                "angle": crop.angle,
            }),
        );
    }
    add_masks(&mut settings, &develop.masks);
    add_spots(&mut settings, &develop.spots);
    (!settings.is_empty()).then_some(Value::Object(settings))
}

/// Adds the masks as LightCraft's `masks` array. Only the set local sliders
/// appear in `adjust`; `amount` is always present. Masks travel only through
/// `develop.merge`, never through `develop.set`.
fn add_masks(settings: &mut Map<String, Value>, masks: &[Mask]) {
    if masks.is_empty() {
        return;
    }
    settings.insert(
        "masks".to_owned(),
        Value::Array(masks.iter().map(mask_json).collect()),
    );
}

/// Adds the spots as LightCraft's `spots` array. Spots travel only through
/// `develop.merge`, never through `develop.set`.
fn add_spots(settings: &mut Map<String, Value>, spots: &[Spot]) {
    if spots.is_empty() {
        return;
    }
    settings.insert(
        "spots".to_owned(),
        Value::Array(spots.iter().map(spot_json).collect()),
    );
}

fn spot_json(spot: &Spot) -> Value {
    json!({
        "mode": match spot.mode {
            SpotMode::Remove => "remove",
            SpotMode::Heal => "heal",
            SpotMode::Clone => "clone",
            SpotMode::Fill => "fill",
        },
        "points": spot.points.iter().map(point_json).collect::<Vec<_>>(),
        "size": spot.size,
        "feather": spot.feather,
        "opacity": spot.opacity,
        "source_offset": spot.source_offset.as_ref().map(point_json),
    })
}

fn mask_json(mask: &Mask) -> Value {
    json!({
        "id": mask.id,
        "name": mask.name,
        "visible": mask.visible,
        "invert": mask.invert,
        "components": mask.components.iter().map(component_json).collect::<Vec<_>>(),
        "adjust": mask.adjust.iter().map(|(key, value)| (key.clone(), json!(value))).collect::<Map<String, Value>>(),
    })
}

fn component_json(component: &MaskComponent) -> Value {
    json!({
        "op": match component.op {
            MaskOp::Add => "add",
            MaskOp::Subtract => "subtract",
            MaskOp::Intersect => "intersect",
        },
        "invert": component.invert,
        "shape": shape_json(&component.shape),
    })
}

fn shape_json(shape: &MaskShape) -> Value {
    match shape {
        MaskShape::Linear { start, end } => {
            json!({"kind": "linear", "start": point_json(start), "end": point_json(end)})
        }
        MaskShape::Radial {
            center,
            rx,
            ry,
            angle,
            feather,
            invert,
        } => json!({
            "kind": "radial",
            "center": point_json(center),
            "rx": rx,
            "ry": ry,
            "angle": angle,
            "feather": feather,
            "invert": invert,
        }),
        MaskShape::Brush { strokes } => json!({
            "kind": "brush",
            "strokes": strokes.iter().map(|stroke| json!({
                "points": stroke.points.iter().map(point_json).collect::<Vec<_>>(),
                "size": stroke.size,
                "feather": stroke.feather,
                "flow": stroke.flow,
                "density": stroke.density,
                "erase": stroke.erase,
            })).collect::<Vec<_>>(),
        }),
        MaskShape::LuminanceRange {
            lo,
            hi,
            lo_feather,
            hi_feather,
        } => json!({
            "kind": "luminanceRange",
            "lo": lo,
            "hi": hi,
            "lo_feather": lo_feather,
            "hi_feather": hi_feather,
        }),
        MaskShape::DepthRange { lo, hi, feather } => {
            json!({"kind": "depthRange", "lo": lo, "hi": hi, "feather": feather})
        }
        MaskShape::Subject => json!({"kind": "subject"}),
        MaskShape::Sky => json!({"kind": "sky"}),
        MaskShape::Background => json!({"kind": "background"}),
    }
}

fn point_json(point: &Point) -> Value {
    json!({"x": point.x, "y": point.y})
}

/// Whether every tone curve is empty.
fn curves_are_empty(curves: &Curves) -> bool {
    curves.master.is_empty()
        && curves.red.is_empty()
        && curves.green.is_empty()
        && curves.blue.is_empty()
}

/// Adds the non-empty curves as LightCraft's `curve` object: one array of
/// `{"x", "y"}` objects per present curve.
fn add_curves(settings: &mut Map<String, Value>, curves: &Curves) {
    let mut present = Map::new();
    for (name, points) in [
        ("master", &curves.master),
        ("red", &curves.red),
        ("green", &curves.green),
        ("blue", &curves.blue),
    ] {
        if !points.is_empty() {
            present.insert(name.to_string(), json!(curve_points(points)));
        }
    }
    if !present.is_empty() {
        settings.insert("curve".to_string(), Value::Object(present));
    }
}

/// One curve as LightCraft's point objects.
fn curve_points(points: &[CurvePoint]) -> Vec<Value> {
    points
        .iter()
        .map(|point| json!({"x": point.x, "y": point.y}))
        .collect()
}

/// Inserts `value` below a dotted path, creating intermediate objects.
fn insert_path(map: &mut Map<String, Value>, path: &str, value: Value) {
    match path.split_once('.') {
        None => {
            map.insert(path.to_string(), value);
        }
        Some((head, rest)) => {
            let entry = object_entry(map, head);
            insert_path(entry, rest, value);
        }
    }
}

/// The nested object stored under `key`, creating or replacing it if it is
/// not an object yet.
fn object_entry<'a>(map: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    let entry = map
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    // Provably infallible: the entry was just made an object.
    #[allow(clippy::expect_used)]
    entry
        .as_object_mut()
        .expect("the entry was just made an object")
}

/// The field values of a photo with its keyword paths rewritten to the
/// recorded path of a keyword renamed in Lightroom. A rename then looks like
/// no membership change at all: the new name is never added and the old one
/// is not counted as removed.
pub(crate) fn snapshot_with_renames(
    photo: &Photo,
    keywords: &[Keyword],
    renamed: &BTreeMap<String, String>,
) -> PhotoFields {
    let mut fields = snapshot(photo, keywords);
    if !renamed.is_empty() {
        for keyword in &mut fields.keywords {
            if let Some(old) = renamed.get(keyword) {
                *keyword = old.clone();
            }
        }
        fields.keywords.sort();
        fields.keywords.dedup();
    }
    fields
}

/// Finds recorded photos that are no longer in the input and recorded photos
/// whose original moved. Only Lightroom ids are reported, never paths.
fn plan_removed_and_moved(input: &WriteInput, state: &State, out: &mut Plan) {
    let mut present: BTreeMap<&str, &Photo> = BTreeMap::new();
    for photo in &input.photos {
        if let Some(gid) = photo.global_id.as_deref() {
            present.entry(gid).or_insert(photo);
        }
    }
    for (global_id, entry) in state.photos() {
        match present.get(global_id) {
            None => {
                out.removed_gids.insert(global_id.to_string());
                out.removed_photos.push(RemovedPhoto {
                    photo: entry.lightroom_id,
                });
            }
            Some(photo) => {
                if let (Some(recorded), Some(current)) =
                    (entry.path.as_deref(), photo.path.to_str())
                    && recorded != current
                {
                    out.moved_photos.push(MovedPhoto {
                        photo: entry.lightroom_id,
                    });
                }
            }
        }
    }
    out.removed_photos.sort_by_key(|r| r.photo);
    out.moved_photos.sort_by_key(|m| m.photo);
}

/// The position of a field name in the fixed conflict order.
fn field_order(field: &str) -> usize {
    match field {
        "rating" => 0,
        "flag" => 1,
        "label" => 2,
        "title" => 3,
        "caption" => 4,
        "copyright" => 5,
        "creator" => 6,
        // Not produced by the plan; keeps the sort total.
        _ => 7,
    }
}

/// The first global id of each Lightroom photo id in the input.
fn global_ids(input: &WriteInput) -> BTreeMap<i64, &str> {
    let mut out: BTreeMap<i64, &str> = BTreeMap::new();
    for p in &input.photos {
        if let Some(gid) = p.global_id.as_deref() {
            out.entry(p.lightroom_id).or_insert(gid);
        }
    }
    out
}

fn plan_stacks(input: &WriteInput, state: &State, out: &mut Plan) {
    let global_ids = global_ids(input);
    let by_id: BTreeMap<i64, &Stack> = input.stacks.iter().map(|s| (s.lightroom_id, s)).collect();
    // A photo can be in only one stack. Stacks are planned in ascending
    // Lightroom id order; the first stack to use a photo wins, and a later
    // stack whose members overlap it is reported (`AlreadyInStack`) rather
    // than half-written: grouping it would merge or steal the shared photo in
    // LightCraft, leaving the earlier stack incomplete.
    let mut used: BTreeSet<String> = BTreeSet::new();
    for s in by_id.values() {
        let mut gids: Vec<String> = Vec::new();
        for member in &s.members {
            if let Some(gid) = global_ids.get(member)
                && !gids.iter().any(|g| g == gid)
            {
                gids.push((*gid).to_string());
            }
        }
        if gids.len() < 2 {
            out.stack_skips.push(Skipped::new(
                SkipCode::TooFewPhotos,
                format!(
                    "stack {}: fewer than two photos with a global id; not grouped",
                    s.lightroom_id
                ),
            ));
            continue;
        }
        if let Some(shared) = gids.iter().find(|g| used.contains(*g)) {
            out.stack_skips.push(Skipped::new(
                SkipCode::AlreadyInStack,
                format!(
                    "stack {}: shares photo {} with an earlier stack; a photo can be in only one stack; not grouped",
                    s.lightroom_id, shared
                ),
            ));
            continue;
        }
        let recorded: &[String] = state
            .stack(s.lightroom_id)
            .map(|e| e.members.as_slice())
            .unwrap_or_default();
        out.stack_members_removed += recorded.iter().filter(|g| !gids.contains(g)).count();
        for g in &gids {
            used.insert(g.clone());
        }
        if state.stack(s.lightroom_id).is_some() && same_members(recorded, &gids) {
            out.stacks_known += 1;
        } else {
            out.steps.push(Step::GroupStack {
                stack_id: s.lightroom_id,
                gids,
            });
        }
    }
}

/// True if `recorded`, without the members that are not in `wanted`
/// (removed in Lightroom), equals `wanted` in the same order.
pub(crate) fn same_members(recorded: &[String], wanted: &[String]) -> bool {
    recorded
        .iter()
        .filter(|g| wanted.contains(g))
        .eq(wanted.iter())
}

fn plan_collections(input: &WriteInput, state: &State, out: &mut Plan) {
    let by_id: BTreeMap<i64, &Collection> = input
        .collections
        .iter()
        .map(|c| (c.lightroom_id, c))
        .collect();

    let mut new: Vec<&Collection> = Vec::new();
    for c in by_id.values() {
        match state.collection(c.lightroom_id) {
            Some(entry) => {
                out.collections_known += 1;
                if entry.name != c.name {
                    out.collections_renamed += 1;
                }
                if entry.parent != c.parent {
                    out.collections_moved += 1;
                }
            }
            None => new.push(c),
        }
    }
    let smart: BTreeSet<i64> = input.smart.iter().map(|s| s.lightroom_id).collect();
    let untranslatable: BTreeSet<i64> = input.smart_untranslatable_ids.iter().copied().collect();
    for (id, entry) in state.collections() {
        if by_id.contains_key(&id) || smart.contains(&id) {
            continue;
        }
        if entry.kind == EntryKind::Smart && untranslatable.contains(&id) {
            out.smart_no_longer_translatable += 1;
        } else {
            out.collections_removed += 1;
        }
    }

    // Folders first, then albums; within each, shallower before deeper, so
    // a parent always comes before its children.
    new.sort_by_key(|c| {
        (
            c.kind == CollectionKind::Album,
            depth(c, &by_id),
            c.lightroom_id,
        )
    });
    for c in new {
        out.steps.push(Step::CreateAlbum {
            collection_id: c.lightroom_id,
            params: create_album_params(c, state),
        });
    }

    let global_ids = global_ids(input);
    for c in by_id.values() {
        if c.kind != CollectionKind::Album {
            continue;
        }
        let mut wanted: Vec<String> = Vec::new();
        for member in &c.members {
            match global_ids.get(member) {
                Some(gid) => {
                    if !wanted.iter().any(|w| w == gid) {
                        wanted.push((*gid).to_string());
                    }
                }
                None => out.member_skips.push(Skipped::new(
                    SkipCode::NoGlobalId,
                    format!(
                        "photo {member}: not added to an album; the photo is not converted or \
                         has no global id"
                    ),
                )),
            }
        }
        let recorded: &[String] = state
            .collection(c.lightroom_id)
            .map(|e| e.members.as_slice())
            .unwrap_or_default();
        let add: Vec<String> = wanted
            .iter()
            .filter(|g| !recorded.contains(g))
            .cloned()
            .collect();
        let remove: Vec<String> = recorded
            .iter()
            .filter(|g| !wanted.contains(g))
            .cloned()
            .collect();
        if !add.is_empty() {
            out.steps.push(Step::AddMembers {
                collection_id: c.lightroom_id,
                gids: add,
            });
        }
        if !remove.is_empty() {
            out.steps.push(Step::RemoveMembers {
                collection_id: c.lightroom_id,
                gids: remove,
            });
        }
    }
    // Recorded albums whose collection is gone from Lightroom are kept, but
    // a photo removed in Lightroom still leaves them when lr2lc had added it.
    for (id, entry) in state.collections() {
        if by_id.contains_key(&id) || entry.kind != EntryKind::Album {
            continue;
        }
        let remove: Vec<String> = entry
            .members
            .iter()
            .filter(|g| out.removed_gids.contains(*g))
            .cloned()
            .collect();
        if !remove.is_empty() {
            out.steps.push(Step::RemoveMembers {
                collection_id: id,
                gids: remove,
            });
        }
    }
}

/// The number of ancestors of a collection within the input. A parent
/// chain that loops stops after as many steps as there are collections.
fn depth(c: &Collection, by_id: &BTreeMap<i64, &Collection>) -> usize {
    std::iter::successors(c.parent, |p| by_id.get(p).and_then(|c| c.parent))
        .take(by_id.len())
        .count()
}

fn create_album_params(c: &Collection, state: &State) -> Value {
    let mut params = json!({
        "name": c.name,
        "folder": c.kind == CollectionKind::Folder,
    });
    if let Some(parent) = c.parent {
        params["parent"] = album_ref(state, parent);
    }
    params
}

/// The commands that turn the fields `old` into `new`, at most one per
/// command, in a fixed order. Without a live state this is the two-way
/// comparison: every change in Lightroom is written. Keywords that left the
/// photo in Lightroom are never removed; the caller only counts them.
fn field_changes(old: &PhotoFields, new: &PhotoFields) -> Vec<(&'static str, Value)> {
    let mut out = Vec::new();
    if old.rating != new.rating {
        out.push(("photo.rate", json!({"rating": new.rating})));
    }
    if old.flag != new.flag {
        out.push(("photo.flag", json!({"flag": new.flag})));
    }
    if old.label != new.label {
        let label = new.label.as_deref().unwrap_or("none");
        out.push(("photo.label", json!({"label": label})));
    }

    let mut meta = Map::new();
    let texts = [
        ("title", &old.title, &new.title),
        ("caption", &old.caption, &new.caption),
        ("copyright", &old.copyright, &new.copyright),
        ("creator", &old.creator, &new.creator),
    ];
    for (key, before, after) in texts {
        if before != after {
            meta.insert(key.to_string(), json!(after.as_deref().unwrap_or("")));
        }
    }
    let before: BTreeSet<&String> = old.keywords.iter().collect();
    let after: BTreeSet<&String> = new.keywords.iter().collect();
    let added: Vec<&String> = after.difference(&before).copied().collect();
    if !added.is_empty() {
        meta.insert("addKeywords".to_string(), json!(added));
    }
    if !meta.is_empty() {
        out.push(("photo.setMeta", Value::Object(meta)));
    }
    out
}

/// What a three-way comparison decides for one field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    /// `lr == last`: Lightroom did not change it; nothing to write.
    Nothing,
    /// `lr != last`, `lc == last`: write the Lightroom value.
    Write,
    /// `lr != last`, `lc == lr`: both sides already agree; nothing to write.
    AlreadyEqual,
    /// `lr != last`, `lc != last`, `lc != lr`: a conflict, write nothing.
    Conflict,
}

fn decide<T: PartialEq>(last: &T, lr: &T, lc: &T) -> Decision {
    if lr == last {
        Decision::Nothing
    } else if lc == last {
        Decision::Write
    } else if lc == lr {
        Decision::AlreadyEqual
    } else {
        Decision::Conflict
    }
}

/// The commands that bring one known photo from its last imported values to
/// the Lightroom values, given what LightCraft has now, and the values to
/// record afterwards. A conflicting field is not written and keeps its last
/// imported value in the mapping file, so a later run reports it again.
/// `photo` is the Lightroom id used in the conflict report.
fn reconcile(
    old: &PhotoFields,
    new: &PhotoFields,
    live: &LivePhoto,
    photo: i64,
    conflicts: &mut Vec<Conflict>,
) -> (Vec<(&'static str, Value)>, PhotoFields) {
    let mut changes = Vec::new();
    let mut recorded = old.clone();
    let mut conflict = |field: &str| {
        conflicts.push(Conflict {
            photo,
            field: field.to_string(),
        })
    };

    let decision = decide(&old.rating, &new.rating, &live.rating);
    if decision == Decision::Write {
        changes.push(("photo.rate", json!({"rating": new.rating})));
    }
    if decision == Decision::Conflict {
        conflict("rating");
    } else {
        recorded.rating = new.rating;
    }

    let decision = decide(&old.flag, &new.flag, &live.flag);
    if decision == Decision::Write {
        changes.push(("photo.flag", json!({"flag": new.flag})));
    }
    if decision == Decision::Conflict {
        conflict("flag");
    } else {
        recorded.flag = new.flag.clone();
    }

    let decision = decide(&old.label, &new.label, &live.label);
    if decision == Decision::Write {
        let label = new.label.as_deref().unwrap_or("none");
        changes.push(("photo.label", json!({"label": label})));
    }
    if decision == Decision::Conflict {
        conflict("label");
    } else {
        recorded.label = new.label.clone();
    }

    let mut meta = Map::new();
    macro_rules! text_field {
        ($field:ident, $name:literal) => {{
            let decision = decide(&old.$field, &new.$field, &live.$field);
            match decision {
                Decision::Write => {
                    meta.insert(
                        $name.to_string(),
                        json!(new.$field.as_deref().unwrap_or("")),
                    );
                    recorded.$field = new.$field.clone();
                }
                Decision::Nothing | Decision::AlreadyEqual => {
                    recorded.$field = new.$field.clone();
                }
                Decision::Conflict => conflict($name),
            }
        }};
    }
    text_field!(title, "title");
    text_field!(caption, "caption");
    text_field!(copyright, "copyright");
    text_field!(creator, "creator");

    // Keywords, element by element: a keyword new in Lightroom is added only
    // if LightCraft does not have it yet; one only in LightCraft is kept.
    let last: BTreeSet<&String> = old.keywords.iter().collect();
    let lightroom: BTreeSet<&String> = new.keywords.iter().collect();
    let added: Vec<&str> = lightroom
        .difference(&last)
        .filter(|k| !live.keywords.iter().any(|have| have.as_str() == k.as_str()))
        .map(|k| k.as_str())
        .collect();
    if !added.is_empty() {
        meta.insert("addKeywords".to_string(), json!(added));
    }
    // Keywords that left the photo in Lightroom stay in LightCraft; they are
    // not recorded any more, so a later run does not report them again.
    recorded.keywords = new.keywords.clone();

    if !meta.is_empty() {
        changes.push(("photo.setMeta", Value::Object(meta)));
    }
    (changes, recorded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{BrushStroke, ColorLabel, CropGeometry, Flag, Rating};

    fn kw(parts: &[&str]) -> Keyword {
        Keyword::new(parts.iter().map(|s| s.to_string()).collect()).unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn import_params_rejects_a_non_utf8_path() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let path = std::path::Path::new(OsStr::from_bytes(b"/photos/\xff.jpg"));
        assert_eq!(import_params(path), None);
        assert_eq!(
            import_params(std::path::Path::new("/photos/a.jpg")),
            Some(json!({"paths": ["/photos/a.jpg"], "mode": "add"}))
        );
    }

    #[test]
    fn smart_entry_without_recorded_rules_counts_as_changed() {
        use crate::state::{CollectionEntry, EntryKind};
        use model::{SmartCombine, SmartRule, SmartRules, SmartValue};
        let rules = SmartRules {
            combine: SmartCombine::Union,
            rules: vec![SmartRule {
                criterion: "rating".into(),
                operation: "==".into(),
                value: Some(SmartValue::Number(2.0)),
                value2: None,
                units: None,
            }],
        };
        let mut state = State::default();
        state.record_collection(
            4,
            CollectionEntry {
                lightcraft_id: json!(9),
                kind: EntryKind::Smart,
                name: "s".into(),
                parent: None,
                members: Vec::new(),
                rules: None,
            },
        );
        let input = WriteInput {
            smart: vec![SmartCollection {
                lightroom_id: 4,
                name: "s".into(),
                parent: None,
                rules,
            }],
            ..WriteInput::default()
        };
        let p = plan(&input, &state, &BTreeSet::new());
        assert!(p.steps.is_empty());
        assert_eq!((p.smart_known(), p.smart_rules_changed()), (1, 1));
        assert_eq!(p.collections_removed(), 0);
        assert_eq!(
            smart_skip_reason(4, "why"),
            "smart collection 4: why; not converted"
        );
    }

    #[test]
    fn untranslatable_smart_entries_are_counted_apart_from_removed() {
        use crate::state::CollectionEntry;
        let entry = |kind| CollectionEntry {
            lightcraft_id: json!(1),
            kind,
            name: "x".into(),
            parent: None,
            members: Vec::new(),
            rules: None,
        };
        let mut state = State::default();
        // 1: smart, still read but untranslatable; 2: smart, gone;
        // 3: regular album whose id is (wrongly) in the list; 4: regular, gone.
        state.record_collection(1, entry(EntryKind::Smart));
        state.record_collection(2, entry(EntryKind::Smart));
        state.record_collection(3, entry(EntryKind::Album));
        state.record_collection(4, entry(EntryKind::Album));
        let input = WriteInput {
            smart_untranslatable_ids: vec![1, 3],
            ..WriteInput::default()
        };
        let p = plan(&input, &state, &BTreeSet::new());
        assert!(p.steps.is_empty());
        assert_eq!(p.smart_no_longer_translatable(), 1);
        assert_eq!(p.collections_removed(), 3);

        let none = plan(&WriteInput::default(), &state, &BTreeSet::new());
        assert_eq!(none.smart_no_longer_translatable(), 0);
        assert_eq!(none.collections_removed(), 4);
    }

    #[test]
    fn the_quick_step_is_not_in_the_script() {
        let p = Plan {
            steps: vec![Step::QuickAdd {
                gids: vec!["g1".into()],
            }],
            ..Plan::default()
        };
        assert_eq!(p.to_script(&State::default()), "");
    }

    #[test]
    fn same_members_ignores_removed_but_not_order_or_new_ones() {
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(same_members(&v(&["a", "b"]), &v(&["a", "b"])));
        assert!(same_members(&v(&["a", "x", "b"]), &v(&["a", "b"])));
        assert!(!same_members(&v(&["b", "a"]), &v(&["a", "b"])));
        assert!(!same_members(&v(&["a", "b"]), &v(&["a", "b", "c"])));
        assert!(!same_members(&v(&[]), &v(&["a", "b"])));
    }

    #[test]
    fn no_change_gives_no_command() {
        let f = PhotoFields::imported_default();
        assert!(field_changes(&f, &f).is_empty());
    }

    #[test]
    fn all_fields_changed_give_four_commands_in_order() {
        let p = Photo::new("a.jpg", 1, Rating::new(2).unwrap())
            .with_flag(Flag::Pick)
            .with_label(ColorLabel::Blue)
            .with_creator("c");
        let new = snapshot(&p, &[kw(&["K"])]);
        let c = field_changes(&PhotoFields::imported_default(), &new);
        let names: Vec<&str> = c.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            ["photo.rate", "photo.flag", "photo.label", "photo.setMeta"]
        );
        assert_eq!(c[3].1, json!({"creator": "c", "addKeywords": ["K"]}));
    }

    #[test]
    fn duplicate_global_id_is_skipped_once() {
        let a = Photo::new("a.jpg", 1, Rating::default()).with_global_id("g");
        let b = Photo::new("b.jpg", 2, Rating::default()).with_global_id("g");
        let input = WriteInput {
            photos: vec![a, b],
            ..WriteInput::default()
        };
        let p = plan(&input, &State::default(), &BTreeSet::new());
        assert_eq!(p.steps.len(), 1);
        assert_eq!(p.skipped.len(), 1);
        assert!(p.skipped[0].reason.contains('2'));
        assert_eq!(p.skipped[0].code, SkipCode::DuplicateGlobalId);
    }

    #[cfg(unix)]
    #[test]
    fn path_that_is_not_utf8_is_skipped_with_its_code() {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::OsStr::from_bytes(b"a\xff.jpg");
        let input = WriteInput {
            photos: vec![Photo::new(path, 3, Rating::default()).with_global_id("g")],
            ..WriteInput::default()
        };
        let p = plan(&input, &State::default(), &BTreeSet::new());
        assert!(p.steps.is_empty());
        assert_eq!(p.photo_skips()[0].code, SkipCode::PathNotUtf8);
    }

    #[test]
    fn members_without_a_global_id_are_skipped_with_their_code() {
        let input = WriteInput {
            photos: vec![Photo::new("a.jpg", 1, Rating::default())],
            collections: vec![Collection {
                kind: CollectionKind::Album,
                members: vec![1],
                ..collection(5, None)
            }],
            quick: Some(QuickCollection {
                lightroom_id: 9,
                members: vec![1],
            }),
            stacks: vec![Stack {
                lightroom_id: 4,
                members: vec![1, 2],
                collapsed: false,
            }],
            ..WriteInput::default()
        };
        let p = plan(&input, &State::default(), &BTreeSet::new());
        assert_eq!(p.member_skips()[0].code, SkipCode::NoGlobalId);
        assert_eq!(p.quick_skips()[0].code, SkipCode::NoGlobalId);
        assert_eq!(p.stack_skips()[0].code, SkipCode::TooFewPhotos);
    }

    #[test]
    fn keyword_listed_twice_is_created_once() {
        let input = WriteInput {
            keywords: vec![kw(&["A"]), kw(&["A"])],
            ..WriteInput::default()
        };
        assert_eq!(
            plan(&input, &State::default(), &BTreeSet::new())
                .steps
                .len(),
            1
        );
    }

    #[test]
    fn implicit_ancestor_is_not_compared_with_the_state() {
        let mut parent = kw(&["A"]);
        parent.person = true;
        let mut state = State::default();
        state.record_keyword(&parent);
        let input = WriteInput {
            keywords: vec![kw(&["A", "B"])],
            ..WriteInput::default()
        };
        let p = plan(&input, &state, &BTreeSet::new());
        assert!(p.skipped.is_empty());
        assert_eq!(p.steps.len(), 1);
    }

    #[test]
    fn skips_are_split_into_keywords_and_photos() {
        let mut changed = kw(&["A"]);
        changed.person = true;
        let mut state = State::default();
        state.record_keyword(&kw(&["A"]));
        let input = WriteInput {
            photos: vec![Photo::new("a.jpg", 7, Rating::default())],
            keywords: vec![changed],
            ..WriteInput::default()
        };
        let p = plan(&input, &state, &BTreeSet::new());
        assert_eq!(p.keyword_skips().len(), 1);
        assert_eq!(p.photo_skips().len(), 1);
        assert!(p.photo_skips()[0].reason.contains('7'));
        assert_eq!(p.keyword_skips()[0].code, SkipCode::KeywordChanged);
        assert_eq!(p.photo_skips()[0].code, SkipCode::MissingGlobalId);
    }

    #[test]
    fn known_photos_count_as_updated_or_unchanged() {
        let photos = vec![
            Photo::new("a.jpg", 1, Rating::new(2).unwrap()).with_global_id("a"),
            Photo::new("b.jpg", 2, Rating::new(3).unwrap()).with_global_id("b"),
            Photo::new("c.jpg", 3, Rating::default()).with_global_id("c"),
            Photo::new("d.jpg", 4, Rating::default()).with_global_id("c"),
            Photo::new("e.jpg", 5, Rating::default()),
        ];
        let mut state = State::default();
        let old = Photo::new("a.jpg", 1, Rating::new(1).unwrap());
        state.record("a", 1, json!(10), snapshot(&old, &[]));
        state.record("c", 3, json!(30), PhotoFields::imported_default());
        let input = WriteInput {
            photos,
            ..WriteInput::default()
        };
        let p = plan(&input, &state, &BTreeSet::new());
        assert_eq!(p.photos_updated(), 1);
        assert_eq!(p.photos_unchanged(), 1);
        let imports = p
            .steps
            .iter()
            .filter(|s| matches!(s, Step::Import { .. }))
            .count();
        assert_eq!(imports, 1);
        assert_eq!(p.photo_skips().len(), 2);
        let total = imports + p.photos_updated() + p.photos_unchanged() + p.photo_skips().len();
        assert_eq!(total, input.photos.len());
    }

    fn collection(id: i64, parent: Option<i64>) -> Collection {
        Collection {
            lightroom_id: id,
            name: format!("c{id}"),
            kind: CollectionKind::Folder,
            parent,
            members: Vec::new(),
        }
    }

    #[test]
    fn depth_counts_ancestors_and_stops_on_a_loop() {
        let cols = [
            collection(1, None),
            collection(2, Some(1)),
            collection(3, Some(2)),
            collection(4, Some(5)),
            collection(5, Some(4)),
            collection(6, Some(99)),
        ];
        let by_id: BTreeMap<i64, &Collection> = cols.iter().map(|c| (c.lightroom_id, c)).collect();
        let d: Vec<usize> = cols.iter().map(|c| depth(c, &by_id)).collect();
        assert_eq!(d, [0, 1, 2, 6, 6, 1]);
    }

    #[test]
    fn members_select_makes_the_first_photo_active() {
        assert_eq!(
            members_select_params(&[json!(3), json!(1)]),
            json!({"ids": [3, 1], "active": 3})
        );
    }

    #[test]
    fn unknown_parent_is_sent_as_placeholder() {
        let params = create_album_params(&collection(6, Some(99)), &State::default());
        assert_eq!(
            params,
            json!({"name": "c6", "folder": true, "parent": "lr-collection:99"})
        );
    }

    #[test]
    fn deep_parent_is_sent_as_path_text() {
        assert_eq!(
            create_keyword_params(&kw(&["A", "B", "C"]))["parent"],
            json!("A|B")
        );
    }

    #[test]
    fn script_line_of_an_update_carries_ids() {
        let plan = Plan {
            steps: vec![Step::Update {
                global_id: "g".into(),
                command: "photo.rate".into(),
                params: json!({"rating": 1}),
            }],
            ..Plan::default()
        };
        let script = plan.to_script(&State::default());
        let lines: Vec<Value> = script
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(
            lines,
            [
                json!({"method": "engine.execute", "params": {
                    "command": "library.select",
                    "params": {"ids": ["lr:g"], "active": "lr:g"}
                }}),
                json!({"method": "engine.execute", "params": {
                    "command": "photo.rate",
                    "params": {"rating": 1, "ids": ["lr:g"]}
                }}),
            ]
        );
    }

    #[test]
    fn script_selects_once_per_photo_before_its_updates() {
        let update = |gid: &str, command: &str| Step::Update {
            global_id: gid.into(),
            command: command.into(),
            params: json!({}),
        };
        let plan = Plan {
            steps: vec![
                update("a", "photo.rate"),
                update("a", "photo.flag"),
                Step::Import {
                    global_id: "b".into(),
                    path: PathBuf::from("b.jpg"),
                },
                update("b", "photo.rate"),
                update("c", "photo.label"),
            ],
            ..Plan::default()
        };
        let mut state = State::default();
        state.record("a", 1, json!(7), PhotoFields::imported_default());
        let commands: Vec<(String, Value)> = plan
            .to_script(&state)
            .lines()
            .map(|l| {
                let v: Value = serde_json::from_str(l).unwrap();
                (
                    v["params"]["command"].as_str().unwrap().to_string(),
                    v["params"]["params"]["ids"].clone(),
                )
            })
            .collect();
        let names: Vec<&str> = commands.iter().map(|(c, _)| c.as_str()).collect();
        assert_eq!(
            names,
            [
                "library.select",
                "photo.rate",
                "photo.flag",
                "library.import",
                "library.select",
                "photo.rate",
                "library.select",
                "photo.label",
            ]
        );
        assert_eq!(commands[0].1, json!([7]));
        assert_eq!(commands[4].1, json!(["lr:b"]));
        assert_eq!(commands[6].1, json!(["lr:c"]));
    }

    #[test]
    fn live_state_is_only_needed_when_lightroom_changed() {
        let photo =
            |rating: u8| Photo::new("a.jpg", 7, Rating::new(rating).unwrap()).with_global_id("g1");
        let mut state = State::default();
        state.record("g1", 7, json!(1001), PhotoFields::imported_default());

        let unchanged = WriteInput {
            photos: vec![photo(0)],
            ..WriteInput::default()
        };
        assert!(!needs_live(&unchanged, &state));

        let changed = WriteInput {
            photos: vec![photo(3)],
            ..WriteInput::default()
        };
        assert!(needs_live(&changed, &state));

        // A keyword that left the photo needs no live read: it is only
        // counted. A keyword new in Lightroom does, as LightCraft may have
        // it already.
        let mut last = PhotoFields::imported_default();
        last.keywords = vec!["K".into()];
        state.record("g1", 7, json!(1001), last);
        let fewer = WriteInput {
            photos: vec![photo(0)],
            ..WriteInput::default()
        };
        assert!(!needs_live(&fewer, &state));
        state.record("g1", 7, json!(1001), PhotoFields::imported_default());
        let mut photo_keywords = BTreeMap::new();
        photo_keywords.insert(7, vec![kw(&["K"])]);
        let more = WriteInput {
            photos: vec![photo(0)],
            photo_keywords,
            ..WriteInput::default()
        };
        assert!(needs_live(&more, &state));
    }

    #[test]
    fn a_recorded_photo_missing_from_the_model_is_reported() {
        let mut state = State::default();
        state.record("gone", 4, json!(1004), PhotoFields::imported_default());
        state.record("kept", 5, json!(1005), PhotoFields::imported_default());
        let input = WriteInput {
            photos: vec![
                Photo::new("kept.jpg", 5, Rating::default()).with_global_id("kept"),
                Photo::new("new.jpg", 6, Rating::default()).with_global_id("new"),
            ],
            ..WriteInput::default()
        };
        let p = plan(&input, &state, &BTreeSet::new());
        assert_eq!(p.removed_photos(), [RemovedPhoto { photo: 4 }]);
        assert!(p.moved_photos().is_empty());
    }

    #[test]
    fn a_moved_photo_is_reported_and_not_imported_again() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.jpg");
        let mut state = State::default();
        state.record("g", 1, json!(1001), PhotoFields::imported_default());
        assert!(state.remember_path("g", &old));
        let input = WriteInput {
            photos: vec![
                Photo::new(dir.path().join("new.jpg"), 1, Rating::default()).with_global_id("g"),
            ],
            ..WriteInput::default()
        };
        let p = plan(&input, &state, &BTreeSet::new());
        assert_eq!(p.moved_photos(), [MovedPhoto { photo: 1 }]);
        assert!(p.steps.is_empty(), "{:?}", p.steps);
        // The recorded base path stays, so the move is reported again.
        assert_eq!(state.photo("g").unwrap().path.as_deref(), old.to_str());
    }

    #[test]
    fn a_renamed_keyword_is_reported_but_neither_created_nor_added() {
        let mut state = State::default();
        state.record_keyword(&kw(&["Old"]).with_global_id("kg-1").with_lightroom_id(13));
        let mut last = PhotoFields::imported_default();
        last.keywords = vec!["Old".into()];
        state.record("g", 7, json!(1001), last);
        let renamed = kw(&["New"]).with_global_id("kg-1").with_lightroom_id(13);
        let mut photo_keywords = BTreeMap::new();
        photo_keywords.insert(7, vec![renamed.clone()]);
        let input = WriteInput {
            photos: vec![Photo::new("a.jpg", 7, Rating::default()).with_global_id("g")],
            keywords: vec![renamed],
            photo_keywords,
            ..WriteInput::default()
        };
        let p = plan(&input, &state, &BTreeSet::new());
        assert_eq!(p.renamed_keywords(), [RenamedKeyword { keyword: 13 }]);
        assert!(
            !p.steps
                .iter()
                .any(|s| matches!(s, Step::CreateKeyword { .. } | Step::Update { .. })),
            "{:?}",
            p.steps
        );
        assert_eq!(p.keywords_removed(), 0);
    }

    #[test]
    fn a_rename_in_an_older_mapping_file_is_reported_by_the_model_id() {
        let mut state = State::default();
        // A mapping file from before the Lightroom id was recorded keeps the
        // global id but no `lightroom_id`.
        state.record_keyword(&kw(&["Old"]).with_global_id("kg-1"));
        let mut last = PhotoFields::imported_default();
        last.keywords = vec!["Old".into()];
        state.record("g", 7, json!(1001), last);
        let renamed = kw(&["New"]).with_global_id("kg-1").with_lightroom_id(13);
        let mut photo_keywords = BTreeMap::new();
        photo_keywords.insert(7, vec![renamed.clone()]);
        let input = WriteInput {
            photos: vec![Photo::new("a.jpg", 7, Rating::default()).with_global_id("g")],
            keywords: vec![renamed],
            photo_keywords,
            ..WriteInput::default()
        };
        let p = plan(&input, &state, &BTreeSet::new());
        // The id comes from the model keyword the reader filled.
        assert_eq!(p.renamed_keywords(), [RenamedKeyword { keyword: 13 }]);
        assert!(
            !p.steps
                .iter()
                .any(|s| matches!(s, Step::CreateKeyword { .. })),
            "{:?}",
            p.steps
        );
        // The new name is never added to a photo.
        let added_new = p.steps.iter().any(|s| match s {
            Step::Update { params, .. } => params
                .get("addKeywords")
                .and_then(Value::as_array)
                .is_some_and(|names| names.iter().any(|n| n.as_str() == Some("New"))),
            _ => false,
        });
        assert!(!added_new, "{:?}", p.steps);
    }

    #[test]
    fn a_renamed_parent_reports_its_descendants_too() {
        let mut state = State::default();
        state.record_keyword(&kw(&["A"]).with_global_id("kg-a").with_lightroom_id(1));
        state.record_keyword(&kw(&["A", "B"]).with_global_id("kg-b").with_lightroom_id(2));
        let parent = kw(&["A2"]).with_global_id("kg-a").with_lightroom_id(1);
        let child = kw(&["A2", "B"]).with_global_id("kg-b").with_lightroom_id(2);
        let input = WriteInput {
            keywords: vec![parent, child],
            ..WriteInput::default()
        };
        let p = plan(&input, &state, &BTreeSet::new());
        // The child kept its name but its path text changed with the parent.
        assert_eq!(
            p.renamed_keywords(),
            [RenamedKeyword { keyword: 1 }, RenamedKeyword { keyword: 2 }]
        );
        assert!(p.steps.is_empty(), "{:?}", p.steps);
    }

    #[test]
    fn develop_content_counts_curves_and_crop_but_not_empty_curves() {
        assert!(!has_develop_content(&Develop::default()));
        let curved = Develop {
            curves: Curves {
                master: vec![CurvePoint { x: 0.0, y: 0.0 }],
                ..Curves::default()
            },
            ..Develop::default()
        };
        assert!(has_develop_content(&curved));
        let cropped = Develop {
            crop: Some(CropGeometry::default()),
            ..Develop::default()
        };
        assert!(has_develop_content(&cropped));
    }

    #[test]
    fn a_camera_profile_alone_plans_no_command_but_is_counted_by_name() {
        let profile = Develop {
            camera_profile: Some("Profil Ärger 📷".to_owned()),
            source_hash: Some([0x11; 32]),
            ..Develop::default()
        };
        let photos = vec![
            Photo::new("a.jpg", 1, Rating::default())
                .with_global_id("a")
                .with_develop(profile.clone()),
            Photo::new("b.jpg", 2, Rating::default())
                .with_global_id("b")
                .with_develop(profile),
        ];
        let input = WriteInput {
            photos,
            ..WriteInput::default()
        };
        let p = plan(&input, &State::default(), &BTreeSet::new());
        assert_eq!(p.develop_photos(), 2);
        assert_eq!(p.develop_planned(), 0);
        assert!(
            !p.steps
                .iter()
                .any(|s| matches!(s, Step::DevelopSet { .. } | Step::DevelopMerge { .. })),
            "{:?}",
            p.steps
        );
        assert_eq!(p.develop_camera_profiles().get("Profil Ärger 📷"), Some(&2));
        assert_eq!(p.develop_photos_with_profile(), 2);
    }

    #[test]
    fn an_empty_camera_profile_is_neither_listed_nor_counted() {
        let develop = Develop {
            camera_profile: Some(String::new()),
            source_hash: Some([0x22; 32]),
            ..Develop::default()
        };
        let input = WriteInput {
            photos: vec![
                Photo::new("a.jpg", 1, Rating::default())
                    .with_global_id("a")
                    .with_develop(develop),
            ],
            ..WriteInput::default()
        };
        let p = plan(&input, &State::default(), &BTreeSet::new());
        assert!(p.develop_camera_profiles().is_empty());
        assert_eq!(p.develop_photos_with_profile(), 0);
    }

    #[test]
    fn a_profile_does_not_suppress_slider_commands() {
        let mut develop = Develop {
            camera_profile: Some("Studio Neutral".to_owned()),
            source_hash: Some([0x33; 32]),
            ..Develop::default()
        };
        develop.controls.insert("light.exposure".to_owned(), 1.25);
        let input = WriteInput {
            photos: vec![
                Photo::new("a.jpg", 1, Rating::default())
                    .with_global_id("a")
                    .with_develop(develop),
            ],
            ..WriteInput::default()
        };
        let p = plan(&input, &State::default(), &BTreeSet::new());
        assert_eq!(p.develop_planned(), 1);
        assert!(matches!(
            p.steps.as_slice(),
            [Step::Import { .. }, Step::DevelopSet { .. }]
        ));
    }

    #[test]
    fn develop_settings_write_only_present_curves_in_lightcraft_shape() {
        let develop = Develop {
            curves: Curves {
                master: vec![
                    CurvePoint { x: 0.0, y: 0.0 },
                    CurvePoint { x: 0.25, y: 0.35 },
                ],
                blue: vec![CurvePoint { x: 1.0, y: 0.5 }],
                ..Curves::default()
            },
            ..Develop::default()
        };
        let settings = develop_settings(&develop).expect("curve settings");
        assert_eq!(
            settings,
            json!({
                "curve": {
                    "master": [{"x": 0.0, "y": 0.0}, {"x": 0.25, "y": 0.35}],
                    "blue": [{"x": 1.0, "y": 0.5}],
                }
            })
        );
        assert!(
            settings["curve"].get("red").is_none() && settings["curve"].get("green").is_none(),
            "empty lists would add keys without a value"
        );
    }

    #[test]
    fn develop_settings_nests_the_crop_geometry_with_its_angle() {
        let develop = Develop {
            crop: Some(CropGeometry {
                x0: 0.1,
                y0: 0.2,
                x1: 0.9,
                y1: 0.8,
                angle: -2.5,
            }),
            ..Develop::default()
        };
        assert_eq!(
            develop_settings(&develop),
            Some(json!({"crop": {"geometry": {
                "rect": {"x0": 0.1, "y0": 0.2, "x1": 0.9, "y1": 0.8},
                "angle": -2.5,
            }}}))
        );
    }

    fn sample_mask(id: u32) -> Mask {
        Mask {
            id,
            name: "Mask".into(),
            visible: true,
            invert: false,
            components: vec![
                MaskComponent {
                    op: MaskOp::Add,
                    invert: false,
                    shape: MaskShape::Linear {
                        start: Point { x: 0.1, y: 0.2 },
                        end: Point { x: 0.9, y: 0.8 },
                    },
                },
                MaskComponent {
                    op: MaskOp::Subtract,
                    invert: true,
                    shape: MaskShape::Brush {
                        strokes: vec![BrushStroke {
                            points: vec![Point { x: 0.1, y: 0.2 }],
                            size: 0.05,
                            feather: 40.0,
                            flow: 75.0,
                            density: 80.0,
                            erase: false,
                        }],
                    },
                },
            ],
            adjust: BTreeMap::from([("amount".to_string(), 100.0), ("exposure".to_string(), 2.0)]),
        }
    }

    #[test]
    fn masks_serialize_in_lightcrafts_shape() {
        let develop = Develop {
            masks: vec![sample_mask(1)],
            ..Develop::default()
        };
        assert_eq!(
            develop_settings(&develop),
            Some(json!({"masks": [{
                "id": 1,
                "name": "Mask",
                "visible": true,
                "invert": false,
                "components": [
                    {
                        "op": "add",
                        "invert": false,
                        "shape": {
                            "kind": "linear",
                            "start": {"x": 0.1, "y": 0.2},
                            "end": {"x": 0.9, "y": 0.8},
                        },
                    },
                    {
                        "op": "subtract",
                        "invert": true,
                        "shape": {
                            "kind": "brush",
                            "strokes": [{
                                "points": [{"x": 0.1, "y": 0.2}],
                                "size": 0.05,
                                "feather": 40.0,
                                "flow": 75.0,
                                "density": 80.0,
                                "erase": false,
                            }],
                        },
                    },
                ],
                "adjust": {"amount": 100.0, "exposure": 2.0},
            }]}))
        );
    }

    #[test]
    fn every_mask_shape_serializes_with_its_kind() {
        let shapes = vec![
            (
                MaskShape::Radial {
                    center: Point { x: 0.5, y: 0.35 },
                    rx: 0.3,
                    ry: 0.125,
                    angle: 15.0,
                    feather: 40.0,
                    invert: false,
                },
                json!({
                    "kind": "radial",
                    "center": {"x": 0.5, "y": 0.35},
                    "rx": 0.3,
                    "ry": 0.125,
                    "angle": 15.0,
                    "feather": 40.0,
                    "invert": false,
                }),
            ),
            (
                MaskShape::LuminanceRange {
                    lo: 0.2,
                    hi: 0.8,
                    lo_feather: 0.1,
                    hi_feather: 0.05,
                },
                json!({
                    "kind": "luminanceRange",
                    "lo": 0.2,
                    "hi": 0.8,
                    "lo_feather": 0.1,
                    "hi_feather": 0.05,
                }),
            ),
            (
                MaskShape::DepthRange {
                    lo: 0.2,
                    hi: 0.8,
                    feather: 0.1,
                },
                json!({"kind": "depthRange", "lo": 0.2, "hi": 0.8, "feather": 0.1}),
            ),
            (MaskShape::Subject, json!({"kind": "subject"})),
            (MaskShape::Sky, json!({"kind": "sky"})),
            (MaskShape::Background, json!({"kind": "background"})),
        ];
        for (shape, expected) in shapes {
            assert_eq!(shape_json(&shape), expected);
        }
    }

    #[test]
    fn masks_travel_only_through_develop_merge() {
        let develop = Develop {
            controls: BTreeMap::from([("light.exposure".to_string(), 0.5)]),
            masks: vec![sample_mask(1)],
            ..Develop::default()
        };
        let mut out = Plan::default();
        plan_develop("gid", None, &develop, false, &mut out);
        assert_eq!(out.steps.len(), 2);
        let Step::DevelopSet { values, .. } = &out.steps[0] else {
            panic!("{:?}", out.steps[0]);
        };
        assert_eq!(values.get("light.exposure"), Some(&0.5));
        assert!(!values.contains_key("masks"));
        let Step::DevelopMerge { settings, .. } = &out.steps[1] else {
            panic!("{:?}", out.steps[1]);
        };
        assert!(settings["masks"].is_array());
        assert_eq!(settings.as_object().map(|map| map.len()), Some(1));
    }

    #[test]
    fn a_recorded_hash_leaves_masks_unwritten() {
        let source = [7u8; 32];
        let develop = Develop {
            source_hash: Some(source),
            masks: vec![sample_mask(1)],
            ..Develop::default()
        };
        assert!(has_develop_content(&develop));
        let known = crate::state::PhotoEntry {
            lightroom_id: 1,
            lightcraft_id: json!(5),
            fields: crate::state::PhotoFields::default(),
            path: None,
            copy_of: None,
            develop_hash: Some(hex_hash(&source)),
        };
        let mut out = Plan::default();
        plan_develop("gid", Some(&known), &develop, false, &mut out);
        assert!(out.steps.is_empty(), "{:?}", out.steps);
        assert_eq!(out.develop_planned, 0);
    }

    #[test]
    fn masks_alone_make_develop_content() {
        let develop = Develop {
            masks: vec![sample_mask(1)],
            ..Develop::default()
        };
        assert!(has_develop_content(&develop));
        assert_eq!(
            develop_settings(&Develop::default()),
            None,
            "an empty develop sends nothing"
        );
    }

    fn sample_spot() -> Spot {
        Spot {
            mode: SpotMode::Heal,
            points: vec![Point { x: 0.4, y: 0.3 }],
            size: 0.04,
            feather: 50.0,
            opacity: 80.0,
            source_offset: Some(Point { x: 0.2, y: 0.2 }),
        }
    }

    #[test]
    fn spots_alone_make_develop_content() {
        let develop = Develop {
            spots: vec![sample_spot()],
            ..Develop::default()
        };
        assert!(has_develop_content(&develop));
        let settings = develop_settings(&develop).expect("spots are merge content");
        assert_eq!(
            settings,
            json!({
                "spots": [{
                    "mode": "heal",
                    "points": [{"x": 0.4, "y": 0.3}],
                    "size": 0.04,
                    "feather": 50.0,
                    "opacity": 80.0,
                    "source_offset": {"x": 0.2, "y": 0.2},
                }]
            })
        );
    }

    #[test]
    fn every_spot_mode_serializes_with_its_name() {
        for (mode, name) in [
            (SpotMode::Remove, "remove"),
            (SpotMode::Heal, "heal"),
            (SpotMode::Clone, "clone"),
            (SpotMode::Fill, "fill"),
        ] {
            let spot = Spot {
                mode,
                ..sample_spot()
            };
            assert_eq!(spot_json(&spot)["mode"], json!(name));
        }
    }

    #[test]
    fn a_recorded_hash_leaves_spots_unwritten() {
        let source = [9u8; 32];
        let develop = Develop {
            source_hash: Some(source),
            spots: vec![sample_spot()],
            ..Develop::default()
        };
        let known = crate::state::PhotoEntry {
            lightroom_id: 1,
            lightcraft_id: json!(5),
            fields: crate::state::PhotoFields::default(),
            path: None,
            copy_of: None,
            develop_hash: Some(hex_hash(&source)),
        };
        let mut out = Plan::default();
        plan_develop("gid", Some(&known), &develop, false, &mut out);
        assert!(out.steps.is_empty(), "{:?}", out.steps);
        assert_eq!(out.develop_planned, 0);
    }

    #[test]
    fn spots_travel_only_through_develop_merge() {
        let develop = Develop {
            controls: BTreeMap::from([("light.exposure".to_string(), 0.5)]),
            spots: vec![sample_spot()],
            ..Develop::default()
        };
        let mut out = Plan::default();
        plan_develop("gid", None, &develop, false, &mut out);
        assert_eq!(out.steps.len(), 2);
        let Step::DevelopSet { values, .. } = &out.steps[0] else {
            panic!("{:?}", out.steps[0]);
        };
        assert!(!values.contains_key("spots"));
        let Step::DevelopMerge { settings, .. } = &out.steps[1] else {
            panic!("{:?}", out.steps[1]);
        };
        assert!(settings["spots"].is_array());
    }
}
