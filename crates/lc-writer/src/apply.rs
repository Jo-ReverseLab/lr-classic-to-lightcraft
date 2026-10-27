//! Runs the writer plan over the control port of a running LightCraft app.
//!
//! Order of a run:
//! 1. `keyword.list {}` reads the keywords the library already has; an
//!    answer in an unknown shape stops the run before anything is written;
//! 2. the plan is computed from the input, the mapping file and that list;
//! 3. its steps run in order: keywords first (parents before children),
//!    then each photo with its import and its updates.
//!
//! Rules:
//! - every finished step is recorded in the mapping file at once, so an
//!   interrupted run loses nothing that was confirmed;
//! - a photo is recorded right after its import with the fields of a fresh
//!   import, and with all its fields once every update succeeded;
//! - an error reply skips only the affected keyword or photo; a lost
//!   connection or a protocol error stops the run; nothing is repeated;
//! - every input photo counts exactly once: imported, existing, updated,
//!   unchanged or skipped; a stopped run hands back the numbers up to the
//!   stop ([`Aborted`]);
//! - reasons never contain texts from the catalog or from the app's error
//!   messages (which may quote file paths); they name the command and, for
//!   photos, the Lightroom id.

use crate::control::{ControlClient, Outcome, WriteError};
use crate::plan::{Step, WriteInput, import_params, plan};
use crate::state::{PhotoFields, State, keyword_text, snapshot};
use model::{Keyword, Photo, Skipped};
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
    };
    match run.steps(&plan.steps) {
        Ok(()) => Ok(run.summary),
        Err(error) => Err(Aborted {
            error,
            summary: Box::new(run.summary),
        }),
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
                        uncounted_update: true,
                    });
                }
                if self.current.as_ref().is_some_and(|c| c.ok) {
                    self.update(global_id, command, params)?;
                }
                Ok(())
            }
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
                self.summary.photos_incomplete.push(Skipped {
                    reason: format!(
                        "photo {lightroom_id}: LightCraft rejected `{command}`; \
                         the photo is incomplete and is updated again by the next run"
                    ),
                });
                self.fail_current();
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
