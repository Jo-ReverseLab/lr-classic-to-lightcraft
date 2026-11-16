//! Progress and cancel while writing: the writer reports its phases, stops
//! only between units (never inside one photo), records what it finished,
//! and a second run completes the library without duplicates.

use lc_test_support::fake_lightcraft::{Config, FakeLightCraft};
use lc_writer::apply::{apply_partial, apply_partial_with};
use lc_writer::control::{ControlClient, WriteError};
use lc_writer::plan::WriteInput;
use lc_writer::progress::{Phase, Progress};
use lc_writer::state::State;
use model::{Photo, Rating};
use std::path::PathBuf;

/// Five photos, each with a rating, so each is an import plus an update.
fn input() -> WriteInput {
    WriteInput {
        photos: (1..=5)
            .map(|i| {
                Photo::new(
                    format!("/Users/example/Pictures/{i}.jpg"),
                    i,
                    Rating::new(3).unwrap(),
                )
                .with_global_id(format!("g{i}"))
            })
            .collect(),
        ..WriteInput::default()
    }
}

/// Records phases and step counts; cancels once `cancel_after` steps are done.
#[derive(Default)]
struct Recorder {
    phases: Vec<(Phase, usize)>,
    last: usize,
    cancel_after: Option<usize>,
}

impl Progress for Recorder {
    fn phase(&mut self, phase: Phase, total: usize) {
        self.phases.push((phase, total));
    }
    fn advance(&mut self, done: usize) {
        self.last = done;
    }
    fn cancelled(&self) -> bool {
        self.cancel_after.is_some_and(|n| self.last >= n)
    }
}

struct Env {
    _dir: tempfile::TempDir,
    fake: FakeLightCraft,
    state_file: PathBuf,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Test.lightcraft");
    let state_file = lc_writer::state::state_path(&library);
    Env {
        fake: FakeLightCraft::start(Config::new(&library)),
        _dir: dir,
        state_file,
    }
}

#[test]
fn writing_reports_every_step() {
    let env = env();
    let mut client = ControlClient::connect(env.fake.port()).unwrap();
    let mut progress = Recorder::default();
    apply_partial_with(
        &mut client,
        &input(),
        &mut State::default(),
        &env.state_file,
        &mut progress,
    )
    .unwrap();
    let [(Phase::Writing, total)] = progress.phases[..] else {
        panic!("{:?}", progress.phases);
    };
    assert!(total >= 10, "import and rate per photo: {total}");
    assert_eq!(progress.last, total);
}

#[test]
fn a_cancel_stops_between_photos_and_a_second_run_completes() {
    let env = env();
    let mut client = ControlClient::connect(env.fake.port()).unwrap();
    // Cancel right after the first step: the import of photo 1. Its rating
    // still goes out before the run stops.
    let mut progress = Recorder {
        cancel_after: Some(1),
        ..Recorder::default()
    };
    let mut state = State::default();
    let aborted = apply_partial_with(
        &mut client,
        &input(),
        &mut state,
        &env.state_file,
        &mut progress,
    )
    .unwrap_err();
    assert!(matches!(aborted.error, WriteError::Cancelled));
    assert_eq!(aborted.summary.photos_imported, 1);
    assert_eq!(env.fake.photos().len(), 1);
    assert_eq!(
        env.fake.ratings().values().copied().collect::<Vec<_>>(),
        [3]
    );
    let recorded = State::load(&env.state_file).unwrap();
    assert_eq!(recorded.photos().count(), 1);

    // The next run imports the other four, nothing twice.
    let before = env.fake.commands().len();
    let mut state = recorded;
    let summary = apply_partial(&mut client, &input(), &mut state, &env.state_file).unwrap();
    assert_eq!(summary.photos_imported, 4);
    assert_eq!(env.fake.photos().len(), 5);
    assert_eq!(env.fake.ratings().len(), 5);
    let imports = env.fake.commands()[before..]
        .iter()
        .filter(|(c, _)| c == "library.import")
        .count();
    assert_eq!(imports, 4);
}

#[test]
fn a_cancel_before_writing_sends_nothing() {
    let env = env();
    let mut client = ControlClient::connect(env.fake.port()).unwrap();
    let mut progress = Recorder {
        cancel_after: Some(0),
        ..Recorder::default()
    };
    let aborted = apply_partial_with(
        &mut client,
        &input(),
        &mut State::default(),
        &env.state_file,
        &mut progress,
    )
    .unwrap_err();
    assert!(matches!(aborted.error, WriteError::Cancelled));
    assert!(env.fake.write_commands().is_empty());
    assert_eq!(aborted.summary.photos_imported, 0);
}
