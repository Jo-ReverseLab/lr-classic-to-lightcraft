//! Photos lr2lc wrote that were deleted in LightCraft afterwards.
//!
//! Rules:
//! - the check before writing reads the ids LightCraft lists inside and
//!   outside its trash (`catalog.query`, ids only) and reports every mapped
//!   photo of the catalog that is missing or only in the trash;
//! - `reimport` (default): a photo deleted for good is forgotten and
//!   imported again as a new photo, with its album memberships; a photo in
//!   the trash is restored by importing its file (same record, mapping
//!   entry kept); a second run is quiet;
//! - `leave`: the mapping entry stays, nothing is sent for the photo (no
//!   field, develop or membership command), and it is skipped with
//!   `removed_in_lightcraft`.

use lc_writer::apply::{ApplySummary, apply};
use lc_writer::control::ControlClient;
use lc_writer::existing::{RemovedChoice, RemovedDecisions, check_library};
use lc_writer::plan::WriteInput;
use lc_writer::progress::NoProgress;
use lc_writer::state::State;
use model::{Collection, CollectionKind, Photo, Rating, SkipCode};
use serde_json::{Value, json};
use std::path::PathBuf;

use fake_lightcraft::{Config, FakeLightCraft, READ_COMMANDS};
use lc_test_support::fake_lightcraft;

fn path(gid: &str) -> String {
    format!("/Users/example/Pictures/{gid}.jpg")
}

/// Two photos, both in one album.
fn input() -> WriteInput {
    let photo =
        |gid: &str, lr: i64| Photo::new(path(gid), lr, Rating::new(3).unwrap()).with_global_id(gid);
    WriteInput {
        photos: vec![photo("g1", 1), photo("g2", 2)],
        collections: vec![Collection {
            lightroom_id: 50,
            name: "Album".into(),
            kind: CollectionKind::Album,
            parent: None,
            members: vec![1, 2],
        }],
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

/// Checks, then writes with `decisions`, like a front end does.
fn run(
    fake: &FakeLightCraft,
    env: &Env,
    input: &WriteInput,
    decisions: RemovedDecisions,
) -> ApplySummary {
    let mut state = State::load(&env.state_file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let mut input = input.clone();
    let check = check_library(&mut client, &input, &state, &mut NoProgress).unwrap();
    input.existing = check.existing;
    input.removed = check.removed;
    input.removed_decisions = decisions;
    apply(&mut client, &input, &mut state, &env.state_file).unwrap()
}

fn writes(fake: &FakeLightCraft, from: usize) -> Vec<(String, Value)> {
    fake.commands()[from..]
        .iter()
        .filter(|(c, _)| !READ_COMMANDS.contains(&c.as_str()))
        .cloned()
        .collect()
}

fn id_of(env: &Env, gid: &str) -> Value {
    State::load(&env.state_file)
        .unwrap()
        .photo(gid)
        .unwrap()
        .lightcraft_id
        .clone()
}

/// A first run, then `change` to the library behind lr2lc's back.
fn written(change: impl FnOnce(&FakeLightCraft, i64)) -> (Env, FakeLightCraft) {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    run(&fake, &env, &input(), RemovedDecisions::default());
    let g1 = id_of(&env, "g1").as_i64().unwrap();
    change(&fake, g1);
    (env, fake)
}

#[test]
fn the_check_lists_photos_deleted_for_good_and_in_the_trash() {
    let (env, fake) = written(|fake, id| fake.delete_permanently(id));
    let g2 = id_of(&env, "g2").as_i64().unwrap();
    fake.trash(g2);
    let mark = fake.commands().len();
    let state = State::load(&env.state_file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let check = check_library(&mut client, &input(), &state, &mut NoProgress).unwrap();
    assert!(writes(&fake, mark).is_empty());
    let found: Vec<(&str, bool)> = check
        .removed
        .iter()
        .map(|r| (r.global_id.as_str(), r.trashed))
        .collect();
    assert_eq!(found, [("g1", false), ("g2", true)]);
    assert_eq!(check.removed[1].lightcraft_id, json!(g2));
    assert_eq!(check.removed[0].file_name, "g1.jpg");
    assert!(check.existing.is_empty());
}

#[test]
fn a_photo_deleted_for_good_is_imported_again_with_its_album() {
    let (env, fake) = written(|fake, id| fake.delete_permanently(id));
    let old = id_of(&env, "g1");
    let s = run(&fake, &env, &input(), RemovedDecisions::default());
    let r = s.removed_in_lightcraft;
    assert_eq!((r.found, r.reimported, r.restored, r.left), (1, 1, 0, 0));
    assert_eq!((s.photos_imported, s.photos_unchanged), (1, 1));
    let new = id_of(&env, "g1");
    assert_ne!(new, old);
    assert_eq!(fake.photo_fields(new.as_i64().unwrap()).unwrap().rating, 3);
    let album = fake.albums()[0].id;
    assert!(fake.album_members(album).contains(&new.as_i64().unwrap()));
    assert!(
        s.entries.iter().any(
            |e| e.code == Some("removed_in_lightcraft") && e.message.contains("imported again")
        )
    );

    let mark = fake.commands().len();
    let second = run(&fake, &env, &input(), RemovedDecisions::default());
    assert!(writes(&fake, mark).is_empty(), "{:?}", writes(&fake, mark));
    assert_eq!(second.removed_in_lightcraft.found, 0);
    assert_eq!(second.photos_unchanged, 2);
}

#[test]
fn a_photo_in_the_trash_is_restored_as_the_same_record() {
    let (env, fake) = written(|fake, id| fake.trash(id));
    let id = id_of(&env, "g1");
    let s = run(&fake, &env, &input(), RemovedDecisions::default());
    let r = s.removed_in_lightcraft;
    assert_eq!((r.found, r.reimported, r.restored), (1, 1, 1));
    assert!(s.photos_incomplete.is_empty(), "{:?}", s.photos_incomplete);
    assert!(!fake.is_trashed(id.as_i64().unwrap()));
    assert_eq!(id_of(&env, "g1"), id, "same record");
    assert_eq!(fake.photos().len(), 2, "no new record");
    assert_eq!(s.journal.restored, vec![id.clone()]);

    let mark = fake.commands().len();
    run(&fake, &env, &input(), RemovedDecisions::default());
    assert!(writes(&fake, mark).is_empty(), "{:?}", writes(&fake, mark));

    // Cancel of the restoring run moves the photo back to the trash.
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let back = lc_writer::rollback::rollback(&mut client, &s.journal, &env.state_file).unwrap();
    assert_eq!(back.photos_trashed_again, 1);
}

#[test]
fn leave_writes_nothing_for_the_photo_and_reports_it() {
    let (env, fake) = written(|fake, id| fake.delete_permanently(id));
    let old = id_of(&env, "g1");
    let mut changed = input();
    changed.photos[0] = Photo::new(path("g1"), 1, Rating::new(5).unwrap()).with_global_id("g1");
    // A new album with both photos: only g2 is added.
    changed.collections.push(Collection {
        lightroom_id: 51,
        name: "New".into(),
        kind: CollectionKind::Album,
        parent: None,
        members: vec![1, 2],
    });
    let mark = fake.commands().len();
    let s = run(
        &fake,
        &env,
        &changed,
        RemovedDecisions::all(RemovedChoice::Leave),
    );
    let r = s.removed_in_lightcraft;
    assert_eq!((r.found, r.reimported, r.left), (1, 0, 1));
    let [skipped] = &s.photos_skipped[..] else {
        panic!("{:?}", s.photos_skipped)
    };
    assert_eq!(skipped.code, SkipCode::RemovedInLightcraft);
    assert_eq!(
        skipped.reason,
        "photo 1: deleted in LightCraft; left as it is"
    );
    assert_eq!(s.photos_imported, 0);
    let added: Vec<Value> = writes(&fake, mark)
        .into_iter()
        .filter(|(c, _)| c == "album.addPhotos")
        .map(|(_, p)| p["ids"].clone())
        .collect();
    assert_eq!(added, [json!([id_of(&env, "g2")])]);
    assert!(
        writes(&fake, mark)
            .iter()
            .all(|(c, _)| c != "photo.rate" && c != "library.import")
    );
    assert_eq!(id_of(&env, "g1"), old, "the entry stays");
}
