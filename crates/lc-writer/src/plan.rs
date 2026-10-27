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
//! - photos are processed in the order of their global ids.

use crate::state::{PhotoFields, State, keyword_text, snapshot};
use model::{Keyword, Photo, Skipped};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

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
}

impl Plan {
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

    /// The plan as a script: one JSON line per step, each an
    /// `engine.execute` request. Photo updates carry `ids`: the known
    /// LightCraft id, or the placeholder `lr:<global id>` for a photo that
    /// is imported by this plan.
    pub fn to_script(&self, state: &State) -> String {
        let mut out = String::new();
        for step in &self.steps {
            let (command, params) = match step {
                Step::CreateKeyword { params, .. } => ("keyword.create", params.clone()),
                Step::Import { path, .. } => ("library.import", import_params(path)),
                Step::Update {
                    global_id,
                    command,
                    params,
                } => {
                    let id = state.photo(global_id).map_or_else(
                        || Value::String(format!("lr:{global_id}")),
                        |e| e.lightcraft_id.clone(),
                    );
                    let mut params = params.clone();
                    if let Value::Object(map) = &mut params {
                        map.insert("ids".to_string(), json!([id]));
                    }
                    (command.as_str(), params)
                }
            };
            let line = json!({
                "method": "engine.execute",
                "params": {"command": command, "params": params},
            });
            out.push_str(&line.to_string());
            out.push('\n');
        }
        out
    }
}

/// The parameters of a reference-only import of one file. Nothing else is
/// ever sent: no copy, move, rename, presets or keywords.
pub fn import_params(path: &std::path::Path) -> Value {
    // The plan only creates imports for paths that are valid UTF-8.
    json!({"paths": [path.to_string_lossy()], "mode": "add"})
}

/// Computes the commands that bring the library up to date.
///
/// `library_keywords` holds the path texts (`a|b|c`) of the keywords the
/// library already has; they and keywords recorded in `state` are not
/// created again.
pub fn plan(input: &WriteInput, state: &State, library_keywords: &BTreeSet<String>) -> Plan {
    let mut out = Plan::default();
    plan_keywords(input, state, library_keywords, &mut out);
    out.keyword_skips = out.skipped.len();
    plan_photos(input, state, &mut out);
    out
}

/// A keyword the input needs, and whether it was given explicitly (with
/// attributes) or only as an ancestor.
struct Wanted {
    keyword: Keyword,
    explicit: bool,
}

fn plan_keywords(
    input: &WriteInput,
    state: &State,
    library_keywords: &BTreeSet<String>,
    out: &mut Plan,
) {
    // Ordered by path parts, so a parent always comes before its children.
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

    for w in wanted.values() {
        let text = keyword_text(&w.keyword);
        if let Some(entry) = state.keyword(&text) {
            if w.explicit && !entry.same_attributes(&w.keyword) {
                out.skipped.push(Skipped {
                    reason: "keyword synonyms or attributes changed after the keyword was \
                             created in LightCraft; not updated"
                        .to_string(),
                });
            }
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

fn plan_photos(input: &WriteInput, state: &State, out: &mut Plan) {
    let mut by_gid: BTreeMap<&str, &Photo> = BTreeMap::new();
    for p in &input.photos {
        match p.global_id.as_deref() {
            None => out.skipped.push(Skipped {
                reason: format!("photo {} has no global id", p.lightroom_id),
            }),
            Some(gid) => {
                if by_gid.contains_key(gid) {
                    out.skipped.push(Skipped {
                        reason: format!(
                            "photo {} has the same global id as another photo",
                            p.lightroom_id
                        ),
                    });
                } else {
                    by_gid.insert(gid, p);
                }
            }
        }
    }

    for (gid, p) in by_gid {
        let keywords = input
            .photo_keywords
            .get(&p.lightroom_id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let new = snapshot(p, keywords);
        let known = state.photo(gid).is_some();
        let old = match state.photo(gid) {
            Some(entry) => entry.fields.clone(),
            None => {
                if p.path.to_str().is_none() {
                    out.skipped.push(Skipped {
                        reason: format!(
                            "photo {} has a file path that is not valid UTF-8",
                            p.lightroom_id
                        ),
                    });
                    continue;
                }
                out.steps.push(Step::Import {
                    global_id: gid.to_string(),
                    path: p.path.clone(),
                });
                PhotoFields::imported_default()
            }
        };
        let changes = field_changes(&old, &new);
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
    }
}

/// The commands that turn the fields `old` into `new`, at most one per
/// command, in a fixed order.
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
    let removed: Vec<&String> = before.difference(&after).copied().collect();
    if !added.is_empty() {
        meta.insert("addKeywords".to_string(), json!(added));
    }
    if !removed.is_empty() {
        meta.insert("removeKeywords".to_string(), json!(removed));
    }
    if !meta.is_empty() {
        out.push(("photo.setMeta", Value::Object(meta)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{ColorLabel, Flag, Rating};

    fn kw(parts: &[&str]) -> Keyword {
        Keyword::new(parts.iter().map(|s| s.to_string()).collect()).unwrap()
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
        let line: Value = serde_json::from_str(plan.to_script(&State::default()).trim()).unwrap();
        assert_eq!(
            line,
            json!({"method": "engine.execute", "params": {
                "command": "photo.rate",
                "params": {"rating": 1, "ids": ["lr:g"]}
            }})
        );
    }
}
