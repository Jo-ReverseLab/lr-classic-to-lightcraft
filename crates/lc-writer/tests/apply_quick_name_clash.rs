//! Acceptance test for a normal Lightroom collection that is named like the
//! quick collection (task P5b-6a), module `lc_writer::apply`, against the
//! stateful fake LightCraft.
//!
//! Rule: albums whose LightCraft id is in `state.collections` (created or
//! adopted by lr2lc) are never candidates for the quick collection. So a
//! lr2lc album named "Quick Collection" never makes the quick step
//! "ambiguous" on later runs.

use lc_writer::apply::{ApplySummary, apply};
use lc_writer::control::ControlClient;
use lc_writer::plan::WriteInput;
use lc_writer::state::State;
use model::{Collection, CollectionKind, Photo, QuickCollection, Rating};
use serde_json::{Value, json};

#[path = "support/fake_lightcraft.rs"]
mod fake_lightcraft;
use fake_lightcraft::{Config, FakeLightCraft};

type Commands = Vec<(String, Value)>;

fn input(quick_members: &[i64]) -> WriteInput {
    let photos = (101..105)
        .map(|i| {
            Photo::new(
                format!("/Users/example/Pictures/{i}.jpg"),
                i,
                Rating::default(),
            )
            .with_global_id(format!("g{i}"))
        })
        .collect();
    WriteInput {
        photos,
        collections: vec![Collection {
            lightroom_id: 4,
            name: "Quick Collection".into(),
            kind: CollectionKind::Album,
            parent: None,
            members: vec![101],
        }],
        quick: Some(QuickCollection {
            lightroom_id: 50,
            members: quick_members.to_vec(),
        }),
        ..WriteInput::default()
    }
}

fn run(
    fake: &FakeLightCraft,
    state_file: &std::path::Path,
    input: &WriteInput,
) -> (ApplySummary, Commands) {
    let before = fake.commands().len();
    let mut state = State::load(state_file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let summary = apply(&mut client, input, &mut state, state_file).unwrap();
    (summary, fake.commands()[before..].to_vec())
}

fn of<'a>(commands: &'a Commands, name: &str) -> Vec<&'a Value> {
    commands
        .iter()
        .filter(|(c, _)| c == name)
        .map(|(_, p)| p)
        .collect()
}

fn quick_commands(commands: &Commands) -> usize {
    commands
        .iter()
        .filter(|(c, _)| {
            ["album.setTarget", "album.toggleTarget", "catalog.query"].contains(&c.as_str())
        })
        .count()
}

fn sorted(mut v: Vec<i64>) -> Vec<i64> {
    v.sort_unstable();
    v
}

#[test]
fn a_collection_named_like_the_quick_collection_is_never_the_quick_collection() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Test.lightcraft");
    let state_file = lc_writer::state::state_path(&library);
    let fake = FakeLightCraft::start(Config::new(&library));

    // The ids of the two top-level albums named "Quick Collection".
    let split = |fake: &FakeLightCraft| {
        let state: Value =
            serde_json::from_str(&std::fs::read_to_string(&state_file).unwrap()).unwrap();
        let own = state["collections"]["4"]["lightcraft_id"].as_u64().unwrap();
        let quick: Vec<u64> = fake
            .albums()
            .iter()
            .filter(|a| a.name == "Quick Collection" && !a.folder && a.id != own)
            .map(|a| a.id)
            .collect();
        assert_eq!(quick.len(), 1, "{:?}", fake.albums());
        (own, quick[0])
    };

    // Run 1: the album is created, the quick collection filled by toggle.
    let (s, commands) = run(&fake, &state_file, &input(&[102]));
    assert_eq!(s.albums_created, 1);
    assert_eq!(s.quick_added, 1);
    assert!(s.quick_skipped.is_empty() && s.quick_incomplete.is_empty());
    let toggles = of(&commands, "album.toggleTarget");
    assert_eq!(toggles.len(), 1);
    assert_eq!(toggles[0]["ids"], json!([1002]));
    let (own, quick) = split(&fake);
    assert_eq!(fake.album_members(own), vec![1001]);
    assert_eq!(fake.album_members(quick), vec![1002]);

    // Run 2: one more quick member; no "ambiguous", only the new photo sent.
    let (s, commands) = run(&fake, &state_file, &input(&[102, 103]));
    assert!(s.quick_skipped.is_empty(), "{:?}", s.quick_skipped);
    assert!(s.quick_incomplete.is_empty());
    assert_eq!((s.quick_added, s.quick_already), (1, 1));
    let toggles = of(&commands, "album.toggleTarget");
    assert_eq!(toggles.len(), 1, "{commands:?}");
    assert_eq!(toggles[0]["ids"], json!([1003]));
    assert_eq!(fake.album_members(own), vec![1001], "own album unchanged");
    assert_eq!(sorted(fake.album_members(quick)), vec![1002, 1003]);

    // Run 3: nothing changed, no quick command at all.
    let (s, commands) = run(&fake, &state_file, &input(&[102, 103]));
    assert_eq!(quick_commands(&commands), 0, "{commands:?}");
    assert!(s.quick_skipped.is_empty());
    assert_eq!((s.quick_added, s.quick_already), (0, 2));
    assert_eq!(fake.album_members(own), vec![1001]);
    assert_eq!(sorted(fake.album_members(quick)), vec![1002, 1003]);
    assert_eq!(fake.albums().len(), 2, "no duplicate albums");
}
