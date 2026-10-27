//! Client for the control port of a running LightCraft app, plus the
//! read-only pre-flight checks that must pass before anything is written.
//!
//! The protocol is JSON lines over a loopback TCP connection: one request
//! object per line, one reply object per line, in order. Every command goes
//! through the `engine.execute` method.
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

/// An open line-based connection.
struct Connection {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

/// Client for the LightCraft control port on `127.0.0.1`.
pub struct ControlClient {
    port: u16,
    connection: Option<Connection>,
    next_id: u64,
}

impl ControlClient {
    /// Connect to the control port on `127.0.0.1:port`.
    pub fn connect(port: u16) -> Result<ControlClient, WriteError> {
        let connection = open(port)?;
        Ok(ControlClient {
            port,
            connection: Some(connection),
            next_id: 1,
        })
    }

    /// Run one engine command and wait for its reply.
    ///
    /// The command is sent exactly once. If the reply cannot be read the
    /// result is `ConnectionLost`, never a retry.
    pub fn execute(&mut self, command: &str, params: Value) -> Result<Outcome, WriteError> {
        let mut connection = match self.connection.take() {
            Some(c) => c,
            None => open(self.port)?,
        };
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({
            "id": id,
            "method": "engine.execute",
            "params": {"command": command, "params": params},
        });
        let lost = || WriteError::ConnectionLost {
            command: command.to_string(),
        };
        // `Value::to_string` never emits a raw newline: newlines inside
        // strings are escaped, so the request is exactly one line.
        let line = format!("{request}\n");
        connection
            .writer
            .write_all(line.as_bytes())
            .and_then(|()| connection.writer.flush())
            .map_err(|_| lost())?;

        let mut reply = String::new();
        match connection.reader.read_line(&mut reply) {
            Ok(0) | Err(_) => return Err(lost()),
            Ok(_) => {}
        }
        if !reply.ends_with('\n') {
            // The connection closed in the middle of a reply line.
            return Err(lost());
        }
        let outcome = parse_reply(reply.trim_end_matches(['\r', '\n']), id, command)?;
        if let Outcome::Applied(_) = outcome {
            // Only a clean success keeps the connection; any other path has
            // already dropped it.
            self.connection = Some(connection);
        }
        Ok(outcome)
    }
}

fn open(port: u16) -> Result<Connection, WriteError> {
    let connect_error = |source| WriteError::Connect { port, source };
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).map_err(connect_error)?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
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
            if message.contains(SAVED_IN_MEMORY) {
                Ok(Outcome::AppliedNotSaved(message.to_string()))
            } else {
                Err(WriteError::Command {
                    command: command.to_string(),
                    message: message.to_string(),
                })
            }
        }
        _ => Err(protocol("reply has no boolean `ok`")),
    }
}

/// Run a read-only command and return its result.
fn read(client: &mut ControlClient, command: &str) -> Result<Value, WriteError> {
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
/// Sends only the read commands `library.info`, `ui.inspect` and
/// `library.xmpPreferences`, each with empty params, and changes nothing.
/// Fails if the session is not persistent, another library is open, the
/// app reports a library problem, or XMP auto-write is on and `allow_xmp`
/// is false.
pub fn preflight(
    client: &mut ControlClient,
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

    let inspect = read(client, "ui.inspect")?;
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
