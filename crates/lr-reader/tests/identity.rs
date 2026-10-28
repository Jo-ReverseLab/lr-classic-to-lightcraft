//! Acceptance tests for stable identity (principle 5).
//!
//! Expected API:
//! - `model::Photo` and `model::Keyword` get `pub global_id: Option<String>`.
//!   `new` sets `None`; `with_global_id(impl Into<String>) -> Self` sets it.
//!   `PartialEq` stays derived (compares `global_id` too).
//! - `Catalog::photos` and `Catalog::keywords` fill `global_id` with
//!   `Adobe_images.id_global` / `AgLibraryKeyword.id_global`, byte for byte.
//! - Value NULL (not generatable: the column is NOT NULL), empty or only
//!   whitespace: the row is skipped with `photo {id}: missing global id` /
//!   `keyword {id}: missing global id`. A skipped keyword takes its subtree
//!   with it (`keyword {id}: parent keyword {p} is skipped`).
//! - A value already used by an earlier row (ordered by `id_local`) of the
//!   same table: `photo {id}: duplicate global id` /
//!   `keyword {id}: duplicate global id`. The same value in the photo table
//!   and the keyword table is not a duplicate.
//! - Reasons never contain the global id itself.
//! - `id_global` of both tables is in the required schema.

use lr_reader::{Catalog, ReadError, checksum};
use lrcat_fixtures::{
    identity, set_keyword_global_id, set_photo_global_id, write_identity_catalog,
    write_small_catalog,
};
use model::{Photo, Report};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

struct Read {
    photos: Vec<Photo>,
    photo_report: Report,
    data: lr_reader::KeywordData,
    keyword_report: Report,
    assignment_report: Report,
}

fn read(path: &Path) -> Read {
    let catalog = Catalog::open(path).unwrap();
    let mut photo_report = Report::new();
    let photos = catalog.photos(&mut photo_report).expect("photos");
    let mut keyword_report = Report::new();
    let mut assignment_report = Report::new();
    let data = catalog
        .keywords(&photos, &mut keyword_report, &mut assignment_report)
        .expect("keywords");
    Read {
        photos,
        photo_report,
        data,
        keyword_report,
        assignment_report,
    }
}

fn reasons(report: &Report) -> Vec<String> {
    report
        .skipped_items()
        .iter()
        .map(|s| s.reason.clone())
        .collect()
}

fn small(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("small.lrcat");
    write_small_catalog(&path).unwrap();
    path
}

fn gid(p: &Photo) -> String {
    p.global_id.clone().expect("photo has a global id")
}

fn kgid(k: &model::Keyword) -> String {
    k.global_id.clone().expect("keyword has a global id")
}

/// global_id -> path relative to `root`, with `/` separators.
fn photo_map(photos: &[Photo], root: &str) -> BTreeMap<String, String> {
    photos
        .iter()
        .map(|p| {
            let full = p.path.to_string_lossy().replace('\\', "/");
            let rel = full.strip_prefix(root).unwrap_or_else(|| panic!("{full}"));
            (gid(p), rel.to_string())
        })
        .collect()
}

fn keyword_map(data: &lr_reader::KeywordData) -> BTreeMap<String, Vec<String>> {
    data.keywords
        .iter()
        .map(|k| (kgid(k), k.path().to_vec()))
        .collect()
}

/// photo global id -> sorted keyword global ids.
fn assignment_map(r: &Read) -> BTreeMap<String, BTreeSet<String>> {
    let by_id: BTreeMap<i64, String> = r.photos.iter().map(|p| (p.lightroom_id, gid(p))).collect();
    r.data
        .assignments
        .iter()
        .map(|(photo, kws)| (by_id[photo].clone(), kws.iter().map(kgid).collect()))
        .collect()
}

#[test]
fn global_ids_are_read_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let path = small(dir.path());
    let r = read(&path);
    assert!(!r.photos.is_empty());
    for p in &r.photos {
        assert_eq!(gid(p), format!("image-{:08}", p.lightroom_id));
    }
    assert!(!r.data.keywords.is_empty());
    // Generated keyword global ids are kw-<id_local>; the keyword path
    // identifies the row (Travel = 2, Italy = 3).
    let italy = r
        .data
        .keywords
        .iter()
        .find(|k| k.path() == ["Travel", "Italy"])
        .unwrap();
    assert_eq!(kgid(italy), "kw-00000003");
    // Assigned keywords carry the id as well.
    for kws in r.data.assignments.values() {
        assert!(kws.iter().all(|k| k.global_id.is_some()));
    }
}

#[test]
fn unusual_global_ids_are_kept_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let path = small(dir.path());
    let odd = " Zyx Ünï-🌅 {id} ";
    set_photo_global_id(&path, 1, odd).unwrap();
    set_keyword_global_id(&path, 2, odd).unwrap();
    let r = read(&path);
    let p = r.photos.iter().find(|p| p.lightroom_id == 1).unwrap();
    assert_eq!(p.global_id.as_deref(), Some(odd));
    let k = r
        .data
        .keywords
        .iter()
        .find(|k| k.path() == ["Travel"])
        .unwrap();
    assert_eq!(k.global_id.as_deref(), Some(odd));
    // Same value in two different tables is no duplicate.
    assert_eq!(r.photo_report.skipped_count(), 1); // only the virtual copy
    assert!(
        reasons(&r.keyword_report)
            .iter()
            .all(|s| !s.contains("duplicate"))
    );
}

#[test]
fn same_content_in_two_catalogs_gives_same_identity() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.lrcat");
    let b = dir.path().join("b.lrcat");
    write_identity_catalog(&a, 0).unwrap();
    write_identity_catalog(&b, 1).unwrap();
    let ra = read(&a);
    let rb = read(&b);
    for r in [&ra, &rb] {
        assert_eq!(r.photos.len(), identity::PHOTOS.len());
        assert_eq!(r.data.keywords.len(), identity::KEYWORDS.len());
        assert_eq!(r.photo_report.skipped_count(), 0);
        assert_eq!(r.keyword_report.skipped_count(), 0);
        assert_eq!(r.assignment_report.skipped_count(), 0);
    }
    // The catalogs really differ in id_local and root path.
    let ids = |r: &Read| r.photos.iter().map(|p| p.lightroom_id).collect::<Vec<_>>();
    assert_ne!(ids(&ra), ids(&rb));
    assert_ne!(ra.photos[0].path, rb.photos[0].path);

    let ma = photo_map(&ra.photos, identity::ROOTS[0]);
    let mb = photo_map(&rb.photos, identity::ROOTS[1]);
    assert_eq!(ma, mb);
    let expected: BTreeMap<String, String> = identity::PHOTOS
        .iter()
        .map(|(g, folder, base, _)| (g.to_string(), format!("{folder}{base}.jpg")))
        .collect();
    assert_eq!(ma, expected);

    let ka = keyword_map(&ra.data);
    assert_eq!(ka, keyword_map(&rb.data));
    assert_eq!(ka["kw-4e5f"], ["Travel", "Italy", "Rome"]);

    let aa = assignment_map(&ra);
    assert_eq!(aa, assignment_map(&rb));
    let mut expected: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (p, k) in identity::ASSIGNMENTS {
        expected
            .entry(identity::PHOTOS[p].0.to_string())
            .or_default()
            .insert(identity::KEYWORDS[k].0.to_string());
    }
    assert_eq!(aa, expected);

    // Ratings follow the identity too.
    let rating = |r: &Read| -> BTreeMap<String, u8> {
        r.photos.iter().map(|p| (gid(p), p.rating.get())).collect()
    };
    assert_eq!(rating(&ra), rating(&rb));
}

#[test]
fn identity_survives_a_second_read_and_catalog_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.lrcat");
    write_identity_catalog(&path, 1).unwrap();
    let before = checksum(&path).unwrap();
    let first = read(&path);
    let second = read(&path);
    assert_eq!(checksum(&path).unwrap(), before);
    assert_eq!(first.photos, second.photos);
    assert_eq!(first.data, second.data);
    let unique: BTreeSet<_> = first.photos.iter().map(gid).collect();
    assert_eq!(unique.len(), first.photos.len(), "duplicate photo identity");
}

#[test]
fn empty_or_blank_photo_global_id_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = small(dir.path());
    set_photo_global_id(&path, 4, "").unwrap();
    set_photo_global_id(&path, 5, "   \t").unwrap();
    let before = checksum(&path).unwrap();
    let r = read(&path);
    assert_eq!(checksum(&path).unwrap(), before);
    let ids: Vec<i64> = r.photos.iter().map(|p| p.lightroom_id).collect();
    assert!(!ids.contains(&4) && !ids.contains(&5));
    assert!(ids.contains(&1) && ids.contains(&6));
    let rs = reasons(&r.photo_report);
    assert!(
        rs.contains(&"photo 4: missing global id".to_string()),
        "{rs:?}"
    );
    assert!(
        rs.contains(&"photo 5: missing global id".to_string()),
        "{rs:?}"
    );
    // The virtual copy keeps its own reason: read = kept + skipped.
    assert_eq!(rs.len(), 3, "{rs:?}");
    assert_eq!(r.photo_report.read_count(), r.photos.len() + rs.len());
    assert_eq!(
        r.photo_report.read_count() as i64,
        lrcat_fixtures::small::PHOTOS
    );
    assert!(r.photo_report.is_balanced() || r.photo_report.converted_count() == 0);
    // Assignments to the skipped photos are skipped, not lost.
    let a = reasons(&r.assignment_report);
    assert!(
        a.iter()
            .any(|s| s.contains("photo 4 is skipped or unknown")),
        "{a:?}"
    );
    assert_eq!(
        r.assignment_report.read_count(),
        r.data.assignments.values().map(Vec::len).sum::<usize>() + a.len()
    );
}

#[test]
fn duplicate_photo_global_id_skips_later_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = small(dir.path());
    set_photo_global_id(&path, 4, "zyx-dup").unwrap();
    set_photo_global_id(&path, 2, "zyx-dup").unwrap();
    set_photo_global_id(&path, 9, "zyx-dup").unwrap();
    let r = read(&path);
    let ids: Vec<i64> = r.photos.iter().map(|p| p.lightroom_id).collect();
    assert!(ids.contains(&2), "first row in id_local order is kept");
    assert!(!ids.contains(&4) && !ids.contains(&9));
    let rs = reasons(&r.photo_report);
    assert!(
        rs.contains(&"photo 4: duplicate global id".to_string()),
        "{rs:?}"
    );
    assert!(
        rs.contains(&"photo 9: duplicate global id".to_string()),
        "{rs:?}"
    );
    assert!(!rs.iter().any(|s| s.contains("photo 2:")), "{rs:?}");
    assert!(rs.iter().all(|s| !s.contains("zyx-dup")), "{rs:?}");
    assert_eq!(r.photo_report.read_count(), r.photos.len() + rs.len());
    let unique: BTreeSet<_> = r.photos.iter().map(gid).collect();
    assert_eq!(unique.len(), r.photos.len());
}

#[test]
fn empty_keyword_global_id_skips_keyword_and_subtree() {
    let dir = tempfile::tempdir().unwrap();
    let path = small(dir.path());
    // Italy (3) has Rome (4) and Colosseum (5) below it; Sea (9) is a leaf.
    set_keyword_global_id(&path, 3, "").unwrap();
    set_keyword_global_id(&path, 9, "  ").unwrap();
    let before = checksum(&path).unwrap();
    let r = read(&path);
    assert_eq!(checksum(&path).unwrap(), before);
    let rs = reasons(&r.keyword_report);
    for want in [
        "keyword 3: missing global id",
        "keyword 4: parent keyword 3 is skipped",
        "keyword 5: parent keyword 4 is skipped",
        "keyword 9: missing global id",
    ] {
        assert!(rs.contains(&want.to_string()), "missing {want:?} in {rs:?}");
    }
    let paths: Vec<Vec<String>> = r.data.keywords.iter().map(|k| k.path().to_vec()).collect();
    assert!(!paths.iter().any(|p| p.contains(&"Italy".to_string())));
    assert!(!paths.iter().any(|p| p == &["Sea".to_string()]));
    assert!(paths.contains(&vec!["Travel".to_string(), "Köln".to_string()]));
    assert_eq!(
        r.keyword_report.read_count(),
        r.data.keywords.len() + rs.len()
    );
    assert!(rs.iter().all(|s| !s.contains("zyx")));
    // Assignments to skipped keywords stay accounted for.
    let a = reasons(&r.assignment_report);
    assert!(
        a.iter().any(|s| s.contains("keyword 3 is skipped")),
        "{a:?}"
    );
    assert_eq!(
        r.assignment_report.read_count(),
        r.data.assignments.values().map(Vec::len).sum::<usize>() + a.len()
    );
}

#[test]
fn duplicate_keyword_global_id_skips_later_row_and_subtree() {
    let dir = tempfile::tempdir().unwrap();
    let path = small(dir.path());
    // Rome (4, child Colosseum 5) repeats the global id of Italy (3);
    // Ocean (10) repeats the one of Sea (9).
    set_keyword_global_id(&path, 4, "zyx-dup-kw-a").unwrap();
    set_keyword_global_id(&path, 3, "zyx-dup-kw-a").unwrap();
    set_keyword_global_id(&path, 10, "zyx-dup-kw-b").unwrap();
    set_keyword_global_id(&path, 9, "zyx-dup-kw-b").unwrap();
    let r = read(&path);
    let rs = reasons(&r.keyword_report);
    for want in [
        "keyword 4: duplicate global id",
        "keyword 5: parent keyword 4 is skipped",
        "keyword 10: duplicate global id",
    ] {
        assert!(rs.contains(&want.to_string()), "missing {want:?} in {rs:?}");
    }
    assert!(
        !rs.iter()
            .any(|s| s.starts_with("keyword 3:") || s.starts_with("keyword 9:"))
    );
    assert!(rs.iter().all(|s| !s.contains("zyx")), "{rs:?}");
    let ids: BTreeSet<_> = r.data.keywords.iter().map(kgid).collect();
    assert_eq!(ids.len(), r.data.keywords.len());
    assert_eq!(
        r.keyword_report.read_count(),
        r.data.keywords.len() + rs.len()
    );
}

fn expect_missing_column(table: &str) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("damaged.lrcat");
    lrcat_fixtures::write_small_catalog_without(&path, table, Some("id_global")).unwrap();
    match Catalog::open(&path) {
        Err(ReadError::MissingSchema { table: t, column }) => {
            assert_eq!(t, table);
            assert_eq!(column.as_deref(), Some("id_global"));
        }
        Err(other) => panic!("expected MissingSchema, got {other:?}"),
        Ok(_) => panic!("open must fail without {table}.id_global"),
    }
}

#[test]
fn photo_global_id_column_is_required() {
    expect_missing_column("Adobe_images");
}

#[test]
fn keyword_global_id_column_is_required() {
    expect_missing_column("AgLibraryKeyword");
}
