//! Log entries for the steps of a plan: what a dry run would do, or what an
//! applied run did. Failures are not described here: they are the skip
//! lists of the plan and the run, each with its code.

use crate::plan::Step;
use model::{Area, Entry, Stage, Status, Subject};
use serde_json::Value;
use std::fmt::Write;

/// How one step ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    /// Planned (dry run).
    Planned,
    /// Done.
    Done,
    /// Done, and the photo was already in the library (linked, not added).
    Existing,
    /// The custom white balance was not written: LightCraft does not edit
    /// this photo in Kelvin.
    NotCarried,
}

/// The stage a step belongs to.
pub fn stage_of(step: &Step) -> Stage {
    match step {
        Step::CreateKeyword { .. } => Stage::Keywords,
        Step::Import { .. }
        | Step::VirtualCopy { .. }
        | Step::Update { .. }
        | Step::DevelopSet { .. }
        | Step::DevelopMerge { .. }
        | Step::DevelopWhiteBalance { .. } => Stage::Photos,
        Step::GroupStack { .. }
        | Step::CreateAlbum { .. }
        | Step::AddMembers { .. }
        | Step::QuickAdd { .. }
        | Step::CreateSmart { .. }
        | Step::RemoveMembers { .. } => Stage::Organize,
    }
}

/// The entries of one step that did not fail.
pub fn step_entries(step: &Step, outcome: StepOutcome) -> Vec<Entry> {
    let dry = outcome == StepOutcome::Planned;
    let say = |done: &str, planned: &str| if dry { planned } else { done }.to_owned();
    let photo = |gid: &str| Subject::PhotoGlobal(gid.to_owned());
    let ok = Status::Transferred;
    match step {
        Step::CreateKeyword { path, .. } => vec![
            Entry::new(
                ok,
                Area::Keywords,
                Stage::Keywords,
                Subject::Keyword(path.clone()),
                say("Keyword created", "Keyword will be created"),
            )
            .item(path.replace('|', " › ")),
        ],
        Step::Import { global_id, path } => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            let message = match outcome {
                StepOutcome::Existing => "Already in the library; linked to it".to_owned(),
                _ => say("Imported as a reference", "Will be imported as a reference"),
            };
            vec![
                Entry::new(ok, Area::Photos, Stage::Photos, photo(global_id), message)
                    .item(name)
                    .source(path.display().to_string()),
            ]
        }
        Step::VirtualCopy { global_id, .. } => vec![Entry::new(
            ok,
            Area::VirtualCopies,
            Stage::Photos,
            photo(global_id),
            say("Virtual copy created", "Virtual copy will be created"),
        )],
        Step::Update {
            global_id,
            command,
            params,
        } => update_entries(global_id, command, params, dry),
        Step::DevelopSet { global_id, values } => vec![
            Entry::new(
                ok,
                Area::Develop,
                Stage::Photos,
                photo(global_id),
                if dry {
                    format!("{} will be set", counted(values.len(), "slider"))
                } else {
                    format!("{} set", counted(values.len(), "slider"))
                },
            )
            .source(listing(
                values.iter().map(|(k, v)| format!("{k} {}", number(*v))),
            )),
        ],
        Step::DevelopMerge {
            global_id,
            settings,
        } => {
            let parts = settings
                .as_object()
                .map(|o| o.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            vec![
                Entry::new(
                    ok,
                    Area::Develop,
                    Stage::Photos,
                    photo(global_id),
                    say(
                        "Structured settings written",
                        "Structured settings will be written",
                    ),
                )
                .source(listing(parts.into_iter())),
            ]
        }
        Step::DevelopWhiteBalance {
            global_id,
            values,
            check_kelvin,
        } => {
            let source = listing(values.iter().map(|(k, v)| format!("{k} {}", number(*v))));
            let entry = match outcome {
                StepOutcome::NotCarried => Entry::new(
                    Status::Skipped,
                    Area::Develop,
                    Stage::Photos,
                    photo(global_id),
                    "Custom white balance not carried over",
                )
                .code("white_balance_not_kelvin")
                .reason(
                    "LightCraft does not edit this photo in Kelvin, so the Kelvin value from \
                     Lightroom would mean something else.",
                ),
                StepOutcome::Planned if *check_kelvin => Entry::new(
                    ok,
                    Area::Develop,
                    Stage::Photos,
                    photo(global_id),
                    "Custom white balance will be written if LightCraft edits this photo in Kelvin",
                ),
                _ if !check_kelvin => Entry::new(
                    Status::Adjusted,
                    Area::Develop,
                    Stage::Photos,
                    photo(global_id),
                    say(
                        "Custom white balance converted to LightCraft's scale",
                        "Custom white balance will be converted to LightCraft's scale",
                    ),
                )
                .reason("Lightroom stores a relative white balance for this file; LightCraft uses its own relative scale."),
                _ => Entry::new(
                    ok,
                    Area::Develop,
                    Stage::Photos,
                    photo(global_id),
                    "Custom white balance written",
                ),
            };
            vec![entry.source(source)]
        }
        Step::GroupStack { stack_id, gids } => vec![Entry::new(
            ok,
            Area::Stacks,
            Stage::Organize,
            Subject::Stack(*stack_id),
            if dry {
                format!("Stack of {} will be grouped", photos(gids.len()))
            } else {
                format!("Stack of {} grouped", photos(gids.len()))
            },
        )],
        Step::CreateAlbum {
            collection_id,
            params,
        } => {
            let folder = params["folder"].as_bool().unwrap_or(false);
            let message = match (folder, dry) {
                (true, true) => "Album folder will be created",
                (true, false) => "Album folder created",
                (false, true) => "Album will be created",
                (false, false) => "Album created",
            };
            let mut entry = Entry::new(
                ok,
                Area::Collections,
                Stage::Organize,
                Subject::Collection(*collection_id),
                message,
            );
            if let Some(name) = params["name"].as_str() {
                entry = entry.item(name);
            }
            vec![entry]
        }
        Step::AddMembers {
            collection_id,
            gids,
        } => vec![Entry::new(
            ok,
            Area::Collections,
            Stage::Organize,
            Subject::Collection(*collection_id),
            if dry {
                format!("{} will be added to the album", photos(gids.len()))
            } else {
                format!("{} added to the album", photos(gids.len()))
            },
        )],
        Step::QuickAdd { gids } => vec![
            Entry::new(
                ok,
                Area::Stacks,
                Stage::Organize,
                Subject::None,
                if dry {
                    format!(
                        "{} will be added to the Quick Collection",
                        photos(gids.len())
                    )
                } else {
                    format!("{} added to the Quick Collection", photos(gids.len()))
                },
            )
            .item("Quick Collection"),
        ],
        Step::CreateSmart {
            collection_id,
            params,
        } => {
            let mut entry = Entry::new(
                ok,
                Area::Collections,
                Stage::Organize,
                Subject::Collection(*collection_id),
                say("Smart album created", "Smart album will be created"),
            );
            if let Some(name) = params["name"].as_str() {
                entry = entry.item(name);
            }
            vec![entry]
        }
        Step::RemoveMembers {
            collection_id,
            gids,
        } => vec![
            Entry::new(
                ok,
                Area::Collections,
                Stage::Organize,
                Subject::Collection(*collection_id),
                if dry {
                    format!("{} will be removed from the album", photos(gids.len()))
                } else {
                    format!("{} removed from the album", photos(gids.len()))
                },
            )
            .reason(
                "They left the collection in Lightroom; lr2lc only removes photos it added itself.",
            ),
        ],
    }
}

fn update_entries(global_id: &str, command: &str, params: &Value, dry: bool) -> Vec<Entry> {
    let subject = || Subject::PhotoGlobal(global_id.to_owned());
    let set = |area: Area, what: &str, value: String| {
        Entry::new(
            Status::Transferred,
            area,
            Stage::Photos,
            subject(),
            if dry {
                format!("{what} will be set")
            } else {
                format!("{what} set")
            },
        )
        .source(value)
    };
    match command {
        "photo.rate" => vec![set(
            Area::Ratings,
            "Star rating",
            params["rating"].as_u64().unwrap_or(0).to_string(),
        )],
        "photo.flag" => vec![set(Area::Flags, "Flag", text(&params["flag"]))],
        "photo.label" => vec![set(Area::Labels, "Color label", text(&params["label"]))],
        "photo.setMeta" => {
            let mut out = Vec::new();
            for (key, name) in [
                ("title", "Title"),
                ("caption", "Caption"),
                ("copyright", "Copyright"),
                ("creator", "Creator"),
            ] {
                if let Some(value) = params.get(key) {
                    out.push(set(Area::Metadata, name, text(value)));
                }
            }
            if let Some(added) = params["addKeywords"].as_array() {
                let names = added
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|k| k.replace('|', " › "));
                out.push(
                    Entry::new(
                        Status::Transferred,
                        Area::Keywords,
                        Stage::Photos,
                        subject(),
                        if dry {
                            format!("{} will be added", counted(added.len(), "keyword"))
                        } else {
                            format!("{} added", counted(added.len(), "keyword"))
                        },
                    )
                    .source(listing(names)),
                );
            }
            out
        }
        other => vec![Entry::new(
            Status::Transferred,
            Area::Photos,
            Stage::Photos,
            subject(),
            format!("{other} sent"),
        )],
    }
}

/// `1 photo`, `3 photos`.
fn photos(n: usize) -> String {
    counted(n, "photo")
}

/// `1 keyword`, `3 keywords`.
pub fn counted(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn number(value: f64) -> String {
    let mut out = format!("{value:.2}");
    while out.ends_with('0') {
        out.pop();
    }
    if out.ends_with('.') {
        out.pop();
    }
    out
}

/// At most eight parts, comma-separated, then how many more.
fn listing(parts: impl Iterator<Item = String>) -> String {
    let parts: Vec<String> = parts.collect();
    let mut out = parts.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
    if parts.len() > 8 {
        let _ = write!(out, ", … ({} more)", parts.len() - 8);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_rating_update_names_its_area_and_value() {
        let step = Step::Update {
            global_id: "g".into(),
            command: "photo.rate".into(),
            params: json!({"rating": 4}),
        };
        let [entry] = &step_entries(&step, StepOutcome::Done)[..] else {
            panic!()
        };
        assert_eq!(entry.area, Area::Ratings);
        assert_eq!(entry.message, "Star rating set");
        assert_eq!(entry.source.as_deref(), Some("4"));
        let planned = step_entries(&step, StepOutcome::Planned);
        assert_eq!(planned[0].message, "Star rating will be set");
    }

    #[test]
    fn set_meta_splits_texts_and_keywords() {
        let step = Step::Update {
            global_id: "g".into(),
            command: "photo.setMeta".into(),
            params: json!({"caption": "c", "addKeywords": ["A|B"]}),
        };
        let entries = step_entries(&step, StepOutcome::Done);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].area, Area::Metadata);
        assert_eq!(entries[1].area, Area::Keywords);
        assert_eq!(entries[1].source.as_deref(), Some("A › B"));
    }

    #[test]
    fn messages_never_carry_names_or_values() {
        let step = Step::Import {
            global_id: "g".into(),
            path: "/Volumes/Secret/IMG_1.CR3".into(),
        };
        let entry = &step_entries(&step, StepOutcome::Done)[0];
        assert!(!entry.message.contains("IMG_1"));
        assert_eq!(entry.item.as_deref(), Some("IMG_1.CR3"));
        assert_eq!(number(0.5), "0.5");
        assert_eq!(number(-10.0), "-10");
        assert_eq!(
            listing((0..10).map(|i| i.to_string())),
            "0, 1, 2, 3, 4, 5, 6, 7, … (2 more)"
        );
    }
}
