//! Acceptance tests for `lc_writer::live::read_live` and photos that
//! LightCraft has deleted (moved to the trash, "Recently Deleted").
//!
//! Contract (A26): `read_live` sends both `catalog.query {"filter": {}}` and
//! `catalog.query {"filter": {"deleted": true}}`, page by page, and merges the
//! LightCraft ids. A photo the mapping file knows that LightCraft deleted is
//! present: it appears in `LiveState::photos` with its live values (summary
//! plus `photo.inspect`) and never in `missing_photos`. `missing_photos` means
//! "listed by neither query". `foreign_photos` counts ids from both queries,
//! each id once. Still only the four read commands `catalog.query`,
//! `photo.inspect`, `albums.list` and `keyword.list` are sent; an unusable
//! reply is `WriteError::Protocol` without values in its text and aborts.

use lc_writer::control::{ControlClient, WriteError};
use lc_writer::live::{LiveState, read_live};
use lc_writer::state::{PhotoFields, State};
use serde_json::{Value, json};
use std::collections::BTreeSet;

use fake_lightcraft::{Config, FakeLightCraft, SeedPhoto};
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

fn query_filters(fake: &FakeLightCraft) -> Vec<Value> {
    fake.commands()
        .into_iter()
        .filter(|(c, _)| c == "catalog.query")
        .map(|(_, p)| p["filter"].clone())
        .collect()
}

#[test]
fn a_deleted_photo_is_present_with_its_live_values() {
    let fake = start(|c| {
        c.seed_photos = vec![SeedPhoto {
            rating: 3,
            flag: "pick".into(),
            label: Some("red".into()),
            title: "Trash ✨".into(),
            keywords: vec!["Reisen|Köln 🌅".into(), "Reisen".into()],
            caption: Some("Kept in the trash".into()),
            copyright: Some("(c) example".into()),
            creator: Some("example".into()),
            deleted: true,
            ..SeedPhoto::plain(1)
        }];
    });
    let live = read(&fake, &state_with(&[1])).unwrap();
    let photo = &live.photos[&gid(1)];
    assert_eq!(photo.rating, 3);
    assert_eq!(photo.flag, "pick");
    assert_eq!(photo.label.as_deref(), Some("red"));
    assert_eq!(photo.title.as_deref(), Some("Trash ✨"));
    assert_eq!(photo.keywords, ["Reisen", "Reisen|Köln 🌅"], "sorted");
    assert_eq!(photo.caption.as_deref(), Some("Kept in the trash"));
    assert_eq!(photo.copyright.as_deref(), Some("(c) example"));
    assert_eq!(photo.creator.as_deref(), Some("example"));
    assert!(
        live.missing_photos.is_empty(),
        "a photo in the trash is not missing: {:?}",
        live.missing_photos
    );
    assert_eq!(live.foreign_photos, 0);
    assert_eq!(
        count(&fake, "photo.inspect"),
        1,
        "a deleted but mapped photo is inspected for its credits"
    );
}

#[test]
fn a_photo_listed_by_neither_query_is_missing() {
    // The contract does not change the meaning of a photo that is gone for
    // good: it is still reported, never fatal.
    let fake = start(|c| c.seed_photos = seeds(1..=1));
    let live = read(&fake, &state_with(&[1, 2])).unwrap();
    assert_eq!(
        live.photos.keys().cloned().collect::<Vec<_>>(),
        [gid(1)],
        "only the photo both queries can list is present"
    );
    assert_eq!(live.missing_photos, BTreeSet::from([gid(2)]));
    assert_eq!(count(&fake, "photo.inspect"), 1);
}

#[test]
fn a_foreign_deleted_photo_is_counted_exactly_once() {
    let fake = start(|c| {
        c.seed_photos = vec![
            SeedPhoto::plain(1),
            SeedPhoto {
                deleted: true,
                ..SeedPhoto::plain(5)
            },
        ];
    });
    let live = read(&fake, &state_with(&[1])).unwrap();
    assert_eq!(
        live.foreign_photos, 1,
        "the deleted foreign photo must be seen and counted once"
    );
    assert_eq!(live.photos.len(), 1);
    assert!(live.missing_photos.is_empty());
}

#[test]
fn an_id_listed_by_both_queries_counts_once() {
    // Both queries overlap: every photo is listed by each stream.
    let fake = start(|c| {
        c.ignore_deleted_filter = true;
        c.seed_photos = vec![
            SeedPhoto::plain(1),
            SeedPhoto {
                deleted: true,
                rating: 2,
                ..SeedPhoto::plain(2)
            },
            SeedPhoto {
                deleted: true,
                ..SeedPhoto::plain(9) // foreign to the mapping file
            },
        ];
    });
    let live = read(&fake, &state_with(&[1, 2])).unwrap();
    assert_eq!(
        live.foreign_photos, 1,
        "the foreign photo is listed twice but counted once"
    );
    assert_eq!(live.photos.len(), 2);
    assert_eq!(live.photos[&gid(2)].rating, 2);
    assert!(live.missing_photos.is_empty());
    assert_eq!(
        count(&fake, "photo.inspect"),
        2,
        "each present mapped photo is inspected once, not once per stream"
    );
    let filters = query_filters(&fake);
    assert!(filters.contains(&json!({})), "{filters:?}");
    assert!(filters.contains(&json!({"deleted": true})), "{filters:?}");
}

#[test]
fn both_queries_are_paged_completely() {
    let fake = start(|c| {
        let mut photos = seeds(1..=450);
        photos.extend((451..=700).map(|id| SeedPhoto {
            deleted: true,
            ..SeedPhoto::plain(id)
        }));
        c.seed_photos = photos;
    });
    let ids: Vec<i64> = (1..=700).collect();
    let live = read(&fake, &state_with(&ids)).unwrap();
    assert_eq!(live.photos.len(), 700, "listed and deleted photos together");
    assert!(live.missing_photos.is_empty());
    assert_eq!(live.foreign_photos, 0);

    let pages: Vec<(Value, u64, u64)> = fake
        .commands()
        .into_iter()
        .filter(|(c, _)| c == "catalog.query")
        .map(|(_, p)| {
            (
                p["filter"].clone(),
                p["offset"].as_u64().expect("offset is sent"),
                p["limit"].as_u64().expect("limit is sent"),
            )
        })
        .collect();
    let stream = |filter: &Value| -> Vec<(u64, u64)> {
        pages
            .iter()
            .filter(|(f, _, _)| f == filter)
            .map(|(_, o, l)| (*o, *l))
            .collect()
    };
    let listed = stream(&json!({}));
    let deleted = stream(&json!({"deleted": true}));
    assert_eq!(
        listed.len() + deleted.len(),
        pages.len(),
        "no third filter shape"
    );
    for (pages, expected) in [(&listed, 450_usize), (&deleted, 250_usize)] {
        let limit = pages[0].1 as usize;
        assert!((1..=200).contains(&limit));
        assert_eq!(pages.len(), expected.div_ceil(limit), "{pages:?}");
        for (i, (offset, page_limit)) in pages.iter().enumerate() {
            assert_eq!(*offset, (i * limit) as u64);
            assert_eq!(*page_limit, limit as u64);
        }
    }
}

#[test]
fn reading_deleted_photos_sends_only_the_four_read_commands() {
    let fake = start(|c| {
        c.seed_photos = vec![
            SeedPhoto::plain(1),
            SeedPhoto {
                deleted: true,
                title: "Secret ✨ title".into(),
                ..SeedPhoto::plain(2)
            },
        ];
        c.keywords = ["Reisen|Köln 🌅".to_string()].into();
    });
    let before = (
        fake.photos(),
        fake.albums(),
        fake.keywords(),
        fake.ratings(),
    );
    let live = read(&fake, &state_with(&[1, 2])).unwrap();
    assert_eq!(live.photos.len(), 2);
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
    let filters = query_filters(&fake);
    assert!(filters.contains(&json!({})), "{filters:?}");
    assert!(filters.contains(&json!({"deleted": true})), "{filters:?}");
}
