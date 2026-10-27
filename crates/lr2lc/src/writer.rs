//! The writer part of a run: the plan in a dry run, writing through the
//! LightCraft control port with `--apply`.

use crate::dry_run::Converted;
use crate::output::{ReasonJson, group_reasons};
use anyhow::{Context, Result};
use lc_writer::apply::{ApplySummary, apply};
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
        updates: usize,
        not_saved: usize,
        photos_skipped: Vec<ReasonJson>,
        photos_incomplete: Vec<ReasonJson>,
        keywords_skipped: Vec<ReasonJson>,
    },
}

/// Number of planned commands by kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PlannedJson {
    pub keywords: usize,
    pub photos: usize,
    pub updates: usize,
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
    let mut out = PlannedJson::default();
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

/// The report section of an applied run.
pub fn applied_json(s: &ApplySummary) -> WriterJson {
    WriterJson::Applied {
        keywords_created: s.keywords_created,
        photos_imported: s.photos_imported,
        photos_existing: s.photos_existing,
        updates: s.updates,
        not_saved: s.not_saved,
        photos_skipped: reasons(&s.photos_skipped),
        photos_incomplete: reasons(&s.photos_incomplete),
        keywords_skipped: reasons(&s.keywords_skipped),
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
         updates {}, not yet saved by LightCraft {}\n",
        s.keywords_created, s.photos_imported, s.photos_existing, s.updates, s.not_saved
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

/// Connects, runs the pre-flight checks and writes `input`.
pub fn write(target: &Target<'_>, input: &WriteInput, mut state: State) -> Result<ApplySummary> {
    let state_file = state_path(target.library);
    let result = ControlClient::connect(target.port).and_then(|mut client| {
        preflight(&mut client, target.library, target.allow_xmp)?;
        apply(&mut client, input, &mut state, &state_file)
    });
    result.map_err(|e| {
        let hint = match &e {
            WriteError::ConnectionLost { .. } => {
                "; outcome unknown, run again; lr2lc re-checks the state"
            }
            WriteError::Connect { .. } => "; is LightCraft running with the control port enabled?",
            _ => "",
        };
        anyhow::anyhow!("writing to LightCraft stopped: {e}{hint}")
    })
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
                updates: 1
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
                "planned": {"keywords": 0, "photos": 0, "updates": 0},
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
        let v = serde_json::to_value(applied_json(&s)).unwrap();
        assert_eq!(v["photos_incomplete"][0]["reason"], "photo 3: x");
        assert_eq!(v["not_saved"], 0);
        assert!(v["keywords_skipped"].is_array());
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
