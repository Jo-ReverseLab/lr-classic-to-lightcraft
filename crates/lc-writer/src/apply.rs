//! Runs the writer plan over the control port of a running LightCraft app.
//!
//! Order of a run:
//! 1. `keyword.list {}` reads the keywords the library already has; an
//!    answer in an unknown shape stops the run before anything is written;
//! 2. the plan is computed from the input, the mapping file and that list;
//! 3. its steps run in order: keywords first (parents before children),
//!    then each photo with its import and its updates; before the first
//!    update of a photo, `library.select {ids: [id], active: id}` selects
//!    exactly that photo, because LightCraft runs `photo.*` commands only
//!    on a selection (the selection is left on the last updated photo);
//! 4. then the stacks, ascending by Lightroom id: each new or changed one
//!    is grouped with `library.select {ids, active: top}` directly before
//!    `stack.group {ids, top, collapsed: false}`; members without a
//!    LightCraft id are left out, and fewer than two left skips the stack;
//!    members that left a stack in Lightroom are only counted, lr2lc never
//!    ungroups, splits or removes; a rejected select or group makes the
//!    stack incomplete and the next run sends it again;
//! 5. then the collections: before the first `album.create`, `albums.list {}`
//!    is read once (only if the plan creates a collection; an answer in an
//!    unknown shape stops the run); album folders and albums missing from
//!    the mapping file are adopted when the library has exactly one
//!    matching node (see [`crate::albums`]), else created (parents first);
//!    then members are added and removed album by album, each call
//!    directly after `library.select {ids, active: first}` of exactly its
//!    photos, because LightCraft changes album members only on a selection;
//! 6. last the Quick Collection, only if some member is not in the mapping
//!    file: `album.setTarget {id: null}` (so a target album the user set is
//!    not used), `albums.list {}` (the answer of step 5 is reused), then the
//!    members of the one top-level album named `Quick Collection` (not a
//!    smart album) are read with `catalog.query {filter: {album}, offset,
//!    limit: 200}` page by page; only the photos it does not hold are sent, with
//!    `library.select {ids, active: first}` and `album.toggleTarget {ids}`
//!    (a toggle of photos that are all in it would remove them). Several
//!    such albums skip the Quick Collection as ambiguous; lr2lc never
//!    removes members and never sends `album.clearQuick`.
//!
//! Rules:
//! - every finished step is recorded in the mapping file at once, so an
//!   interrupted run loses nothing that was confirmed;
//! - a photo is recorded right after its import with the fields of a fresh
//!   import, and with all its fields once every update succeeded;
//! - a rejected select makes the photo incomplete like a rejected update;
//!   a select is not counted in `updates`;
//! - an error reply skips only the affected keyword or photo; a lost
//!   connection or a protocol error stops the run; nothing is repeated;
//! - a rejected `album.create` skips the collection and every collection
//!   below it; a rejected select or member change makes the collection
//!   incomplete, its members are not recorded and the next run sends them
//!   again; albums are never renamed, moved or deleted;
//! - a collection that matches several nodes, or whose one node several
//!   collections match, is skipped as ambiguous with its descendants;
//!   nothing is created for it;
//! - every input collection counts exactly once: created as folder or
//!   album, known, adopted (existing), or skipped;
//! - every input photo counts exactly once: imported, existing, updated,
//!   unchanged or skipped; a stopped run hands back the numbers up to the
//!   stop ([`Aborted`]);
//! - reasons never contain texts from the catalog or from the app's error
//!   messages (which may quote file paths); they name the command and, for
//!   photos and collections, the Lightroom id; never a collection name.

use crate::albums::{Decision, Existing, Node, parse_album_list};
use crate::control::{ControlClient, Outcome, WriteError};
use crate::plan::{
    Step, WriteInput, group_params, import_params, members_select_params, plan, same_members,
    select_params,
};
use crate::state::{CollectionEntry, PhotoFields, State, keyword_text, snapshot};
use model::{Collection, CollectionKind, Keyword, Photo, Skipped};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// What a run did. After a complete run every photo of the input counts in
/// exactly one of `photos_imported`, `photos_existing`, `photos_updated`,
/// `photos_unchanged` and `photos_skipped`. A photo imported or found as a
/// duplicate by this run counts only there, whatever its updates do.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ApplySummary {
    /// Keywords created by this run.
    pub keywords_created: usize,
    /// Photos imported by this run.
    pub photos_imported: usize,
    /// Photos the library already had (reported as duplicates on import).
    pub photos_existing: usize,
    /// Photos known from the mapping file for which at least one update was
    /// answered (applied or rejected).
    pub photos_updated: usize,
    /// Photos known from the mapping file that needed no command.
    pub photos_unchanged: usize,
    /// Field updates applied.
    pub updates: usize,
    /// Commands applied in memory but not yet saved to disk by the app;
    /// they also count as created, imported or updated.
    pub not_saved: usize,
    /// Photos not written, each with a reason.
    pub photos_skipped: Vec<Skipped>,
    /// Imported, existing or updated photos with an update that failed; a
    /// later run sends their updates again.
    pub photos_incomplete: Vec<Skipped>,
    /// Keywords not created, each with a reason.
    pub keywords_skipped: Vec<Skipped>,
    /// Album folders created by this run.
    pub folders_created: usize,
    /// Albums created by this run.
    pub albums_created: usize,
    /// Input collections already in the mapping file. Together with the
    /// created and skipped ones every input collection counts once.
    pub collections_known: usize,
    /// Input collections missing from the mapping file that the library
    /// already had (exactly one node with the same name, kind and parent);
    /// adopted instead of created.
    pub collections_existing: usize,
    /// Photos added to albums.
    pub members_added: usize,
    /// Photos removed from albums (only ones lr2lc added before).
    pub members_removed: usize,
    /// Known collections renamed in Lightroom; not renamed in LightCraft.
    pub collections_renamed: usize,
    /// Known collections moved in Lightroom; not moved in LightCraft.
    pub collections_moved: usize,
    /// Recorded collections no longer in Lightroom; not deleted in
    /// LightCraft.
    pub collections_removed: usize,
    /// Collections not created, each with a reason.
    pub collections_skipped: Vec<Skipped>,
    /// Albums whose members could not all be changed; a later run sends
    /// them again.
    pub collections_incomplete: Vec<Skipped>,
    /// Album members not written, each with a reason.
    pub members_skipped: Vec<Skipped>,
    /// Stacks grouped by this run (new or changed).
    pub stacks_grouped: usize,
    /// Input stacks already recorded with the same members in the same
    /// order. Together with the grouped, skipped and incomplete ones every
    /// input stack counts once.
    pub stacks_known: usize,
    /// Recorded members of input stacks that left the stack in Lightroom;
    /// they stay in the LightCraft stack.
    pub stack_members_removed: usize,
    /// Stacks not grouped (fewer than two usable photos), each with a
    /// reason.
    pub stacks_skipped: Vec<Skipped>,
    /// Stacks whose select or group was rejected; a later run sends them
    /// again.
    pub stacks_incomplete: Vec<Skipped>,
    /// Photos this run added to the Quick Collection.
    pub quick_added: usize,
    /// Quick Collection members already recorded or already in LightCraft.
    pub quick_already: usize,
    /// Recorded Quick Collection members that left it in Lightroom; they
    /// stay in LightCraft.
    pub quick_removed: usize,
    /// Quick Collection members (or the whole Quick Collection) not
    /// written, each with a reason.
    pub quick_skipped: Vec<Skipped>,
    /// Quick Collection commands that LightCraft rejected; a later run
    /// sends the members again.
    pub quick_incomplete: Vec<Skipped>,
}

/// A run that stopped: the error and what was done up to it.
#[derive(Debug, thiserror::Error)]
#[error("{error}")]
pub struct Aborted {
    /// Why the run stopped.
    pub error: WriteError,
    /// The numbers up to the stop; a command without a reply counts
    /// nowhere. Boxed to keep the error small.
    pub summary: Box<ApplySummary>,
}

/// Brings the library behind `client` up to date with `input`.
///
/// `state` is the content of the mapping file at `state_file`; it is
/// updated and saved after every finished step. The pre-flight checks
/// ([`crate::control::preflight`]) must have passed before. Use
/// [`apply_partial`] to keep the numbers of a run that stops.
pub fn apply(
    client: &mut ControlClient,
    input: &WriteInput,
    state: &mut State,
    state_file: &Path,
) -> Result<ApplySummary, WriteError> {
    apply_partial(client, input, state, state_file).map_err(|a| a.error)
}

/// Like [`apply`], but a run that stops hands back what it did up to the
/// stop.
pub fn apply_partial(
    client: &mut ControlClient,
    input: &WriteInput,
    state: &mut State,
    state_file: &Path,
) -> Result<ApplySummary, Aborted> {
    let listed = list_keywords(client).map_err(|error| Aborted {
        error,
        summary: Box::default(),
    })?;
    let plan = plan(input, state, &listed);
    let mut run = Run {
        client,
        state,
        state_file,
        summary: ApplySummary {
            photos_unchanged: plan.photos_unchanged(),
            photos_skipped: plan.photo_skips().to_vec(),
            keywords_skipped: plan.keyword_skips().to_vec(),
            collections_known: plan.collections_known(),
            collections_renamed: plan.collections_renamed(),
            collections_moved: plan.collections_moved(),
            collections_removed: plan.collections_removed(),
            members_skipped: plan.member_skips().to_vec(),
            stacks_known: plan.stacks_known(),
            stack_members_removed: plan.stack_members_removed(),
            stacks_skipped: plan.stack_skips().to_vec(),
            quick_already: plan.quick_already(),
            quick_removed: plan.quick_removed(),
            quick_skipped: plan.quick_skips().to_vec(),
            ..ApplySummary::default()
        },
        keywords: wanted_keywords(input),
        photos: input
            .photos
            .iter()
            .filter_map(|p| p.global_id.as_deref().map(|g| (g, p)))
            .collect(),
        photo_keywords: &input.photo_keywords,
        failed_keywords: BTreeSet::new(),
        current: None,
        collections: input
            .collections
            .iter()
            .map(|c| (c.lightroom_id, c))
            .collect(),
        incomplete: BTreeSet::new(),
        existing: None,
        album_nodes: None,
        wanting: wanting(input, &plan.steps),
    };
    match run.steps(&plan.steps) {
        Ok(()) => Ok(run.summary),
        Err(error) => Err(Aborted {
            error,
            summary: Box::new(run.summary),
        }),
    }
}

/// The name LightCraft gives the Quick Collection.
const QUICK_NAME: &str = "Quick Collection";

/// The page size of `catalog.query`.
const QUERY_PAGE: usize = 200;

/// The ids of the nodes that may be the Quick Collection: top-level albums
/// named [`QUICK_NAME`] that are neither folders nor smart albums.
fn quick_candidates(nodes: Vec<Node>) -> Vec<Value> {
    nodes
        .into_iter()
        .filter(|n| n.parent.is_none() && !n.folder && !n.smart && n.name == QUICK_NAME)
        .map(|n| n.id)
        .collect()
}

/// One page of a `catalog.query` answer.
#[derive(Debug, PartialEq)]
struct QueryPage {
    total: usize,
    ids: Vec<Value>,
}

/// Reads `{total, photos: [{id, …}]}`; any other shape is a protocol error.
fn parse_query_page(value: &Value) -> Result<QueryPage, WriteError> {
    let bad = || WriteError::Protocol("catalog.query answered in an unknown shape".into());
    let total = value["total"].as_u64().ok_or_else(bad)? as usize;
    let ids = value["photos"]
        .as_array()
        .ok_or_else(bad)?
        .iter()
        .map(|p| valid_id(&p["id"]).ok_or_else(bad))
        .collect::<Result<_, _>>()?;
    Ok(QueryPage { total, ids })
}

/// True if two LightCraft ids are equal, as numbers or as text.
fn same_id(a: &Value, b: &Value) -> bool {
    a == b || id_text(a).is_some_and(|t| Some(t) == id_text(b))
}

fn id_text(v: &Value) -> Option<String> {
    match v {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// The keywords the library has, from `keyword.list`.
fn list_keywords(client: &mut ControlClient) -> Result<BTreeSet<String>, WriteError> {
    match client.execute("keyword.list", json!({}))? {
        Outcome::Applied(value) => parse_keyword_list(&value),
        Outcome::AppliedNotSaved(_) => Err(WriteError::Protocol(
            "keyword.list answered like a writing command".into(),
        )),
    }
}

/// The photo whose steps are running.
struct Current {
    global_id: String,
    /// False once a step of the photo failed; its later steps are skipped.
    ok: bool,
    /// True once `library.select` named this photo.
    selected: bool,
    /// True for a photo known from the mapping file (no import in this
    /// run) that is not yet counted as updated.
    uncounted_update: bool,
}

struct Run<'a> {
    client: &'a mut ControlClient,
    state: &'a mut State,
    state_file: &'a Path,
    summary: ApplySummary,
    /// Every keyword the plan may create, by path text.
    keywords: BTreeMap<String, Keyword>,
    /// The input photos by global id (first one wins, as in the plan).
    photos: BTreeMap<&'a str, &'a Photo>,
    photo_keywords: &'a BTreeMap<i64, Vec<Keyword>>,
    /// Path texts of keywords that could not be created.
    failed_keywords: BTreeSet<String>,
    current: Option<Current>,
    /// The input collections by Lightroom id.
    collections: BTreeMap<i64, &'a Collection>,
    /// Lightroom ids of the collections already reported as incomplete.
    incomplete: BTreeSet<i64>,
    /// The library's albums that may be adopted, read before the first
    /// `album.create`.
    existing: Option<Existing>,
    /// The answer of `albums.list`, once read.
    album_nodes: Option<Vec<Node>>,
    /// The number of collections to create per trimmed name, folder flag
    /// and Lightroom parent.
    wanting: BTreeMap<CollectionKey, usize>,
}

/// Trimmed name, folder flag and Lightroom parent of a collection.
type CollectionKey = (String, bool, Option<i64>);

fn collection_key(c: &Collection) -> CollectionKey {
    (
        c.name.trim().to_string(),
        c.kind == CollectionKind::Folder,
        c.parent,
    )
}

/// Counts the collections the plan creates by [`collection_key`].
fn wanting(input: &WriteInput, steps: &[Step]) -> BTreeMap<CollectionKey, usize> {
    let mut out = BTreeMap::new();
    for step in steps {
        if let Step::CreateAlbum { collection_id, .. } = step
            && let Some(c) = input
                .collections
                .iter()
                .find(|c| c.lightroom_id == *collection_id)
        {
            *out.entry(collection_key(c)).or_insert(0) += 1;
        }
    }
    out
}

impl Run<'_> {
    fn steps(&mut self, steps: &[Step]) -> Result<(), WriteError> {
        for step in steps {
            self.step(step)?;
        }
        self.finish_photo()
    }

    fn step(&mut self, step: &Step) -> Result<(), WriteError> {
        match step {
            Step::CreateKeyword { path, params } => self.create_keyword(path, params),
            Step::Import { global_id, path } => {
                self.finish_photo()?;
                self.current = Some(Current {
                    global_id: global_id.clone(),
                    ok: true,
                    selected: false,
                    uncounted_update: false,
                });
                self.import(global_id, path)
            }
            Step::Update {
                global_id,
                command,
                params,
            } => {
                if self.current.as_ref().map(|c| &c.global_id) != Some(global_id) {
                    self.finish_photo()?;
                    self.current = Some(Current {
                        global_id: global_id.clone(),
                        ok: true,
                        selected: false,
                        uncounted_update: true,
                    });
                }
                if self.current.as_ref().is_some_and(|c| c.ok && !c.selected) {
                    self.select(global_id)?;
                }
                if self.current.as_ref().is_some_and(|c| c.ok) {
                    self.update(global_id, command, params)?;
                }
                Ok(())
            }
            Step::CreateAlbum {
                collection_id,
                params,
            } => {
                self.finish_photo()?;
                self.create_album(*collection_id, params)
            }
            Step::GroupStack { stack_id, gids } => {
                self.finish_photo()?;
                self.group_stack(*stack_id, gids)
            }
            Step::AddMembers {
                collection_id,
                gids,
            } => {
                self.finish_photo()?;
                self.change_members(*collection_id, gids, Change::Add)
            }
            Step::RemoveMembers {
                collection_id,
                gids,
            } => {
                self.finish_photo()?;
                self.change_members(*collection_id, gids, Change::Remove)
            }
            Step::QuickAdd { gids } => {
                self.finish_photo()?;
                self.quick_add(gids)
            }
        }
    }

    /// The library's album tree from `albums.list`, read at most once per
    /// run.
    fn album_nodes(&mut self) -> Result<Vec<Node>, WriteError> {
        if let Some(nodes) = &self.album_nodes {
            return Ok(nodes.clone());
        }
        let nodes = match self.client.execute("albums.list", json!({}))? {
            Outcome::Applied(value) => parse_album_list(&value)?,
            Outcome::AppliedNotSaved(_) => {
                return Err(WriteError::Protocol(
                    "albums.list answered like a writing command".into(),
                ));
            }
        };
        self.album_nodes = Some(nodes.clone());
        Ok(nodes)
    }

    /// Adds the photos `gids` (not yet recorded) to the Quick Collection,
    /// sending only those that LightCraft does not hold in it yet.
    fn quick_add(&mut self, gids: &[String]) -> Result<(), WriteError> {
        let mut wanted: Vec<(String, Value)> = Vec::new();
        for gid in gids {
            match self.state.photo(gid) {
                Some(entry) => wanted.push((gid.clone(), entry.lightcraft_id.clone())),
                None => {
                    let id = self.photos.get(gid.as_str()).map(|p| p.lightroom_id);
                    self.summary.quick_skipped.push(Skipped {
                        reason: match id {
                            Some(id) => format!(
                                "photo {id}: no LightCraft id; not added to the quick collection"
                            ),
                            None => "a photo without a LightCraft id was not added to the \
                                     quick collection"
                                .to_string(),
                        },
                    });
                }
            }
        }
        if wanted.is_empty() {
            return Ok(());
        }
        if self
            .quick_command("album.setTarget", json!({"id": null}))?
            .is_none()
        {
            return Ok(());
        }
        let candidates = quick_candidates(self.album_nodes()?);
        let held: Vec<Value> = match candidates.as_slice() {
            [] => Vec::new(),
            [id] => match self.album_members(id)? {
                Some(held) => held,
                None => return Ok(()),
            },
            more => {
                self.summary.quick_skipped.push(Skipped {
                    reason: format!(
                        "quick collection: ambiguous, the library has {} top-level albums \
                         named {QUICK_NAME:?}; nothing added",
                        more.len()
                    ),
                });
                return Ok(());
            }
        };
        let (present, missing): (Vec<_>, Vec<_>) = wanted
            .into_iter()
            .partition(|(_, id)| held.iter().any(|h| same_id(h, id)));
        if !present.is_empty() {
            self.summary.quick_already += present.len();
            self.state.record_quick(present.into_iter().map(|(g, _)| g));
            self.state.save(self.state_file)?;
        }
        if missing.is_empty() {
            return Ok(());
        }
        let ids: Vec<Value> = missing.iter().map(|(_, id)| id.clone()).collect();
        if self
            .quick_command("library.select", members_select_params(&ids))?
            .is_none()
        {
            return Ok(());
        }
        let Some(outcome) = self.quick_command("album.toggleTarget", json!({"ids": ids}))? else {
            return Ok(());
        };
        self.count_not_saved(&outcome);
        self.summary.quick_added += missing.len();
        self.state.record_quick(missing.into_iter().map(|(g, _)| g));
        self.state.save(self.state_file)?;
        Ok(())
    }

    /// Runs one Quick Collection command; a rejection is reported as
    /// incomplete and gives `None`.
    fn quick_command(
        &mut self,
        command: &str,
        params: Value,
    ) -> Result<Option<Outcome>, WriteError> {
        match self.client.execute(command, params) {
            Ok(outcome) => Ok(Some(outcome)),
            Err(WriteError::Command { command, .. }) => {
                self.summary.quick_incomplete.push(Skipped {
                    reason: format!(
                        "quick collection: LightCraft rejected `{command}`; \
                         its members are written again by the next run"
                    ),
                });
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// The photo ids of album `album`, read page by page with
    /// `catalog.query`; `None` if a query was rejected.
    fn album_members(&mut self, album: &Value) -> Result<Option<Vec<Value>>, WriteError> {
        let mut held: Vec<Value> = Vec::new();
        loop {
            let params = json!({
                "filter": {"album": album},
                "offset": held.len(),
                "limit": QUERY_PAGE,
            });
            let Some(outcome) = self.quick_command("catalog.query", params)? else {
                return Ok(None);
            };
            let value = match outcome {
                Outcome::Applied(v) => v,
                Outcome::AppliedNotSaved(_) => {
                    return Err(WriteError::Protocol(
                        "catalog.query answered like a writing command".into(),
                    ));
                }
            };
            let page = parse_query_page(&value)?;
            let done = page.ids.is_empty()
                || page.ids.len() < QUERY_PAGE
                || held.len() + page.ids.len() >= page.total;
            held.extend(page.ids);
            if done {
                return Ok(Some(held));
            }
        }
    }

    /// Reads `albums.list` once, before the first `album.create`; the
    /// nodes already in the mapping file are left out.
    fn load_existing(&mut self) -> Result<(), WriteError> {
        if self.existing.is_some() {
            return Ok(());
        }
        let nodes = self.album_nodes()?;
        let claimed: Vec<Value> = self
            .state
            .collections()
            .map(|(_, e)| e.lightcraft_id.clone())
            .collect();
        self.existing = Some(Existing::new(nodes, &claimed));
        Ok(())
    }

    /// Creates one album or album folder, or adopts the one node the
    /// library already has for it. The parent's LightCraft id is taken from
    /// the mapping file, so a parent created or adopted earlier in this run
    /// replaces its placeholder; a parent that was not created skips the
    /// collection.
    fn create_album(&mut self, collection_id: i64, params: &Value) -> Result<(), WriteError> {
        let Some(c) = self.collections.get(&collection_id).copied() else {
            // Not reachable: the plan only creates input collections.
            return Ok(());
        };
        self.load_existing()?;
        let mut params = params.clone();
        let mut parent_id = None;
        if let Some(parent) = c.parent {
            match self.state.collection(parent) {
                Some(entry) => {
                    params["parent"] = entry.lightcraft_id.clone();
                    parent_id = Some(entry.lightcraft_id.clone());
                }
                None => {
                    self.skip_collection(
                        collection_id,
                        &format!("its parent collection {parent} was not created"),
                    );
                    return Ok(());
                }
            }
        }
        let key = collection_key(c);
        let wanting = self.wanting.get(&key).copied().unwrap_or(1);
        let decision = self.existing.as_ref().map_or(Decision::Create, |e| {
            e.decide(&c.name, key.1, parent_id.as_ref(), wanting)
        });
        match decision {
            Decision::Create => {}
            Decision::Adopt(id) => {
                self.summary.collections_existing += 1;
                self.record_created(collection_id, c, id)?;
                return Ok(());
            }
            Decision::Ambiguous(n) => {
                self.skip_collection(
                    collection_id,
                    &format!(
                        "ambiguous: {n} albums with the same name and parent in LightCraft; \
                         nothing was created or adopted"
                    ),
                );
                return Ok(());
            }
        }
        match self.client.execute("album.create", params) {
            Ok(Outcome::Applied(result)) => {
                let Some(id) = result.get("id").and_then(valid_id) else {
                    self.skip_collection(collection_id, "`album.create` returned no id");
                    return Ok(());
                };
                match c.kind {
                    CollectionKind::Folder => self.summary.folders_created += 1,
                    CollectionKind::Album => self.summary.albums_created += 1,
                }
                self.record_created(collection_id, c, id)
            }
            Ok(Outcome::AppliedNotSaved(_)) => {
                self.summary.not_saved += 1;
                self.skip_collection(
                    collection_id,
                    "`album.create` was applied but not saved and returned no id; \
                     the next run creates it again, check LightCraft for a duplicate",
                );
                Ok(())
            }
            Err(WriteError::Command { command, .. }) => {
                self.skip_collection(collection_id, &format!("LightCraft rejected `{command}`"));
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Records a created or adopted collection, without members.
    fn record_created(
        &mut self,
        collection_id: i64,
        c: &Collection,
        id: Value,
    ) -> Result<(), WriteError> {
        let entry = CollectionEntry {
            lightcraft_id: id,
            kind: c.kind.into(),
            name: c.name.clone(),
            parent: c.parent,
            members: Vec::new(),
        };
        self.state.record_collection(collection_id, entry);
        self.state.save(self.state_file)?;
        Ok(())
    }

    fn skip_collection(&mut self, collection_id: i64, why: &str) {
        self.summary.collections_skipped.push(Skipped {
            reason: format!("collection {collection_id}: {why}"),
        });
    }

    /// Adds photos to an album or removes them, after selecting exactly
    /// those photos. Members whose photo has no LightCraft id are not sent.
    fn change_members(
        &mut self,
        collection_id: i64,
        gids: &[String],
        change: Change,
    ) -> Result<(), WriteError> {
        let Some(album_id) = self
            .state
            .collection(collection_id)
            .map(|e| e.lightcraft_id.clone())
        else {
            // The collection was not created; it is already skipped.
            return Ok(());
        };
        let mut sent: Vec<String> = Vec::new();
        let mut ids: Vec<Value> = Vec::new();
        for gid in gids {
            if let Some(entry) = self.state.photo(gid) {
                sent.push(gid.clone());
                ids.push(entry.lightcraft_id.clone());
            } else if change == Change::Add {
                let lightroom_id = self.photos.get(gid.as_str()).map(|p| p.lightroom_id);
                let photo = lightroom_id.map_or_else(|| "?".to_string(), |id| id.to_string());
                self.summary.members_skipped.push(Skipped {
                    reason: format!(
                        "photo {photo}: not added to an album; the photo has no LightCraft id"
                    ),
                });
            }
            // Not reachable for removals: lr2lc only recorded members that
            // had a LightCraft id.
        }
        if ids.is_empty() {
            return Ok(());
        }
        match self
            .client
            .execute("library.select", members_select_params(&ids))
        {
            Ok(_) => {}
            Err(WriteError::Command { command, .. }) => {
                self.mark_incomplete(collection_id, &command);
                return Ok(());
            }
            Err(e) => return Err(e),
        }
        let command = match change {
            Change::Add => "album.addPhotos",
            Change::Remove => "album.removePhotos",
        };
        match self
            .client
            .execute(command, json!({"id": album_id, "ids": ids}))
        {
            Ok(outcome) => {
                self.count_not_saved(&outcome);
                let Some(mut entry) = self.state.collection(collection_id).cloned() else {
                    return Ok(());
                };
                match change {
                    Change::Add => {
                        self.summary.members_added += sent.len();
                        entry.members.extend(sent);
                    }
                    Change::Remove => {
                        self.summary.members_removed += sent.len();
                        entry.members.retain(|m| !sent.contains(m));
                    }
                }
                self.state.record_collection(collection_id, entry);
                self.state.save(self.state_file)?;
                Ok(())
            }
            Err(WriteError::Command { command, .. }) => {
                self.mark_incomplete(collection_id, &command);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Groups one stack: selects exactly its photos with the top active,
    /// then `stack.group`. Members without a LightCraft id are not sent. On
    /// success the stack is recorded with the sent members followed by
    /// recorded members that left it in Lightroom (they stay in the
    /// LightCraft stack, since grouping merges touched stacks).
    fn group_stack(&mut self, stack_id: i64, gids: &[String]) -> Result<(), WriteError> {
        let mut sent: Vec<String> = Vec::new();
        let mut ids: Vec<Value> = Vec::new();
        for gid in gids {
            if let Some(entry) = self.state.photo(gid) {
                sent.push(gid.clone());
                ids.push(entry.lightcraft_id.clone());
            }
        }
        if ids.len() < 2 {
            self.summary.stacks_skipped.push(Skipped {
                reason: format!(
                    "stack {stack_id}: fewer than two photos with a LightCraft id; not grouped"
                ),
            });
            return Ok(());
        }
        let recorded: Vec<String> = self
            .state
            .stack(stack_id)
            .map(|e| e.members.clone())
            .unwrap_or_default();
        if self.state.stack(stack_id).is_some() && same_members(&recorded, &sent) {
            // Only photos without a LightCraft id differ; nothing to send.
            self.summary.stacks_known += 1;
            return Ok(());
        }
        for command in ["library.select", "stack.group"] {
            let params = match command {
                "library.select" => members_select_params(&ids),
                _ => group_params(&ids),
            };
            match self.client.execute(command, params) {
                Ok(outcome) => {
                    if command == "stack.group" {
                        self.count_not_saved(&outcome);
                    }
                }
                Err(WriteError::Command { command, .. }) => {
                    self.summary.stacks_incomplete.push(Skipped {
                        reason: format!(
                            "stack {stack_id}: LightCraft rejected `{command}`; \
                             the stack is grouped again by the next run"
                        ),
                    });
                    return Ok(());
                }
                Err(e) => return Err(e),
            }
        }
        self.summary.stacks_grouped += 1;
        let mut members = sent.clone();
        members.extend(recorded.into_iter().filter(|g| !sent.contains(g)));
        self.state.record_stack(stack_id, members);
        self.state.save(self.state_file)?;
        Ok(())
    }

    /// Reports an album as incomplete, once per run.
    fn mark_incomplete(&mut self, collection_id: i64, command: &str) {
        if self.incomplete.insert(collection_id) {
            self.summary.collections_incomplete.push(Skipped {
                reason: format!(
                    "collection {collection_id}: LightCraft rejected `{command}`; \
                     its members are written again by the next run"
                ),
            });
        }
    }

    fn create_keyword(&mut self, path: &str, params: &Value) -> Result<(), WriteError> {
        let parent_failed = self.failed_keywords.iter().any(|f| {
            path.strip_prefix(f.as_str())
                .is_some_and(|r| r.starts_with('|'))
        });
        if parent_failed {
            self.failed_keywords.insert(path.to_string());
            self.summary.keywords_skipped.push(Skipped {
                reason: "keyword not created: `keyword.create` failed for its parent keyword"
                    .to_string(),
            });
            return Ok(());
        }
        match self.client.execute("keyword.create", params.clone()) {
            Ok(outcome) => {
                self.count_not_saved(&outcome);
                self.summary.keywords_created += 1;
                let keyword = self.keywords.get(path).cloned().or_else(|| {
                    // Not reachable: the plan only creates wanted keywords.
                    Keyword::new(path.split('|').map(String::from).collect()).ok()
                });
                if let Some(keyword) = keyword {
                    self.state.record_keyword(&keyword);
                    self.state.save(self.state_file)?;
                }
                Ok(())
            }
            Err(WriteError::Command { command, .. }) => {
                self.failed_keywords.insert(path.to_string());
                self.summary.keywords_skipped.push(Skipped {
                    reason: format!("keyword not created: LightCraft rejected `{command}`"),
                });
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn import(&mut self, global_id: &str, path: &Path) -> Result<(), WriteError> {
        let Some(photo) = self.photos.get(global_id).copied() else {
            // Not reachable: the plan only imports input photos.
            self.fail_current();
            return Ok(());
        };
        let params = import_params(path);
        let sent_path = params["paths"][0].clone();
        let skip = |summary: &mut ApplySummary, why: &str| {
            summary.photos_skipped.push(Skipped {
                reason: format!("photo {}: {why}", photo.lightroom_id),
            });
        };
        match self.client.execute("library.import", params) {
            Ok(Outcome::Applied(result)) => match import_id(&result, &sent_path) {
                ImportResult::Imported(id) => {
                    self.summary.photos_imported += 1;
                    self.record_import(global_id, photo.lightroom_id, id)?;
                }
                ImportResult::Existing(id) => {
                    self.summary.photos_existing += 1;
                    self.record_import(global_id, photo.lightroom_id, id)?;
                }
                ImportResult::Failed => {
                    skip(
                        &mut self.summary,
                        "`library.import` reported the file as failed",
                    );
                    self.fail_current();
                }
                ImportResult::NoId => {
                    skip(
                        &mut self.summary,
                        "`library.import` returned no id for the file",
                    );
                    self.fail_current();
                }
            },
            Ok(Outcome::AppliedNotSaved(_)) => {
                self.summary.not_saved += 1;
                skip(
                    &mut self.summary,
                    "`library.import` was applied but not saved and returned no id; \
                     run again to find the photo",
                );
                self.fail_current();
            }
            Err(WriteError::Command { command, .. }) => {
                skip(
                    &mut self.summary,
                    &format!("LightCraft rejected `{command}`"),
                );
                self.fail_current();
            }
            Err(e) => return Err(e),
        }
        Ok(())
    }

    /// Records a photo right after its import, with the fields of a fresh
    /// import, so an interruption never leads to a second import.
    fn record_import(
        &mut self,
        global_id: &str,
        lightroom_id: i64,
        id: Value,
    ) -> Result<(), WriteError> {
        self.state
            .record(global_id, lightroom_id, id, PhotoFields::imported_default());
        self.state.save(self.state_file)?;
        Ok(())
    }

    /// Selects the running photo before its first update. A select is
    /// sent even right after an import, which selects as a side effect.
    fn select(&mut self, global_id: &str) -> Result<(), WriteError> {
        let Some(entry) = self.state.photo(global_id) else {
            // Not reachable: an update follows a recorded import or a known
            // photo.
            self.fail_current();
            return Ok(());
        };
        let lightroom_id = entry.lightroom_id;
        let params = select_params(&entry.lightcraft_id);
        match self.client.execute("library.select", params) {
            Ok(_) => {
                if let Some(c) = &mut self.current {
                    c.selected = true;
                }
                Ok(())
            }
            Err(WriteError::Command { command, .. }) => {
                self.count_updated();
                self.reject(lightroom_id, &command);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Marks the running photo as incomplete after a rejected command.
    fn reject(&mut self, lightroom_id: i64, command: &str) {
        self.summary.photos_incomplete.push(Skipped {
            reason: format!(
                "photo {lightroom_id}: LightCraft rejected `{command}`; \
                 the photo is incomplete and is updated again by the next run"
            ),
        });
        self.fail_current();
    }

    fn update(&mut self, global_id: &str, command: &str, params: &Value) -> Result<(), WriteError> {
        let Some(entry) = self.state.photo(global_id) else {
            // Not reachable: an update follows a recorded import or a known
            // photo.
            self.fail_current();
            return Ok(());
        };
        let lightroom_id = entry.lightroom_id;
        let mut params = params.clone();
        if let Value::Object(map) = &mut params {
            map.insert("ids".to_string(), json!([entry.lightcraft_id.clone()]));
        }
        let result = self.client.execute(command, params);
        if matches!(result, Ok(_) | Err(WriteError::Command { .. })) {
            self.count_updated();
        }
        match result {
            Ok(outcome) => {
                self.count_not_saved(&outcome);
                self.summary.updates += 1;
                Ok(())
            }
            Err(WriteError::Command { command, .. }) => {
                self.reject(lightroom_id, &command);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Records the photo whose steps all succeeded with its full fields.
    fn finish_photo(&mut self) -> Result<(), WriteError> {
        let Some(current) = self.current.take() else {
            return Ok(());
        };
        if !current.ok {
            return Ok(());
        }
        let (Some(photo), Some(entry)) = (
            self.photos.get(current.global_id.as_str()).copied(),
            self.state.photo(&current.global_id),
        ) else {
            return Ok(());
        };
        let keywords = self
            .photo_keywords
            .get(&photo.lightroom_id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let fields = snapshot(photo, keywords);
        if entry.fields == fields && entry.lightroom_id == photo.lightroom_id {
            return Ok(());
        }
        let id = entry.lightcraft_id.clone();
        self.state
            .record(current.global_id, photo.lightroom_id, id, fields);
        self.state.save(self.state_file)?;
        Ok(())
    }

    fn fail_current(&mut self) {
        if let Some(c) = &mut self.current {
            c.ok = false;
        }
    }

    /// Counts the running photo as updated once, if it is a known photo.
    fn count_updated(&mut self) {
        if let Some(c) = &mut self.current
            && c.uncounted_update
        {
            c.uncounted_update = false;
            self.summary.photos_updated += 1;
        }
    }

    fn count_not_saved(&mut self, outcome: &Outcome) {
        if let Outcome::AppliedNotSaved(_) = outcome {
            self.summary.not_saved += 1;
        }
    }
}

/// Whether album members are added or removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    Add,
    Remove,
}

/// Every keyword the plan may create, by path text: explicit keywords with
/// their attributes (first occurrence wins, as in the plan), then missing
/// ancestors with default attributes.
fn wanted_keywords(input: &WriteInput) -> BTreeMap<String, Keyword> {
    let mut out = BTreeMap::new();
    for k in input
        .keywords
        .iter()
        .chain(input.photo_keywords.values().flatten())
    {
        out.entry(keyword_text(k)).or_insert_with(|| k.clone());
    }
    let ancestors: Vec<Keyword> = out
        .values()
        .flat_map(|k| std::iter::successors(k.parent(), Keyword::parent))
        .collect();
    for a in ancestors {
        out.entry(keyword_text(&a)).or_insert(a);
    }
    out
}

/// What `library.import` reported for one file.
#[derive(Debug, PartialEq)]
enum ImportResult {
    Imported(Value),
    Existing(Value),
    Failed,
    NoId,
}

/// Reads the id of the one file sent from an import result.
fn import_id(result: &Value, sent_path: &Value) -> ImportResult {
    let failed = match result.get("failed") {
        None | Some(Value::Null) => false,
        Some(Value::Number(n)) => n.as_u64() != Some(0),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(_) => true,
    };
    if failed {
        return ImportResult::Failed;
    }
    let duplicate = result
        .get("duplicates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|d| d.get("path") == Some(sent_path))
        .and_then(|d| d.get("existing"))
        .and_then(valid_id);
    if let Some(id) = duplicate {
        return ImportResult::Existing(id);
    }
    match result.get("imported").and_then(Value::as_array) {
        Some(items) if items.len() == 1 => {
            let item = &items[0];
            let id = valid_id(item)
                .or_else(|| item.get("id").and_then(valid_id))
                .or_else(|| item.get("photoId").and_then(valid_id));
            id.map_or(ImportResult::NoId, ImportResult::Imported)
        }
        _ => ImportResult::NoId,
    }
}

/// A photo id is a number or a non-empty text.
fn valid_id(value: &Value) -> Option<Value> {
    match value {
        Value::Number(_) => Some(value.clone()),
        Value::String(s) if !s.is_empty() => Some(value.clone()),
        _ => None,
    }
}

/// The keyword path texts (`a|b`) of a `keyword.list` answer: an array of
/// path texts, an array of `{name, children}` trees, or an object with a
/// `keywords` field holding either.
fn parse_keyword_list(value: &Value) -> Result<BTreeSet<String>, WriteError> {
    let items = match value {
        Value::Array(items) => items,
        Value::Object(map) => match map.get("keywords") {
            Some(Value::Array(items)) => items,
            _ => return Err(list_error()),
        },
        _ => return Err(list_error()),
    };
    let mut out = BTreeSet::new();
    collect_keywords(items, None, &mut out)?;
    Ok(out)
}

fn collect_keywords(
    items: &[Value],
    prefix: Option<&str>,
    out: &mut BTreeSet<String>,
) -> Result<(), WriteError> {
    for item in items {
        match (item, prefix) {
            (Value::String(text), None) => {
                out.insert(text.clone());
            }
            (Value::Object(map), _) => {
                let name = map
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(list_error)?;
                let text = match prefix {
                    Some(p) => format!("{p}|{name}"),
                    None => name.to_string(),
                };
                match map.get("children") {
                    None | Some(Value::Null) => {}
                    Some(Value::Array(children)) => {
                        collect_keywords(children, Some(&text), out)?;
                    }
                    Some(_) => return Err(list_error()),
                }
                out.insert(text);
            }
            _ => return Err(list_error()),
        }
    }
    Ok(())
}

fn list_error() -> WriteError {
    WriteError::Protocol("keyword.list answered in an unknown format".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn keyword_list_of_texts() {
        let v = json!(["a", "a|b"]);
        assert_eq!(parse_keyword_list(&v).unwrap(), set(&["a", "a|b"]));
    }

    #[test]
    fn keyword_list_of_trees_without_children() {
        let v = json!({"keywords": [{"name": "a", "children": [{"name": "b"}]}]});
        assert_eq!(parse_keyword_list(&v).unwrap(), set(&["a", "a|b"]));
    }

    #[test]
    fn keyword_list_rejects_other_shapes() {
        for v in [
            json!("a"),
            json!(null),
            json!({"other": []}),
            json!([1]),
            json!([{"children": []}]),
            json!([{"name": "a", "children": "b"}]),
            json!([{"name": "a", "children": ["b"]}]),
        ] {
            assert!(
                matches!(parse_keyword_list(&v), Err(WriteError::Protocol(_))),
                "{v}"
            );
        }
    }

    #[test]
    fn query_pages_are_parsed_or_rejected() {
        let page = parse_query_page(&json!({"total": 3, "photos": [{"id": 1}, {"id": "2"}]}));
        assert_eq!(
            page.unwrap(),
            QueryPage {
                total: 3,
                ids: vec![json!(1), json!("2")]
            }
        );
        assert!(parse_query_page(&json!({"photos": []})).is_err());
        assert!(parse_query_page(&json!({"total": 1, "photos": [{}]})).is_err());
        assert!(parse_query_page(&json!([])).is_err());
    }

    #[test]
    fn only_plain_top_level_albums_are_quick_candidates() {
        let node = |id: i64, name: &str, folder: bool, smart: bool, parent: Option<Value>| Node {
            id: json!(id),
            name: name.into(),
            folder,
            smart,
            parent,
        };
        let nodes = vec![
            node(1, QUICK_NAME, false, false, None),
            node(2, QUICK_NAME, false, true, None),
            node(3, QUICK_NAME, true, false, None),
            node(4, QUICK_NAME, false, false, Some(json!(3))),
            node(5, "quick collection", false, false, None),
        ];
        assert_eq!(quick_candidates(nodes), vec![json!(1)]);
    }

    #[test]
    fn ids_match_as_numbers_or_text() {
        assert!(same_id(&json!(4), &json!(4)));
        assert!(same_id(&json!(4), &json!("4")));
        assert!(!same_id(&json!(4), &json!(5)));
    }

    #[test]
    fn import_id_prefers_the_matching_duplicate() {
        let p = json!("/x/a.jpg");
        let r = json!({"imported": [], "failed": 0, "duplicates": [
            {"path": "/x/b.jpg", "existing": 1},
            {"path": "/x/a.jpg", "existing": 2},
        ]});
        assert_eq!(import_id(&r, &p), ImportResult::Existing(json!(2)));
    }

    #[test]
    fn import_id_accepts_text_ids() {
        let r = json!({"imported": ["abc"]});
        assert_eq!(
            import_id(&r, &json!("p")),
            ImportResult::Imported(json!("abc"))
        );
    }

    #[test]
    fn import_without_a_single_id_has_no_id() {
        let p = json!("p");
        for r in [
            json!({"imported": []}),
            json!({"imported": [1, 2]}),
            json!({"imported": [{"name": "x"}]}),
            json!({"imported": [""]}),
            json!({"duplicates": [{"path": "other", "existing": 3}]}),
            json!(null),
        ] {
            assert_eq!(import_id(&r, &p), ImportResult::NoId, "{r}");
        }
    }

    #[test]
    fn failed_import_wins_over_an_id() {
        let p = json!("p");
        for failed in [json!(1), json!(["p"]), json!("yes")] {
            let r = json!({"imported": [5], "failed": failed});
            assert_eq!(import_id(&r, &p), ImportResult::Failed);
        }
        let r = json!({"imported": [5], "failed": []});
        assert_eq!(import_id(&r, &p), ImportResult::Imported(json!(5)));
    }

    #[test]
    fn wanted_keywords_include_ancestors_with_defaults() {
        let mut k = Keyword::new(vec!["A".into(), "B".into()]).unwrap();
        k.person = true;
        let input = WriteInput {
            keywords: vec![k],
            ..WriteInput::default()
        };
        let w = wanted_keywords(&input);
        assert!(w["A|B"].person);
        assert!(!w["A"].person);
    }
}
