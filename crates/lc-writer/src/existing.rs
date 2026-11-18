//! Photos whose file LightCraft already has although the mapping file does
//! not know them (imported by hand, by LightCraft's own Lightroom import, or
//! by an earlier run whose mapping file was lost).
//!
//! Before anything is written, [`find_existing`] asks LightCraft which of the
//! photos a run would import it already holds (`library.importPreview`,
//! which adds nothing) and reads their current values. The person running
//! lr2lc then decides per photo ([`ExistingDecisions`]) what happens to it;
//! the plan adopts each found photo instead of importing it and writes only
//! what the choice allows ([`ExistingChoice`]). A decision applies to this run
//! only: afterwards the photo is a known photo and is reconciled like any
//! other.
//!
//! Only `library.importPreview`, `catalog.query` and `photo.inspect` are
//! sent here; nothing is changed in the library.

use crate::apply::id_text;
use crate::control::{CommandSink, Outcome, WriteError};
use crate::live::{LivePhoto, read_credits, read_summaries};
use crate::plan::{WriteInput, import_candidates};
use crate::progress::{Phase, Progress};
use crate::state::State;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Files per `library.importPreview`.
const PREVIEW_BATCH: usize = 64;

/// What happens to a photo LightCraft already has.
///
/// | Choice | Fields (stars, flag, label, texts) | Keywords | Development |
/// | --- | --- | --- | --- |
/// | `Overwrite` | the Lightroom value wherever it differs | added | written |
/// | `Develop` | only empty fields are filled | added | written |
/// | `DevelopIfEmpty` | only empty fields are filled | added | written only if LightCraft shows the photo as not edited |
/// | `Skip` | nothing | nothing | nothing |
///
/// Album, stack and Quick Collection memberships are added for every
/// choice. A field that LightCraft and Lightroom both have with different
/// values is a conflict for `Develop` and `DevelopIfEmpty`: it is kept and
/// reported. Keywords are only ever added, never removed.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum ExistingChoice {
    /// Write the Lightroom values and development over the photo.
    Overwrite,
    /// Fill empty fields only; write the development settings.
    Develop,
    /// Fill empty fields only; write the development settings only if the
    /// photo has none in LightCraft. The default.
    #[default]
    DevelopIfEmpty,
    /// Link the photo and leave it as it is (memberships are still added).
    Skip,
}

impl ExistingChoice {
    /// Every choice, in the order a front end lists them.
    pub const ALL: [ExistingChoice; 4] = [
        ExistingChoice::Overwrite,
        ExistingChoice::Develop,
        ExistingChoice::DevelopIfEmpty,
        ExistingChoice::Skip,
    ];

    /// The name as `--existing` takes it and the report gives it.
    pub fn as_str(self) -> &'static str {
        match self {
            ExistingChoice::Overwrite => "overwrite",
            ExistingChoice::Develop => "develop",
            ExistingChoice::DevelopIfEmpty => "develop-if-empty",
            ExistingChoice::Skip => "skip",
        }
    }
}

impl std::fmt::Display for ExistingChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ExistingChoice {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ExistingChoice::ALL
            .into_iter()
            .find(|c| c.as_str() == s)
            .ok_or_else(|| {
                format!(
                    "unknown choice `{s}` (one of {})",
                    ExistingChoice::ALL.map(ExistingChoice::as_str).join(", ")
                )
            })
    }
}

/// The choices for the photos LightCraft already has, for one run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExistingDecisions {
    /// The choice for every photo without its own entry, and for photos
    /// that turn out to be in the library only while importing.
    pub default: ExistingChoice,
    /// Choices per photo, by Lightroom global id.
    pub per_photo: BTreeMap<String, ExistingChoice>,
}

impl ExistingDecisions {
    /// The same choice for every photo.
    pub fn all(choice: ExistingChoice) -> ExistingDecisions {
        ExistingDecisions {
            default: choice,
            per_photo: BTreeMap::new(),
        }
    }

    /// The choice for the photo with this global id.
    pub fn choice_for(&self, global_id: &str) -> ExistingChoice {
        self.per_photo
            .get(global_id)
            .copied()
            .unwrap_or(self.default)
    }
}

/// Photos found in the library this run, by choice. Only counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExistingCounts {
    /// Photos LightCraft already had (adopted, not imported), whatever the
    /// choice; also counted in `photos_existing`.
    pub found: usize,
    pub overwrite: usize,
    pub develop: usize,
    pub develop_if_empty: usize,
    pub skip: usize,
    /// `develop-if-empty` photos LightCraft shows as edited: their
    /// development settings were kept (also counted in `not_applied` of the
    /// develop section).
    pub develop_kept: usize,
    /// Of `found`, the photos found only while importing (not by the check
    /// before writing); they got the default choice.
    pub found_while_importing: usize,
}

impl ExistingCounts {
    /// Counts one found photo with its choice.
    pub fn count(&mut self, choice: ExistingChoice) {
        self.found += 1;
        match choice {
            ExistingChoice::Overwrite => self.overwrite += 1,
            ExistingChoice::Develop => self.develop += 1,
            ExistingChoice::DevelopIfEmpty => self.develop_if_empty += 1,
            ExistingChoice::Skip => self.skip += 1,
        }
    }
}

/// How LightCraft recognised the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExistingReason {
    /// A photo with the same file path.
    Path,
    /// A photo whose file has the same bytes (moved or copied file).
    Content,
}

/// A photo the run would import that LightCraft already has.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExistingPhoto {
    /// Lightroom global id of the photo.
    pub global_id: String,
    /// The photo's id in the Lightroom catalog.
    pub lightroom_id: i64,
    /// The file the run would import.
    pub path: PathBuf,
    /// The file name of `path`, for display.
    pub file_name: String,
    /// The photo's id in LightCraft, exactly as the app returned it.
    pub lightcraft_id: Value,
    pub reason: ExistingReason,
    /// The photo's values in LightCraft now.
    pub live: LivePhoto,
}

/// Finds the photos of `input` that a run would import and that LightCraft
/// already has, with their current values, sorted by global id. Reports
/// [`Phase::CheckingLibrary`] per file; a cancel between two blocks stops
/// with [`WriteError::Cancelled`] (nothing is written here).
///
/// Only files the run would import are asked about: photos with a global id
/// that the mapping file does not know, not virtual copies (they are made
/// from their master) and with a UTF-8 path. Rules for what is found:
/// - a photo only in LightCraft's trash is left out: importing its file
///   restores it, and the run then handles it like a photo found while
///   importing (with [`ExistingDecisions::default`]);
/// - a photo the mapping file already maps to another Lightroom photo is
///   left out: the run gives the file its own record, as before;
/// - when several files name the same LightCraft photo, only one is listed:
///   the one with the same path, else the first by global id. The others
///   are imported and get their own records.
///
/// A LightCraft that does not know `library.importPreview` (an error reply)
/// gives an empty list: every duplicate is then found while importing and
/// handled with the default choice.
pub fn find_existing(
    client: &mut dyn CommandSink,
    input: &WriteInput,
    state: &State,
    progress: &mut dyn Progress,
) -> Result<Vec<ExistingPhoto>, WriteError> {
    let candidates = import_candidates(input, state);
    progress.phase(Phase::CheckingLibrary, candidates.len());
    let by_path: BTreeMap<&str, usize> = candidates
        .iter()
        .enumerate()
        .filter_map(|(i, (_, photo))| photo.path.to_str().map(|p| (p, i)))
        .collect();
    let paths: Vec<&str> = by_path.keys().copied().collect();
    // LightCraft ids (as text) the mapping file maps, to their global id.
    let mapped: BTreeMap<String, &str> = state
        .photos()
        .filter_map(|(gid, e)| id_text(&e.lightcraft_id).map(|t| (t, gid)))
        .collect();
    // LightCraft id (as text) -> (candidate index, reason, id).
    let mut hits: BTreeMap<String, (usize, ExistingReason, Value)> = BTreeMap::new();
    let mut done = 0;
    for chunk in paths.chunks(PREVIEW_BATCH) {
        if progress.cancelled() {
            return Err(WriteError::Cancelled);
        }
        progress.advance(done);
        let answer = match client.execute("library.importPreview", json!({"paths": chunk})) {
            Ok(Outcome::Applied(value)) => value,
            Ok(Outcome::AppliedNotSaved(_)) => {
                return Err(WriteError::Protocol(
                    "library.importPreview answered like a writing command".into(),
                ));
            }
            Err(WriteError::Command { .. }) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        for (path, reason, id) in parse_preview(&answer)? {
            let Some(&index) = by_path.get(path.as_str()) else {
                continue;
            };
            let gid = candidates[index].0;
            let Some(key) = id_text(&id) else { continue };
            // Mapped to another Lightroom photo: the file gets its own record.
            if mapped.get(&key).is_some_and(|other| *other != gid) {
                continue;
            }
            let better = match hits.get(&key) {
                None => true,
                Some((_, ExistingReason::Path, _)) => false,
                Some((held, ExistingReason::Content, _)) => {
                    reason == ExistingReason::Path || index < *held
                }
            };
            if better {
                hits.insert(key, (index, reason, id));
            }
        }
        done += chunk.len();
    }
    progress.advance(candidates.len());
    let ids: Vec<Value> = hits.values().map(|(_, _, id)| id.clone()).collect();
    let mut found = Vec::new();
    for (id, mut live) in read_summaries(client, &ids, false)? {
        let Some((index, reason, _)) = id_text(&id).and_then(|k| hits.get(&k)) else {
            continue;
        };
        if progress.cancelled() {
            return Err(WriteError::Cancelled);
        }
        read_credits(client, &id, &mut live)?;
        let (gid, photo) = candidates[*index];
        found.push(ExistingPhoto {
            global_id: gid.to_string(),
            lightroom_id: photo.lightroom_id,
            path: photo.path.clone(),
            file_name: photo
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            lightcraft_id: id,
            reason: *reason,
            live,
        });
    }
    found.sort_by(|a, b| a.global_id.cmp(&b.global_id));
    Ok(found)
}

/// The current values of one photo LightCraft reported as a duplicate while
/// importing: listed in the library or, if the import did not restore it,
/// in the trash. `None` when LightCraft lists it nowhere or refuses a read.
pub(crate) fn read_one(
    client: &mut dyn CommandSink,
    id: &Value,
) -> Result<Option<LivePhoto>, WriteError> {
    let ids = std::slice::from_ref(id);
    let found = match read_summaries(client, ids, false).and_then(|mut listed| {
        if listed.is_empty() {
            listed = read_summaries(client, ids, true)?;
        }
        Ok(listed)
    }) {
        Ok(listed) => listed,
        Err(WriteError::Command { .. }) => return Ok(None),
        Err(e) => return Err(e),
    };
    let Some((id, mut live)) = found.into_iter().next() else {
        return Ok(None);
    };
    match read_credits(client, &id, &mut live) {
        Ok(()) => Ok(Some(live)),
        Err(WriteError::Command { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The files of a `library.importPreview` answer that LightCraft already
/// has: `{candidates: [{path, duplicate?: "path"|"content", existing?}]}`.
/// A candidate without `duplicate` or without a usable id is new.
fn parse_preview(value: &Value) -> Result<Vec<(String, ExistingReason, Value)>, WriteError> {
    let bad = || WriteError::Protocol("library.importPreview answered in an unknown shape".into());
    let items = value
        .get("candidates")
        .and_then(Value::as_array)
        .ok_or_else(bad)?;
    let mut out = Vec::new();
    for item in items {
        let reason = match item.get("duplicate").and_then(Value::as_str) {
            Some("path") => ExistingReason::Path,
            Some("content") => ExistingReason::Content,
            _ => continue,
        };
        let (Some(path), Some(id)) = (
            item.get("path").and_then(Value::as_str),
            item.get("existing").filter(|id| match id {
                Value::Number(_) => true,
                Value::String(s) => !s.is_empty(),
                _ => false,
            }),
        ) else {
            continue;
        };
        out.push((path.to_string(), reason, id.clone()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_parse_from_their_names() {
        for choice in ExistingChoice::ALL {
            assert_eq!(choice.as_str().parse::<ExistingChoice>(), Ok(choice));
            assert_eq!(
                serde_json::to_value(choice).unwrap(),
                json!(choice.as_str())
            );
        }
        assert!("Skip".parse::<ExistingChoice>().is_err());
        assert_eq!(ExistingChoice::default(), ExistingChoice::DevelopIfEmpty);
    }

    #[test]
    fn a_photo_without_its_own_choice_gets_the_default() {
        let mut d = ExistingDecisions::all(ExistingChoice::Skip);
        d.per_photo.insert("g".into(), ExistingChoice::Overwrite);
        assert_eq!(d.choice_for("g"), ExistingChoice::Overwrite);
        assert_eq!(d.choice_for("h"), ExistingChoice::Skip);
    }

    #[test]
    fn preview_lists_only_duplicates_with_an_id() {
        let v = json!({"candidates": [
            {"path": "/a.jpg", "duplicate": "path", "existing": 4},
            {"path": "/b.jpg", "duplicate": "content", "existing": "x9"},
            {"path": "/c.jpg"},
            {"path": "/d.jpg", "duplicate": "path"},
            {"path": "/e.jpg", "duplicate": "other", "existing": 5},
        ], "duplicates": 2, "scanned": 5});
        assert_eq!(
            parse_preview(&v).unwrap(),
            vec![
                ("/a.jpg".to_string(), ExistingReason::Path, json!(4)),
                ("/b.jpg".to_string(), ExistingReason::Content, json!("x9")),
            ]
        );
        assert!(matches!(
            parse_preview(&json!({"photos": []})),
            Err(WriteError::Protocol(_))
        ));
    }
}
