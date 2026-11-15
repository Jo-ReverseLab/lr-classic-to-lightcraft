//! Photos whose files have identical bytes at different paths.
//!
//! LightCraft's `library.import` treats a file whose bytes match a photo it
//! already has as a duplicate (`reason: "content"`) and imports nothing.
//! Two Lightroom photos must still become two LightCraft photos: the writer
//! makes a copy of the first record, points it at the second file with
//! `photo.relink` and resets it to import defaults, so neither photo's
//! fields leak into the other.
//!
//! A duplicate of a photo the mapping file does not know (a photo the library
//! had before) is still adopted as `photos_existing`.

use lc_writer::apply::{ApplySummary, apply};
use lc_writer::control::{ControlClient, WriteError};
use lc_writer::plan::WriteInput;
use lc_writer::state::State;
use model::{ColorLabel, Keyword, Photo, Rating};
use std::collections::BTreeMap;
use std::path::PathBuf;

use fake_lightcraft::{Config, Failure, FakeLightCraft, SeedPhoto};
use lc_test_support::fake_lightcraft;

const FIRST: &str = "/Users/example/Pictures/a/IMG_0001.jpg";
const SECOND: &str = "/Users/example/Pictures/b/IMG_0001.jpg";

fn kw(name: &str) -> Keyword {
    Keyword::new(vec![name.to_string()]).unwrap()
}

/// Two photos at different paths with the same bytes; each has its own
/// rating, label and keyword.
fn twins() -> WriteInput {
    let first = Photo::new(FIRST, 1, Rating::new(5).unwrap())
        .with_global_id("g1")
        .with_label(ColorLabel::Red);
    let second = Photo::new(SECOND, 2, Rating::new(2).unwrap()).with_global_id("g2");
    let mut photo_keywords = BTreeMap::new();
    photo_keywords.insert(1, vec![kw("First")]);
    photo_keywords.insert(2, vec![kw("Second")]);
    WriteInput {
        photos: vec![first, second],
        keywords: vec![kw("First"), kw("Second")],
        photo_keywords,
        ..WriteInput::default()
    }
}

fn config() -> Config {
    let mut config = Config::new(&PathBuf::from("Test.lightcraft"));
    config
        .file_contents
        .insert(FIRST.into(), "same bytes".into());
    config
        .file_contents
        .insert(SECOND.into(), "same bytes".into());
    config
}

struct Env {
    _dir: tempfile::TempDir,
    state_file: PathBuf,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let state_file = lc_writer::state::state_path(&dir.path().join("Test.lightcraft"));
    Env {
        _dir: dir,
        state_file,
    }
}

fn run(fake: &FakeLightCraft, env: &Env, input: &WriteInput) -> Result<ApplySummary, WriteError> {
    let mut state = State::load(&env.state_file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    apply(&mut client, input, &mut state, &env.state_file)
}

fn id_of(env: &Env, gid: &str) -> i64 {
    State::load(&env.state_file)
        .unwrap()
        .photo(gid)
        .unwrap()
        .lightcraft_id
        .as_i64()
        .unwrap()
}

#[test]
fn identical_files_become_two_photos() {
    let fake = FakeLightCraft::start(config());
    let env = env();
    let summary = run(&fake, &env, &twins()).unwrap();
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
    assert_eq!(summary.photos_imported, 2);
    assert_eq!(summary.photos_existing, 0);
    assert_eq!(summary.photos_same_content, 1);

    let (first, second) = (id_of(&env, "g1"), id_of(&env, "g2"));
    assert_ne!(
        first, second,
        "two Lightroom photos mapped to one LightCraft photo"
    );
    assert_eq!(fake.path_of(first).as_deref(), Some(FIRST));
    assert_eq!(fake.path_of(second).as_deref(), Some(SECOND));

    let a = fake.photo_fields(first).unwrap();
    let b = fake.photo_fields(second).unwrap();
    assert_eq!((a.rating, a.label.as_deref()), (5, Some("red")));
    assert_eq!(a.keywords, vec!["First".to_string()]);
    assert_eq!((b.rating, b.label.as_deref()), (2, None));
    assert_eq!(b.keywords, vec!["Second".to_string()]);
}

#[test]
fn second_run_changes_nothing() {
    let fake = FakeLightCraft::start(config());
    let env = env();
    let input = twins();
    run(&fake, &env, &input).unwrap();
    let mark = fake.commands().len();
    fake.clear_selection();
    let summary = run(&fake, &env, &input).unwrap();
    assert_eq!(summary.photos_imported, 0);
    assert_eq!(summary.photos_same_content, 0);
    assert_eq!(summary.photos_unchanged, 2);
    let writes: Vec<String> = fake.commands()[mark..]
        .iter()
        .map(|(c, _)| c.clone())
        .filter(|c| !fake_lightcraft::READ_COMMANDS.contains(&c.as_str()))
        .collect();
    assert!(writes.is_empty(), "second run wrote {writes:?}");
}

#[test]
fn duplicate_of_a_photo_the_library_already_had_is_adopted() {
    // The library has a photo with the same bytes that lr2lc never wrote:
    // that is the documented "already in the library" case, no copy is made.
    let mut config = config();
    config.seed_photos.push(SeedPhoto::plain(7));
    config.file_contents.insert(
        "/Users/example/Pictures/seed-7.jpg".into(),
        "same bytes".into(),
    );
    let fake = FakeLightCraft::start(config);
    let env = env();
    let input = WriteInput {
        photos: vec![Photo::new(FIRST, 1, Rating::new(0).unwrap()).with_global_id("g1")],
        ..WriteInput::default()
    };
    let summary = run(&fake, &env, &input).unwrap();
    assert_eq!(summary.photos_existing, 1);
    assert_eq!(summary.photos_same_content, 0);
    assert_eq!(id_of(&env, "g1"), 7);
    assert!(
        fake.commands()
            .iter()
            .all(|(c, _)| c != "photo.virtualCopy")
    );
}

#[test]
fn rejected_relink_skips_the_photo_and_removes_the_copy() {
    let mut config = config();
    config.failures.push(Failure {
        command: "photo.relink".into(),
        photo_id: None,
        message: "no such file".into(),
    });
    let fake = FakeLightCraft::start(config);
    let env = env();
    let summary = run(&fake, &env, &twins()).unwrap();
    assert_eq!(summary.photos_imported, 1);
    let skip = summary
        .photos_skipped
        .iter()
        .find(|s| s.code.as_str() == "same_content")
        .expect("second photo skipped with same_content");
    assert!(skip.reason.starts_with("photo 2:"), "{}", skip.reason);
    assert!(State::load(&env.state_file).unwrap().photo("g2").is_none());
    // The half-made copy does not stay in the library.
    assert!(fake.commands().iter().any(|(c, _)| c == "photo.delete"));
    assert_eq!(fake.photos().len(), 1);
}
