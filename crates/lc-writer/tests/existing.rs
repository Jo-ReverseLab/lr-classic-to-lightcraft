//! Photos LightCraft already has although the mapping file does not know
//! them (imported by hand or by LightCraft's own Lightroom import).
//!
//! Rules:
//! - `find_existing` asks with `library.importPreview` only about the files a
//!   run would import and reads the found photos back; it never writes;
//! - a found photo is adopted (`photos_existing`), never imported, and gets
//!   what its choice allows: `overwrite` writes every differing field and the
//!   development settings; `develop` fills empty fields (a field set on both
//!   sides is a conflict) and writes the development settings;
//!   `develop-if-empty` does the same but keeps the development settings of
//!   a photo LightCraft shows as edited; `skip` writes no field, keyword or
//!   development setting, only memberships;
//! - the mapping file records the reconciled values, and a development hash
//!   only when the settings were written, so a second run sends nothing;
//! - a duplicate found only while importing is read back and gets the
//!   default choice instead of the steps planned for a fresh import;
//! - Cancel sets the fields of an adopted photo back to its own values.

use lc_writer::apply::{ApplySummary, apply};
use lc_writer::control::ControlClient;
use lc_writer::existing::{
    ExistingChoice, ExistingDecisions, ExistingPhoto, ExistingReason, find_existing,
};
use lc_writer::plan::{Areas, WriteInput};
use lc_writer::progress::NoProgress;
use lc_writer::state::{PhotoFields, State};
use model::{Collection, CollectionKind, ColorLabel, Develop, Keyword, Photo, Rating};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

use fake_lightcraft::{Config, FakeLightCraft, READ_COMMANDS, SeedPhoto};
use lc_test_support::fake_lightcraft;

fn seed_path(id: i64) -> String {
    format!("/Users/example/Pictures/seed-{id}.jpg")
}

fn kw(name: &str) -> Keyword {
    Keyword::new(vec![name.to_string()]).unwrap()
}

fn develop(byte: u8) -> Develop {
    let mut d = Develop::default();
    d.controls.insert("exposure".into(), 0.5);
    d.source_hash = Some([byte; 32]);
    d
}

/// Seed 7: edited in LightCraft, with stars, a title and a keyword. Seed 8:
/// as imported, not edited.
fn config(library: &std::path::Path) -> Config {
    let mut config = Config::new(library);
    config.seed_photos = vec![
        SeedPhoto {
            rating: 2,
            title: "Title in LightCraft".into(),
            keywords: vec!["Old".into()],
            edited: true,
            ..SeedPhoto::plain(7)
        },
        SeedPhoto::plain(8),
    ];
    config
}

/// g7 and g8 are the seeded files; g9 is new. Every photo carries
/// Lightroom values and development settings.
fn input() -> WriteInput {
    let photo = |gid: &str, lr: i64, path: String| {
        Photo::new(path, lr, Rating::new(5).unwrap())
            .with_global_id(gid)
            .with_label(ColorLabel::Red)
            .with_title("Title in Lightroom")
            .with_caption("Caption in Lightroom")
            .with_develop(develop(lr as u8))
    };
    let mut photo_keywords = BTreeMap::new();
    for lr in [7, 8, 9] {
        photo_keywords.insert(lr, vec![kw("New")]);
    }
    WriteInput {
        photos: vec![
            photo("g7", 7, seed_path(7)),
            photo("g8", 8, seed_path(8)),
            photo("g9", 9, "/Users/example/Pictures/new.jpg".into()),
        ],
        keywords: vec![kw("New")],
        photo_keywords,
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
    decisions: ExistingDecisions,
) -> ApplySummary {
    let mut state = State::load(&env.state_file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let mut input = input.clone();
    input.existing = find_existing(&mut client, &input, &state, &mut NoProgress).unwrap();
    input.existing_decisions = decisions;
    apply(&mut client, &input, &mut state, &env.state_file).unwrap()
}

fn all(choice: ExistingChoice) -> ExistingDecisions {
    ExistingDecisions::all(choice)
}

/// The commands sent from `from` on that name LightCraft photo `id`.
fn writes_to(fake: &FakeLightCraft, from: usize, id: i64) -> Vec<(String, Value)> {
    fake.commands()[from..]
        .iter()
        .filter(|(c, _)| !READ_COMMANDS.contains(&c.as_str()))
        .filter(|(_, p)| {
            p["ids"]
                .as_array()
                .is_some_and(|ids| ids.contains(&json!(id)))
        })
        .cloned()
        .collect()
}

fn writes(fake: &FakeLightCraft, from: usize) -> Vec<String> {
    fake.commands()[from..]
        .iter()
        .map(|(c, _)| c.clone())
        .filter(|c| !READ_COMMANDS.contains(&c.as_str()))
        .collect()
}

fn develop_commands(fake: &FakeLightCraft, from: usize) -> Vec<(String, Value)> {
    fake.commands()[from..]
        .iter()
        .filter(|(c, _)| c.starts_with("develop."))
        .cloned()
        .collect()
}

fn entry(env: &Env, gid: &str) -> lc_writer::state::PhotoEntry {
    State::load(&env.state_file)
        .unwrap()
        .photo(gid)
        .unwrap()
        .clone()
}

fn lightroom_fields() -> PhotoFields {
    PhotoFields {
        rating: 5,
        label: Some("red".into()),
        title: Some("Title in Lightroom".into()),
        caption: Some("Caption in Lightroom".into()),
        keywords: vec!["New".into()],
        ..PhotoFields::imported_default()
    }
}

/// A second run checks again, finds nothing new and writes nothing.
fn assert_second_run_is_quiet(fake: &FakeLightCraft, env: &Env, decisions: ExistingDecisions) {
    let mark = fake.commands().len();
    let second = run(fake, env, &input(), decisions);
    assert_eq!(writes(fake, mark), Vec::<String>::new());
    assert_eq!(second.photos_existing, 0);
    assert_eq!(second.existing.found, 0);
    assert_eq!(second.photos_unchanged + second.photos_updated, 3);
}

#[test]
fn the_check_lists_only_photos_the_library_has_and_writes_nothing() {
    let env = env();
    let mut config = config(&env.library);
    config.seed_photos[0].caption = Some("Caption in LightCraft".into());
    // g10's file has the same bytes as seed 8: also found, by content.
    let copy = "/Users/example/Pictures/elsewhere/copy.jpg";
    config.file_contents.insert(seed_path(8), "bytes".into());
    config.file_contents.insert(copy.into(), "bytes".into());
    let fake = FakeLightCraft::start(config);
    let mut input = input();
    input
        .photos
        .push(Photo::new(copy, 10, Rating::default()).with_global_id("g10"));
    // A virtual copy of g7 is made from its master, never imported.
    input.photos.push(
        Photo::new(seed_path(7), 11, Rating::default())
            .with_global_id("g11")
            .with_master(7),
    );

    let mut client = ControlClient::connect(fake.port()).unwrap();
    let found = find_existing(&mut client, &input, &State::default(), &mut NoProgress).unwrap();
    assert!(fake.write_commands().is_empty(), "{:?}", fake.commands());
    let previews: Vec<Value> = fake
        .commands()
        .into_iter()
        .filter(|(c, _)| c == "library.importPreview")
        .map(|(_, p)| p)
        .collect();
    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0]["paths"].as_array().unwrap().len(), 4);

    let summary: Vec<(&str, ExistingReason, Value)> = found
        .iter()
        .map(|e| (e.global_id.as_str(), e.reason, e.lightcraft_id.clone()))
        .collect();
    // g10 names the same photo as g8 by content; g8 has its path and wins.
    assert_eq!(
        summary,
        vec![
            ("g7", ExistingReason::Path, json!(7)),
            ("g8", ExistingReason::Path, json!(8)),
        ]
    );
    let g7: &ExistingPhoto = &found[0];
    assert_eq!(g7.lightroom_id, 7);
    assert_eq!(g7.file_name, "seed-7.jpg");
    assert_eq!(g7.live.rating, 2);
    assert_eq!(g7.live.title.as_deref(), Some("Title in LightCraft"));
    assert_eq!(g7.live.caption.as_deref(), Some("Caption in LightCraft"));
    assert_eq!(g7.live.keywords, ["Old"]);
    assert!(g7.live.edited);
    assert!(!found[1].live.edited);
    // The list can travel to a front end and back.
    let back: Vec<ExistingPhoto> =
        serde_json::from_value(serde_json::to_value(&found).unwrap()).unwrap();
    assert_eq!(back, found);
}

#[test]
fn overwrite_writes_the_lightroom_values_and_the_development() {
    let env = env();
    let fake = FakeLightCraft::start(config(&env.library));
    let s = run(&fake, &env, &input(), all(ExistingChoice::Overwrite));
    assert_eq!((s.photos_existing, s.photos_imported), (2, 1));
    assert_eq!((s.existing.found, s.existing.overwrite), (2, 2));
    assert!(s.conflicts.is_empty(), "{:?}", s.conflicts);
    assert_eq!(fake.photos().len(), 3, "nothing imported twice");
    assert!(
        !fake
            .commands()
            .iter()
            .any(|(c, p)| c == "library.import" && p["paths"][0] == json!(seed_path(7)))
    );

    let seven = fake.photo_fields(7).unwrap();
    assert_eq!(seven.rating, 5);
    assert_eq!(seven.title, "Title in Lightroom");
    assert_eq!(seven.label.as_deref(), Some("red"));
    assert_eq!(seven.caption.as_deref(), Some("Caption in Lightroom"));
    // Keywords are only added.
    assert_eq!(seven.keywords, ["Old", "New"]);
    assert_eq!(develop_commands(&fake, 0).len(), 3, "g7, g8 and g9");
    assert_eq!(s.develop_written, 3);

    let e = entry(&env, "g7");
    assert_eq!(e.lightcraft_id, json!(7));
    assert_eq!(e.fields, lightroom_fields());
    assert!(e.develop_hash.is_some());
    assert_eq!(e.path.as_deref(), Some(seed_path(7).as_str()));
    // One log entry per adopted photo, naming the choice.
    let adopted: Vec<_> = s
        .entries
        .iter()
        .filter(|e| e.code == Some("existing_adopted"))
        .collect();
    assert_eq!(adopted.len(), 2);
    assert!(adopted[0].reason.as_deref().unwrap().contains("overwrite"));

    assert_second_run_is_quiet(&fake, &env, all(ExistingChoice::Overwrite));
}

#[test]
fn overwrite_leaves_an_area_that_is_not_selected_as_it_is() {
    let env = env();
    let fake = FakeLightCraft::start(config(&env.library));
    let mut input = input();
    input.areas = Areas::skipping("ratings").unwrap();
    run(&fake, &env, &input, all(ExistingChoice::Overwrite));
    assert_eq!(fake.photo_fields(7).unwrap().rating, 2);
    // The base is a fresh import's: selecting ratings later fills only an
    // empty rating and reports this one as a conflict.
    assert_eq!(entry(&env, "g7").fields.rating, 0);
}

#[test]
fn develop_fills_empty_fields_reports_conflicts_and_writes_the_development() {
    let env = env();
    let fake = FakeLightCraft::start(config(&env.library));
    let s = run(&fake, &env, &input(), all(ExistingChoice::Develop));
    assert_eq!(s.existing.develop, 2);
    let conflicts: Vec<(i64, &str)> = s
        .conflicts
        .iter()
        .map(|c| (c.photo, c.field.as_str()))
        .collect();
    assert_eq!(conflicts, vec![(7, "rating"), (7, "title")]);

    let seven = fake.photo_fields(7).unwrap();
    assert_eq!(seven.rating, 2, "kept");
    assert_eq!(seven.title, "Title in LightCraft", "kept");
    assert_eq!(seven.label.as_deref(), Some("red"), "was empty");
    assert_eq!(seven.caption.as_deref(), Some("Caption in Lightroom"));
    assert_eq!(seven.keywords, ["Old", "New"]);
    assert_eq!(fake.photo_fields(8).unwrap().rating, 5);
    assert_eq!(
        develop_commands(&fake, 0)
            .iter()
            .filter(|(_, p)| p["ids"] == json!([7]))
            .count(),
        1,
        "edited, but the choice writes it"
    );

    // Conflicting fields keep a fresh import's value as their base.
    let e = entry(&env, "g7");
    assert_eq!(
        e.fields,
        PhotoFields {
            rating: 0,
            title: None,
            ..lightroom_fields()
        }
    );
    assert!(e.develop_hash.is_some());
    assert_eq!(entry(&env, "g8").fields, lightroom_fields());

    let mark = fake.commands().len();
    let second = run(&fake, &env, &input(), all(ExistingChoice::Develop));
    assert_eq!(writes(&fake, mark), Vec::<String>::new());
    assert_eq!(second.conflicts, s.conflicts, "reported again");
}

#[test]
fn develop_if_empty_keeps_the_development_of_an_edited_photo() {
    let env = env();
    let fake = FakeLightCraft::start(config(&env.library));
    let s = run(&fake, &env, &input(), ExistingDecisions::default());
    assert_eq!(s.existing.develop_if_empty, 2);
    assert_eq!(s.existing.develop_kept, 1);
    assert_eq!(s.existing_develop_kept_ids, ["g7"]);
    assert_eq!(s.develop_not_applied, 1);
    assert_eq!(s.develop_not_applied_ids, ["g7"]);
    let ids: Vec<Value> = develop_commands(&fake, 0)
        .iter()
        .map(|(_, p)| p["ids"][0].clone())
        .collect();
    assert_eq!(ids, [json!(8), json!(1001)], "not g7");
    // Fields as with `develop`.
    assert_eq!(fake.photo_fields(7).unwrap().rating, 2);
    assert_eq!(fake.photo_fields(7).unwrap().label.as_deref(), Some("red"));

    assert!(entry(&env, "g7").develop_hash.is_none(), "not written");
    assert!(entry(&env, "g8").develop_hash.is_some());

    let mark = fake.commands().len();
    let second = run(&fake, &env, &input(), ExistingDecisions::default());
    assert_eq!(writes(&fake, mark), Vec::<String>::new());
    // Afterwards the photo is reconciled like any known photo: LightCraft's
    // edit is kept and reported.
    assert_eq!(second.develop_not_applied_ids, ["g7"]);
}

#[test]
fn skip_writes_nothing_to_the_photo_but_adds_its_memberships() {
    let env = env();
    let fake = FakeLightCraft::start(config(&env.library));
    let mut input = input();
    input.collections = vec![Collection {
        lightroom_id: 50,
        name: "Album".into(),
        kind: CollectionKind::Album,
        parent: None,
        members: vec![7],
    }];
    let mut decisions = ExistingDecisions::default();
    decisions
        .per_photo
        .insert("g7".into(), ExistingChoice::Skip);
    let s = run(&fake, &env, &input, decisions);
    assert_eq!((s.existing.skip, s.existing.develop_if_empty), (1, 1));
    assert!(s.conflicts.is_empty(), "{:?}", s.conflicts);

    let to_seven: Vec<String> = writes_to(&fake, 0, 7).into_iter().map(|(c, _)| c).collect();
    assert_eq!(to_seven, ["album.addPhotos"]);
    let seven = fake.photo_fields(7).unwrap();
    assert_eq!(
        (seven.rating, seven.keywords.clone()),
        (2, vec!["Old".into()])
    );
    assert_eq!(fake.album_members(5001), [7]);

    let e = entry(&env, "g7");
    assert_eq!(e.fields, PhotoFields::imported_default());
    assert!(e.develop_hash.is_none());
    let log = s
        .entries
        .iter()
        .find(|e| e.code == Some("existing_adopted") && e.message.contains("left as it is"))
        .expect("an entry for the skipped photo");
    assert_eq!(log.status, model::Status::Skipped);
}

#[test]
fn a_duplicate_found_only_while_importing_gets_the_default_choice() {
    let env = env();
    let fake = FakeLightCraft::start(config(&env.library));
    // No check before writing: `library.import` reports the duplicates.
    let mut state = State::default();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let s = apply(&mut client, &input(), &mut state, &env.state_file).unwrap();
    assert_eq!((s.photos_existing, s.photos_imported), (2, 1));
    assert_eq!(s.existing.found_while_importing, 2);
    assert_eq!(s.existing.develop_if_empty, 2);
    assert_eq!(s.existing_develop_kept_ids, ["g7"]);

    // Nothing planned for a fresh import is written over LightCraft's values.
    let seven = fake.photo_fields(7).unwrap();
    assert_eq!(seven.rating, 2);
    assert_eq!(seven.title, "Title in LightCraft");
    assert_eq!(seven.label.as_deref(), Some("red"));
    assert!(
        develop_commands(&fake, 0)
            .iter()
            .all(|(_, p)| p["ids"] != json!([7]))
    );
    assert_eq!(fake.photo_fields(8).unwrap().rating, 5, "empty, filled");
    let conflicts: Vec<&str> = s.conflicts.iter().map(|c| c.field.as_str()).collect();
    assert_eq!(conflicts, ["rating", "title"]);
    let late = s
        .entries
        .iter()
        .filter(|e| e.code == Some("existing_adopted"))
        .filter(|e| e.reason.as_deref().unwrap().contains("while importing"))
        .count();
    assert_eq!(late, 2);
    assert_eq!(
        entry(&env, "g7").fields,
        PhotoFields {
            rating: 0,
            title: None,
            ..lightroom_fields()
        }
    );
}

#[test]
fn a_photo_in_the_trash_is_restored_by_the_import_and_handled_with_the_default() {
    let env = env();
    let mut config = config(&env.library);
    config.seed_photos[0].deleted = true;
    let fake = FakeLightCraft::start(config);
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let found = find_existing(&mut client, &input(), &State::default(), &mut NoProgress).unwrap();
    let gids: Vec<&str> = found.iter().map(|e| e.global_id.as_str()).collect();
    assert_eq!(gids, ["g8"]);
    drop(client);

    let s = run(&fake, &env, &input(), all(ExistingChoice::Overwrite));
    assert_eq!(s.photos_existing, 2);
    assert_eq!(s.existing.found_while_importing, 1);
    assert_eq!(s.existing.overwrite, 2, "the default is the choice for all");
    assert!(!fake.photo_fields(7).unwrap().deleted, "restored");
    assert_eq!(entry(&env, "g7").lightcraft_id, json!(7));
}

#[test]
fn cancel_sets_an_adopted_photo_back_to_its_own_values() {
    let env = env();
    let fake = FakeLightCraft::start(config(&env.library));
    let before = fake.photo_fields(7).unwrap();
    let s = run(&fake, &env, &input(), all(ExistingChoice::Overwrite));
    assert_eq!(fake.photo_fields(7).unwrap().rating, 5);

    let mut client = ControlClient::connect(fake.port()).unwrap();
    let r = lc_writer::rollback::rollback(&mut client, &s.journal, &env.state_file).unwrap();
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    let after = fake.photo_fields(7).unwrap();
    assert_eq!(after.rating, before.rating);
    assert_eq!(after.title, before.title);
    assert_eq!(after.label, before.label);
    assert_eq!(after.caption, before.caption);
    assert_eq!(after.keywords, before.keywords);
    // The adopted photos stay; only the new one is removed.
    assert_eq!(r.photos_removed, 1);
    assert!(fake.photos().values().any(|id| *id == 7));
    // Development settings cannot be set back; they are counted.
    assert_eq!(r.develop_not_reset, 2);
    assert!(State::load(&env.state_file).unwrap().photo("g7").is_none());
}
