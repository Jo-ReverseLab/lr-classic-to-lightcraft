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

/// LightCraft does not list a batch's ids in file order. Each photo must
/// still get the id of its own file, and a name two files share falls back
/// to the single import.
#[test]
fn batch_ids_are_matched_to_files_by_name_not_position() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Test.lightcraft");
    let state_file = lc_writer::state::state_path(&library);
    let mut config = Config::new(&library);
    config.batch_ids_reversed = true;
    let fake = FakeLightCraft::start(config);
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let mut photos = input(120);
    // Same file name in another folder: ambiguous within the second batch.
    photos.photos.insert(
        1,
        Photo::new(
            "/Users/example/Other/0100.jpg",
            999,
            Rating::new(2).unwrap(),
        )
        .with_global_id("other"),
    );
    let summary = apply_partial(&mut client, &photos, &mut State::default(), &state_file).unwrap();

    assert_eq!(summary.photos_imported, 121);
    assert_eq!(summary.photos_existing, 0);
    assert_eq!(summary.journal.created_photos.len(), 121);
    let by_path = fake.photos();
    let state = State::load(&state_file).unwrap();
    for photo in &photos.photos {
        let gid = photo.global_id.as_deref().unwrap();
        let entry = state.photo(gid).unwrap();
        assert_eq!(
            entry.lightcraft_id,
            serde_json::json!(by_path[photo.path.to_str().unwrap()]),
            "{gid} has the id of its own file"
        );
    }
    let singles = imports(&fake).iter().filter(|n| **n == 1).count();
    assert_eq!(singles, 2, "only the two files sharing a name");
}

/// Cancel right after the batches, before any photo's own step: the
/// journal already holds every imported photo, so the rollback removes
/// them all.
#[test]
fn a_cancel_after_the_batches_takes_every_imported_photo_back() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Test.lightcraft");
    let state_file = lc_writer::state::state_path(&library);
    let fake = FakeLightCraft::start(Config::new(&library));
    let mut client = ControlClient::connect(fake.port()).unwrap();
    // Stops after the first step; the batches ran before it.
    let aborted = apply_partial_with(
        &mut client,
        &input(130),
        &mut State::default(),
        &state_file,
        &mut StopAfter(1, 0),
    )
    .unwrap_err();
    assert!(matches!(aborted.error, WriteError::Cancelled));
    let summary = aborted.summary;
    assert_eq!(
        summary.photos_imported, 130,
        "counted when LightCraft imported them"
    );
    assert_eq!(fake.photos().len(), 130, "the batches ran");
    assert_eq!(summary.journal.created_photos.len(), 130);
    let done = lc_writer::rollback::rollback(&mut client, &summary.journal, &state_file).unwrap();
    assert!(done.failed.is_empty(), "{:?}", done.failed);
    assert!(fake.photos().is_empty());
}

/// The keywords are created before any file is imported, batched or not:
/// LightCraft adds a sidecar's keywords on import.
#[test]
fn keywords_are_created_before_the_batches() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Test.lightcraft");
    let state_file = lc_writer::state::state_path(&library);
    let fake = FakeLightCraft::start(Config::new(&library));
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let mut photos = input(110);
    photos.photo_keywords.insert(
        5,
        vec![model::Keyword::new(vec!["Trip".to_string()]).unwrap()],
    );
    apply_partial(&mut client, &photos, &mut State::default(), &state_file).unwrap();
    let commands: Vec<String> = fake.commands().into_iter().map(|(c, _)| c).collect();
    let first_import = commands.iter().position(|c| c == "library.import").unwrap();
    let last_keyword = commands
        .iter()
        .rposition(|c| c == "keyword.create")
        .unwrap();
    assert!(last_keyword < first_import, "{commands:?}");
    assert_eq!(imports(&fake), [64, 46]);
}
