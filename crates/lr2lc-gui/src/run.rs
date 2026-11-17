//! Running a conversion off the UI thread and reporting it as [`Event`]s.
//!
//! The window only sees events and a [`RunResult`]; how the run reaches
//! LightCraft (the control port here, a host's own command dispatch when the
//! window lives inside LightCraft) is the [`crate::host::Host`]'s business.

use crate::logdata::{LogData, ScanSummary};
use lr2lc_core::log::{Log, LogEntry};
use lr2lc_core::{CommandSink, ErrorKind, Job, Phase, Progress, finish, finish_with, prepare};
use model::{Entry, Stage};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

/// What a running conversion reports.
#[derive(Debug)]
pub enum Event {
    /// A reading phase started; `total` units, 0 when unknown.
    Phase(Phase, usize),
    /// Units done in the current phase.
    Advance(usize),
    /// The catalog is read and planned: steps per stage.
    Planned(BTreeMap<Stage, usize>),
    /// A step finished while writing.
    Entry(Box<LogEntry>),
    /// The run stopped and is being taken back (Cancel).
    RollingBack,
    /// The run ended, successfully or not.
    Done(Box<RunResult>),
}

/// The end of a run: what the window shows and can save.
#[derive(Debug, Clone)]
pub struct RunResult {
    /// Whether this run wrote to LightCraft.
    pub apply: bool,
    /// `report.json` with names (empty when reading failed).
    pub report_text: String,
    /// `report.json` without names, paths and values.
    pub anonymized_text: String,
    /// The report's log, parsed off the UI thread.
    pub data: LogData,
    /// The numbers of the Transfer screen.
    pub summary: Option<ScanSummary>,
    /// Why the run failed or stopped.
    pub error: Option<(ErrorKind, String)>,
}

impl RunResult {
    pub fn failed(apply: bool, kind: ErrorKind, message: String) -> RunResult {
        RunResult {
            apply,
            report_text: String::new(),
            anonymized_text: String::new(),
            data: LogData::default(),
            summary: None,
            error: Some((kind, message)),
        }
    }
}

/// Sends progress as events and answers the cancel flag.
pub struct ChannelProgress {
    events: Sender<Event>,
    cancel: Arc<AtomicBool>,
    /// Take the run back after it stopped.
    roll_back: Arc<AtomicBool>,
    total: usize,
    /// Names entries as they arrive, once the catalog is read.
    live: Option<Log>,
}

impl ChannelProgress {
    pub fn new(events: Sender<Event>, cancel: Arc<AtomicBool>) -> Self {
        ChannelProgress {
            events,
            cancel,
            roll_back: Arc::new(AtomicBool::new(false)),
            total: 0,
            live: None,
        }
    }
}

impl ChannelProgress {
    /// Takes the run back once it stopped or ended (Cancel).
    pub fn set_roll_back(&mut self, on: bool) {
        self.roll_back.store(on, Ordering::Relaxed);
    }
}

impl Progress for ChannelProgress {
    fn phase(&mut self, phase: Phase, total: usize) {
        self.total = total;
        let _ = self.events.send(Event::Phase(phase, total));
    }

    fn advance(&mut self, done: usize) {
        // About 200 updates per phase are plenty for a progress bar.
        let step = (self.total / 200).max(1);
        if done.is_multiple_of(step) || done == self.total {
            let _ = self.events.send(Event::Advance(done));
        }
    }

    fn entry(&mut self, entry: &Entry) {
        if let Some(live) = &self.live {
            let _ = self
                .events
                .send(Event::Entry(Box::new(live.render_entry(entry, false))));
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Runs `job` to the end: reads and plans, then writes through `sink` (the
/// control port on `job.port` when `None`) or keeps the plan in a dry run.
pub fn execute(
    job: &Job,
    progress: &mut ChannelProgress,
    sink: Option<&mut dyn CommandSink>,
) -> RunResult {
    execute_as(job, progress, sink, false)
}

/// [`execute`]; a `scan` only needs the summary, so its report texts and
/// log are dropped.
pub fn execute_as(
    job: &Job,
    progress: &mut ChannelProgress,
    sink: Option<&mut dyn CommandSink>,
    scan: bool,
) -> RunResult {
    let prepared = match prepare(job, progress) {
        Ok(prepared) => prepared,
        Err(e) => return RunResult::failed(job.apply, e.kind, e.to_string()),
    };
    if job.writes_to().is_some()
        && let Err(e) = prepared.check_writable()
    {
        return RunResult::failed(job.apply, e.kind, e.to_string());
    }
    progress.live = Some(prepared.live_log());
    let _ = progress
        .events
        .send(Event::Planned(prepared.stage_totals()));
    let mut sink = sink;
    let finished = match sink.as_deref_mut() {
        Some(sink) => finish_with(job, prepared, sink, progress),
        None => finish(job, prepared, progress),
    };
    let mut finished = match finished {
        Ok(finished) => finished,
        Err(e) => return RunResult::failed(job.apply, e.kind, e.to_string()),
    };
    // Cancel: take back what the run wrote, even if it reached its end.
    let mut rollback_error = None;
    if progress.roll_back.load(Ordering::Relaxed) && finished.can_roll_back() {
        let _ = progress.events.send(Event::RollingBack);
        if let Err(e) = finished.roll_back(job, sink) {
            rollback_error = Some((e.kind, format!("taking the run back stopped: {e}")));
        }
    }
    let report_text = finished.report_json_as(false);
    let value: serde_json::Value = serde_json::from_str(&report_text).unwrap_or_default();
    let summary = Some(ScanSummary::from_report(&value));
    let error = match (&finished.rollback, rollback_error) {
        (_, Some(error)) => Some(error),
        (Some(_), None) => Some((
            ErrorKind::Cancelled,
            "Run cancelled: what this run transferred was taken back".to_owned(),
        )),
        (None, None) => finished.result().err().map(|e| (e.kind, e.to_string())),
    };
    if scan {
        return RunResult {
            apply: false,
            report_text: String::new(),
            anonymized_text: String::new(),
            data: LogData::default(),
            summary,
            error,
        };
    }
    RunResult {
        apply: job.apply,
        data: LogData::from_report(&value),
        summary,
        report_text,
        anonymized_text: finished.report_json_as(true),
        error,
    }
}

/// A run on its own thread.
pub struct Running {
    pub events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    roll_back: Arc<AtomicBool>,
}

impl Running {
    /// Starts `job` on a new thread, writing through the control port.
    pub fn spawn(job: Job) -> Running {
        let (tx, events) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let roll_back = Arc::new(AtomicBool::new(false));
        let mut progress = ChannelProgress::new(tx.clone(), cancel.clone());
        progress.roll_back = roll_back.clone();
        std::thread::spawn(move || {
            let result = execute(&job, &mut progress, None);
            let _ = tx.send(Event::Done(Box::new(result)));
        });
        Running {
            events,
            cancel,
            roll_back,
        }
    }

    /// Asks the run to stop at the next safe point; with `roll_back` it
    /// is then taken back.
    pub fn cancel(&self, roll_back: bool) {
        if roll_back {
            self.roll_back.store(true, Ordering::Relaxed);
        }
        self.cancel.store(true, Ordering::Relaxed);
    }
}
