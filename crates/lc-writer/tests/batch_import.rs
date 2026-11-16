//! Large runs send their imports in batches before the steps run; every
//! photo is still recorded on its own, a failed file is skipped alone, and
//! a stopped run continues without importing anything twice.

use lc_test_support::fake_lightcraft::{Config, FakeLightCraft};
use lc_writer::apply::{apply_partial, apply_partial_with};
use lc_writer::control::{ControlClient, WriteError};
use lc_writer::plan::WriteInput;
use lc_writer::progress::{Phase, Progress};
use lc_writer::state::State;
use model::{Photo, Rating};

fn input(n: i64) -> WriteInput {
    WriteInput {
        photos: (1..=n)
            .map(|i| {
                Photo::new(
                    format!("/Users/example/Pictures/{i:04}.jpg"),
                    i,
                    Rating::new(2).unwrap(),
                )
                .with_global_id(format!("g{i:04}"))
            })
            .collect(),
        ..WriteInput::default()
    }
}

fn imports(fake: &FakeLightCraft) -> Vec<usize> {
    fake.commands()
        .iter()
        .filter(|(c, _)| c == "library.import")
        .map(|(_, p)| p["paths"].as_array().map_or(0, Vec::len))
        .collect()
}

#[test]
fn a_large_run_imports_in_batches_and_records_every_photo() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Test.lightcraft");
    let state_file = lc_writer::state::state_path(&library);
    let mut config = Config::new(&library);
    config.import_failed_for = Some("0007.jpg".into());
    let fake = FakeLightCraft::start(config);
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let summary =
        apply_partial(&mut client, &input(150), &mut State::default(), &state_file).unwrap();

    assert_eq!(
        imports(&fake),
        [64, 64, 22],
        "three batches, no single import"
    );
    assert_eq!(summary.photos_imported, 149);
    assert_eq!(summary.photos_skipped.len(), 1, "the failed file alone");
    assert_eq!(fake.ratings().len(), 149);
    let state = State::load(&state_file).unwrap();
    assert_eq!(state.photos().count(), 149);
    assert!(state.photo("g0007").is_none());

    // A second run has nothing to import.
    let before = fake.commands().len();
    let mut state = state;
    let again = apply_partial(&mut client, &input(150), &mut state, &state_file).unwrap();
    assert_eq!(again.photos_imported, 0);
    let sent: Vec<_> = fake.commands()[before..]
        .iter()
        .map(|(c, _)| c.clone())
        .collect();
    assert_eq!(
        sent.iter().filter(|c| *c == "library.import").count(),
        1,
        "only the failed file again"
    );
}

struct StopAfter(usize, usize);

impl Progress for StopAfter {
    fn phase(&mut self, _phase: Phase, _total: usize) {}
    fn advance(&mut self, done: usize) {
        self.1 = done;
    }
    fn cancelled(&self) -> bool {
        self.1 >= self.0
    }
}

#[test]
fn a_stopped_batch_run_continues_without_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Test.lightcraft");
    let state_file = lc_writer::state::state_path(&library);
    let fake = FakeLightCraft::start(Config::new(&library));
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let mut state = State::default();
    let aborted = apply_partial_with(
        &mut client,
        &input(120),
        &mut state,
        &state_file,
        &mut StopAfter(20, 0),
    )
    .unwrap_err();
    assert!(matches!(aborted.error, WriteError::Cancelled));
    let recorded = State::load(&state_file).unwrap();
    let done = recorded.photos().count();
    assert!((1..120).contains(&done), "stopped part way: {done}");
    // LightCraft already holds every batched file; the next run links the
    // rest instead of adding them again.
    assert_eq!(fake.photos().len(), 120);

    let mut state = recorded;
    let summary = apply_partial(&mut client, &input(120), &mut state, &state_file).unwrap();
    assert_eq!(fake.photos().len(), 120, "nothing imported twice");
    assert_eq!(
        summary.photos_imported + summary.photos_existing,
        120 - done
    );
    assert_eq!(State::load(&state_file).unwrap().photos().count(), 120);
    assert_eq!(fake.ratings().len(), 120);
}
