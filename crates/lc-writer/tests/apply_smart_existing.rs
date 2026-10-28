//! Acceptance tests for recovering smart collections after the mapping file
//! was lost (principle 5), module `lc_writer::apply`, against
//! the stateful fake LightCraft. Model: `apply_albums_existing.rs`.
//!
//! API under test:
//! - `ApplySummary.smart_existing: usize`: smart collections adopted from
//!   `albums.list` (not in the mapping file, no `album.createSmart`);
//! - balance, in every run: `input.smart.len() == smart_created +
//!   smart_known + smart_existing + smart_skipped.len()`.
//!
//! Rules:
//! - a translatable smart collection that is missing in the mapping file is
//!   adopted when `albums.list` holds EXACTLY ONE smart album (`smart:
//!   true`) with the same name (byte for byte, the Lightroom name trimmed as
//!   the app does), the same parent (both top level, or the LightCraft id of
//!   the parent folder, known or just adopted) and `rules` equal (JSON
//!   equality) to the translation, and no other mapping entry claims it. The
//!   entry is recorded in the mapping file (`kind: "smart"`, `lightcraft_id`,
//!   `name`, `parent`, `members: []`, `rules`);
//! - no hit: `album.createSmart` as before;
//! - several hits (or several smart collections wanting the one album):
//!   neither created nor adopted; `smart_skipped` entry with code
//!   `SkipCode::Ambiguous` and a reason containing `ambiguous: N smart
//!   albums with the same name, parent and rules in LightCraft` and no name;
//! - a regular collection never adopts a smart album and a smart collection
//!   never adopts a regular album;
//! - `albums.list` is sent at most once per run, before the first
//!   `album.create` or `album.createSmart`, and not at all when nothing is
//!   created (a normal second run).

use lc_writer::apply::{ApplySummary, apply};
use lc_writer::control::{ControlClient, WriteError};
use lc_writer::plan::WriteInput;
use lc_writer::smart::translate;
use lc_writer::state::State;
use model::{
    Collection, CollectionKind, SkipCode, SmartCollection, SmartCombine, SmartRule, SmartRules,
    SmartValue,
};
use serde_json::{Value, json};
use std::path::PathBuf;

#[path = "support/fake_lightcraft.rs"]
mod fake_lightcraft;
use fake_lightcraft::{AlbumView, Config, FakeLightCraft, SeedAlbum};

const AMBIGUOUS: &str =
    "ambiguous: 2 smart albums with the same name, parent and rules in LightCraft";

fn rules(op: &str, stars: f64) -> SmartRules {
    SmartRules {
        combine: SmartCombine::Intersect,
        rules: vec![SmartRule {
            criterion: "rating".into(),
            operation: op.into(),
            value: Some(SmartValue::Number(stars)),
            value2: None,
            units: None,
        }],
    }
}

fn smart(id: i64, name: &str, parent: Option<i64>, r: SmartRules) -> SmartCollection {
    SmartCollection {
        lightroom_id: id,
        name: name.into(),
        parent,
        rules: r,
    }
}

fn collection(id: i64, name: &str, kind: CollectionKind, parent: Option<i64>) -> Collection {
    Collection {
        lightroom_id: id,
        name: name.into(),
        kind,
        parent,
        members: Vec::new(),
    }
}

/// Folder 1 "Set"; smart collections: 10 "Top Smart" (>= 3, top level),
/// 11 "Deep Smart" (== 5, in the folder).
fn input() -> WriteInput {
    WriteInput {
        collections: vec![collection(1, "Set", CollectionKind::Folder, None)],
        smart: vec![
            smart(10, "Top Smart", None, rules(">=", 3.0)),
            smart(11, "Deep Smart", Some(1), rules("==", 5.0)),
        ],
        ..WriteInput::default()
    }
}

struct Env {
    _dir: tempfile::TempDir,
    library: PathBuf,
    state_file: PathBuf,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Test.lightcraft");
    let state_file = lc_writer::state::state_path(&library);
    Env {
        _dir: dir,
        library,
        state_file,
    }
}

type Commands = Vec<(String, Value)>;

fn run(
    fake: &FakeLightCraft,
    env: &Env,
    input: &WriteInput,
) -> (Result<ApplySummary, WriteError>, Commands) {
    let before = fake.commands().len();
    let mut state = State::load(&env.state_file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let result = apply(&mut client, input, &mut state, &env.state_file);
    (result, fake.commands()[before..].to_vec())
}

fn ok(fake: &FakeLightCraft, env: &Env, input: &WriteInput) -> (ApplySummary, Commands) {
    let (result, commands) = run(fake, env, input);
    let s = result.unwrap();
    assert_eq!(
        input.smart.len(),
        s.smart_created + s.smart_known + s.smart_existing + s.smart_skipped.len(),
        "{s:?}"
    );
    assert_eq!(
        input.collections.len(),
        s.folders_created
            + s.albums_created
            + s.collections_known
            + s.collections_existing
            + s.collections_skipped.len(),
        "{s:?}"
    );
    (s, commands)
}

fn of<'a>(commands: &'a Commands, name: &str) -> Vec<&'a Value> {
    commands
        .iter()
        .filter(|(c, _)| c == name)
        .map(|(_, p)| p)
        .collect()
}

fn state(env: &Env) -> Value {
    serde_json::from_str(&std::fs::read_to_string(&env.state_file).unwrap()).unwrap()
}

fn named(fake: &FakeLightCraft, name: &str) -> Vec<AlbumView> {
    fake.albums()
        .into_iter()
        .filter(|a| a.name == name)
        .collect()
}

fn lose_state(env: &Env) {
    std::fs::remove_file(&env.state_file).unwrap();
}

#[test]
fn lost_state_file_adopts_smart_albums_and_creates_nothing() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let inp = input();
    ok(&fake, &env, &inp);
    let albums = fake.albums();
    assert_eq!(albums.len(), 3);
    lose_state(&env);

    let (s, commands) = ok(&fake, &env, &inp);
    assert_eq!(s.smart_existing, 2, "{s:?}");
    assert_eq!((s.smart_created, s.smart_known), (0, 0));
    assert!(s.smart_skipped.is_empty());
    assert!(
        of(&commands, "album.createSmart").is_empty(),
        "{commands:?}"
    );
    assert!(of(&commands, "album.create").is_empty(), "{commands:?}");
    assert_eq!(fake.albums(), albums, "nothing created or changed");

    // The mapping file is complete again.
    let st = state(&env);
    let c = &st["collections"];
    assert_eq!(c.as_object().unwrap().len(), 3);
    let top = named(&fake, "Top Smart").remove(0);
    let deep = named(&fake, "Deep Smart").remove(0);
    let set = named(&fake, "Set").remove(0);
    assert_eq!(c["10"]["lightcraft_id"], top.id);
    assert_eq!(c["10"]["kind"], "smart");
    assert_eq!(c["10"]["name"], "Top Smart");
    assert_eq!(c["10"]["parent"], Value::Null);
    assert_eq!(c["10"]["members"], json!([]));
    assert_eq!(c["10"]["rules"], translate(&rules(">=", 3.0)).unwrap());
    assert_eq!(c["11"]["lightcraft_id"], deep.id);
    assert_eq!(c["11"]["parent"], 1);
    assert_eq!(deep.parent, Some(set.id));

    // Now it is stable: no album command, no albums.list.
    let (s, again) = ok(&fake, &env, &inp);
    assert_eq!(
        (s.smart_known, s.smart_existing, s.smart_created),
        (2, 0, 0)
    );
    assert!(of(&again, "albums.list").is_empty());
    assert!(!again.iter().any(|(c, _)| c.starts_with("album.")));
    assert_eq!(fake.albums(), albums);
}

#[test]
fn albums_list_is_read_once_before_the_first_create_smart() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let (_, commands) = ok(&fake, &env, &input());
    assert_eq!(of(&commands, "albums.list").len(), 1);
    let list = commands
        .iter()
        .position(|(c, _)| c == "albums.list")
        .unwrap();
    let first = commands
        .iter()
        .position(|(c, _)| c == "album.create" || c == "album.createSmart")
        .unwrap();
    assert!(list < first);

    // Only smart collections to create: still read once.
    let env2 = self::env();
    let fake2 = FakeLightCraft::start(Config::new(&env2.library));
    let inp = WriteInput {
        smart: vec![smart(10, "Top Smart", None, rules(">=", 3.0))],
        ..WriteInput::default()
    };
    let (_, c2) = ok(&fake2, &env2, &inp);
    assert_eq!(of(&c2, "albums.list").len(), 1, "{c2:?}");
}

#[test]
fn two_equal_smart_albums_are_reported_not_guessed() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let twins = WriteInput {
        smart: vec![
            smart(20, "Twin", None, rules(">=", 2.0)),
            smart(21, "Twin", None, rules(">=", 2.0)),
        ],
        ..WriteInput::default()
    };
    ok(&fake, &env, &twins);
    assert_eq!(named(&fake, "Twin").len(), 2);
    lose_state(&env);

    // Only one of the two is still in Lightroom: two albums match it.
    let one = WriteInput {
        smart: vec![smart(20, "Twin", None, rules(">=", 2.0))],
        ..WriteInput::default()
    };
    let (s, commands) = ok(&fake, &env, &one);
    assert_eq!((s.smart_existing, s.smart_created), (0, 0));
    assert_eq!(s.smart_skipped.len(), 1, "{:?}", s.smart_skipped);
    assert_eq!(s.smart_skipped[0].code, SkipCode::Ambiguous);
    assert!(s.smart_skipped[0].reason.contains(AMBIGUOUS));
    assert!(!s.smart_skipped[0].reason.contains("Twin"));
    assert!(of(&commands, "album.createSmart").is_empty());
    assert_eq!(named(&fake, "Twin").len(), 2);
    // The run stored nothing, so the mapping file may not exist at all.
    if env.state_file.exists() {
        assert!(
            state(&env)["collections"]
                .as_object()
                .is_none_or(|c| c.is_empty())
        );
    }

    // Both still in Lightroom: two collections want two albums, and none of
    // them can tell which is which.
    let (s, commands) = ok(&fake, &env, &twins);
    assert_eq!(s.smart_skipped.len(), 2);
    assert!(
        s.smart_skipped
            .iter()
            .all(|k| k.code == SkipCode::Ambiguous)
    );
    assert!(of(&commands, "album.createSmart").is_empty());
    assert_eq!(named(&fake, "Twin").len(), 2);
}

#[test]
fn same_name_with_other_rules_is_created_not_adopted() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let a = WriteInput {
        smart: vec![smart(10, "Picks", None, rules(">=", 3.0))],
        ..WriteInput::default()
    };
    ok(&fake, &env, &a);
    lose_state(&env);
    let changed = WriteInput {
        smart: vec![smart(10, "Picks", None, rules(">=", 4.0))],
        ..WriteInput::default()
    };
    let (s, commands) = ok(&fake, &env, &changed);
    assert_eq!((s.smart_existing, s.smart_created), (0, 1), "{s:?}");
    assert_eq!(of(&commands, "album.createSmart").len(), 1);
    let both = named(&fake, "Picks");
    assert_eq!(both.len(), 2);
    assert!(
        both.iter()
            .any(|b| b.rules == Some(translate(&rules(">=", 4.0)).unwrap()))
    );
}

#[test]
fn name_case_and_parent_must_match() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    ok(
        &fake,
        &env,
        &WriteInput {
            smart: vec![smart(10, "Picks", None, rules(">=", 3.0))],
            ..WriteInput::default()
        },
    );
    lose_state(&env);
    let other = WriteInput {
        collections: vec![collection(1, "Set", CollectionKind::Folder, None)],
        smart: vec![
            smart(10, "picks", None, rules(">=", 3.0)),
            smart(11, "Picks", Some(1), rules(">=", 3.0)),
        ],
        ..WriteInput::default()
    };
    let (s, _) = ok(&fake, &env, &other);
    assert_eq!((s.smart_existing, s.smart_created), (0, 2), "{s:?}");
    assert_eq!(fake.albums().len(), 4);
}

#[test]
fn regular_collections_never_adopt_smart_albums() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    ok(
        &fake,
        &env,
        &WriteInput {
            smart: vec![smart(10, "Mix", None, rules(">=", 3.0))],
            ..WriteInput::default()
        },
    );
    lose_state(&env);
    let regular = WriteInput {
        collections: vec![collection(7, "Mix", CollectionKind::Album, None)],
        ..WriteInput::default()
    };
    let (s, commands) = ok(&fake, &env, &regular);
    assert_eq!((s.albums_created, s.collections_existing), (1, 0), "{s:?}");
    assert_eq!(of(&commands, "album.create").len(), 1);
    let mix = named(&fake, "Mix");
    assert_eq!(mix.len(), 2);
    assert_eq!(mix.iter().filter(|a| a.rules.is_some()).count(), 1);
}

#[test]
fn smart_collections_never_adopt_regular_albums() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.albums = vec![SeedAlbum::album("Mix", None)];
    let fake = FakeLightCraft::start(config);
    let inp = WriteInput {
        smart: vec![smart(10, "Mix", None, rules(">=", 3.0))],
        ..WriteInput::default()
    };
    let (s, commands) = ok(&fake, &env, &inp);
    assert_eq!((s.smart_created, s.smart_existing), (1, 0), "{s:?}");
    assert_eq!(of(&commands, "album.createSmart").len(), 1);
    let mix = named(&fake, "Mix");
    assert_eq!(mix.len(), 2);
    assert_eq!(mix.iter().filter(|a| a.rules.is_none()).count(), 1);
}

#[test]
fn a_smart_album_claimed_by_another_entry_is_not_adopted_twice() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let r = rules(">=", 3.0);
    let first = WriteInput {
        smart: vec![smart(10, "Twin", None, r.clone())],
        ..WriteInput::default()
    };
    ok(&fake, &env, &first);
    // Collection 11 is equal to collection 10; the state keeps 10.
    let both = WriteInput {
        smart: vec![
            smart(10, "Twin", None, r.clone()),
            smart(11, "Twin", None, r),
        ],
        ..WriteInput::default()
    };
    let (s, commands) = ok(&fake, &env, &both);
    assert_eq!(
        (s.smart_known, s.smart_created, s.smart_existing),
        (1, 1, 0)
    );
    assert_eq!(of(&commands, "album.createSmart").len(), 1);
    assert_eq!(named(&fake, "Twin").len(), 2);
    let st = state(&env);
    assert_ne!(
        st["collections"]["10"]["lightcraft_id"],
        st["collections"]["11"]["lightcraft_id"]
    );
}

#[test]
fn recovered_smart_albums_are_not_duplicated_by_a_third_lost_state() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let inp = input();
    ok(&fake, &env, &inp);
    let albums = fake.albums();
    for _ in 0..2 {
        lose_state(&env);
        let (s, commands) = ok(&fake, &env, &inp);
        assert_eq!(s.smart_existing, 2);
        assert!(of(&commands, "album.createSmart").is_empty());
        assert_eq!(fake.albums(), albums);
    }
}

#[test]
fn no_smart_album_is_written_without_the_apply_call_and_the_library_is_not_changed_by_adoption() {
    // Adoption sends no command that changes the library.
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let inp = input();
    ok(&fake, &env, &inp);
    lose_state(&env);
    let (_, commands) = ok(&fake, &env, &inp);
    for word in [
        "album.delete",
        "album.rename",
        "album.move",
        "album.createSmart",
        "removePhotos",
    ] {
        assert!(!commands.iter().any(|(c, _)| c.contains(word)), "{word}");
    }
}
