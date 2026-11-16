//! Client for the control port of a running LightCraft app, plus the
//! read-only pre-flight checks that must pass before anything is written.
//!
//! The protocol is JSON lines over a loopback TCP connection: one request
//! object per line, one reply object per line, in order. Engine commands go
//! through the `engine.execute` method; `ui.inspect` is a method of the port
//! itself and is sent as such (see `ControlClient::call`).
//!
//! Safety rules implemented here:
//! - only `127.0.0.1` is ever contacted;
//! - a command is sent at most once; a lost connection is reported, never
//!   retried, because its outcome is unknown;
//! - after an error reply or a protocol error the connection is dropped and
//!   the next command opens a fresh one.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The app answers within 60 s; leave some headroom on top of that.
const IO_TIMEOUT: Duration = Duration::from_secs(75);
/// Connecting to loopback is immediate when the port is open.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Error text of a command that was applied but not yet saved to disk.
const SAVED_IN_MEMORY: &str = "saved in memory but not written to disk";

/// Errors of the control-port client and the pre-flight checks.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    /// No connection to the control port could be opened.
    #[error("cannot connect to the LightCraft control port {port}: {source}")]
    Connect {
        port: u16,
        #[source]
        source: std::io::Error,
    },
    /// The app answered something that does not follow the protocol.
    #[error("unexpected reply from the LightCraft control port: {0}")]
    Protocol(String),
    /// The app rejected a command.
    #[error("LightCraft rejected `{command}`: {message}")]
    Command { command: String, message: String },
    /// The connection broke after the command was sent; its outcome is
    /// unknown and it is not repeated.
    #[error("connection lost while running `{command}`; its outcome is unknown")]
    ConnectionLost { command: String },
    /// The app runs a temporary in-memory session.
    #[error("LightCraft has no persistent library open")]
    NotPersistent,
    /// The app has a different library open than the one requested.
    #[error(
        "LightCraft has the library {} open, expected {}",
        open.display(),
        expected.display()
    )]
    WrongLibrary { expected: PathBuf, open: PathBuf },
    /// The app reports a problem with its library.
    #[error("LightCraft reports a library problem: {0}")]
    LibraryProblem(String),
    /// The library writes XMP sidecars next to the originals.
    #[error(
        "the library writes XMP sidecars automatically; turn this off in LightCraft or allow it explicitly"
    )]
    XmpAutoWrite,
    /// The mapping file could not be written; the run stops so that no
    /// command is sent whose result could not be recorded.
    #[error(transparent)]
    State(#[from] crate::state::StateError),
    /// An internal invariant broke: a step was run for an element the plan
    /// or the mapping file does not hold. Not reachable through normal input
    /// or a fake control-port server; the run stops with a clear message
    /// instead of silently returning `Ok(())`. The detail names only ids,
    /// never user content (paths or names).
    #[error("internal invariant broken: {detail}")]
    Internal { detail: String },
}

impl WriteError {
    /// A short reason for reports: fixed texts and command names only, never
    /// a path, a name or a message of the app.
    pub fn short_reason(&self) -> String {
        match self {
            WriteError::Connect { .. } => "cannot connect to the LightCraft control port".into(),
            WriteError::Protocol(_) => "unexpected reply from the LightCraft control port".into(),
            WriteError::Command { command, .. } => format!("LightCraft rejected {command}"),
            WriteError::ConnectionLost { command } => {
                format!("connection lost during {command}; outcome unknown")
            }
            WriteError::NotPersistent => "LightCraft has no persistent library open".into(),
            WriteError::WrongLibrary { .. } => "LightCraft has another library open".into(),
            WriteError::LibraryProblem(_) => "LightCraft reports a library problem".into(),
            WriteError::XmpAutoWrite => "the library writes XMP sidecars automatically".into(),
            WriteError::State(_) => "the mapping file could not be read or written".into(),
            WriteError::Internal { .. } => "an internal invariant broke; the run stopped".into(),
        }
    }
}

/// The result of a command the app has applied.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Applied and saved; carries the command's result.
    Applied(Value),
    /// Applied in memory, but the journal write failed; the app retries the
    /// save later. Carries the app's message.
    AppliedNotSaved(String),
}

/// Where the writer sends LightCraft commands. [`ControlClient`] sends them
/// to the control port of the running app; a host that runs LightCraft's
/// engine in the same process can implement this on top of its own command
/// dispatch, and tests use an in-process fake.
///
/// Both calls carry the same contract as the control port: a command runs
/// at most once, an error reply is [`WriteError::Command`], and an applied
/// command whose save failed is [`Outcome::AppliedNotSaved`].
pub trait CommandSink {
    /// Run one engine command (`library.import`, `photo.rate`, ...).
    fn execute(&mut self, command: &str, params: Value) -> Result<Outcome, WriteError>;

    /// Call a method of the host that is not an engine command (today only
    /// `ui.inspect` during the pre-flight checks).
    fn call(&mut self, method: &str, params: Value) -> Result<Outcome, WriteError>;
}

impl CommandSink for ControlClient {
    fn execute(&mut self, command: &str, params: Value) -> Result<Outcome, WriteError> {
        ControlClient::execute(self, command, params)
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Outcome, WriteError> {
        ControlClient::call(self, method, params)
    }
}

/// What a [`HostSink`] is asked to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request<'a> {
    /// An engine command, as `engine.execute` would run it.
    Command(&'a str),
    /// A method of the host itself, such as `ui.inspect`.
    Method(&'a str),
}

/// A [`CommandSink`] for a host that runs LightCraft's commands in the same
/// process: `run` gets the request and its params and answers with the
/// command's result or the host's error text, exactly what the control port
/// would put in `result` or `error`. The error text is interpreted as the
/// control port's is: a failed save becomes [`Outcome::AppliedNotSaved`],
/// anything else [`WriteError::Command`].
pub struct HostSink<F> {
    run: F,
}

impl<F> HostSink<F>
where
    F: FnMut(Request<'_>, Value) -> Result<Value, String>,
{
    pub fn new(run: F) -> Self {
        HostSink { run }
    }
}

impl<F> CommandSink for HostSink<F>
where
    F: FnMut(Request<'_>, Value) -> Result<Value, String>,
{
    fn execute(&mut self, command: &str, params: Value) -> Result<Outcome, WriteError> {
        host_reply(command, (self.run)(Request::Command(command), params))
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Outcome, WriteError> {
        host_reply(method, (self.run)(Request::Method(method), params))
    }
}

/// A host's answer as the control port would report it.
fn host_reply(command: &str, reply: Result<Value, String>) -> Result<Outcome, WriteError> {
    match reply {
        Ok(result) => Ok(Outcome::Applied(result)),
        Err(message) => failed(command, message),
    }
}

/// An error reply: applied but not saved, or rejected.
fn failed(command: &str, message: String) -> Result<Outcome, WriteError> {
    if message.contains(SAVED_IN_MEMORY) {
        Ok(Outcome::AppliedNotSaved(message))
    } else {
        Err(WriteError::Command {
            command: command.to_string(),
            message,
        })
    }
}

/// An open line-based connection.
struct Connection {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

/// Client for the LightCraft control port on `127.0.0.1`.
pub struct ControlClient {
    port: u16,
    timeout: Duration,
    connection: Option<Connection>,
    next_id: u64,
}

impl ControlClient {
    /// Connect to the control port on `127.0.0.1:port`.
    pub fn connect(port: u16) -> Result<ControlClient, WriteError> {
        Self::connect_with_timeout(port, IO_TIMEOUT)
    }

    /// Connect with a custom read/write timeout. Production code uses
    /// [`ControlClient::connect`] (the fixed 75 s [`IO_TIMEOUT`]); tests use
    /// this with a short timeout so a stalled server fails the test instead
    /// of hanging for 75 seconds.
    pub fn connect_with_timeout(port: u16, timeout: Duration) -> Result<ControlClient, WriteError> {
        let connection = open(port, timeout)?;
        Ok(ControlClient {
            port,
            timeout,
            connection: Some(connection),
            next_id: 1,
        })
    }

    /// Run one engine command and wait for its reply.
    ///
    /// The command is sent exactly once. If the reply cannot be read the
    /// result is `ConnectionLost`, never a retry. The one exception is the
    /// app's idle-close notice (see [`is_idle_notice`]): the request crossed
    /// it and never ran, so it is sent once more on a new connection.
    pub fn execute(&mut self, command: &str, params: Value) -> Result<Outcome, WriteError> {
        self.send(
            "engine.execute",
            json!({"command": command, "params": params}),
            command,
        )
    }

    /// Call a method of the control port itself (for example `ui.inspect`),
    /// which is not an engine command.
    ///
    /// Same rules as `execute`: sent exactly once, never retried. Errors
    /// name the port method.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Outcome, WriteError> {
        self.send(method, params, method)
    }

    /// Send one request and read its reply; `name` is used in errors. A
    /// request answered by the idle-close notice is sent once more on a new
    /// connection; a second notice counts as a lost connection.
    fn send(&mut self, method: &str, params: Value, name: &str) -> Result<Outcome, WriteError> {
        match self.send_once(method, &params, name)? {
            Sent::Reply(outcome) => Ok(outcome),
            Sent::IdleClosed => match self.send_once(method, &params, name)? {
                Sent::Reply(outcome) => Ok(outcome),
                Sent::IdleClosed => Err(WriteError::ConnectionLost {
                    command: name.to_string(),
                }),
            },
        }
    }

    fn send_once(&mut self, method: &str, params: &Value, name: &str) -> Result<Sent, WriteError> {
        let command = name;
        let mut connection = match self.connection.take() {
            Some(c) => c,
            None => open(self.port, self.timeout)?,
        };
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({
            "id": id,
            "method": method,
            "params": params,
        });
        let lost = || WriteError::ConnectionLost {
            command: command.to_string(),
        };
        // `Value::to_string` never emits a raw newline: newlines inside
        // strings are escaped, so the request is exactly one line.
        let line = format!("{request}\n");
        let written = connection
            .writer
            .write_all(line.as_bytes())
            .and_then(|()| connection.writer.flush());

        // Even if the write failed, the app may have sent its idle notice
        // before it closed the connection: read what is there.
        let mut reply = String::new();
        match connection.reader.read_line(&mut reply) {
            Ok(0) | Err(_) => return Err(lost()),
            Ok(_) => {}
        }
        if !reply.ends_with('\n') {
            // The connection closed in the middle of a reply line.
            return Err(lost());
        }
        let reply = reply.trim_end_matches(['\r', '\n']);
        if is_idle_notice(reply) {
            // The connection is closed; `self.connection` stays empty, so
            // the next attempt opens a new one.
            return Ok(Sent::IdleClosed);
        }
        if written.is_err() {
            return Err(lost());
        }
        let outcome = parse_reply(reply, id, command)?;
        if let Outcome::Applied(_) = outcome {
            // Only a clean success keeps the connection; any other path has
            // already dropped it.
            self.connection = Some(connection);
        }
        Ok(Sent::Reply(outcome))
    }
}

/// What one attempt to send a request got back.
enum Sent {
    Reply(Outcome),
    /// The app's idle-close notice: the request did not run.
    IdleClosed,
}

/// The error text of the app's idle-close notice. Apps from before the
/// `closed` marker (LightCraft < 0.21) are recognised by this exact text.
const IDLE_NOTICE: &str = "control connection idle read timed out; closing the connection";

/// Whether `line` is the notice LightCraft sends when it closes an idle
/// connection: `ok: false`, no `id`, and either `closed: "idle"` or exactly
/// the notice text. Such a reply answers no request, so the request that
/// crossed it never ran and may be sent again once.
fn is_idle_notice(line: &str) -> bool {
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(line) else {
        return false;
    };
    let no_id = matches!(object.get("id"), None | Some(Value::Null));
    let failed = object.get("ok") == Some(&Value::Bool(false));
    let marked = object.get("closed").and_then(Value::as_str) == Some("idle");
    let text = object.get("error").and_then(Value::as_str) == Some(IDLE_NOTICE);
    no_id && failed && (marked || text)
}

fn open(port: u16, timeout: Duration) -> Result<Connection, WriteError> {
    let connect_error = |source| WriteError::Connect { port, source };
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).map_err(connect_error)?;
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|()| stream.set_write_timeout(Some(timeout)))
        .map_err(connect_error)?;
    let writer = stream.try_clone().map_err(connect_error)?;
    Ok(Connection {
        reader: BufReader::new(stream),
        writer,
    })
}

/// Interpret one reply line to the request with id `id`.
fn parse_reply(line: &str, id: u64, command: &str) -> Result<Outcome, WriteError> {
    let protocol = |what: &str| WriteError::Protocol(format!("{what} (reply to `{command}`)"));
    let value: Value = serde_json::from_str(line).map_err(|_| protocol("reply is not JSON"))?;
    let object = value
        .as_object()
        .ok_or_else(|| protocol("reply is not a JSON object"))?;
    match object.get("id") {
        // The id echo is documented but not guaranteed; accept its absence.
        None | Some(Value::Null) => {}
        Some(echoed) if echoed.as_u64() == Some(id) => {}
        Some(_) => return Err(protocol("reply id does not match the request")),
    }
    match object.get("ok") {
        Some(Value::Bool(true)) => Ok(Outcome::Applied(
            object.get("result").cloned().unwrap_or(Value::Null),
        )),
        Some(Value::Bool(false)) => {
            let message = object
                .get("error")
                .and_then(Value::as_str)
                .ok_or_else(|| protocol("error reply without error text"))?;
            failed(command, message.to_string())
        }
        _ => Err(protocol("reply has no boolean `ok`")),
    }
}

/// Run a read-only command and return its result.
fn read(client: &mut dyn CommandSink, command: &str) -> Result<Value, WriteError> {
    match client.execute(command, json!({}))? {
        Outcome::Applied(value) => Ok(value),
        Outcome::AppliedNotSaved(message) => Err(WriteError::Command {
            command: command.to_string(),
            message,
        }),
    }
}

/// Check that the app is ready for writing into `library`.
///
/// Sends only the read requests `library.info` and `library.xmpPreferences`
/// (engine commands) and the port method `ui.inspect`, each with empty
/// params, and changes nothing.
/// Fails if the session is not persistent, another library is open, the
/// app reports a library problem, or XMP auto-write is on and `allow_xmp`
/// is false.
pub fn preflight(
    client: &mut dyn CommandSink,
    library: &Path,
    allow_xmp: bool,
) -> Result<(), WriteError> {
    let info = read(client, "library.info")?;
    match info.get("persistent") {
        Some(Value::Bool(true)) => {}
        Some(Value::Bool(false)) => return Err(WriteError::NotPersistent),
        _ => {
            return Err(WriteError::Protocol(
                "library.info has no boolean `persistent`".into(),
            ));
        }
    }
    let open = info
        .get("path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| WriteError::Protocol("library.info has no `path`".into()))?;
    if !same_library(library, &open) {
        return Err(WriteError::WrongLibrary {
            expected: library.to_path_buf(),
            open,
        });
    }

    let inspect = match client.call("ui.inspect", json!({}))? {
        Outcome::Applied(value) => value,
        Outcome::AppliedNotSaved(message) => {
            return Err(WriteError::Command {
                command: "ui.inspect".to_string(),
                message,
            });
        }
    };
    match inspect.get("libraryProblem") {
        None | Some(Value::Null) => {}
        Some(Value::String(text)) => return Err(WriteError::LibraryProblem(text.clone())),
        Some(other) => return Err(WriteError::LibraryProblem(other.to_string())),
    }

    let xmp = read(client, "library.xmpPreferences")?;
    let auto_write = xmp
        .get("autoWrite")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            WriteError::Protocol("library.xmpPreferences has no boolean `autoWrite`".into())
        })?;
    if auto_write && !allow_xmp {
        return Err(WriteError::XmpAutoWrite);
    }
    Ok(())
}

/// Compare canonicalized when both paths exist, literally otherwise.
fn same_library(expected: &Path, open: &Path) -> bool {
    match (expected.canonicalize(), open.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => expected == open,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_reasons_contain_no_paths_or_app_messages() {
        let secret = "/private/dir/a.jpg";
        let errors = [
            WriteError::Connect {
                port: 1,
                source: std::io::Error::other(secret),
            },
            WriteError::Protocol(secret.into()),
            WriteError::Command {
                command: "photo.rate".into(),
                message: secret.into(),
            },
            WriteError::ConnectionLost {
                command: "library.import".into(),
            },
            WriteError::NotPersistent,
            WriteError::WrongLibrary {
                expected: secret.into(),
                open: secret.into(),
            },
            WriteError::LibraryProblem(secret.into()),
            WriteError::XmpAutoWrite,
            WriteError::State(crate::state::StateError::Io {
                path: secret.into(),
                source: std::io::Error::other(secret),
            }),
            WriteError::Internal {
                detail: secret.into(),
            },
        ];
        for e in &errors {
            let text = e.short_reason();
            assert!(!text.is_empty() && !text.contains("private"), "{text}");
        }
        assert_eq!(
            errors[3].short_reason(),
            "connection lost during library.import; outcome unknown"
        );
    }

    #[test]
    fn idle_notice_is_recognised_with_marker_or_exact_text_only() {
        let text = IDLE_NOTICE;
        assert!(is_idle_notice(&format!(
            r#"{{"ok":false,"error":"{text}","closed":"idle"}}"#
        )));
        assert!(is_idle_notice(&format!(
            r#"{{"ok":false,"error":"{text}"}}"#
        )));
        assert!(is_idle_notice(
            r#"{"ok":false,"error":"other","closed":"idle"}"#
        ));
        // A reply to a request (with id) or another error is not the notice.
        assert!(!is_idle_notice(&format!(
            r#"{{"id":3,"ok":false,"error":"{text}"}}"#
        )));
        assert!(!is_idle_notice(
            r#"{"ok":false,"error":"no photos selected"}"#
        ));
        assert!(!is_idle_notice(r#"{"ok":true,"closed":"idle"}"#));
        assert!(!is_idle_notice("not json"));
    }

    #[test]
    fn success_reply_yields_result() {
        let outcome = parse_reply(r#"{"id":4,"ok":true,"result":[1]}"#, 4, "x").unwrap();
        assert_eq!(outcome, Outcome::Applied(json!([1])));
    }

    #[test]
    fn success_without_result_is_null() {
        let outcome = parse_reply(r#"{"id":4,"ok":true}"#, 4, "x").unwrap();
        assert_eq!(outcome, Outcome::Applied(Value::Null));
    }

    #[test]
    fn missing_id_is_accepted() {
        assert!(parse_reply(r#"{"ok":true,"result":1}"#, 4, "x").is_ok());
    }

    #[test]
    fn mismatched_id_is_protocol_error() {
        let e = parse_reply(r#"{"id":5,"ok":true}"#, 4, "x").unwrap_err();
        assert!(matches!(e, WriteError::Protocol(_)));
    }

    #[test]
    fn error_text_must_be_a_string() {
        let e = parse_reply(r#"{"id":4,"ok":false,"error":3}"#, 4, "x").unwrap_err();
        assert!(matches!(e, WriteError::Protocol(_)));
    }

    #[test]
    fn saved_in_memory_is_applied_not_saved() {
        let line = r#"{"id":1,"ok":false,"error":"saved in memory but not written to disk: full"}"#;
        assert!(matches!(
            parse_reply(line, 1, "x"),
            Ok(Outcome::AppliedNotSaved(_))
        ));
    }

    #[test]
    fn missing_paths_compare_literally() {
        let a = Path::new("/nonexistent/a");
        assert!(same_library(a, a));
        assert!(!same_library(a, Path::new("/nonexistent/b")));
    }

    #[test]
    fn existing_and_missing_path_do_not_match() {
        let dir = std::env::temp_dir();
        assert!(!same_library(&dir, &dir.join("does-not-exist-lr2lc")));
    }
}
