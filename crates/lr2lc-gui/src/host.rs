//! What the window needs from its surroundings. The standalone app uses
//! native file dialogs and runs through the control port
//! ([`StandaloneHost`]); a host inside LightCraft would use its own dialogs,
//! its activity list and its command dispatch, and tests use a scripted
//! host.

use crate::run::{ChannelProgress, Event, RunResult, Running, execute_as};
use lr2lc_core::Job;
use lr2lc_core::probe::{LibraryAction, Probe};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

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
    /// The probe's answer once it is in, with the port it was asked on.
    fn poll_probe(&mut self) -> Option<(u16, Result<Probe, String>)>;

    /// Lets the person choose the folder a new library is created in.
    fn pick_new_library_folder(&mut self) -> Option<PathBuf>;
    /// Opens or creates a library in LightCraft on `port`.
    fn library_action(&mut self, port: u16, action: LibraryAction);
    /// The result of the library action once it is done.
    fn poll_library_action(&mut self) -> Option<Result<Probe, String>>;
    /// Brings LightCraft's window forward: it answers commands while its
    /// window is drawn.
    fn show_lightcraft(&mut self) {}
}

/// The standalone app: native dialogs; runs, scans and probes on threads
/// through the control port.
#[derive(Default)]
pub struct StandaloneHost {
    running: Option<Running>,
    scan: Option<(Receiver<RunResult>, Arc<AtomicBool>)>,
    probe: Option<(u16, Receiver<Result<Probe, String>>)>,
    library: Option<Receiver<Result<Probe, String>>>,
}

/// What a worker left in its channel: its answer, nothing yet, or `lost`
/// when the worker died without answering.
fn take<T>(receiver: &Receiver<T>, lost: impl FnOnce() -> T) -> Option<T> {
    match receiver.try_recv() {
        Ok(value) => Some(value),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => Some(lost()),
    }
}

const WORKER_LOST: &str = "the background task ended unexpectedly";

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
        let mut events = Vec::new();
        loop {
            match running.events.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // The worker is gone; without a final event it died.
                    if !events.iter().any(|e| matches!(e, Event::Done(_))) {
                        events.push(Event::Done(Box::new(RunResult::failed(
                            true,
                            lr2lc_core::ErrorKind::Stopped,
                            WORKER_LOST.to_owned(),
                        ))));
                    }
                    break;
                }
            }
        }
        if events.iter().any(|e| matches!(e, Event::Done(_))) {
            self.running = None;
        }
        events
    }

    fn scan(&mut self, mut job: Job) {
        // A newer scan replaces the running one, which stops at its next phase.
        if let Some((_, cancel)) = &self.scan {
            cancel.store(true, Ordering::Relaxed);
        }
        job.apply = false;
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        std::thread::spawn(move || {
            let (events, _) = channel();
            let mut progress = ChannelProgress::new(events, flag);
            let _ = tx.send(execute_as(&job, &mut progress, None, true));
        });
        self.scan = Some((rx, cancel));
    }

    fn poll_scan(&mut self) -> Option<RunResult> {
        let (receiver, _) = self.scan.as_ref()?;
        let result = take(receiver, || {
            RunResult::failed(
                false,
                lr2lc_core::ErrorKind::Stopped,
                WORKER_LOST.to_owned(),
            )
        })?;
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
        self.probe = Some((port, rx));
    }

    fn poll_probe(&mut self) -> Option<(u16, Result<Probe, String>)> {
        let (port, receiver) = self.probe.as_ref()?;
        let answer = take(receiver, || Err(WORKER_LOST.to_owned()))?;
        let port = *port;
        self.probe = None;
        Some((port, answer))
    }

    fn pick_new_library_folder(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Folder for the new library")
            .pick_folder()
    }

    fn library_action(&mut self, port: u16, action: LibraryAction) {
        self.show_lightcraft();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let answer =
                lr2lc_core::probe::library_action(port, &action).map_err(|e| e.to_string());
            let _ = tx.send(answer);
        });
        self.library = Some(rx);
    }

    fn poll_library_action(&mut self) -> Option<Result<Probe, String>> {
        let answer = take(self.library.as_ref()?, || Err(WORKER_LOST.to_owned()))?;
        self.library = None;
        Some(answer)
    }

    fn show_lightcraft(&mut self) {
        // macOS draws (and so runs commands of) a window that is visible.
        #[cfg(target_os = "macos")]
        if let Ok(mut child) = std::process::Command::new("open")
            .args(["-g", "-a", "LightCraft"])
            .spawn()
        {
            // Reaped off the UI thread so no zombie is left.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}
