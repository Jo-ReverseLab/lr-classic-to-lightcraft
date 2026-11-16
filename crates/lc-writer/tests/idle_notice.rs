//! The control port's idle-close notice.
//!
//! LightCraft closes a connection that stayed idle too long and sends one
//! line without `id`: `{"ok": false, "error": "control connection idle read
//! timed out; closing the connection", "closed": "idle"}` (apps before 0.21
//! send it without `closed`). A request that crossed this notice never ran,
//! so the client may send it again, once, on a new connection. Any other
//! lost connection is never retried.

use lc_test_support::fake_lightcraft::{Config, FakeLightCraft};
use lc_writer::apply::apply;
use lc_writer::control::{ControlClient, Outcome, WriteError};
use lc_writer::plan::WriteInput;
use lc_writer::state::State;
use model::{Photo, Rating};
use serde_json::json;
use std::path::PathBuf;

fn fake(idle_at: &[usize], marker: bool) -> FakeLightCraft {
    let mut config = Config::new(&PathBuf::from("Test.lightcraft"));
    config.idle_close_at = idle_at.to_vec();
    config.idle_notice_without_marker = !marker;
    FakeLightCraft::start(config)
}

fn sent(fake: &FakeLightCraft, command: &str) -> usize {
    fake.commands().iter().filter(|(c, _)| c == command).count()
}

#[test]
fn a_request_that_crossed_the_notice_is_sent_once_more() {
    for marker in [true, false] {
        let fake = fake(&[2], marker);
        let mut client = ControlClient::connect(fake.port()).unwrap();
        assert!(matches!(
            client.execute("library.info", json!({})),
            Ok(Outcome::Applied(_))
        ));
        // The second line gets the idle notice; the client reconnects and
        // sends it again, and the answer is the real one.
        let outcome = client.execute("keyword.list", json!({})).unwrap();
        assert!(
            matches!(outcome, Outcome::Applied(_)),
            "marker {marker}: {outcome:?}"
        );
        assert_eq!(sent(&fake, "keyword.list"), 2, "marker {marker}");
        // The client keeps working on the new connection.
        assert!(client.execute("library.info", json!({})).is_ok());
    }
}

#[test]
fn a_second_notice_for_the_same_request_is_a_lost_connection() {
    let fake = fake(&[1, 2], true);
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let err = client.execute("keyword.list", json!({})).unwrap_err();
    assert!(matches!(err, WriteError::ConnectionLost { .. }), "{err:?}");
    assert_eq!(sent(&fake, "keyword.list"), 2, "resent exactly once");
}

#[test]
fn an_apply_run_survives_the_notice_without_skipping_anything() {
    let fake = fake(&[3], true);
    let dir = tempfile::tempdir().unwrap();
    let state_file = lc_writer::state::state_path(&dir.path().join("Test.lightcraft"));
    let input = WriteInput {
        photos: vec![
            Photo::new("/Users/example/Pictures/a.jpg", 1, Rating::new(3).unwrap())
                .with_global_id("g1"),
            Photo::new("/Users/example/Pictures/b.jpg", 2, Rating::new(4).unwrap())
                .with_global_id("g2"),
        ],
        ..WriteInput::default()
    };
    let mut state = State::load(&state_file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let summary = apply(&mut client, &input, &mut state, &state_file).unwrap();
    assert_eq!(summary.photos_imported, 2);
    assert!(
        summary.photos_skipped.is_empty(),
        "{:?}",
        summary.photos_skipped
    );
    assert!(
        summary.photos_incomplete.is_empty(),
        "{:?}",
        summary.photos_incomplete
    );
    assert_eq!(fake.photos().len(), 2, "no photo imported twice");
}
