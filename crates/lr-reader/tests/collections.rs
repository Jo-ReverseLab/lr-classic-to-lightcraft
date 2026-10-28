//! Acceptance tests for reading collections from the small fixture.
//!
//!
//! Expected API (see `collection_edge.rs` for the full rules):
//! `Catalog::collection_data(&self, photos: &[model::Photo])
//!  -> Result<CollectionData, ReadError>`.
//!
//! Small fixture: set 1 holds album 2; albums 3 and 4 are on top level;
//! 5 and 6 are smart, 7 and 8 are `systemOnly` (quick), 9 has an unknown
//! kind. Member rows: (2,1) (2,2) (3,3) (3,4) (4,5) (7,1) (8,6).

use lr_reader::{Catalog, CollectionData, checksum};
use lrcat_fixtures::small;
use model::{CollectionKind, Report};
use std::path::{Path, PathBuf};

fn small_fixture(dir: &Path) -> PathBuf {
    let path = dir.join("small.lrcat");
    lrcat_fixtures::write_small_catalog(&path).expect("generate fixture");
    path
}

fn read(path: &Path) -> CollectionData {
    let catalog = Catalog::open(path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    catalog.collection_data(&photos).expect("read collections")
}

fn reasons(report: &Report) -> Vec<String> {
    report
        .skipped_items()
        .iter()
        .map(|s| s.reason.to_lowercase())
        .collect()
}

#[test]
fn small_fixture_collections_are_converted_or_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&small_fixture(dir.path()));
    assert_eq!(d.report.read_count() as i64, small::COLLECTIONS);
    assert_eq!(d.collections.len(), 4);
    // Quick collection 7 is converted (not an album), 8 is skipped.
    assert_eq!(d.report.skipped_count(), 4);
    assert_eq!(
        d.report.read_count(),
        d.collections.len() + 1 + d.report.skipped_count()
    );
    let folders: Vec<i64> = d
        .collections
        .iter()
        .filter(|c| c.kind == CollectionKind::Folder)
        .map(|c| c.lightroom_id)
        .collect();
    assert_eq!(folders, vec![1]);
    let mut albums: Vec<i64> = d
        .collections
        .iter()
        .filter(|c| c.kind == CollectionKind::Album)
        .map(|c| c.lightroom_id)
        .collect();
    albums.sort();
    assert_eq!(albums, vec![2, 3, 4]);
}

#[test]
fn small_fixture_parents_and_members() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&small_fixture(dir.path()));
    let get = |id: i64| d.collections.iter().find(|c| c.lightroom_id == id).unwrap();
    assert_eq!(get(1).parent, None);
    assert_eq!(get(2).parent, Some(1));
    assert_eq!(get(3).parent, None);
    assert_eq!(get(2).members, vec![1, 2]);
    assert_eq!(get(3).members, vec![3, 4]);
    assert_eq!(get(4).members, vec![5]);
    assert!(get(1).members.is_empty());
}

#[test]
fn small_fixture_member_balance() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&small_fixture(dir.path()));
    // Quick collection 7 keeps its member 1 (see quick_collection.rs).
    let kept: usize = d.collections.iter().map(|c| c.members.len()).sum::<usize>() + 1;
    assert_eq!(d.members.read_count() as i64, small::COLLECTION_IMAGES);
    assert_eq!(kept, 6);
    // Only (8,6) belongs to a skipped collection.
    assert_eq!(d.members.skipped_count(), 1);
    assert_eq!(d.members.read_count(), kept + d.members.skipped_count());
    assert!(
        reasons(&d.members)
            .iter()
            .all(|r| r.contains("collection skipped"))
    );
}

#[test]
fn small_fixture_names_do_not_appear_in_any_reason() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    let names: Vec<String> = {
        let c = rusqlite::Connection::open(&path).unwrap();
        let mut st = c.prepare("SELECT name FROM AgLibraryCollection").unwrap();
        st.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let d = read(&path);
    for reason in reasons(&d.report).into_iter().chain(reasons(&d.members)) {
        for name in &names {
            assert!(
                !reason.contains(&name.to_lowercase()),
                "{reason:?} has name"
            );
        }
    }
}

#[test]
fn reading_collections_does_not_change_the_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    let before = checksum(&path).unwrap();
    let _ = read(&path);
    assert_eq!(before, checksum(&path).unwrap());
}

#[test]
fn reading_twice_gives_identical_results() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    let (a, b) = (read(&path), read(&path));
    assert_eq!(
        format!("{:?}", a.collections),
        format!("{:?}", b.collections)
    );
    assert_eq!(reasons(&a.report), reasons(&b.report));
    assert_eq!(reasons(&a.members), reasons(&b.members));
}

#[test]
fn catalog_without_collections_reads_zero() {
    let dir = tempfile::tempdir().unwrap();
    let path = small_fixture(dir.path());
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch("DELETE FROM AgLibraryCollectionImage; DELETE FROM AgLibraryCollection;")
            .unwrap();
    }
    let d = read(&path);
    assert!(d.collections.is_empty());
    assert_eq!(d.report.read_count(), 0);
    assert_eq!(d.members.read_count(), 0);
}
