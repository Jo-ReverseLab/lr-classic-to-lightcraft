//! A quick, read-only look at the running LightCraft app, for a front end's
//! status line: is the control port answering, which library is open, and
//! does it write XMP sidecars automatically. Sends only `library.info` and
//! `library.xmpPreferences`; changes nothing.

use lc_writer::control::{CommandSink, ControlClient, Outcome, WriteError};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;

/// What the app reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// A library on disk is open (not a temporary session).
    pub persistent: bool,
    /// The open library's folder.
    pub library: Option<PathBuf>,
    /// The library writes XMP sidecars next to the originals.
    pub xmp_auto_write: Option<bool>,
}

impl Probe {
    /// Whether `library` is the one the app has open.
    pub fn has_open(&self, library: &std::path::Path) -> bool {
        self.library.as_deref().is_some_and(|open| {
            match (open.canonicalize(), library.canonicalize()) {
                (Ok(a), Ok(b)) => a == b,
                _ => open == library,
            }
        })
    }
}

/// Asks the app on `port`; waits at most a few seconds.
pub fn probe(port: u16) -> Result<Probe, WriteError> {
    let mut client = ControlClient::connect_with_timeout(port, Duration::from_secs(5))?;
    probe_with(&mut client)
}

/// Like [`probe`], through any sink.
pub fn probe_with(sink: &mut dyn CommandSink) -> Result<Probe, WriteError> {
    let info = read(sink, "library.info")?;
    let xmp = read(sink, "library.xmpPreferences")?;
    Ok(Probe {
        persistent: info["persistent"].as_bool().unwrap_or(false),
        library: info["path"].as_str().map(PathBuf::from),
        xmp_auto_write: xmp["autoWrite"].as_bool(),
    })
}

fn read(sink: &mut dyn CommandSink, command: &str) -> Result<Value, WriteError> {
    match sink.execute(command, json!({}))? {
        Outcome::Applied(value) => Ok(value),
        Outcome::AppliedNotSaved(message) => Err(WriteError::Command {
            command: command.to_owned(),
            message,
        }),
    }
}
