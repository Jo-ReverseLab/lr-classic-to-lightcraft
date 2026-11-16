//! What the window needs from its surroundings. The standalone app uses
//! native file dialogs and runs through the control port
//! ([`StandaloneHost`]); a host inside LightCraft would use its own dialogs,
//! its activity list and its command dispatch, and tests use a scripted
//! host.

use crate::run::{ChannelProgress, Event, RunResult, Running, execute};
use lr2lc_core::Job;
use lr2lc_core::probe::Probe;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, channel};

/// The window's surroundings.
pub trait Host {
    /// Lets the person choose a Lightroom catalog (`.lrcat`).
    fn pick_catalog(&mut self) -> Option<PathBuf>;
    /// Lets the person choose a LightCraft library folder.
    fn pick_library(&mut self) -> Option<PathBuf>;
    /// Lets the person choose a label set file (`.json`).
    fn pick_label_file(&mut self) -> Option<PathBuf>;
    /// Lets the person choose where to save a report.
    fn pick_report(&mut self) -> Option<PathBuf>;

    /// Starts `job`; its events arrive through [`Host::poll`].
    fn start(&mut self, job: Job);
    /// Asks the running job to stop at the next safe point.
    fn cancel(&mut self);
    /// The events since the last call.
    fn poll(&mut self) -> Vec<Event>;

    /// Starts a read-only scan of the catalog (a dry run) for the summary.
    fn scan(&mut self, job: Job);
    /// The scan's result once it is done.
    fn poll_scan(&mut self) -> Option<RunResult>;

    /// Asks LightCraft on `port` for its state.
    fn probe(&mut self, port: u16);
    /// The probe's answer once it is in.
    fn poll_probe(&mut self) -> Option<Result<Probe, String>>;
}

/// The standalone app: native dialogs; runs, scans and probes on threads
/// through the control port.
#[derive(Default)]
pub struct StandaloneHost {
    running: Option<Running>,
    scan: Option<Receiver<RunResult>>,
    probe: Option<Receiver<Result<Probe, String>>>,
}

impl Host for StandaloneHost {
    fn pick_catalog(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter("Lightroom catalog", &["lrcat"])
            .pick_file()
    }

    fn pick_library(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new().pick_folder()
    }

    fn pick_label_file(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .pick_file()
    }

    fn pick_report(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_file_name("report.json")
            .add_filter("JSON", &["json"])
            .save_file()
    }

    fn start(&mut self, job: Job) {
        self.running = Some(Running::spawn(job));
    }

    fn cancel(&mut self) {
        if let Some(running) = &self.running {
            running.cancel();
        }
    }

    fn poll(&mut self) -> Vec<Event> {
        let Some(running) = &self.running else {
            return Vec::new();
        };
        let events: Vec<Event> = running.events.try_iter().collect();
        if events.iter().any(|e| matches!(e, Event::Done(_))) {
            self.running = None;
        }
        events
    }

    fn scan(&mut self, mut job: Job) {
        job.apply = false;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let (events, _) = channel();
            let mut progress = ChannelProgress::new(events, Arc::new(AtomicBool::new(false)));
            let _ = tx.send(execute(&job, &mut progress, None));
        });
        self.scan = Some(rx);
    }

    fn poll_scan(&mut self) -> Option<RunResult> {
        let result = self.scan.as_ref()?.try_recv().ok()?;
        self.scan = None;
        Some(result)
    }

    fn probe(&mut self, port: u16) {
        if self.probe.is_some() {
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let answer = lr2lc_core::probe::probe(port).map_err(|e| e.short_reason());
            let _ = tx.send(answer);
        });
        self.probe = Some(rx);
    }

    fn poll_probe(&mut self) -> Option<Result<Probe, String>> {
        let answer = self.probe.as_ref()?.try_recv().ok()?;
        self.probe = None;
        Some(answer)
    }
}
