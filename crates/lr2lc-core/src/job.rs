//! One conversion run, split into steps a front end can drive: [`prepare`]
//! reads the catalog and plans the writer commands, [`finish`] writes them
//! (with `apply` and a library) or keeps the plan, and [`Finished`] holds
//! everything a front end shows or saves afterwards.

use crate::dry_run::{self, Outcome};
use crate::labels::LabelMode;
use crate::output;
use crate::writer::{self, DevelopJson, WriterJson};
use lc_writer::apply::{Aborted, ApplySummary};
use lc_writer::control::{CommandSink, WriteError};
use lc_writer::plan::{Plan, WriteInput};
use lc_writer::state::State;
use model::PathMappings;
use std::collections::BTreeSet;
use std::fmt;
use std::path::PathBuf;

/// What to convert and where to write it.
#[derive(Debug, Clone)]
pub struct Job {
    pub catalog: PathBuf,
    pub mappings: PathMappings,
    pub labels: LabelMode,
    /// The LightCraft library: needed to write, and read in a dry run for
    /// the mapping file of earlier runs.
    pub library: Option<PathBuf>,
    /// Write through the control port (needs `library`); otherwise a dry run.
    pub apply: bool,
    pub port: u16,
    pub allow_xmp: bool,
    pub overwrite_develop: bool,
}

impl Job {
    /// The library to write to, when this run writes.
    pub fn writes_to(&self) -> Option<&std::path::Path> {
        self.library.as_deref().filter(|_| self.apply)
    }
}

/// Why a run failed, so a front end can react without parsing the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The catalog cannot be opened or read.
    Catalog,
    /// The mapping file of the library cannot be read.
    MappingFile,
    /// The catalog changed while it was read or written; is Lightroom running?
    CatalogChanged,
    /// Writing to LightCraft stopped (no connection, pre-flight, lost connection, ...).
    Stopped,
}

/// A failed run: its kind and the message the command line prints.
#[derive(Debug)]
pub struct RunError {
    pub kind: ErrorKind,
    message: String,
}

impl RunError {
    fn new(kind: ErrorKind, error: &anyhow::Error) -> Self {
        RunError {
            kind,
            message: format!("{error:#}"),
        }
    }

    fn changed(message: &str) -> Self {
        RunError {
            kind: ErrorKind::CatalogChanged,
            message: message.to_owned(),
        }
    }
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RunError {}

/// A catalog read and planned, nothing written yet.
pub struct Prepared {
    pub outcome: Outcome,
    pub input: WriteInput,
    pub state: State,
    pub plan: Plan,
}

/// Reads the catalog, loads the mapping file and plans the writer commands.
pub fn prepare(job: &Job) -> Result<Prepared, RunError> {
    let mut outcome = dry_run::run(&job.catalog, &job.mappings, &job.labels)
        .map_err(|e| RunError::new(ErrorKind::Catalog, &e))?;
    let mut input = writer::input(std::mem::take(&mut outcome.converted));
    input.overwrite_develop = job.overwrite_develop;
    let state = writer::load_state(job.library.as_deref())
        .map_err(|e| RunError::new(ErrorKind::MappingFile, &e))?;
    // Without a connection the library's own keywords are unknown; the
    // plan assumes none beyond the mapping file.
    let plan = lc_writer::plan::plan(&input, &state, &BTreeSet::new());
    Ok(Prepared {
        outcome,
        input,
        state,
        plan,
    })
}

impl Prepared {
    /// The planned commands as JSON lines (`--script`).
    pub fn script(&self) -> String {
        self.plan.to_script(&self.state)
    }

    /// Refuses to write when the catalog changed while it was read.
    pub fn check_writable(&self) -> Result<(), RunError> {
        if self.outcome.catalog_unchanged {
            Ok(())
        } else {
            Err(RunError::changed(
                "the catalog changed while it was read (checksum differs); is Lightroom \
                 running? Nothing was written to LightCraft",
            ))
        }
    }

    /// The readable summary of the catalog part.
    pub fn catalog_summary(&self, job: &Job) -> String {
        output::summary_with(&self.outcome, job.writes_to().is_none())
    }
}

/// A finished run: the writer's numbers, and whether it stopped.
pub struct Finished {
    pub outcome: Outcome,
    /// The readable summary of the writer part.
    pub writer_summary: String,
    pub writer: WriterJson,
    pub develop: DevelopJson,
    /// Why writing stopped, if it did.
    pub stop: Option<WriteError>,
    dry_run: bool,
}

/// Writes the plan (when `job` writes) through the control port on
/// `job.port`, or keeps it as the dry-run result, then checks that the
/// catalog is still unchanged.
pub fn finish(job: &Job, prepared: Prepared) -> Result<Finished, RunError> {
    finish_by(job, prepared, writer::write)
}

/// Like [`finish`], but writes through `sink`, for a host that runs
/// LightCraft's commands itself; `job.port` is not used.
pub fn finish_with(
    job: &Job,
    prepared: Prepared,
    sink: &mut dyn CommandSink,
) -> Result<Finished, RunError> {
    finish_by(job, prepared, |target, input, state| {
        writer::write_to(sink, target, input, state)
    })
}

fn finish_by(
    job: &Job,
    prepared: Prepared,
    write: impl FnOnce(&writer::Target<'_>, &WriteInput, State) -> Result<ApplySummary, Aborted>,
) -> Result<Finished, RunError> {
    let Prepared {
        mut outcome,
        input,
        state,
        plan,
    } = prepared;
    let (writer_summary, writer, develop, stop) = match job.writes_to() {
        Some(library) => {
            if !outcome.catalog_unchanged {
                return Err(RunError::changed(
                    "the catalog changed while it was read (checksum differs); is Lightroom \
                     running? Nothing was written to LightCraft",
                ));
            }
            let target = writer::Target {
                library,
                port: job.port,
                allow_xmp: job.allow_xmp,
            };
            // A run that stops still reports what it did (Principle 4).
            let (summary, stop) = match write(&target, &input, state) {
                Ok(summary) => (summary, None),
                Err(aborted) => (*aborted.summary, Some(aborted.error)),
            };
            (
                writer::applied_summary(&summary),
                writer::applied_json(&summary, stop.as_ref()),
                writer::applied_develop(&summary),
                stop,
            )
        }
        None => (
            writer::planned_summary(&plan),
            writer::planned_json(&plan),
            writer::planned_develop(&plan),
            None,
        ),
    };
    // Check again after writing: the catalog must still be unchanged.
    outcome.catalog_unchanged = outcome.catalog_unchanged
        && lr_reader::checksum(&job.catalog).ok() == Some(outcome.catalog_checksum);
    Ok(Finished {
        outcome,
        writer_summary,
        writer,
        develop,
        stop,
        dry_run: !job.apply,
    })
}

impl Finished {
    /// `report.json` of this run.
    pub fn report_json(&self) -> String {
        output::report_json_with(
            &self.outcome,
            self.dry_run,
            Some(&self.writer),
            &self.develop,
        )
    }

    /// The run's verdict: a stop while writing first, then a catalog that
    /// changed during the run.
    pub fn result(&self) -> Result<(), RunError> {
        if let Some(e) = &self.stop {
            return Err(RunError::new(ErrorKind::Stopped, &writer::stop_error(e)));
        }
        if !self.outcome.catalog_unchanged {
            return Err(RunError::changed(
                "the catalog changed during the run (checksum differs); is Lightroom running?",
            ));
        }
        Ok(())
    }
}
