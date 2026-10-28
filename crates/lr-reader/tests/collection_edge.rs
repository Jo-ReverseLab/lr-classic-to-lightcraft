//! Acceptance tests for reading collections and collection sets into the
//! model (task P5b-3a), on the collection fixture
//! (`lrcat_fixtures::collection_edge`).
//!
//! Expected API:
//!
//! ```ignore
//! // model
//! pub struct Collection { pub lightroom_id: i64, pub name: String,
//!     pub kind: CollectionKind, pub parent: Option<i64>, pub members: Vec<i64> }
//! pub enum CollectionKind { Album, Folder }
//! // lr-reader (re-exported from lib.rs)
//! pub struct CollectionData {
//!     pub collections: Vec<model::Collection>,
//!     pub report: model::Report,  // one entry per AgLibraryCollection row
//!     pub members: model::Report, // one entry per AgLibraryCollectionImage row
//! }
//! impl Catalog {
//!     /// `photos` are the converted photos; members outside it are skipped.
//!     pub fn collection_data(&self, photos: &[model::Photo])
//!         -> Result<CollectionData, ReadError>;
//! }
//! ```
//!
//! Rules under test:
//! - `…library.collection` with `systemOnly` 0/NULL (INTEGER or REAL): Album.
//!   `…library.group`: Folder. Smart: skipped ("smart collection").
//!   `systemOnly` != 0 (REAL 1.0 or INTEGER 1): skipped ("quick collection").
//!   Any other `creationId`: skipped, reason names the kind, not the name.
//! - parent NULL: top level. Parent is a converted Folder: kept. Parent
//!   missing, skipped or an album, and cycles: skipped ("parent"), including
//!   whole subtrees.
//! - Members: sorted by `positionInCollection` (NULL last), then row id.
//!   Repeated (collection, image) pair: kept once, the repeat is skipped
//!   ("duplicate member"). Photo not in `photos` or unknown: member skipped,
//!   reason names the photo id, the collection stays. Members of skipped
//!   collections: skipped ("collection skipped"). Folders have no members;
//!   rows for them are skipped. Empty collections stay.
//! - The reader fills `read` and `skipped` of both reports; whether the
//!   reader or the caller marks `converted` is not tested here, only
//!   `read == collections/members kept + skipped`.
//! - Reasons name ids and kinds, never collection names. The exact wording
//!   is not fixed: tests look for the phrases above (case-insensitive) and
//!   for the ids as whole numbers.

use lr_reader::{Catalog, CollectionData, checksum};
use lrcat_fixtures::collection_edge as e;
use model::{CollectionKind, Photo, Report};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn fixture(dir: &Path) -> PathBuf {
    let path = dir.join("collections.lrcat");
    lrcat_fixtures::write_collection_catalog(&path).expect("generate fixture");
    path
}

fn photos_of(catalog: &Catalog) -> Vec<Photo> {
    catalog.photos(&mut Report::new()).unwrap()
}

fn read(path: &Path) -> CollectionData {
    let catalog = Catalog::open(path).unwrap();
    let photos = photos_of(&catalog);
    catalog.collection_data(&photos).expect("read collections")
}

fn reasons(report: &Report) -> Vec<String> {
    report
        .skipped_items()
        .iter()
        .map(|s| s.reason.to_lowercase())
        .collect()
}

/// The whole numbers in `text`.
fn numbers(text: &str) -> BTreeSet<i64> {
    text.split(|c: char| !c.is_ascii_digit())
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect()
}

fn mentions(reason: &str, id: i64) -> bool {
    numbers(reason).contains(&id)
}

fn has_reason(report: &Report, phrase: &str, id: i64) -> bool {
    reasons(report)
        .iter()
        .any(|r| r.contains(phrase) && mentions(r, id))
}

fn album(d: &CollectionData, id: i64) -> &model::Collection {
    d.collections
        .iter()
        .find(|c| c.lightroom_id == id)
        .unwrap_or_else(|| panic!("collection {id} not converted"))
}

#[test]
#[ignore = "waiting for P5b-6: the quick collection counts as converted, not skipped"]
fn counts_and_kinds() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    assert_eq!(d.report.read_count() as i64, e::COLLECTIONS);
    assert_eq!(d.collections.len() as i64, e::CONVERTED);
    assert_eq!(d.report.skipped_count() as i64, e::SKIPPED);
    assert_eq!(
        d.report.read_count(),
        d.collections.len() + d.report.skipped_count()
    );
    let ids: BTreeSet<i64> = d.collections.iter().map(|c| c.lightroom_id).collect();
    assert_eq!(ids.len(), d.collections.len(), "no collection twice");
    let want: BTreeSet<i64> = e::FOLDER_IDS.iter().chain(&e::ALBUM_IDS).copied().collect();
    assert_eq!(ids, want);
    for c in &d.collections {
        let kind = if e::FOLDER_IDS.contains(&c.lightroom_id) {
            CollectionKind::Folder
        } else {
            CollectionKind::Album
        };
        assert_eq!(c.kind, kind, "kind of {}", c.lightroom_id);
    }
    let folders = d
        .collections
        .iter()
        .filter(|c| c.kind == CollectionKind::Folder)
        .count();
    assert_eq!(folders as i64, e::FOLDERS);
}

#[test]
fn parents_follow_the_folder_tree() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    for (id, _, parent) in e::NAMES {
        assert_eq!(album(&d, id).parent, parent, "parent of {id}");
    }
    // Every kept parent is a kept folder (depth 3: 1 > 2 > 3 > album 4).
    for c in &d.collections {
        if let Some(p) = c.parent {
            assert_eq!(album(&d, p).kind, CollectionKind::Folder);
        }
    }
}

#[test]
fn names_are_kept_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    for (id, name, _) in e::NAMES {
        assert_eq!(
            album(&d, id).name.as_bytes(),
            name.as_bytes(),
            "name of {id}"
        );
    }
}

#[test]
fn same_names_below_one_parent_are_both_kept() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    assert_eq!(album(&d, 11).name, album(&d, 12).name);
    assert_eq!(album(&d, 11).parent, album(&d, 12).parent);
    assert_eq!(album(&d, 11).members, vec![13]);
    assert_eq!(album(&d, 12).members, vec![14]);
}

#[test]
fn members_are_in_lightroom_order() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    // 4: REAL positions; 5: all NULL (row order); 6: mixed, NULL last.
    for (id, members) in e::MEMBERS {
        assert_eq!(album(&d, id).members, members, "members of {id}");
    }
    for id in e::FOLDER_IDS {
        assert!(album(&d, id).members.is_empty(), "folder {id}");
    }
    assert!(album(&d, 10).members.is_empty(), "empty album stays");
}

#[test]
#[ignore = "waiting for P5b-6: the quick collection counts as converted, not skipped"]
fn skipped_collections_have_reasons_with_id_and_kind() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    for id in e::SMART_IDS {
        assert!(has_reason(&d.report, "smart collection", id), "smart {id}");
    }
    for id in e::QUICK_IDS {
        assert!(has_reason(&d.report, "quick collection", id), "quick {id}");
    }
    for id in e::PARENT_IDS {
        assert!(has_reason(&d.report, "parent", id), "parent {id}");
    }
    // Other kinds: id present, and none of the phrases above.
    for id in e::OTHER_KIND_IDS {
        let found = reasons(&d.report).into_iter().any(|r| {
            mentions(&r, id)
                && !r.contains("smart collection")
                && !r.contains("quick collection")
                && !r.contains("parent")
        });
        assert!(found, "other kind {id}");
    }
    let r = reasons(&d.report);
    let distinct: BTreeSet<&String> = r.iter().collect();
    assert_eq!(distinct.len(), r.len(), "one distinct reason per row");
}

#[test]
fn smart_and_quick_collections_are_not_misfiled() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    // Quick wins over the plain creationId (15) and the kind (14).
    for id in e::QUICK_IDS {
        assert!(!has_reason(&d.report, "smart collection", id));
        assert!(!has_reason(&d.report, "parent", id));
    }
    for id in e::SMART_IDS {
        assert!(!has_reason(&d.report, "quick collection", id));
    }
}

#[test]
#[ignore = "waiting for P5b-6: the quick collection counts as converted, not skipped"]
fn member_report_balance_and_reasons() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    let kept: usize = d.collections.iter().map(|c| c.members.len()).sum();
    assert_eq!(d.members.read_count() as i64, e::MEMBER_ROWS);
    assert_eq!(kept as i64, e::MEMBERS_CONVERTED);
    assert_eq!(d.members.skipped_count() as i64, e::MEMBERS_SKIPPED);
    assert_eq!(d.members.read_count(), kept + d.members.skipped_count());
    let r = reasons(&d.members);
    // Repeat of (7, 15): exactly one.
    let dup: Vec<&String> = r
        .iter()
        .filter(|r| r.contains("duplicate member"))
        .collect();
    assert_eq!(dup.len(), 1, "{r:?}");
    assert!(mentions(dup[0], e::DUPLICATE_MEMBER.0) || mentions(dup[0], e::DUPLICATE_MEMBER.1));
    // Photos that are not converted: virtual copy 20, unknown 999.
    for photo in e::NOT_CONVERTED_PHOTOS {
        let n = r
            .iter()
            .filter(|r| mentions(r, photo) && !r.contains("collection skipped"))
            .count();
        assert_eq!(n, 1, "photo {photo}: {r:?}");
    }
    // Members of skipped collections: smart 13, quick 14 and 15, web 16,
    // missing parent 18, album below a skipped set 22.
    let skipped_coll = r
        .iter()
        .filter(|r| r.contains("collection skipped"))
        .count();
    assert_eq!(skipped_coll, 6, "{r:?}");
    // The folder's row (folder 3, photo 19) is the last one.
    assert!(
        r.iter()
            .any(|r| mentions(r, e::FOLDER_WITH_MEMBER) && !r.contains("collection skipped")),
        "{r:?}"
    );
}

#[test]
fn photo_missing_from_the_converted_list_is_a_skipped_member() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture(dir.path());
    let catalog = Catalog::open(&path).unwrap();
    // Photo 5 is a member of album 4 (and of skipped collection 18).
    let photos: Vec<Photo> = photos_of(&catalog)
        .into_iter()
        .filter(|p| p.lightroom_id != 5)
        .collect();
    let d = catalog.collection_data(&photos).unwrap();
    assert_eq!(album(&d, 4).members, vec![4, 1]);
    assert_eq!(d.collections.len() as i64, e::CONVERTED, "album stays");
    assert_eq!(d.members.skipped_count() as i64, e::MEMBERS_SKIPPED + 1);
    assert!(
        reasons(&d.members)
            .iter()
            .any(|r| mentions(r, 5) && !r.contains("collection skipped"))
    );
    let kept: usize = d.collections.iter().map(|c| c.members.len()).sum();
    assert_eq!(d.members.read_count(), kept + d.members.skipped_count());
}

#[test]
fn no_photos_means_no_members_but_all_collections() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(&fixture(dir.path())).unwrap();
    let d = catalog.collection_data(&[]).unwrap();
    assert_eq!(d.collections.len() as i64, e::CONVERTED);
    assert!(d.collections.iter().all(|c| c.members.is_empty()));
    assert_eq!(d.members.read_count() as i64, e::MEMBER_ROWS);
    assert_eq!(d.members.skipped_count() as i64, e::MEMBER_ROWS);
}

#[test]
fn no_reason_contains_a_collection_name() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    let mut names: Vec<String> = e::SKIPPED_NAMES.iter().map(|s| s.to_lowercase()).collect();
    names.extend(
        e::NAMES
            .iter()
            .map(|(_, n, _)| n.to_lowercase())
            .filter(|n| n.trim().len() > 4),
    );
    for reason in reasons(&d.report).into_iter().chain(reasons(&d.members)) {
        for name in &names {
            assert!(!reason.contains(name.as_str()), "{reason:?} has {name:?}");
        }
    }
}

#[test]
fn reading_does_not_change_the_catalog_and_is_repeatable() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture(dir.path());
    let before = checksum(&path).unwrap();
    let a = read(&path);
    let b = read(&path);
    assert_eq!(before, checksum(&path).unwrap());
    assert_eq!(
        format!("{:?}", a.collections),
        format!("{:?}", b.collections)
    );
    assert_eq!(reasons(&a.report), reasons(&b.report));
    assert_eq!(reasons(&a.members), reasons(&b.members));
}
