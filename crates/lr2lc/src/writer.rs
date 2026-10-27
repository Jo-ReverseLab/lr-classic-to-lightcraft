//! The writer part of a run: the plan in a dry run, writing through the
//! LightCraft control port with `--apply`.

use crate::dry_run::Converted;
use crate::output::{ReasonJson, group_reasons};
use anyhow::{Context, Result};
use lc_writer::apply::{Aborted, ApplySummary, apply_partial};
use lc_writer::control::{ControlClient, WriteError, preflight};
use lc_writer::plan::{Plan, Step, WriteInput};
use lc_writer::state::{State, state_path};
use model::Skipped;
use serde::Serialize;
use std::fmt::Write;
use std::path::Path;

/// The `writer` section of `report.json`.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum WriterJson {
    /// Dry run: what would be sent.
    Planned {
        planned: PlannedJson,
        photos_skipped: Vec<ReasonJson>,
        keywords_skipped: Vec<ReasonJson>,
    },
    /// `--apply`: what was sent.
    Applied {
        keywords_created: usize,
        photos_imported: usize,
        photos_existing: usize,
        photos_updated: usize,
        photos_unchanged: usize,
        updates: usize,
        not_saved: usize,
        photos_skipped: Vec<ReasonJson>,
        photos_incomplete: Vec<ReasonJson>,
        keywords_skipped: Vec<ReasonJson>,
        /// Why the run stopped early; absent after a complete run.
        #[serde(skip_serializing_if = "Option::is_none")]
        aborted: Option<String>,
    },
}

/// Number of planned commands by kind, and the known photos with and
/// without updates. Every converted photo counts once in `photos`,
/// `photos_updated`, `photos_unchanged` or the writer's skipped photos.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PlannedJson {
    pub keywords: usize,
    pub photos: usize,
    pub updates: usize,
    pub photos_updated: usize,
    pub photos_unchanged: usize,
}

/// The writer input from the converted elements of the catalog.
pub fn input(converted: &Converted) -> WriteInput {
    WriteInput {
        photos: converted.photos.clone(),
        keywords: converted.keywords.clone(),
        photo_keywords: converted.photo_keywords.clone(),
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
        ..PlannedJson::default()
    };
    for step in &plan.steps {
        match step {
            Step::CreateKeyword { .. } => out.keywords += 1,
            Step::Import { .. } => out.photos += 1,
            Step::Update { .. } => out.updates += 1,
        }
    }
    out
}

/// The report section of a dry run.
pub fn planned_json(plan: &Plan) -> WriterJson {
    WriterJson::Planned {
        planned: planned(plan),
        photos_skipped: reasons(plan.photo_skips()),
        keywords_skipped: reasons(plan.keyword_skips()),
    }
}

/// The report section of an applied run; `aborted` is the error that
/// stopped it early.
pub fn applied_json(s: &ApplySummary, aborted: Option<&WriteError>) -> WriterJson {
    WriterJson::Applied {
        keywords_created: s.keywords_created,
        photos_imported: s.photos_imported,
        photos_existing: s.photos_existing,
        photos_updated: s.photos_updated,
        photos_unchanged: s.photos_unchanged,
        updates: s.updates,
        not_saved: s.not_saved,
        photos_skipped: reasons(&s.photos_skipped),
        photos_incomplete: reasons(&s.photos_incomplete),
        keywords_skipped: reasons(&s.keywords_skipped),
        aborted: aborted.map(WriteError::short_reason),
    }
}

fn reasons(items: &[Skipped]) -> Vec<ReasonJson> {
    items
        .iter()
        .map(|s| ReasonJson {
            reason: s.reason.clone(),
        })
        .collect()
}

/// The readable summary of a dry run's plan.
pub fn planned_summary(plan: &Plan) -> String {
    let p = planned(plan);
    let mut out = format!(
        "writer: keywords to create {}, photos to import {}, updates {}\n",
        p.keywords, p.photos, p.updates
    );
    skipped_lines(&mut out, "writer would skip photos", plan.photo_skips());
    skipped_lines(&mut out, "writer would skip keywords", plan.keyword_skips());
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
        "not written: photos skipped {}, photos incomplete {}, keywords skipped {}",
        s.photos_skipped.len(),
        s.photos_incomplete.len(),
        s.keywords_skipped.len()
    );
    skipped_lines(&mut out, "skipped photos", &s.photos_skipped);
    skipped_lines(&mut out, "incomplete photos", &s.photos_incomplete);
    skipped_lines(&mut out, "skipped keywords", &s.keywords_skipped);
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
    use model::{Photo, Rating};

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
        assert!(text.starts_with("writer: keywords to create 0, photos to import 2, updates 1\n"));
        assert!(text.contains("writer would skip photos:"));
    }

    #[test]
    fn json_sections_have_the_documented_keys() {
        let v = serde_json::to_value(planned_json(&plan_of(vec![]))).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "planned": {"keywords": 0, "photos": 0, "updates": 0,
                        "photos_updated": 0, "photos_unchanged": 0},
                "photos_skipped": [],
                "keywords_skipped": [],
            })
        );
        let s = ApplySummary {
            photos_incomplete: vec![Skipped {
                reason: "photo 3: x".into(),
            }],
            ..ApplySummary::default()
        };
        let v = serde_json::to_value(applied_json(&s, None)).unwrap();
        assert_eq!(v["photos_incomplete"][0]["reason"], "photo 3: x");
        assert_eq!(v["not_saved"], 0);
        assert_eq!(v["photos_updated"], 0);
        assert_eq!(v["photos_unchanged"], 0);
        assert!(v["keywords_skipped"].is_array());
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
        assert!(
            planned_summary(&plan)
                .starts_with("writer: keywords to create 0, photos to import 1, updates 1\n")
        );
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
