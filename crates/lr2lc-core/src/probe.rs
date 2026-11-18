//! A quick, read-only look at the running LightCraft app, for a front end's
//! status line: is the control port answering, which library is open, and
//! does it write XMP sidecars automatically. Sends only `library.info` and
//! `library.xmpPreferences`; changes nothing. Also the library actions a
//! front end offers: open or create a library, select one photo to show it.

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

/// What to do with LightCraft's open library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryAction {
    /// Switch LightCraft to the library at this folder.
    Open(PathBuf),
    /// Create a new library folder `name` inside `folder` and switch to it.
    Create { folder: PathBuf, name: String },
}

/// Runs `action` in the app on `port` and reports its state afterwards.
/// Switching libraries can take a while; waits up to two minutes.
pub fn library_action(port: u16, action: &LibraryAction) -> Result<Probe, WriteError> {
    let mut client = ControlClient::connect_with_timeout(port, Duration::from_secs(120))?;
    library_action_with(&mut client, action)
}

/// Like [`library_action`], through any sink.
pub fn library_action_with(
    sink: &mut dyn CommandSink,
    action: &LibraryAction,
) -> Result<Probe, WriteError> {
    let (command, params) = match action {
        LibraryAction::Open(path) => ("app.openLibrary", json!({"path": path})),
        LibraryAction::Create { folder, name } => {
            ("app.newLibrary", json!({"path": folder, "name": name}))
        }
    };
    match sink.execute(command, params)? {
        Outcome::Applied(_) => probe_with(sink),
        Outcome::AppliedNotSaved(message) => Err(WriteError::Command {
            command: command.to_owned(),
            message,
        }),
    }
}

/// Selects photo `id` in the app on `port` and makes it the active one, so
/// the person can look at it there. Changes only the selection.
pub fn select_photo(port: u16, id: &Value) -> Result<(), WriteError> {
    let mut client = ControlClient::connect_with_timeout(port, Duration::from_secs(5))?;
    select_photo_with(&mut client, id)
}

/// Like [`select_photo`], through any sink.
pub fn select_photo_with(sink: &mut dyn CommandSink, id: &Value) -> Result<(), WriteError> {
    let command = "library.select";
    match sink.execute(command, lc_writer::plan::select_params(id))? {
        Outcome::Applied(_) => Ok(()),
        Outcome::AppliedNotSaved(message) => Err(WriteError::Command {
            command: command.to_owned(),
            message,
        }),
    }
}
