//! Taking a run back (Cancel in the window): everything the run added to
//! the library is removed again and every field it wrote is set back, then
//! the mapping file is restored to its state before the run.
//!
//! Only what this run did is touched, from its [`Journal`]: photos it
//! imported (records only; LightCraft never deletes an original here),
//! albums, smart albums and keywords it created, album members and stacks
//! it changed, and fields it wrote. Development settings written to photos
//! that were already in the library cannot be set back through the control
//! port; they are counted and reported, never hidden.

use crate::control::{CommandSink, Outcome, WriteError};
use crate::plan::members_select_params;
use crate::state::{PhotoFields, State};
use serde_json::{Value, json};
use std::path::Path;

/// One field command a run sent to a photo.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldWrite {
    pub global_id: String,
    pub id: Value,
    /// `photo.rate`, `photo.flag`, `photo.label` or `photo.setMeta`.
    pub command: String,
    /// The parameters without `ids`.
    pub params: Value,
}

/// Photos a run added to an album or took out of it.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberChange {
    pub album: Value,
    pub ids: Vec<Value>,
    pub added: bool,
}

/// What a run changed in the library, in the order it happened.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Journal {
    /// The mapping file as it was before the run.
    pub before: Option<State>,
    /// Photos and virtual copies this run created (LightCraft ids).
    pub created_photos: Vec<Value>,
    /// Albums, album folders and smart albums this run created (not those
    /// it adopted).
    pub created_albums: Vec<Value>,
    /// Keywords this run created, by path text.
    pub created_keywords: Vec<String>,
    /// Stacks this run grouped, by their members' ids.
    pub stacks: Vec<Vec<Value>>,
    pub members: Vec<MemberChange>,
    /// Photos this run added to the Quick Collection.
    pub quick: Vec<Value>,
    pub fields: Vec<FieldWrite>,
    /// Photos whose development settings this run wrote.
    pub developed: Vec<Value>,
}

impl Journal {
    /// Whether the run changed anything.
    pub fn is_empty(&self) -> bool {
        self.created_photos.is_empty()
            && self.created_albums.is_empty()
            && self.created_keywords.is_empty()
            && self.stacks.is_empty()
            && self.members.is_empty()
            && self.quick.is_empty()
            && self.fields.is_empty()
            && self.developed.is_empty()
    }
}

/// What taking a run back did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RollbackSummary {
    pub photos_removed: usize,
    pub albums_removed: usize,
    pub keywords_removed: usize,
    pub stacks_ungrouped: usize,
    pub members_restored: usize,
    pub quick_removed: usize,
    pub fields_restored: usize,
    /// Photos already in the library before the run whose development
    /// settings stay as the run wrote them.
    pub develop_not_reset: usize,
    /// Commands LightCraft refused, by name; their part stays as it is.
    pub failed: Vec<String>,
    /// The mapping file was set back to its state before the run.
    pub mapping_restored: bool,
}

fn same(a: &Value, b: &Value) -> bool {
    a == b || (a.as_str().is_some() && a.as_str() == b.as_str())
}

/// Runs one command; a refused command is noted and the rollback goes on.
fn send(
    sink: &mut dyn CommandSink,
    command: &str,
    params: Value,
    out: &mut RollbackSummary,
) -> Result<bool, WriteError> {
    match sink.execute(command, params) {
        Ok(Outcome::Applied(_)) => Ok(true),
        Ok(Outcome::AppliedNotSaved(_)) => Ok(true),
        Err(WriteError::Command { command, .. }) => {
            out.failed.push(command);
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

/// Selects `ids`, then runs `command` on them.
fn on_selection(
    sink: &mut dyn CommandSink,
    ids: &[Value],
    command: &str,
    params: Value,
    out: &mut RollbackSummary,
) -> Result<bool, WriteError> {
    if !send(sink, "library.select", members_select_params(ids), out)? {
        return Ok(false);
    }
    send(sink, command, params, out)
}

/// The commands that set one field write back to `before`.
fn undo_field(write: &FieldWrite, before: &PhotoFields) -> Option<(&'static str, Value)> {
    match write.command.as_str() {
        "photo.rate" => Some(("photo.rate", json!({"rating": before.rating}))),
        "photo.flag" => Some(("photo.flag", json!({"flag": before.flag}))),
        "photo.label" => Some((
            "photo.label",
            json!({"label": before.label.as_deref().unwrap_or("none")}),
        )),
        "photo.setMeta" => {
            let mut meta = serde_json::Map::new();
            for (key, value) in [
                ("title", &before.title),
                ("caption", &before.caption),
                ("copyright", &before.copyright),
                ("creator", &before.creator),
            ] {
                if write.params.get(key).is_some() {
                    meta.insert(key.into(), json!(value.as_deref().unwrap_or("")));
                }
            }
            if let Some(added) = write.params.get("addKeywords") {
                meta.insert("removeKeywords".into(), added.clone());
            }
            (!meta.is_empty()).then_some(("photo.setMeta", Value::Object(meta)))
        }
        _ => None,
    }
}

/// Takes back what `journal` records, newest first, and restores the
/// mapping file at `state_file` to the state before the run. A refused
/// command is listed and the rest goes on; a lost connection stops it.
pub fn rollback(
    sink: &mut dyn CommandSink,
    journal: &Journal,
    state_file: &Path,
) -> Result<RollbackSummary, WriteError> {
    let mut out = RollbackSummary::default();
    let created = |id: &Value| journal.created_photos.iter().any(|c| same(c, id));

    // Fields of photos that were there before; created photos go anyway.
    let empty = State::default();
    let before = journal.before.as_ref().unwrap_or(&empty);
    for write in journal.fields.iter().rev() {
        if created(&write.id) {
            continue;
        }
        let fields = before
            .photo(&write.global_id)
            .map(|e| e.fields.clone())
            .unwrap_or_else(PhotoFields::imported_default);
        if let Some((command, mut params)) = undo_field(write, &fields) {
            params["ids"] = json!([write.id]);
            if on_selection(
                sink,
                std::slice::from_ref(&write.id),
                command,
                params,
                &mut out,
            )? {
                out.fields_restored += 1;
            }
        }
    }
    // The Quick Collection: toggling again takes the photos out.
    let quick: Vec<Value> = journal
        .quick
        .iter()
        .filter(|id| !created(id))
        .cloned()
        .collect();
    if !quick.is_empty()
        && send(sink, "album.setTarget", json!({"id": null}), &mut out)?
        && on_selection(
            sink,
            &quick,
            "album.toggleTarget",
            json!({"ids": quick}),
            &mut out,
        )?
    {
        out.quick_removed += quick.len();
    }
    // Album members of albums that stay; created albums go as a whole.
    for change in journal.members.iter().rev() {
        if journal
            .created_albums
            .iter()
            .any(|a| same(a, &change.album))
        {
            continue;
        }
        let ids: Vec<Value> = change
            .ids
            .iter()
            .filter(|id| !created(id))
            .cloned()
            .collect();
        if ids.is_empty() {
            continue;
        }
        let command = if change.added {
            "album.removePhotos"
        } else {
            "album.addPhotos"
        };
        let params = json!({"id": change.album, "ids": ids});
        if on_selection(sink, &ids, command, params, &mut out)? {
            out.members_restored += ids.len();
        }
    }
    // Stacks of photos that stay.
    for stack in journal.stacks.iter().rev() {
        let ids: Vec<Value> = stack.iter().filter(|id| !created(id)).cloned().collect();
        if ids.len() < 2 {
            continue;
        }
        if on_selection(sink, &ids, "stack.ungroup", json!({"ids": ids}), &mut out)? {
            out.stacks_ungrouped += 1;
        }
    }
    // Albums, newest first, so a folder goes after what it holds.
    for album in journal.created_albums.iter().rev() {
        if send(sink, "album.delete", json!({"id": album}), &mut out)? {
            out.albums_removed += 1;
        }
    }
    // Photos: the library records only; the files stay where they are.
    if !journal.created_photos.is_empty() {
        let ids = &journal.created_photos;
        if on_selection(
            sink,
            ids,
            "photo.deletePermanently",
            json!({"ids": ids}),
            &mut out,
        )? {
            out.photos_removed = ids.len();
        }
    }
    // Keywords, deepest first.
    let mut keywords = journal.created_keywords.clone();
    keywords.sort_by_key(|k| std::cmp::Reverse(k.matches('|').count()));
    for keyword in keywords {
        if send(
            sink,
            "keyword.delete",
            json!({"keyword": keyword}),
            &mut out,
        )? {
            out.keywords_removed += 1;
        }
    }
    out.develop_not_reset = journal.developed.iter().filter(|id| !created(id)).count();
    if let Some(before) = &journal.before {
        before.save(state_file)?;
        out.mapping_restored = true;
    }
    Ok(out)
}
