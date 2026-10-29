//! Acceptance tests for the control-port client and the pre-flight checks,
//! against a fake port built from `std::net::TcpListener`.
//!
//! API under test (crate `lc-writer`, module `control`; all names below are
//! imported from `lc_writer::control`):
//! - `ControlClient::connect(port) -> Result<ControlClient, WriteError>`
//!   (127.0.0.1 only);
//! - `ControlClient::execute(command, params) -> Result<Outcome, WriteError>`
//!   sends one line `{"id":<n>,"method":"engine.execute","params":
//!   {"command":..,"params":..}}` and reads exactly one reply line;
//! - `Outcome::Applied(Value)`, `Outcome::AppliedNotSaved(String)` (reply
//!   `ok:false` whose text contains "saved in memory but not written to
//!   disk");
//! - `WriteError::{Connect, Protocol(String), Command { command, message },
//!   ConnectionLost { command }, NotPersistent, WrongLibrary { expected, open },
//!   LibraryProblem(String), XmpAutoWrite}` (`Connect` is matched by its
//!   Debug name only, so its payload is free);
//! - after an `ok:false` reply or a protocol error the connection is
//!   unusable: the next `execute` opens a new connection; a lost connection
//!   is never retried (the command is received exactly once);
//! - `preflight(&mut client, library, allow_xmp)` sends only the three
//!   read-only requests `library.info`, `ui.inspect` and
//!   `library.xmpPreferences`, always with empty params; `ui.inspect` goes as
//!   a method of the port itself (`ControlClient::call`), the others through
//!   `engine.execute`.
//!
//! The fake port is in `tests/support/fake_port.rs`; it records every line.
//!

#[path = "support/fake_port.rs"]
mod fake_port;

use fake_port::{FakePort, Reply};
use lc_writer::control::{ControlClient, Outcome, WriteError, preflight};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Run a test body on its own thread so a hanging client fails the test
/// instead of blocking the build.
fn with_timeout<F: FnOnce() + Send + 'static>(f: F) {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        f();
        let _ = tx.send(());
    });
    match rx.recv_timeout(Duration::from_secs(30)) {
        Ok(()) => {}
        // Disconnected means the body panicked; surface that.
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("test body panicked"),
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("test timed out after 30 s"),
    }
}

fn err<T>(result: Result<T, WriteError>) -> WriteError {
    match result {
        Ok(_) => panic!("expected an error, got Ok"),
        Err(e) => e,
    }
}

fn is_connect(e: &WriteError) -> bool {
    format!("{e:?}").starts_with("Connect")
}

// ---------------------------------------------------------------- execute

#[test]
fn execute_sends_documented_line_and_returns_applied_result() {
    with_timeout(|| {
        let port = FakePort::start(|_, _| Reply::Ok(json!({"changed": 3})));
        let mut client = ControlClient::connect(port.port()).unwrap();
        let params = json!({"rating": 4, "ids": [7, 8]});
        let outcome = client.execute("photo.rate", params.clone()).unwrap();
        let Outcome::Applied(result) = outcome else {
            panic!("expected Applied");
        };
        assert_eq!(result, json!({"changed": 3}));

        let requests = port.requests();
        assert_eq!(requests.len(), 1);
        let request = requests[0].as_object().unwrap();
        let mut keys: Vec<&str> = request.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["id", "method", "params"]);
        assert!(request["id"].is_number(), "id must be a number");
        assert_eq!(request["method"], "engine.execute");
        assert_eq!(
            request["params"],
            json!({"command": "photo.rate", "params": params})
        );
        // One request is exactly one line.
        assert!(!port.received()[0].line.contains('\n'));
    });
}

#[test]
fn consecutive_executes_reuse_one_connection() {
    with_timeout(|| {
        let port = FakePort::start(|_, _| Reply::Ok(json!(null)));
        let mut client = ControlClient::connect(port.port()).unwrap();
        for _ in 0..3 {
            assert!(matches!(
                client.execute("library.info", json!({})),
                Ok(Outcome::Applied(_))
            ));
        }
        assert_eq!(port.received().len(), 3);
        assert_eq!(port.connections(), 1);
    });
}

#[test]
fn error_reply_becomes_command_error_with_name_and_text() {
    with_timeout(|| {
        let port = FakePort::start(|_, _| Reply::Err("rating must be 0..5".into()));
        let mut client = ControlClient::connect(port.port()).unwrap();
        match err(client.execute("photo.rate", json!({"rating": 9}))) {
            WriteError::Command { command, message } => {
                assert_eq!(command, "photo.rate");
                assert!(
                    message.contains("rating must be 0..5"),
                    "message: {message}"
                );
            }
            other => panic!("expected Command, got {other:?}"),
        }
    });
}

// A server that accepts the request but never replies must surface as a
// clear error, not a hang. The 75 s production timeout is shortened for the
// test through `ControlClient::connect_with_timeout`; the fake stalls longer
// than that, so the read times out.
#[test]
fn stalled_server_times_out_instead_of_hanging() {
    with_timeout(|| {
        let port = FakePort::start(|_, _| {
            // Hold the connection open without replying longer than the
            // client's read timeout.
            std::thread::sleep(Duration::from_secs(2));
            Reply::Ok(json!(null))
        });
        let mut client =
            ControlClient::connect_with_timeout(port.port(), Duration::from_millis(100)).unwrap();
        match err(client.execute("photo.rate", json!({"rating": 3}))) {
            WriteError::ConnectionLost { command } => {
                assert_eq!(command, "photo.rate");
            }
            other => panic!("expected ConnectionLost from a stalled server, got {other:?}"),
        }
        // The request was received exactly once; it is not retried.
        assert_eq!(port.received().len(), 1);
    });
}

#[test]
fn saved_in_memory_error_counts_as_applied_not_saved() {
    with_timeout(|| {
        let text = "saved in memory but not written to disk: No space left on device";
        let port = FakePort::start(move |_, _| Reply::Err(text.into()));
        let mut client = ControlClient::connect(port.port()).unwrap();
        match client.execute("photo.label", json!({"label": "red", "ids": [1]})) {
            Ok(Outcome::AppliedNotSaved(message)) => {
                assert!(message.contains("saved in memory but not written to disk"));
            }
            Ok(_) => panic!("expected AppliedNotSaved"),
            Err(e) => panic!("expected AppliedNotSaved, got error {e:?}"),
        }
        // The command was sent once and not repeated.
        assert_eq!(port.received().len(), 1);
    });
}

#[test]
fn other_error_text_is_not_mistaken_for_saved_in_memory() {
    with_timeout(|| {
        let port = FakePort::start(|_, _| Reply::Err("not written to disk".into()));
        let mut client = ControlClient::connect(port.port()).unwrap();
        assert!(matches!(
            err(client.execute("photo.label", json!({}))),
            WriteError::Command { .. }
        ));
    });
}

#[test]
fn connection_closed_after_receive_is_connection_lost_and_never_retried() {
    with_timeout(|| {
        let port = FakePort::start(|_, _| Reply::Close);
        let mut client = ControlClient::connect(port.port()).unwrap();
        match err(client.execute("photo.virtualCopy", json!({"ids": [5]}))) {
            WriteError::ConnectionLost { command } => assert_eq!(command, "photo.virtualCopy"),
            other => panic!("expected ConnectionLost, got {other:?}"),
        }
        // Give a (wrong) retry time to show up.
        thread::sleep(Duration::from_millis(300));
        assert_eq!(
            port.received().len(),
            1,
            "command must be received exactly once"
        );
        assert_eq!(port.connections(), 1, "client must not reconnect to retry");
    });
}

#[test]
fn invalid_reply_line_is_protocol_error_and_next_execute_reconnects() {
    with_timeout(|| {
        // First connection answers garbage, later connections answer properly.
        let port = FakePort::start(|conn, _| {
            if conn == 0 {
                Reply::Raw("this is not json".into())
            } else {
                Reply::Ok(json!("fine"))
            }
        });
        let mut client = ControlClient::connect(port.port()).unwrap();
        assert!(matches!(
            err(client.execute("library.info", json!({}))),
            WriteError::Protocol(_)
        ));
        let outcome = client.execute("library.info", json!({})).unwrap();
        assert!(matches!(outcome, Outcome::Applied(v) if v == json!("fine")));

        let received = port.received();
        assert_eq!(received.len(), 2);
        assert_eq!(received[0].conn, 0);
        assert_eq!(
            received[1].conn, 1,
            "second execute must use a new connection"
        );
        assert_eq!(port.connections(), 2);
    });
}

#[test]
fn unexpected_reply_shapes_are_protocol_errors() {
    for raw in [
        "[1,2,3]",
        "{\"id\":1}",
        "{\"id\":1,\"ok\":\"yes\"}",
        "{\"id\":1,\"ok\":false}",
        "",
    ] {
        with_timeout(move || {
            let port = FakePort::start(move |_, _| Reply::Raw(raw.into()));
            let mut client = ControlClient::connect(port.port()).unwrap();
            let e = err(client.execute("library.info", json!({})));
            assert!(
                matches!(e, WriteError::Protocol(_)),
                "reply {raw:?} gave {e:?}"
            );
        });
    }
}

#[test]
fn next_execute_after_error_reply_uses_a_new_connection() {
    with_timeout(|| {
        let port = FakePort::start(|conn, _| {
            if conn == 0 {
                Reply::Err("unknown command".into())
            } else {
                Reply::Ok(json!({"ok": 1}))
            }
        });
        let mut client = ControlClient::connect(port.port()).unwrap();
        assert!(matches!(
            err(client.execute("nope", json!({}))),
            WriteError::Command { .. }
        ));
        assert!(matches!(
            client.execute("library.info", json!({})),
            Ok(Outcome::Applied(_))
        ));
        let received = port.received();
        assert_eq!(received.len(), 2);
        assert_eq!(received[1].conn, 1);
    });
}

#[test]
fn no_server_on_port_is_connect_error() {
    with_timeout(|| {
        // Reserve a free port, then close it again.
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let e = err(ControlClient::connect(port));
        assert!(is_connect(&e), "expected Connect, got {e:?}");
    });
}

// -------------------------------------------------------------- preflight

const READ_COMMANDS: [&str; 3] = ["library.info", "ui.inspect", "library.xmpPreferences"];

/// A fake port answering the three read commands with the given results.
fn preflight_port(info: Value, inspect: Value, xmp: Value) -> FakePort {
    FakePort::start(move |_, request| {
        // Like the real app: `ui.inspect` is a method of the port, and
        // unknown as an `engine.execute` command.
        let is_engine = request["method"] == "engine.execute";
        let name = if is_engine {
            request["params"]["command"].as_str().unwrap_or("")
        } else {
            request["method"].as_str().unwrap_or("")
        };
        match (is_engine, name) {
            (true, "library.info") => Reply::Ok(info.clone()),
            (false, "ui.inspect") => Reply::Ok(inspect.clone()),
            (true, "library.xmpPreferences") => Reply::Ok(xmp.clone()),
            (true, "ui.inspect") => Reply::Err("unknown command: ui.inspect".into()),
            (_, other) => Reply::Err(format!("unexpected request {other}")),
        }
    })
}

fn info_for(path: &Path) -> Value {
    json!({"persistent": true, "path": path.to_string_lossy(), "photos": 0, "albums": 0})
}

fn healthy_inspect() -> Value {
    json!({"libraryProblem": null, "tasks": [], "import": null})
}

fn xmp_off() -> Value {
    json!({"autoWrite": false, "naming": "stem"})
}

/// Only the three read commands, always with empty params, never anything
/// that could change state (no `library.xmpPreferences` with parameters, no
/// `library.toggleAutoWriteXmp`).
fn assert_only_reads(port: &FakePort) {
    for (command, params) in port.commands() {
        assert!(
            READ_COMMANDS.contains(&command.as_str()),
            "pre-flight sent {command}"
        );
        assert_eq!(params, json!({}), "pre-flight sent params with {command}");
    }
    // `ui.inspect` goes as a method of the port, everything else through
    // `engine.execute`.
    for request in port.requests() {
        let expected = if request["method"] == "ui.inspect" {
            true
        } else {
            request["method"] == "engine.execute" && request["params"]["command"] != "ui.inspect"
        };
        assert!(expected, "wrong request form: {request}");
    }
}

fn run_preflight(port: &FakePort, library: &Path, allow_xmp: bool) -> Result<(), WriteError> {
    let mut client = ControlClient::connect(port.port()).unwrap();
    preflight(&mut client, library, allow_xmp)
}

#[test]
fn preflight_succeeds_and_sends_exactly_the_three_reads_in_order() {
    with_timeout(|| {
        let dir = tempfile::tempdir().unwrap();
        let port = preflight_port(info_for(dir.path()), healthy_inspect(), xmp_off());
        run_preflight(&port, dir.path(), false).unwrap();
        let commands: Vec<String> = port.commands().into_iter().map(|(c, _)| c).collect();
        assert_eq!(commands, READ_COMMANDS);
        assert_only_reads(&port);
    });
}

#[test]
fn preflight_rejects_a_non_persistent_session() {
    with_timeout(|| {
        let dir = tempfile::tempdir().unwrap();
        let port = preflight_port(json!({"persistent": false}), healthy_inspect(), xmp_off());
        let e = err(run_preflight(&port, dir.path(), false));
        assert!(matches!(e, WriteError::NotPersistent), "got {e:?}");
        assert_only_reads(&port);
    });
}

#[test]
fn preflight_rejects_another_open_library() {
    with_timeout(|| {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let port = preflight_port(info_for(b.path()), healthy_inspect(), xmp_off());
        let e = err(run_preflight(&port, a.path(), false));
        assert!(matches!(e, WriteError::WrongLibrary { .. }), "got {e:?}");
        assert_only_reads(&port);
    });
}

#[test]
fn wrong_library_error_names_both_paths_as_given() {
    with_timeout(|| {
        // Neither path exists, so they are compared and reported literally.
        let expected = PathBuf::from("/nonexistent/example/expected.lcl");
        let open = PathBuf::from("/nonexistent/example/open.lcl");
        let port = preflight_port(info_for(&open), healthy_inspect(), xmp_off());
        match err(run_preflight(&port, &expected, false)) {
            WriteError::WrongLibrary {
                expected: e,
                open: o,
            } => {
                assert_eq!(e, expected);
                assert_eq!(o, open);
            }
            other => panic!("expected WrongLibrary, got {other:?}"),
        }
        assert_only_reads(&port);
    });
}

#[test]
fn nonexistent_identical_paths_are_compared_literally_and_match() {
    with_timeout(|| {
        let library = PathBuf::from("/nonexistent/example/library.lcl");
        let port = preflight_port(info_for(&library), healthy_inspect(), xmp_off());
        run_preflight(&port, &library, false).unwrap();
        assert_only_reads(&port);
    });
}

#[test]
fn existing_paths_are_compared_canonicalized() {
    with_timeout(|| {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("lib");
        std::fs::create_dir(&library).unwrap();
        // The app reports the same folder through a `..` detour.
        let aliased = library.join("..").join("lib");
        let port = preflight_port(info_for(&aliased), healthy_inspect(), xmp_off());
        run_preflight(&port, &library, false).unwrap();
        assert_only_reads(&port);
    });
}

#[test]
fn preflight_rejects_a_library_problem() {
    with_timeout(|| {
        let dir = tempfile::tempdir().unwrap();
        let inspect = json!({"libraryProblem": "library is locked by another process"});
        let port = preflight_port(info_for(dir.path()), inspect, xmp_off());
        match err(run_preflight(&port, dir.path(), false)) {
            WriteError::LibraryProblem(text) => {
                assert!(text.contains("locked by another process"), "text: {text}");
            }
            other => panic!("expected LibraryProblem, got {other:?}"),
        }
        assert_only_reads(&port);
    });
}

#[test]
fn preflight_rejects_xmp_auto_write_by_default() {
    with_timeout(|| {
        let dir = tempfile::tempdir().unwrap();
        let xmp = json!({"autoWrite": true, "naming": "stem"});
        let port = preflight_port(info_for(dir.path()), healthy_inspect(), xmp);
        let e = err(run_preflight(&port, dir.path(), false));
        assert!(matches!(e, WriteError::XmpAutoWrite), "got {e:?}");
        assert_only_reads(&port);
    });
}

#[test]
fn xmp_auto_write_is_accepted_with_allow_xmp_and_never_changed() {
    with_timeout(|| {
        let dir = tempfile::tempdir().unwrap();
        let xmp = json!({"autoWrite": true, "naming": "full"});
        let port = preflight_port(info_for(dir.path()), healthy_inspect(), xmp);
        run_preflight(&port, dir.path(), true).unwrap();
        // Still read-only: the preference is left as the user set it.
        assert_only_reads(&port);
    });
}

#[test]
fn preflight_sends_ui_inspect_as_its_own_method_with_empty_params() {
    with_timeout(|| {
        let dir = tempfile::tempdir().unwrap();
        let port = preflight_port(info_for(dir.path()), healthy_inspect(), xmp_off());
        run_preflight(&port, dir.path(), false).unwrap();
        let requests = port.requests();
        let inspects: Vec<_> = requests
            .iter()
            .filter(|r| r["method"] == "ui.inspect")
            .collect();
        assert_eq!(inspects.len(), 1, "exactly one ui.inspect request");
        assert_eq!(inspects[0]["params"], json!({}));
        assert!(
            requests
                .iter()
                .all(|r| r["params"]["command"] != "ui.inspect"),
            "ui.inspect must never be sent through engine.execute"
        );
        assert_eq!(
            port.methods(),
            ["engine.execute", "ui.inspect", "engine.execute"]
        );
    });
}

#[test]
fn library_problem_stops_the_preflight_before_the_xmp_check() {
    with_timeout(|| {
        let dir = tempfile::tempdir().unwrap();
        let inspect = json!({"libraryProblem": "journal is damaged"});
        let port = preflight_port(info_for(dir.path()), inspect, xmp_off());
        let e = err(run_preflight(&port, dir.path(), false));
        assert!(matches!(e, WriteError::LibraryProblem(_)), "got {e:?}");
        let commands: Vec<String> = port.commands().into_iter().map(|(c, _)| c).collect();
        assert_eq!(commands, ["library.info", "ui.inspect"]);
    });
}

#[test]
fn preflight_error_reply_to_ui_inspect_is_not_retried_or_repeated() {
    with_timeout(|| {
        let dir = tempfile::tempdir().unwrap();
        let info = info_for(dir.path());
        let port = FakePort::start(move |_, request| {
            if request["method"] == "ui.inspect" {
                Reply::Err("unknown method".into())
            } else {
                Reply::Ok(info.clone())
            }
        });
        let e = err(run_preflight(&port, dir.path(), false));
        assert!(matches!(e, WriteError::Command { .. }), "got {e:?}");
        thread::sleep(Duration::from_millis(300));
        let inspects = port.methods().iter().filter(|m| *m == "ui.inspect").count();
        assert_eq!(inspects, 1, "ui.inspect must be sent once");
    });
}

#[test]
fn preflight_lost_connection_during_ui_inspect_is_never_retried() {
    with_timeout(|| {
        let dir = tempfile::tempdir().unwrap();
        let info = info_for(dir.path());
        let port = FakePort::start(move |_, request| {
            if request["method"] == "ui.inspect" {
                Reply::Close
            } else {
                Reply::Ok(info.clone())
            }
        });
        match err(run_preflight(&port, dir.path(), false)) {
            WriteError::ConnectionLost { command } => assert_eq!(command, "ui.inspect"),
            other => panic!("expected ConnectionLost, got {other:?}"),
        }
        thread::sleep(Duration::from_millis(300));
        let inspects = port.methods().iter().filter(|m| *m == "ui.inspect").count();
        assert_eq!(inspects, 1, "ui.inspect must be sent once");
        assert_eq!(port.connections(), 1, "client must not reconnect to retry");
    });
}

// ------------------------------------------------------------------ call

#[test]
fn call_sends_a_method_of_the_port_itself_not_engine_execute() {
    with_timeout(|| {
        let port = FakePort::start(|_, _| Reply::Ok(json!({"libraryProblem": null})));
        let mut client = ControlClient::connect(port.port()).unwrap();
        let outcome = client.call("ui.inspect", json!({})).unwrap();
        assert_eq!(outcome, Outcome::Applied(json!({"libraryProblem": null})));
        let requests = port.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["method"], "ui.inspect");
        assert_eq!(requests[0]["params"], json!({}));
        assert!(requests[0]["id"].is_number());
    });
}

#[test]
fn call_error_reply_is_a_command_error_naming_the_method() {
    with_timeout(|| {
        let port = FakePort::start(|_, _| Reply::Err("boom".into()));
        let mut client = ControlClient::connect(port.port()).unwrap();
        match err(client.call("ui.inspect", json!({}))) {
            WriteError::Command { command, message } => {
                assert_eq!(command, "ui.inspect");
                assert_eq!(message, "boom");
            }
            other => panic!("expected Command, got {other:?}"),
        }
    });
}

#[test]
fn call_is_sent_once_and_a_lost_connection_is_never_retried() {
    with_timeout(|| {
        let port = FakePort::start(|_, _| Reply::Close);
        let mut client = ControlClient::connect(port.port()).unwrap();
        match err(client.call("ui.inspect", json!({}))) {
            WriteError::ConnectionLost { command } => assert_eq!(command, "ui.inspect"),
            other => panic!("expected ConnectionLost, got {other:?}"),
        }
        thread::sleep(Duration::from_millis(300));
        assert_eq!(port.received().len(), 1, "must be received exactly once");
        assert_eq!(port.connections(), 1, "client must not reconnect to retry");
    });
}

#[test]
fn call_after_a_protocol_error_reconnects_for_the_next_request() {
    with_timeout(|| {
        let port = FakePort::start(|conn, _| {
            if conn == 0 {
                Reply::Raw("not json".into())
            } else {
                Reply::Ok(json!("fine"))
            }
        });
        let mut client = ControlClient::connect(port.port()).unwrap();
        assert!(matches!(
            err(client.call("ui.inspect", json!({}))),
            WriteError::Protocol(_)
        ));
        let outcome = client.call("ui.inspect", json!({})).unwrap();
        assert_eq!(outcome, Outcome::Applied(json!("fine")));
        assert_eq!(port.connections(), 2);
    });
}

#[test]
fn execute_still_wraps_commands_in_engine_execute() {
    with_timeout(|| {
        let port = FakePort::start(|_, _| Reply::Ok(json!(1)));
        let mut client = ControlClient::connect(port.port()).unwrap();
        client.execute("library.info", json!({"a": 1})).unwrap();
        let requests = port.requests();
        assert_eq!(requests[0]["method"], "engine.execute");
        assert_eq!(requests[0]["params"]["command"], "library.info");
        assert_eq!(requests[0]["params"]["params"], json!({"a": 1}));
        assert_eq!(
            port.commands(),
            [("library.info".to_string(), json!({"a": 1}))]
        );
    });
}
