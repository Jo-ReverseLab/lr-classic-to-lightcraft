//! The writer runs the same through `HostSink` (a host that runs LightCraft's
//! commands in its own process) as through the control port: same commands
//! in the same order, same library, same mapping file. Error texts of the
//! host are read like the port's error replies.

use lc_test_support::fake_lightcraft::{Config, Failure, FakeLightCraft};
use lc_writer::apply::apply;
use lc_writer::control::{CommandSink, ControlClient, HostSink, Outcome, Request, WriteError};
use lc_writer::plan::WriteInput;
use lc_writer::state::State;
use model::{Collection, CollectionKind, Photo, Rating};
use serde_json::json;
use std::path::PathBuf;

fn input() -> WriteInput {
    let photos = (1..=3)
        .map(|i| {
            Photo::new(
                format!("/Users/example/Pictures/{i}.jpg"),
                i,
                Rating::new(u8::try_from(i).unwrap()).unwrap(),
            )
            .with_global_id(format!("g{i}"))
        })
        .collect();
    WriteInput {
        photos,
        collections: vec![Collection {
            lightroom_id: 10,
            name: "Album".into(),
            kind: CollectionKind::Album,
            parent: None,
            members: vec![3, 1],
        }],
        ..WriteInput::default()
    }
}

struct Run {
    _dir: tempfile::TempDir,
    fake: FakeLightCraft,
    state_file: PathBuf,
}

fn start() -> Run {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Test.lightcraft");
    let state_file = lc_writer::state::state_path(&library);
    let fake = FakeLightCraft::start(Config::new(&library));
    Run {
        _dir: dir,
        fake,
        state_file,
    }
}

fn host(fake: &FakeLightCraft) -> impl CommandSink + '_ {
    HostSink::new(move |request, params| match request {
        Request::Command(command) => fake.dispatch(command, params, true),
        Request::Method(method) => fake.dispatch(method, params, false),
    })
}

#[test]
fn a_host_sink_writes_what_the_control_port_writes() {
    let input = input();

    let port = start();
    let mut state = State::default();
    let mut client = ControlClient::connect(port.fake.port()).unwrap();
    let by_port = apply(&mut client, &input, &mut state, &port.state_file).unwrap();

    let local = start();
    let mut state = State::default();
    let by_host = apply(
        &mut host(&local.fake),
        &input,
        &mut state,
        &local.state_file,
    )
    .unwrap();

    assert_eq!(by_port.photos_imported, 3);
    assert_eq!(by_host.photos_imported, by_port.photos_imported);
    assert_eq!(by_host.members_added, by_port.members_added);
    assert_eq!(local.fake.commands(), port.fake.commands());
    assert_eq!(local.fake.photos(), port.fake.photos());
    assert_eq!(local.fake.ratings(), port.fake.ratings());
    assert_eq!(local.fake.albums(), port.fake.albums());
    assert_eq!(
        std::fs::read_to_string(&local.state_file).unwrap(),
        std::fs::read_to_string(&port.state_file).unwrap()
    );
}

#[test]
fn a_host_error_is_a_rejected_command_and_a_failed_save_is_applied() {
    let run = start();
    let mut sink = host(&run.fake);
    let error = sink.execute("no.such", json!({})).unwrap_err();
    assert!(
        matches!(&error, WriteError::Command { command, .. } if command == "no.such"),
        "{error}"
    );

    let mut saved_late = HostSink::new(|_, _| {
        Err("applied: saved in memory but not written to disk (disk full)".to_string())
    });
    assert!(matches!(
        saved_late.execute("photo.rate", json!({})),
        Ok(Outcome::AppliedNotSaved(_))
    ));
    assert!(matches!(
        saved_late.call("ui.inspect", json!({})),
        Ok(Outcome::AppliedNotSaved(_))
    ));
}

#[test]
fn preflight_runs_through_a_host_sink() {
    let run = start();
    let library = run.state_file.parent().unwrap().join("Test.lightcraft");
    lc_writer::control::preflight(&mut host(&run.fake), &library, false).unwrap();
    assert_eq!(
        run.fake.methods(),
        ["engine.execute", "ui.inspect", "engine.execute"]
    );
}

fn failing() -> Run {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Test.lightcraft");
    let state_file = lc_writer::state::state_path(&library);
    let mut config = Config::new(&library);
    config.failures.push(Failure {
        command: "library.import".into(),
        photo_id: None,
        message: "boom".into(),
    });
    Run {
        _dir: dir,
        fake: FakeLightCraft::start(config),
        state_file,
    }
}

#[test]
fn a_rejected_command_is_handled_like_over_the_port() {
    let port = failing();
    let mut client = ControlClient::connect(port.fake.port()).unwrap();
    let by_port = apply(
        &mut client,
        &input(),
        &mut State::default(),
        &port.state_file,
    )
    .unwrap();

    let local = failing();
    let by_host = apply(
        &mut host(&local.fake),
        &input(),
        &mut State::default(),
        &local.state_file,
    )
    .unwrap();

    assert_eq!(by_port.photos_imported, 0);
    assert_eq!(by_port.photos_skipped.len(), 3);
    assert_eq!(by_host.photos_skipped, by_port.photos_skipped);
    assert_eq!(local.fake.commands(), port.fake.commands());
}
