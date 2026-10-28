//! Acceptance tests for writing smart collections as smart albums, module `lc_writer::apply`, against the stateful fake LightCraft
//! (which implements `album.createSmart` like the real app: `rules` is
//! mandatory in the fake, a parent must be an existing folder, names are
//! trimmed, smart albums refuse `album.addPhotos`).
//!
//! API under test (see `plan_smart.rs` and `smart_translate.rs`):
//! - `WriteInput.smart: Vec<model::SmartCollection>`;
//! - `ApplySummary { smart_created, smart_known, smart_rules_changed: usize,
//!   smart_skipped: Vec<Skipped> }`;
//! - the mapping file entry `collections.<Lightroom id>` = `{lightcraft_id,
//!   kind: "smart", name, parent, members: [], rules}`.
//!
//! Rules:
//! - `album.createSmart {name, parent?, rules}` is sent after every
//!   `album.create` and member command, once per translatable smart
//!   collection, with the translated rules and the LightCraft id of the
//!   parent folder (no `parent` key on top level); nothing else is sent for
//!   it (no select, no addPhotos);
//! - every created smart album is recorded at once;
//! - a second run sends no `album.createSmart` (and no other album command
//!   and no select); a changed rule is counted in `smart_rules_changed`, not
//!   written;
//! - untranslatable collections, a parent folder that was not created, and a
//!   rejected `album.createSmart` are in `smart_skipped` (nothing recorded;
//!   a later run tries again); a lost connection aborts;
//! - balance, in every run: `input.smart.len() == smart_created +
//!   smart_known + smart_skipped.len()`; the regular-collection balance of
//!   `apply_albums.rs` is not disturbed (smart entries are not counted in
//!   `collections_removed`);
//! - reasons never contain a name, value, operation or combine text.

use lc_writer::apply::{ApplySummary, apply};
use lc_writer::control::{ControlClient, WriteError};
use lc_writer::plan::WriteInput;
use lc_writer::smart::translate;
use lc_writer::state::State;
use model::{
    Collection, CollectionKind, SmartCollection, SmartCombine, SmartRule, SmartRules, SmartValue,
};
use serde_json::{Value, json};
use std::path::PathBuf;

#[path = "support/fake_lightcraft.rs"]
mod fake_lightcraft;
use fake_lightcraft::{AlbumFailure, Config, FakeLightCraft};

const SECRET: &str = "ZyxSecret";

fn rating(op: &str, stars: f64) -> SmartRule {
    SmartRule {
        criterion: "rating".into(),
        operation: op.into(),
        value: Some(SmartValue::Number(stars)),
        value2: None,
        units: None,
    }
}

fn rules(combine: SmartCombine, r: Vec<SmartRule>) -> SmartRules {
    SmartRules { combine, rules: r }
}

fn good(stars: f64) -> SmartRules {
    rules(SmartCombine::Intersect, vec![rating(">=", stars)])
}

fn bad() -> SmartRules {
    rules(
        SmartCombine::Intersect,
        vec![SmartRule {
            criterion: "labelColor".into(),
            operation: "==".into(),
            value: Some(SmartValue::Text(format!("red {SECRET}"))),
            value2: None,
            units: None,
        }],
    )
}

fn smart(id: i64, name: &str, parent: Option<i64>, rules: SmartRules) -> SmartCollection {
    SmartCollection {
        lightroom_id: id,
        name: name.into(),
        parent,
        rules,
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

/// Folder 1 "Set", album 2 "Plain" below it; smart collections: 10 top
/// level (3 stars), 11 in the folder (== 5), 12 untranslatable.
fn input() -> WriteInput {
    WriteInput {
        collections: vec![
            collection(1, "Set", CollectionKind::Folder, None),
            collection(2, "Plain", CollectionKind::Album, Some(1)),
        ],
        smart: vec![
            smart(10, "Top Smart", None, good(3.0)),
            smart(
                11,
                "Deep Smart",
                Some(1),
                rules(SmartCombine::Union, vec![rating("==", 5.0)]),
            ),
            smart(12, &format!("Bad {SECRET}"), None, bad()),
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
        s.smart_created + s.smart_known + s.smart_skipped.len(),
        "{s:?}"
    );
    assert_eq!(
        input.collections.len(),
        s.folders_created + s.albums_created + s.collections_known + s.collections_skipped.len(),
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

fn album_named(fake: &FakeLightCraft, name: &str) -> fake_lightcraft::AlbumView {
    let found: Vec<_> = fake
        .albums()
        .into_iter()
        .filter(|a| a.name == name)
        .collect();
    assert_eq!(found.len(), 1, "{name}: {found:?}");
    found.into_iter().next().unwrap()
}

fn first_run() -> (FakeLightCraft, Env) {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    ok(&fake, &env, &input());
    (fake, env)
}

#[test]
fn create_smart_gets_the_translated_rules_and_the_right_parent() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let (s, commands) = ok(&fake, &env, &input());
    assert_eq!((s.smart_created, s.smart_known), (2, 0));
    assert_eq!(s.smart_skipped.len(), 1);

    let set = album_named(&fake, "Set");
    let calls = of(&commands, "album.createSmart");
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[0],
        &json!({"name": "Top Smart", "rules": translate(&good(3.0)).unwrap()})
    );
    assert_eq!(
        calls[1],
        &json!({
            "name": "Deep Smart",
            "parent": set.id,
            "rules": translate(&input().smart[1].rules).unwrap(),
        })
    );
    // The fake keeps what it received.
    let top = album_named(&fake, "Top Smart");
    let deep = album_named(&fake, "Deep Smart");
    assert_eq!(top.rules, Some(translate(&good(3.0)).unwrap()));
    assert_eq!((top.parent, deep.parent), (None, Some(set.id)));
    assert!(!top.folder && !deep.folder);
    // Set, Plain, Top Smart and Deep Smart exist; smart collection 12 is not
    // translatable and is not in the library.
    assert_eq!(fake.albums().len(), 4);
}

#[test]
fn smart_albums_come_after_all_regular_album_commands_and_get_no_members() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let mut inp = input();
    inp.photos = vec![
        model::Photo::new(
            "/Users/example/Pictures/101.jpg",
            101,
            model::Rating::default(),
        )
        .with_global_id("g101"),
    ];
    inp.collections[1].members = vec![101];
    let (_, commands) = ok(&fake, &env, &inp);
    let first_smart = commands
        .iter()
        .position(|(c, _)| c == "album.createSmart")
        .unwrap();
    assert!(
        commands[..first_smart]
            .iter()
            .any(|(c, _)| c == "album.addPhotos")
    );
    assert!(
        commands[first_smart..]
            .iter()
            .all(|(c, _)| c == "album.createSmart"),
        "{commands:?}"
    );
}

#[test]
fn mapping_file_records_smart_albums() {
    let (fake, env) = first_run();
    let st = state(&env);
    let c = &st["collections"];
    let top = album_named(&fake, "Top Smart");
    assert_eq!(
        c["10"],
        json!({
            "lightcraft_id": top.id,
            "kind": "smart",
            "name": "Top Smart",
            "parent": null,
            "members": [],
            "rules": translate(&good(3.0)).unwrap(),
        })
    );
    assert_eq!(c["11"]["parent"], 1);
    assert_eq!(c["11"]["kind"], "smart");
    // Skipped: not recorded. Regular collections still are.
    assert!(c.get("12").is_none());
    assert_eq!(c.as_object().unwrap().len(), 4);
}

#[test]
fn second_run_sends_no_create_smart_and_no_album_command() {
    let (fake, env) = first_run();
    let albums = fake.albums();
    let (s, commands) = ok(&fake, &env, &input());
    assert!(
        commands
            .iter()
            .all(|(c, _)| !c.starts_with("album.") && c != "library.select"),
        "{commands:?}"
    );
    assert_eq!((s.smart_created, s.smart_known), (0, 2));
    assert_eq!(
        s.smart_skipped.len(),
        1,
        "the untranslatable one stays skipped"
    );
    assert_eq!(s.smart_rules_changed, 0);
    assert_eq!(s.collections_removed, 0);
    assert_eq!(fake.albums(), albums);
}

#[test]
fn changed_rules_are_counted_but_not_written() {
    let (fake, env) = first_run();
    let albums = fake.albums();
    let before = state(&env);
    let mut inp = input();
    inp.smart[0].rules = good(4.0);
    inp.smart[0].name = "Renamed".into();
    let (s, commands) = ok(&fake, &env, &inp);
    assert_eq!((s.smart_created, s.smart_known), (0, 2));
    assert_eq!(s.smart_rules_changed, 1);
    assert_eq!(s.collections_renamed, 1);
    assert!(of(&commands, "album.createSmart").is_empty());
    assert_eq!(fake.albums(), albums);
    assert_eq!(state(&env), before);
}

#[test]
fn rejected_create_smart_is_skipped_and_tried_again_next_run() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.album_failures = vec![AlbumFailure {
        command: "album.createSmart".into(),
        album_name: Some("Top Smart".into()),
        message: format!("rule 1.1: refused {SECRET}"),
    }];
    let fake = FakeLightCraft::start(config);
    let (s, commands) = ok(&fake, &env, &input());
    assert_eq!(s.smart_created, 1, "the run goes on");
    assert_eq!(s.smart_skipped.len(), 2);
    assert_eq!(of(&commands, "album.createSmart").len(), 2);
    assert!(state(&env)["collections"].get("10").is_none());
    for r in &s.smart_skipped {
        assert!(
            !r.reason.contains("Top Smart") && !r.reason.contains(SECRET),
            "{}",
            r.reason
        );
    }
    // Same fake, still failing: the next run tries again for 10 only.
    let (s, commands) = ok(&fake, &env, &input());
    assert_eq!((s.smart_created, s.smart_known), (0, 1));
    assert_eq!(of(&commands, "album.createSmart").len(), 1);
}

#[test]
fn parent_folder_that_was_not_created_skips_the_smart_collection() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.album_failures = vec![AlbumFailure {
        command: "album.create".into(),
        album_name: Some("Set".into()),
        message: "refused".into(),
    }];
    let fake = FakeLightCraft::start(config);
    let (s, commands) = ok(&fake, &env, &input());
    assert_eq!(s.smart_created, 1, "only the top-level one");
    assert_eq!(s.smart_skipped.len(), 2);
    assert!(
        s.smart_skipped
            .iter()
            .any(|r| r.reason.contains("11") && r.reason.contains("parent")),
        "{:?}",
        s.smart_skipped
    );
    assert_eq!(of(&commands, "album.createSmart").len(), 1);
}

#[test]
fn reasons_name_the_criterion_never_names_or_values() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let (s, _) = ok(&fake, &env, &input());
    let why = &s.smart_skipped[0].reason;
    assert!(why.contains("12") && why.contains("labelColor"), "{why}");
    assert!(!why.contains(SECRET) && !why.contains("red"), "{why}");
}

#[test]
fn only_untranslatable_collections_send_nothing() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let inp = WriteInput {
        smart: vec![smart(12, "x", None, bad())],
        ..WriteInput::default()
    };
    let (s, commands) = ok(&fake, &env, &inp);
    assert_eq!((s.smart_created, s.smart_skipped.len()), (0, 1));
    assert!(of(&commands, "album.createSmart").is_empty());
    assert!(fake.albums().is_empty());
}
