//! Acceptance tests for reading the quick collection.
//!
//! API: `CollectionData.quick: Option<model::QuickCollection>` with
//! `QuickCollection { lightroom_id: i64, members: Vec<i64> }`.
//!
//! Rules:
//! - an `AgLibraryCollection` row with `systemOnly` != 0 (REAL 1.0 or
//!   INTEGER 1) is the quick collection; it is converted (counts as read, not
//!   as skipped in `report`) and is not in `collections`;
//! - more than one such row: the first by id is taken, the others are
//!   skipped with a reason containing "more than one quick collection" and
//!   their id; their members are skipped ("collection skipped");
//! - members follow the album rules: by `positionInCollection` (NULL last,
//!   then row id), photos that were not converted are skipped with the photo
//!   id in the reason; kept members count as converted in `members`;
//! - no row: `quick` is `None`;
//! - reasons never contain a collection name.

use lr_reader::{Catalog, CollectionData, checksum};
use lrcat_fixtures::{collection_edge as e, quick_edge as q};
use model::{Photo, Report};
use std::path::Path;

fn read(path: &Path, photos: impl Fn(Vec<Photo>) -> Vec<Photo>) -> CollectionData {
    let catalog = Catalog::open(path).unwrap();
    let all = catalog.photos(&mut Report::new()).unwrap();
    catalog.collection_data(&photos(all)).unwrap()
}

fn reasons(report: &Report) -> Vec<String> {
    report
        .skipped_items()
        .iter()
        .map(|s| s.reason.to_lowercase())
        .collect()
}

fn has_number(text: &str, id: i64) -> bool {
    text.split(|c: char| !c.is_ascii_digit())
        .any(|t| t == id.to_string())
}

fn edge(dir: &Path) -> std::path::PathBuf {
    let p = dir.join("collections.lrcat");
    lrcat_fixtures::write_collection_catalog(&p).unwrap();
    p
}

fn quick_fixture(dir: &Path) -> std::path::PathBuf {
    lrcat_fixtures::write_quick_catalog_with_files(dir).unwrap()
}

#[test]
fn the_first_quick_collection_by_id_is_taken() {
    // 14 has systemOnly REAL 1.0, 15 INTEGER 1.
    let dir = tempfile::tempdir().unwrap();
    let d = read(&edge(dir.path()), |p| p);
    let quick = d.quick.expect("quick collection");
    assert_eq!(quick.lightroom_id, e::QUICK_IDS[0]);
    assert!(
        d.collections
            .iter()
            .all(|c| !e::QUICK_IDS.contains(&c.lightroom_id))
    );
}

#[test]
fn a_second_quick_collection_is_skipped_with_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&edge(dir.path()), |p| p);
    let r = reasons(&d.report);
    let first = e::QUICK_IDS[0];
    let second = e::QUICK_IDS[1];
    assert!(
        r.iter()
            .any(|r| r.contains("more than one quick collection") && has_number(r, second)),
        "{r:?}"
    );
    assert!(
        !r.iter()
            .any(|r| has_number(r, first) && r.contains("quick"))
    );
    // Balance: the quick collection is converted, not skipped.
    assert_eq!(d.report.read_count() as i64, e::COLLECTIONS);
    assert_eq!(d.report.skipped_count() as i64, e::SKIPPED - 1);
    assert_eq!(
        d.report.read_count(),
        d.collections.len() + 1 + d.report.skipped_count()
    );
}

#[test]
fn the_real_and_the_integer_flag_both_count() {
    // Without the REAL row the INTEGER row is the quick collection.
    let dir = tempfile::tempdir().unwrap();
    let path = edge(dir.path());
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("DELETE FROM AgLibraryCollectionImage WHERE collection = 14; DELETE FROM AgLibraryCollection WHERE id_local = 14;")
        .unwrap();
    let d = read(&path, |p| p);
    assert_eq!(d.quick.expect("quick").lightroom_id, e::QUICK_IDS[1]);
    assert!(
        !reasons(&d.report)
            .iter()
            .any(|r| r.contains("more than one"))
    );
}

#[test]
fn members_follow_the_album_rules() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&quick_fixture(dir.path()), |p| p);
    let quick = d.quick.expect("quick collection");
    assert_eq!(quick.lightroom_id, q::QUICK_ID);
    assert_eq!(quick.members, q::MEMBERS, "position order, skipped removed");
    let r = reasons(&d.members);
    // Virtual copy 20 and unknown photo 999: skipped, each named once.
    for photo in [20, 999] {
        let n = r
            .iter()
            .filter(|r| {
                has_number(r, photo) && has_number(r, 14) && !r.contains("collection skipped")
            })
            .count();
        assert_eq!(n, 1, "photo {photo}: {r:?}");
    }
    // Balance over albums and the quick collection.
    let kept: usize =
        d.collections.iter().map(|c| c.members.len()).sum::<usize>() + quick.members.len();
    assert_eq!(d.members.read_count(), kept + d.members.skipped_count());
    // The only collection reason left over by the fixture's single quick row
    // is none about quick.
    assert!(!reasons(&d.report).iter().any(|r| r.contains("quick")));
    let skipped_quick_rows = q::MEMBERS_SKIPPED as usize;
    assert!(d.members.skipped_count() >= skipped_quick_rows);
}

#[test]
fn members_not_converted_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&quick_fixture(dir.path()), |p| {
        p.into_iter().filter(|p| p.lightroom_id != 7).collect()
    });
    assert_eq!(d.quick.unwrap().members, vec![9, 1]);
    assert!(reasons(&d.members).iter().any(|r| has_number(r, 7)));
}

#[test]
fn no_quick_row_means_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("none.lrcat");
    lrcat_fixtures::write_collection_catalog(&path).unwrap();
    lrcat_fixtures::clear_collections(&path).unwrap();
    assert!(read(&path, |p| p).quick.is_none());
}

#[test]
fn reading_leaves_the_catalog_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = quick_fixture(dir.path());
    let before = checksum(&path).unwrap();
    let a = read(&path, |p| p);
    let b = read(&path, |p| p);
    assert_eq!(before, checksum(&path).unwrap());
    assert_eq!(a.quick, b.quick);
}

#[test]
fn no_collection_name_appears_in_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&edge(dir.path()), |p| p);
    let all = [reasons(&d.report), reasons(&d.members)]
        .concat()
        .join("\n");
    for name in ["zyx-quick-real", "zyx-quick-int"] {
        assert!(!all.contains(name), "{name}");
    }
}
