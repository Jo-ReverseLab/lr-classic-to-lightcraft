//! Acceptance tests for reading the current LightCraft state,
//! module `lc_writer::live`, against the stateful fake LightCraft. Nothing is
//! written and nothing is decided here.
//!
//! API under test:
//! - `read_live(client: &mut ControlClient, state: &State) -> Result<LiveState, WriteError>`;
//! - `LiveState` derives Debug, Clone, PartialEq and has public fields
//!   - `photos: BTreeMap<String, LivePhoto>`, keyed by global id; every photo
//!     of the mapping file that LightCraft lists in either query, including
//!     photos in the trash (Recently Deleted);
//!   - `missing_photos: BTreeSet<String>`: global ids that the mapping file
//!     has but neither query lists (removed for good). A photo LightCraft
//!     deleted but still lists under `deleted: true` is present, never
//!     missing. Not an error;
//!   - `foreign_photos: usize`: photos either query lists that the mapping
//!     file does not know; a photo listed by both queries counts once.
//!     DECISION: they are counted, never stored. They are the user's own
//!     photos; keeping their values would be data we have no use for, and the
//!     count lets the report show the library is bigger;
//!   - `albums: BTreeMap<i64, LiveAlbum>`, keyed by Lightroom collection id,
//!     for every mapped collection that `albums.list` still shows;
//!   - `missing_albums: BTreeSet<i64>`: mapped collections not in the list;
//!   - `keywords: BTreeSet<String>`: all keyword path texts (`a|b|c`) of the
//!     library, from `keyword.list`;
//! - `LivePhoto { rating: u8, flag: String, label: Option<String>,
//!   title: Option<String>, keywords: Vec<String> (sorted), caption,
//!   copyright, creator: Option<String> }`; an empty text becomes `None`
//!   (the mapping file never stores `""`), flag spelling as `PhotoFields`;
//! - `LiveAlbum { name: String, members: BTreeSet<String> }`: the members
//!   as global ids; members the mapping file does not know are dropped
//!   (they are foreign photos).
//!
//! Requests, in total for a library of N listed non-deleted photos, D listed
//! deleted photos, M mapped photos present (deleted ones included),
//! A mapped collections present, page size P <= 200:
//! max(1, ceil(N/P)) x `catalog.query {filter: {}, offset, limit}` and
//! max(1, ceil(D/P)) x `catalog.query {filter: {deleted: true}, offset,
//! limit}` (all pages of each stream, in order; the ids of the two streams are
//! merged, each id once), M x `photo.inspect` (caption, copyright and creator
//! are not in the summary, so every present mapped photo, deleted or not, is
//! inspected once; foreign and missing ones are not), 1 x `albums.list`,
//! 1 x `keyword.list`, and per present album `ceil(members/P)` (at least 1) x
//! `catalog.query {filter: {album}}`.
//! Only these four commands are sent: never a setter, `album.create`,
//! `album.addPhotos`, `library.import` or `library.select`.
//!
//! Errors: an unusable reply is `WriteError::Protocol`, whose text (Display
//! and Debug) never contains a value from the reply (titles, names, paths).

use lc_writer::control::{ControlClient, WriteError};
use lc_writer::live::{LiveState, read_live};
use lc_writer::state::{CollectionEntry, EntryKind, PhotoFields, State};
use serde_json::{Value, json};
use std::collections::BTreeSet;

use fake_lightcraft::{Config, FakeLightCraft, SeedAlbum, SeedPhoto};
use lc_test_support::fake_lightcraft;

const READS: [&str; 4] = [
    "catalog.query",
    "photo.inspect",
    "albums.list",
    "keyword.list",
];

fn gid(id: i64) -> String {
    format!("gid-{id}")
}

/// A mapping file with photos `ids` (Lightroom id = LightCraft id).
fn state_with(ids: &[i64]) -> State {
    let mut state = State::default();
    for id in ids {
        state.record(gid(*id), *id, json!(id), PhotoFields::imported_default());
    }
    state
}

fn start(config: impl FnOnce(&mut Config)) -> FakeLightCraft {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Config::new(dir.path());
    config(&mut c);
    FakeLightCraft::start(c)
}

fn seeds(ids: std::ops::RangeInclusive<i64>) -> Vec<SeedPhoto> {
    ids.map(SeedPhoto::plain).collect()
}

fn read(fake: &FakeLightCraft, state: &State) -> Result<LiveState, WriteError> {
    let mut client = ControlClient::connect(fake.port()).unwrap();
    read_live(&mut client, state)
}

fn count(fake: &FakeLightCraft, command: &str) -> usize {
    fake.commands().iter().filter(|(c, _)| c == command).count()
}

fn album_entry(id: u64, name: &str, kind: EntryKind, members: &[i64]) -> CollectionEntry {
    CollectionEntry {
        lightcraft_id: json!(id),
        kind,
        name: name.to_string(),
        parent: None,
        members: members.iter().map(|m| gid(*m)).collect(),
        rules: None,
    }
}

#[test]
fn only_read_commands_are_sent() {
    let fake = start(|c| {
        c.seed_photos = seeds(1..=3);
        c.albums = vec![SeedAlbum::album("Set", None)];
        c.seed_members = vec![(0, 1)];
    });
    let mut state = state_with(&[1, 2, 3]);
    state.record_collection(10, album_entry(5001, "Set", EntryKind::Album, &[1]));
    let before = (
        fake.photos(),
        fake.albums(),
        fake.keywords(),
        fake.ratings(),
    );
    read(&fake, &state).unwrap();
    let sent = fake.commands();
    assert!(!sent.is_empty());
    for (command, _) in &sent {
        assert!(READS.contains(&command.as_str()), "sent {command}");
    }
    assert!(fake.write_commands().is_empty());
    assert!(fake.selection().is_empty());
    assert_eq!(
        before,
        (
            fake.photos(),
            fake.albums(),
            fake.keywords(),
            fake.ratings()
        )
    );
}

#[test]
fn paging_reads_every_page_of_both_queries_in_order() {
    let fake = start(|c| c.seed_photos = seeds(1..=450));
    let ids: Vec<i64> = (1..=450).collect();
    let live = read(&fake, &state_with(&ids)).unwrap();
    assert_eq!(live.photos.len(), 450);
    assert_eq!(live.foreign_photos, 0);
    assert!(live.missing_photos.is_empty());
    let pages: Vec<Value> = fake
        .commands()
        .into_iter()
        .filter(|(c, _)| c == "catalog.query")
        .map(|(_, p)| p)
        .collect();
    let (listed, deleted): (Vec<&Value>, Vec<&Value>) =
        pages.iter().partition(|p| p["filter"] == json!({}));
    assert_eq!(
        deleted.len() + listed.len(),
        pages.len(),
        "only the two photo streams and no third filter shape"
    );
    // The non-deleted stream covers all 450 photos, page by page.
    let limit = listed[0]["limit"].as_u64().expect("limit is sent") as usize;
    assert!((1..=200).contains(&limit));
    assert_eq!(listed.len(), 450_usize.div_ceil(limit));
    for (i, p) in listed.iter().enumerate() {
        assert_eq!(p["offset"].as_u64(), Some((i * limit) as u64));
        assert_eq!(p["limit"].as_u64(), Some(limit as u64));
    }
    // The deleted stream is sent too; with no deleted photos it is one page.
    assert_eq!(deleted.len(), 1, "the deleted query is always sent");
    assert_eq!(deleted[0]["filter"], json!({"deleted": true}));
    assert_eq!(deleted[0]["offset"].as_u64(), Some(0));
    assert!((1..=200).contains(&deleted[0]["limit"].as_u64().unwrap()));
}

#[test]
fn request_counts_are_the_expected_ones() {
    let fake = start(|c| {
        c.seed_photos = seeds(1..=5);
        c.albums = vec![SeedAlbum::album("Set", None)];
        c.seed_members = vec![(0, 1), (0, 2)];
    });
    let mut state = state_with(&[1, 2, 3, 4]); // photo 5 is foreign
    state.record_collection(10, album_entry(5001, "Set", EntryKind::Album, &[1, 2]));
    read(&fake, &state).unwrap();
    // Listed photos (one page), deleted photos (one page), album members.
    assert_eq!(count(&fake, "catalog.query"), 3);
    assert_eq!(count(&fake, "photo.inspect"), 4); // mapped photos only
    assert_eq!(count(&fake, "albums.list"), 1);
    assert_eq!(count(&fake, "keyword.list"), 1);
    assert_eq!(fake.commands().len(), 9);
    let filters: Vec<Value> = fake
        .commands()
        .into_iter()
        .filter(|(c, _)| c == "catalog.query")
        .map(|(_, p)| p["filter"].clone())
        .collect();
    assert!(filters.contains(&json!({})));
    assert!(filters.contains(&json!({"deleted": true})));
}

#[test]
fn foreign_photos_are_counted_and_not_stored() {
    let fake = start(|c| {
        c.seed_photos = seeds(1..=5);
        // The foreign photos carry values that must not show up anywhere.
        c.seed_photos[3].title = "Foreign title".into();
        c.seed_photos[4].rating = 5;
    });
    let live = read(&fake, &state_with(&[1, 2, 3])).unwrap();
    assert_eq!(
        live.photos.keys().cloned().collect::<Vec<_>>(),
        [gid(1), gid(2), gid(3)]
    );
    assert_eq!(live.foreign_photos, 2);
    assert!(!format!("{live:?}").contains("Foreign title"));
    // Read = present + missing; foreign ones come on top.
    assert_eq!(live.photos.len() + live.missing_photos.len(), 3);
}

#[test]
fn a_photo_missing_in_lightcraft_is_marked_not_fatal() {
    let fake = start(|c| c.seed_photos = seeds(1..=2));
    let live = read(&fake, &state_with(&[1, 2, 3])).unwrap();
    assert_eq!(live.missing_photos, BTreeSet::from([gid(3)]));
    assert_eq!(live.photos.len(), 2);
    assert!(!live.photos.contains_key(&gid(3)));
    assert_eq!(
        count(&fake, "photo.inspect"),
        2,
        "no inspect of a missing photo"
    );
}

#[test]
fn values_and_empty_values_are_parsed() {
    let fake = start(|c| {
        c.seed_photos = seeds(1..=2);
        c.seed_photos[0] = SeedPhoto {
            rating: 4,
            flag: "pick".into(),
            label: Some("red".into()),
            title: "Grüße 🎉".into(),
            keywords: vec!["b|c".into(), "a".into()],
            caption: Some("Cap".into()),
            copyright: Some("(c) example".into()),
            creator: Some("example".into()),
            ..SeedPhoto::plain(1)
        };
    });
    let live = read(&fake, &state_with(&[1, 2])).unwrap();
    let full = &live.photos[&gid(1)];
    assert_eq!(full.rating, 4);
    assert_eq!(full.flag, "pick");
    assert_eq!(full.label.as_deref(), Some("red"));
    assert_eq!(full.title.as_deref(), Some("Grüße 🎉"));
    assert_eq!(full.keywords, ["a", "b|c"], "sorted");
    assert_eq!(full.caption.as_deref(), Some("Cap"));
    assert_eq!(full.copyright.as_deref(), Some("(c) example"));
    assert_eq!(full.creator.as_deref(), Some("example"));
    let empty = &live.photos[&gid(2)];
    assert_eq!(empty.rating, 0);
    assert_eq!(empty.flag, "none");
    assert_eq!(empty.label, None);
    assert_eq!(empty.title, None, "an empty title becomes None");
    assert!(empty.keywords.is_empty());
    assert_eq!(
        (&empty.caption, &empty.copyright, &empty.creator),
        (&None, &None, &None)
    );
}

#[test]
fn albums_hold_global_ids_and_folders_have_no_members() {
    let fake = start(|c| {
        c.seed_photos = seeds(1..=4);
        c.albums = vec![
            SeedAlbum::folder("Folder", None),
            SeedAlbum::album("Set", Some(0)),
            SeedAlbum::album("Gone", None),
        ];
        // 3 is foreign to the mapping file; 4 is a member of the unmapped album only.
        c.seed_members = vec![(1, 2), (1, 1), (1, 3)];
    });
    let mut state = state_with(&[1, 2]);
    state.record_collection(10, album_entry(5001, "Folder", EntryKind::Folder, &[]));
    state.record_collection(11, album_entry(5002, "Set", EntryKind::Album, &[1, 2]));
    state.record_collection(
        12,
        album_entry(5999, "Deleted by user", EntryKind::Album, &[1]),
    );
    let live = read(&fake, &state).unwrap();
    assert_eq!(live.albums.len(), 2);
    assert_eq!(live.albums[&10].name, "Folder");
    assert!(live.albums[&10].members.is_empty());
    assert_eq!(live.albums[&11].name, "Set");
    assert_eq!(live.albums[&11].members, BTreeSet::from([gid(1), gid(2)]));
    assert_eq!(live.missing_albums, BTreeSet::from([12]));
    // The missing album is never queried.
    let queried: Vec<Value> = fake
        .commands()
        .into_iter()
        .filter(|(c, p)| c == "catalog.query" && p["filter"].get("album").is_some())
        .map(|(_, p)| p["filter"]["album"].clone())
        .collect();
    assert!(!queried.contains(&json!(5999)));
}

#[test]
fn keywords_of_the_library_are_listed() {
    let fake = start(|c| {
        c.keywords = ["Reisen", "Reisen|Köln 🌅", "Zebra"]
            .map(String::from)
            .into();
    });
    let live = read(&fake, &State::default()).unwrap();
    assert_eq!(live.keywords, fake.keywords());
}

#[test]
fn errors_contain_no_values() {
    let fake = start(|c| {
        c.seed_photos = seeds(1..=1);
        c.seed_photos[0].title = "Secret ✨ title".into();
        c.summary_broken = true;
    });
    let err = read(&fake, &state_with(&[1])).unwrap_err();
    assert!(matches!(err, WriteError::Protocol(_)), "{err:?}");
    for text in [err.to_string(), format!("{err:?}"), err.short_reason()] {
        assert!(!text.contains("Secret"), "{text}");
        assert!(!text.contains("many"), "{text}");
    }
    assert!(fake.write_commands().is_empty());
}

#[test]
fn reading_twice_gives_the_same_result() {
    let fake = start(|c| {
        c.seed_photos = seeds(1..=3);
        c.seed_photos[0].title = "A".into();
        c.albums = vec![SeedAlbum::album("Set", None)];
        c.seed_members = vec![(0, 1)];
        c.keywords = ["k".to_string()].into();
    });
    let mut state = state_with(&[1, 2, 9]);
    state.record_collection(10, album_entry(5001, "Set", EntryKind::Album, &[1]));
    let first = read(&fake, &state).unwrap();
    let second = read(&fake, &state).unwrap();
    assert_eq!(first, second);
    assert!(fake.write_commands().is_empty());
}
