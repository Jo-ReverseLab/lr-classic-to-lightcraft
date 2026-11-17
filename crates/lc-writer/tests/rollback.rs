//! Cancel takes a run back: what the run added is removed, what it changed
//! is set back, and the mapping file is as before the run. Anything that was
//! in the library before stays.

use lc_test_support::fake_lightcraft::{Config, FakeLightCraft};
use lc_writer::apply::apply_partial;
use lc_writer::control::ControlClient;
use lc_writer::plan::WriteInput;
use lc_writer::rollback::rollback;
use lc_writer::state::State;
use model::{Collection, CollectionKind, Keyword, Photo, Rating, Stack};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn keyword(name: &str) -> Keyword {
    Keyword::new(vec!["Trip".to_string(), name.to_string()]).unwrap()
}

/// Photos 1..=n with 3 stars and a keyword, an album with all of them, a
/// stack of 1 and 2, and a virtual copy of photo 1 (id 100).
fn input(n: i64, stars: u8, album: &str) -> WriteInput {
    let mut photos: Vec<Photo> = (1..=n)
        .map(|i| {
            Photo::new(
                format!("/Users/example/Pictures/{i}.jpg"),
                i,
                Rating::new(stars).unwrap(),
            )
            .with_global_id(format!("g{i}"))
        })
        .collect();
    photos.push(
        Photo::new("/Users/example/Pictures/1.jpg", 100, Rating::default())
            .with_global_id("copy")
            .with_master(1),
    );
    let mut photo_keywords = BTreeMap::new();
    for i in 1..=n {
        photo_keywords.insert(i, vec![keyword(&format!("Place {i}"))]);
    }
    WriteInput {
        photos,
        keywords: Vec::new(),
        photo_keywords,
        collections: vec![Collection {
            lightroom_id: 10,
            name: album.into(),
            kind: CollectionKind::Album,
            parent: None,
            members: (1..=n).collect(),
        }],
        stacks: vec![Stack {
            lightroom_id: 20,
            members: vec![1, 2],
            collapsed: false,
        }],
        ..WriteInput::default()
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
    Env {
        state_file: lc_writer::state::state_path(&library),
        fake: FakeLightCraft::start(Config::new(&library)),
        _dir: dir,
    }
}

#[test]
fn cancelling_a_first_run_leaves_an_empty_library() {
    let env = env();
    let mut client = ControlClient::connect(env.fake.port()).unwrap();
    let summary = apply_partial(
        &mut client,
        &input(3, 3, "Album"),
        &mut State::default(),
        &env.state_file,
    )
    .unwrap();
    assert_eq!(env.fake.photos().len(), 4);
    assert!(!env.fake.keywords().is_empty());
    assert_eq!(env.fake.albums().len(), 1);
    assert_eq!(env.fake.stacks().len(), 1);

    let done = rollback(&mut client, &summary.journal, &env.state_file).unwrap();
    assert!(done.failed.is_empty(), "{:?}", done.failed);
    assert_eq!(done.photos_removed, 4);
    assert!(env.fake.photos().is_empty());
    assert!(env.fake.albums().is_empty());
    assert!(env.fake.stacks().is_empty());
    assert!(env.fake.keywords().is_empty(), "{:?}", env.fake.keywords());
    assert_eq!(State::load(&env.state_file).unwrap(), State::default());
}

#[test]
fn cancelling_a_second_run_restores_the_first() {
    let env = env();
    let mut client = ControlClient::connect(env.fake.port()).unwrap();
    let mut state = State::default();
    apply_partial(
        &mut client,
        &input(2, 3, "Album"),
        &mut state,
        &env.state_file,
    )
    .unwrap();
    let after_first = State::load(&env.state_file).unwrap();
    let photos_first = env.fake.photos();
    let ratings_first = env.fake.ratings();
    let keywords_first = env.fake.keywords();

    // In Lightroom: 4 stars now, a third photo, another album.
    let mut second = input(3, 4, "Album");
    second.collections.push(Collection {
        lightroom_id: 11,
        name: "New".into(),
        kind: CollectionKind::Album,
        parent: None,
        members: vec![3],
    });
    let mut state = after_first.clone();
    let summary = apply_partial(&mut client, &second, &mut state, &env.state_file).unwrap();
    assert_eq!(summary.photos_imported, 1);
    assert_eq!(env.fake.albums().len(), 2);

    let done = rollback(&mut client, &summary.journal, &env.state_file).unwrap();
    assert!(done.failed.is_empty(), "{:?}", done.failed);
    assert_eq!(
        env.fake.photos(),
        photos_first,
        "photo 3 is gone, the others stay"
    );
    let ratings: BTreeMap<i64, i64> = env
        .fake
        .ratings()
        .into_iter()
        .filter(|(id, _)| photos_first.values().any(|p| p == id))
        .collect();
    assert_eq!(ratings, ratings_first, "the stars are back");
    assert_eq!(env.fake.keywords(), keywords_first);
    let albums = env.fake.albums();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].photos.len(), 2, "photo 3 left the first album");
    assert_eq!(env.fake.stacks().len(), 1, "the first run's stack stays");
    assert_eq!(State::load(&env.state_file).unwrap(), after_first);

    // The next run is again a second run.
    let mut state = after_first;
    let again = apply_partial(&mut client, &second, &mut state, &env.state_file).unwrap();
    assert_eq!(again.photos_imported, 1);
}
