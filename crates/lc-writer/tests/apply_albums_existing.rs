//! Acceptance tests for recovering collections after the mapping file was
//! lost (principle 5), module `lc_writer::apply`, against the
//! stateful fake LightCraft with pre-existing albums (`Config::albums`).
//!
//! Rules under test:
//! - when the plan has at least one `album.create`, apply sends `albums.list {}`
//!   exactly once, before the first `album.create` (and not otherwise);
//! - for each collection without a mapping entry it looks for exactly one node
//!   with the same name, the same folder flag and the same parent (both on top
//!   level, or the parent node is the LightCraft folder known or just adopted
//!   for the Lightroom parent). The app trims names on create, so the
//!   TRIMMED Lightroom name is compared with the listed name, byte for byte
//!   (case matters);
//! - one hit: adopt it (`collections_existing`), no `album.create`; the mapping
//!   entry gets `lightcraft_id`, `name`, `parent`, and the members are then
//!   added as usual (`album.addPhotos` is idempotent) and recorded;
//! - several hits: create nothing, `collections_skipped` with a reason that
//!   contains `ambiguous: N albums with the same name and parent in
//!   LightCraft` and no collection name; the descendants of a skipped
//!   collection are skipped too (as after a rejected `album.create`);
//! - no hit: `album.create` as before;
//! - `albums.list` in an unexpected form: `WriteError::Protocol` before the
//!   first `album.create`;
//! - balance: `input.collections.len() == folders_created + albums_created +
//!   collections_known + collections_existing + collections_skipped.len()`.

use lc_writer::apply::{ApplySummary, apply};
use lc_writer::control::{ControlClient, WriteError};
use lc_writer::plan::WriteInput;
use lc_writer::state::{CollectionEntry, EntryKind, State};
use model::{Collection, CollectionKind, Photo, Rating};
use serde_json::{Value, json};
use std::path::PathBuf;

#[path = "support/fake_lightcraft.rs"]
mod fake_lightcraft;
use fake_lightcraft::{AlbumView, Config, FakeLightCraft, SeedAlbum};

const NAMES: [&str; 5] = ["Set A", "Set B", "Deep", "Top Album", "Empty"];
const AMBIGUOUS: &str = "ambiguous: 2 albums with the same name and parent in LightCraft";

fn collection(
    id: i64,
    name: &str,
    kind: CollectionKind,
    parent: Option<i64>,
    members: &[i64],
) -> Collection {
    Collection {
        lightroom_id: id,
        name: name.to_string(),
        kind,
        parent,
        members: members.to_vec(),
    }
}

/// Set A (1) > Set B (2) > Deep (3: 103, 101); Top Album (4: 102) on top;
/// Empty (5) below Set A.
fn tree() -> Vec<Collection> {
    use CollectionKind::{Album, Folder};
    vec![
        collection(1, "Set A", Folder, None, &[]),
        collection(2, "Set B", Folder, Some(1), &[]),
        collection(3, "Deep", Album, Some(2), &[103, 101]),
        collection(4, "Top Album", Album, None, &[102]),
        collection(5, "Empty", Album, Some(1), &[]),
    ]
}

fn input_with(collections: Vec<Collection>) -> WriteInput {
    let photos = (101..=104)
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
        collections,
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
    let summary = result.unwrap();
    assert_balance(input, &summary);
    (summary, commands)
}

fn assert_balance(input: &WriteInput, s: &ApplySummary) {
    assert_eq!(
        input.collections.len(),
        s.folders_created
            + s.albums_created
            + s.collections_known
            + s.collections_existing
            + s.collections_skipped.len(),
        "{s:?}"
    );
}

fn state(env: &Env) -> Value {
    serde_json::from_str(&std::fs::read_to_string(&env.state_file).unwrap()).unwrap()
}

fn of<'a>(commands: &'a Commands, name: &str) -> Vec<&'a Value> {
    commands
        .iter()
        .filter(|(c, _)| c == name)
        .map(|(_, p)| p)
        .collect()
}

fn position(commands: &Commands, name: &str) -> Option<usize> {
    commands.iter().position(|(c, _)| c == name)
}

fn album_commands(commands: &Commands) -> usize {
    commands
        .iter()
        .filter(|(c, _)| c.starts_with("album."))
        .count()
}

fn named(fake: &FakeLightCraft, name: &str) -> Vec<AlbumView> {
    fake.albums()
        .into_iter()
        .filter(|a| a.name == name)
        .collect()
}

fn assert_no_names(text: &str) {
    for name in NAMES {
        assert!(!text.contains(name), "{text}");
    }
}

/// The library of a complete first run, as seeds: ids 5001.. in this order.
fn seeds_of_tree() -> Vec<SeedAlbum> {
    vec![
        SeedAlbum::folder("Set A", None),    // 5001
        SeedAlbum::folder("Set B", Some(0)), // 5002
        SeedAlbum::album("Deep", Some(1)),   // 5003
        SeedAlbum::album("Top Album", None), // 5004
        SeedAlbum::album("Empty", Some(0)),  // 5005
    ]
}

fn with_seeds(env: &Env, seeds: Vec<SeedAlbum>) -> FakeLightCraft {
    let mut config = Config::new(&env.library);
    config.albums = seeds;
    FakeLightCraft::start(config)
}

#[test]
fn lost_state_file_adopts_every_collection_and_creates_nothing() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let input = input_with(tree());
    ok(&fake, &env, &input);
    let albums = fake.albums();
    let members: Vec<Vec<i64>> = albums.iter().map(|a| fake.album_members(a.id)).collect();
    std::fs::remove_file(&env.state_file).unwrap();

    let (s, commands) = ok(&fake, &env, &input);
    assert_eq!(s.collections_existing, 5);
    assert_eq!(
        (s.folders_created, s.albums_created, s.collections_known),
        (0, 0, 0)
    );
    assert!(s.collections_skipped.is_empty() && s.collections_incomplete.is_empty());
    assert!(of(&commands, "album.create").is_empty(), "{commands:?}");
    assert_eq!(fake.albums(), albums, "no album created, none changed");
    let after: Vec<Vec<i64>> = fake
        .albums()
        .iter()
        .map(|a| fake.album_members(a.id))
        .collect();
    assert_eq!(
        members, after,
        "members unchanged (addPhotos is idempotent)"
    );
    for word in ["album.delete", "album.rename", "album.move", "removePhotos"] {
        assert!(!fake.received().iter().any(|r| r.line.contains(word)));
    }

    // The mapping file is rebuilt, with the real ids, names and parents and
    // the members recorded after the (idempotent) adds.
    let st = state(&env);
    let c = &st["collections"];
    assert_eq!(c.as_object().unwrap().len(), 5);
    assert_eq!(c["1"]["lightcraft_id"], 5001);
    assert_eq!(c["1"]["kind"], "folder");
    assert_eq!(c["2"]["lightcraft_id"], 5002);
    assert_eq!(c["2"]["parent"], 1);
    assert_eq!(c["3"]["lightcraft_id"], 5005);
    assert_eq!(c["3"]["name"], "Deep");
    assert_eq!(c["3"]["parent"], 2);
    assert_eq!(c["3"]["members"], json!(["g101", "g103"]));
    assert_eq!(c["4"]["lightcraft_id"], 5003);
    assert_eq!(c["4"]["parent"], Value::Null);
    assert_eq!(c["5"]["parent"], 1);

    // And now it is stable: a normal run neither reads albums.list nor sends
    // an album command.
    let (s, again) = ok(&fake, &env, &input);
    assert_eq!(s.collections_known, 5);
    assert_eq!(s.collections_existing, 0);
    assert_eq!(album_commands(&again), 0);
    assert!(of(&again, "albums.list").is_empty());
    assert_eq!(fake.albums(), albums);
}

#[test]
fn albums_list_is_read_once_before_the_first_create() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let (_, commands) = ok(&fake, &env, &input_with(tree()));
    assert_eq!(of(&commands, "albums.list"), [&json!({})]);
    let list = position(&commands, "albums.list").unwrap();
    assert!(list < position(&commands, "album.create").unwrap());
}

#[test]
fn normal_second_run_does_not_read_albums_list() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let input = input_with(tree());
    ok(&fake, &env, &input);
    let (s, commands) = ok(&fake, &env, &input);
    assert_eq!(s.collections_known, 5);
    assert!(of(&commands, "albums.list").is_empty(), "{commands:?}");
}

#[test]
fn run_without_collections_does_not_read_albums_list() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let (_, commands) = ok(&fake, &env, &input_with(Vec::new()));
    assert!(of(&commands, "albums.list").is_empty(), "{commands:?}");
}

#[test]
fn albums_already_in_the_library_are_adopted_without_state() {
    let env = env();
    let fake = with_seeds(&env, seeds_of_tree());
    let input = input_with(tree());
    let (s, commands) = ok(&fake, &env, &input);
    assert_eq!(s.collections_existing, 5);
    assert!(of(&commands, "album.create").is_empty());
    assert_eq!(fake.albums().len(), 5);
    // The members are added to the adopted albums.
    assert_eq!(fake.album_members(5003), [1003, 1001]);
    assert_eq!(fake.album_members(5004), [1002]);
    assert_eq!(
        state(&env)["collections"]["3"]["members"],
        json!(["g101", "g103"])
    );
}

#[test]
fn partly_existing_tree_creates_only_the_missing_collections() {
    let env = env();
    // Only Set A and Top Album exist; the rest must be created below the
    // adopted folder.
    let fake = with_seeds(
        &env,
        vec![
            SeedAlbum::folder("Set A", None),
            SeedAlbum::album("Top Album", None),
        ],
    );
    let (s, commands) = ok(&fake, &env, &input_with(tree()));
    assert_eq!(s.collections_existing, 2);
    assert_eq!((s.folders_created, s.albums_created), (1, 2));
    let creates = of(&commands, "album.create");
    assert_eq!(creates.len(), 3);
    let set_b = &creates[0];
    assert_eq!(set_b["name"], "Set B");
    assert_eq!(set_b["parent"], 5001, "below the adopted folder");
    assert_eq!(fake.albums().len(), 5);
    let empty = &named(&fake, "Empty")[0];
    assert_eq!(empty.parent, Some(5001));
}

#[test]
fn two_equal_albums_in_the_same_folder_are_reported_not_guessed() {
    let env = env();
    let mut seeds = seeds_of_tree();
    seeds.push(SeedAlbum::album("Deep", Some(1))); // 5006, second "Deep"
    let fake = with_seeds(&env, seeds);
    let input = input_with(tree());
    let (s, commands) = ok(&fake, &env, &input);
    assert_eq!(s.collections_existing, 4);
    assert_eq!(
        s.collections_skipped.len(),
        1,
        "{:?}",
        s.collections_skipped
    );
    let reason = &s.collections_skipped[0].reason;
    assert!(reason.contains(AMBIGUOUS), "{reason}");
    assert_no_names(reason);
    assert!(of(&commands, "album.create").is_empty());
    assert_eq!(fake.albums().len(), 6);
    // Neither "Deep" got members; nothing is recorded for it.
    assert!(fake.album_members(5003).is_empty());
    assert!(fake.album_members(5006).is_empty());
    assert!(
        !state(&env)["collections"]
            .as_object()
            .unwrap()
            .contains_key("3")
    );
    // The others work as usual.
    assert_eq!(fake.album_members(5004), [1002]);

    // The next run reports it again and still creates nothing.
    let (s2, c2) = ok(&fake, &env, &input);
    assert_eq!(s2.collections_known, 4);
    assert_eq!(s2.collections_skipped.len(), 1);
    assert!(of(&c2, "album.create").is_empty());
    assert_eq!(fake.albums().len(), 6);
}

#[test]
fn three_equal_albums_say_how_many() {
    let env = env();
    let mut seeds = seeds_of_tree();
    seeds.push(SeedAlbum::album("Top Album", None));
    seeds.push(SeedAlbum::album("Top Album", None));
    let fake = with_seeds(&env, seeds);
    let (s, commands) = ok(&fake, &env, &input_with(tree()));
    assert_eq!(s.collections_skipped.len(), 1);
    assert!(
        s.collections_skipped[0]
            .reason
            .contains("ambiguous: 3 albums with the same name and parent in LightCraft"),
        "{:?}",
        s.collections_skipped
    );
    assert!(of(&commands, "album.create").is_empty());
}

#[test]
fn two_equal_folders_skip_the_collection_and_its_descendants() {
    let env = env();
    // Two top-level folders "Set A": Set A, Set B, Deep and Empty cannot be
    // placed; Top Album is unaffected and does not exist yet.
    let fake = with_seeds(
        &env,
        vec![
            SeedAlbum::folder("Set A", None),
            SeedAlbum::folder("Set A", None),
        ],
    );
    let (s, commands) = ok(&fake, &env, &input_with(tree()));
    assert_eq!(
        s.collections_skipped.len(),
        4,
        "{:?}",
        s.collections_skipped
    );
    assert!(s.collections_skipped[0].reason.contains(AMBIGUOUS));
    for skipped in &s.collections_skipped {
        assert_no_names(&skipped.reason);
    }
    assert_eq!((s.folders_created, s.albums_created), (0, 1));
    let creates = of(&commands, "album.create");
    assert_eq!(creates.len(), 1);
    assert_eq!(creates[0]["name"], "Top Album");
    assert_eq!(fake.albums().len(), 3);
    assert!(state(&env)["collections"].as_object().unwrap().len() == 1);
}

#[test]
fn equal_name_in_another_folder_is_no_hit() {
    let env = env();
    // "Deep" exists on top level and "Empty" below Set B, but the
    // Lightroom collections are below Set B and below Set A.
    let fake = with_seeds(
        &env,
        vec![
            SeedAlbum::folder("Set A", None),    // 5001
            SeedAlbum::folder("Set B", Some(0)), // 5002
            SeedAlbum::album("Deep", None),      // 5003 (wrong place)
            SeedAlbum::album("Empty", Some(1)),  // 5004 (wrong place)
        ],
    );
    let (s, commands) = ok(&fake, &env, &input_with(tree()));
    assert_eq!(s.collections_existing, 2);
    assert_eq!((s.folders_created, s.albums_created), (0, 3));
    assert!(s.collections_skipped.is_empty());
    let creates = of(&commands, "album.create");
    let deep = creates.iter().find(|p| p["name"] == "Deep").unwrap();
    assert_eq!(deep["parent"], 5002);
    let empty = creates.iter().find(|p| p["name"] == "Empty").unwrap();
    assert_eq!(empty["parent"], 5001);
    assert_eq!(named(&fake, "Deep").len(), 2);
    assert_eq!(named(&fake, "Empty").len(), 2);
    // The wrong-place albums stay untouched and empty.
    assert!(fake.album_members(5003).is_empty());
}

#[test]
fn equal_name_with_another_kind_is_no_hit() {
    let env = env();
    // A folder named like the album "Top Album" and an album named like the
    // folder "Set A".
    let fake = with_seeds(
        &env,
        vec![
            SeedAlbum::folder("Top Album", None),
            SeedAlbum::album("Set A", None),
        ],
    );
    let (s, commands) = ok(&fake, &env, &input_with(tree()));
    assert_eq!(s.collections_existing, 0);
    assert_eq!((s.folders_created, s.albums_created), (2, 3));
    assert_eq!(of(&commands, "album.create").len(), 5);
    assert!(s.collections_skipped.is_empty());
    assert_eq!(fake.albums().len(), 7);
    let tops = named(&fake, "Top Album");
    assert_eq!(tops.len(), 2);
    assert_eq!(tops.iter().filter(|a| a.folder).count(), 1);
}

#[test]
fn names_are_compared_byte_for_byte_but_trimmed_like_the_app_does() {
    use CollectionKind::Album;
    let env = env();
    let fake = with_seeds(
        &env,
        vec![
            SeedAlbum::album("padded", None),
            SeedAlbum::album("Case", None),
            SeedAlbum::album("Gru\u{308}n", None), // decomposed
        ],
    );
    let input = input_with(vec![
        collection(1, "  padded \t", Album, None, &[]), // trimmed by the app
        collection(2, "case", Album, None, &[]),        // other case: no hit
        collection(3, "Gr\u{fc}n", Album, None, &[]),   // composed: no hit
    ]);
    let (s, commands) = ok(&fake, &env, &input);
    assert_eq!(s.collections_existing, 1, "{s:?}");
    assert_eq!(s.albums_created, 2);
    let names: Vec<&Value> = of(&commands, "album.create")
        .into_iter()
        .map(|p| &p["name"])
        .collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&&json!("case")));
    assert_eq!(state(&env)["collections"]["1"]["lightcraft_id"], 5001);
}

#[test]
fn subtree_below_ambiguous_folders_is_never_adopted_by_accident() {
    // Below the second of two equal folders "Set A" sit a matching "Set B"
    // and "Deep". The folder is ambiguous, so its whole subtree is skipped
    // and nothing below either folder is adopted.
    let env = env();
    let fake = with_seeds(
        &env,
        vec![
            SeedAlbum::folder("Set A", None),
            SeedAlbum::folder("Set A", None),
            SeedAlbum::folder("Set B", Some(1)),
            SeedAlbum::album("Deep", Some(2)),
        ],
    );
    let (s, commands) = ok(&fake, &env, &input_with(tree()));
    assert!(
        of(&commands, "album.create")
            .iter()
            .all(|p| p["name"] == "Top Album")
    );
    assert_eq!(s.collections_existing, 0);
    assert_eq!(s.collections_skipped.len(), 4);
    assert!(fake.album_members(5004).is_empty());
}

#[test]
fn unexpected_albums_list_is_a_protocol_error_before_any_create() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.albums_list_garbage = true;
    let fake = FakeLightCraft::start(config);
    let (result, commands) = run(&fake, &env, &input_with(tree()));
    assert!(matches!(result, Err(WriteError::Protocol(_))), "{result:?}");
    assert!(of(&commands, "album.create").is_empty(), "{commands:?}");
    assert!(of(&commands, "album.addPhotos").is_empty());
    assert!(fake.albums().is_empty());
    // Nothing about collections was recorded.
    if env.state_file.exists() {
        assert!(
            state(&env)["collections"]
                .as_object()
                .is_none_or(|c| c.is_empty())
        );
    }
}

#[test]
fn garbage_albums_list_does_not_matter_when_nothing_is_created() {
    let env = env();
    let input = input_with(tree());
    let fake = FakeLightCraft::start(Config::new(&env.library));
    ok(&fake, &env, &input);
    // Same state, same library, but an app that answers albums.list badly:
    // it is not asked, so the run succeeds.
    let mut config = Config::new(&env.library);
    config.albums_list_garbage = true;
    let broken = FakeLightCraft::start(config);
    let (result, commands) = run(&broken, &env, &input);
    let s = result.unwrap();
    assert_eq!(s.collections_known, 5);
    assert!(of(&commands, "albums.list").is_empty());
}

#[test]
fn recovered_collections_are_not_duplicated_by_a_third_lost_state() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let input = input_with(tree());
    ok(&fake, &env, &input);
    let albums = fake.albums();
    for _ in 0..3 {
        std::fs::remove_file(&env.state_file).unwrap();
        let (s, _) = ok(&fake, &env, &input);
        assert_eq!(s.collections_existing, 5);
        assert_eq!(fake.albums(), albums);
    }
}

#[test]
fn a_lightcraft_side_rename_is_not_detected() {
    // The user renamed the album in LightCraft: the library has "Deep Renamed"
    // at id 5001, but the mapping file still records the name "Deep" and the
    // Lightroom collection is still "Deep". The writer trusts the recorded id:
    // it sends no `album.create`, does not read `albums.list`, and adds the
    // members to id 5001 by id. The LightCraft-side rename is NOT detected
    // (`collections_renamed` counts only Lightroom-side renames); reconciling
    // library-side renames is an open item of the safe re-sync.
    use CollectionKind::Album;
    let env = env();
    let fake = with_seeds(&env, vec![SeedAlbum::album("Deep Renamed", None)]); // id 5001
    {
        let mut state = State::load(&env.state_file).unwrap();
        state.record_collection(
            1,
            CollectionEntry {
                lightcraft_id: json!(5001),
                kind: EntryKind::Album,
                name: "Deep".to_string(),
                parent: None,
                members: Vec::new(),
                rules: None,
            },
        );
        state.save(&env.state_file).unwrap();
    }
    let input = input_with(vec![collection(1, "Deep", Album, None, &[101])]);
    let (s, commands) = ok(&fake, &env, &input);
    assert_eq!(s.collections_known, 1);
    assert_eq!(
        (
            s.collections_renamed,
            s.collections_existing,
            s.albums_created
        ),
        (0, 0, 0)
    );
    assert!(of(&commands, "album.create").is_empty(), "{commands:?}");
    assert!(of(&commands, "albums.list").is_empty(), "{commands:?}");
    // The members went to id 5001, which the app still calls "Deep Renamed".
    assert_eq!(fake.album_members(5001), [1001]);
    let album = fake.albums().iter().find(|a| a.id == 5001).unwrap();
    assert_eq!(album.name, "Deep Renamed");
    assert_eq!(state(&env)["collections"]["1"]["members"], json!(["g101"]));
}
