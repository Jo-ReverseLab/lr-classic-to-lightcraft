//! Acceptance tests for reading photos.
//!
//! Expected API:
//! - `Catalog::photos(&self, report: &mut model::Report)
//!   -> Result<Vec<model::Photo>, ReadError>`.
//!   Reads `Adobe_images`, `AgLibraryFile`, `AgLibraryFolder` and
//!   `AgLibraryRootFolder`. Every `Adobe_images` row calls `report.read()`
//!   exactly once. The path is the folder path (as in `Catalog::folders`) plus
//!   `baseName` and `.extension` (no dot when the extension is empty), built
//!   from stored strings only. `lightroom_id` is `Adobe_images.id_local`; a
//!   NULL rating becomes 0. Skipped with a reason (photo not returned, no
//!   abort): virtual copies (reason contains "virtual copy"), a missing file,
//!   folder or root folder row (reason names what is missing), a rating
//!   outside 0..=5 (reason contains "rating"). `photos` never calls
//!   `report.converted()`; the writer does that later. Order: by `id_local`.
//! - `lr_reader::drop_missing_originals(photos: Vec<model::Photo>,
//!   report: &mut model::Report) -> Vec<model::Photo>`. Photos whose file does
//!   not exist go to `report.skipped` (reason contains "missing") and leave
//!   the list. Files are only checked for existence. No `report.read()`.

use lr_reader::{Catalog, checksum, drop_missing_originals};
use lrcat_fixtures::small;
use model::{Photo, Report};
use rusqlite::Connection;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Rating of the virtual copy (photo 20) in the generator.
const VIRTUAL_COPY_RATING: u8 = 5;
const VIRTUAL_COPY_ID: i64 = 20;

fn with_files(dir: &Path) -> PathBuf {
    lrcat_fixtures::write_small_catalog_with_files(dir).expect("generate fixture")
}

fn without_files(dir: &Path) -> PathBuf {
    let path = dir.join("small.lrcat");
    lrcat_fixtures::write_small_catalog(&path).expect("generate fixture");
    path
}

fn read_photos(path: &Path) -> (Vec<Photo>, Report) {
    let catalog = Catalog::open(path).unwrap();
    let mut report = Report::new();
    let photos = catalog.photos(&mut report).expect("read photos");
    (photos, report)
}

fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn ids(photos: &[Photo]) -> Vec<i64> {
    photos.iter().map(|p| p.lightroom_id).collect()
}

#[test]
fn every_row_is_read_and_virtual_copy_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let (photos, report) = read_photos(&with_files(dir.path()));
    assert_eq!(report.read_count() as i64, small::PHOTOS);
    assert_eq!(report.skipped_count() as i64, small::VIRTUAL_COPIES);
    assert_eq!(photos.len() as i64, small::PHOTOS - small::VIRTUAL_COPIES);
    assert!(!ids(&photos).contains(&VIRTUAL_COPY_ID));
    let reason = &report.skipped_items()[0].reason;
    assert!(reason.contains("virtual copy"), "reason: {reason}");
}

#[test]
fn photos_does_not_mark_anything_converted() {
    let dir = tempfile::tempdir().unwrap();
    let (_, report) = read_photos(&with_files(dir.path()));
    assert_eq!(report.converted_count(), 0);
}

#[test]
fn read_equals_returned_plus_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let (photos, report) = read_photos(&with_files(dir.path()));
    assert_eq!(report.read_count(), photos.len() + report.skipped_count());
}

#[test]
fn photos_are_ordered_by_lightroom_id_and_ids_are_unique() {
    let dir = tempfile::tempdir().unwrap();
    let (photos, _) = read_photos(&with_files(dir.path()));
    let got = ids(&photos);
    let mut sorted = got.clone();
    sorted.sort();
    assert_eq!(got, sorted);
    let unique: HashSet<_> = got.iter().collect();
    assert_eq!(unique.len(), got.len());
}

#[test]
fn ratings_match_the_fixture_counts() {
    let dir = tempfile::tempdir().unwrap();
    let (photos, _) = read_photos(&with_files(dir.path()));
    let count = |r: u8| photos.iter().filter(|p| p.rating.get() == r).count() as i64;
    // NULL and 0 both map to 0 stars.
    assert_eq!(count(0), small::RATING_NULL + small::RATING_ZERO);
    for stars in 1..=5_u8 {
        // The virtual copy is not part of the result.
        let skipped_here = i64::from(stars == VIRTUAL_COPY_RATING);
        assert_eq!(
            count(stars),
            small::RATING_STARS[(stars - 1) as usize] - skipped_here,
            "photos with {stars} stars"
        );
    }
}

#[test]
fn same_file_name_in_different_folders_gives_different_paths() {
    let dir = tempfile::tempdir().unwrap();
    let (photos, _) = read_photos(&with_files(dir.path()));
    let twins: Vec<String> = photos
        .iter()
        .filter(|p| norm(&p.path).ends_with("/IMG_0001.jpg"))
        .map(|p| norm(&p.path))
        .collect();
    assert_eq!(twins.len(), 2);
    assert_ne!(twins[0], twins[1]);
}

#[test]
fn paths_lie_below_their_root_folders() {
    let dir = tempfile::tempdir().unwrap();
    let (photos, _) = read_photos(&with_files(dir.path()));
    let roots = [
        norm(&dir.path().join(small::MACOS_ROOT_DIR)),
        norm(&dir.path().join(small::WINDOWS_ROOT_DIR)),
    ];
    for p in &photos {
        let s = norm(&p.path);
        assert!(
            roots.iter().any(|r| s.starts_with(&format!("{r}/"))),
            "path {s:?} is not below a root folder"
        );
        assert!(s.ends_with(".jpg"), "path {s:?} lacks the extension");
    }
}

#[test]
fn photos_without_files_still_read_with_stored_paths() {
    let dir = tempfile::tempdir().unwrap();
    let (photos, report) = read_photos(&without_files(dir.path()));
    assert_eq!(photos.len() as i64, small::PHOTOS - small::VIRTUAL_COPIES);
    assert_eq!(report.read_count() as i64, small::PHOTOS);
    let paths: Vec<String> = photos.iter().map(|p| norm(&p.path)).collect();
    assert!(
        paths
            .iter()
            .any(|p| p.starts_with("/Users/example/Pictures/"))
    );
    assert!(
        paths
            .iter()
            .any(|p| p.starts_with("C:/Users/example/Pictures/"))
    );
}

#[test]
fn missing_originals_are_dropped_with_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let (photos, mut report) = read_photos(&with_files(dir.path()));
    let read_before = report.read_count();
    let kept = drop_missing_originals(photos, &mut report);
    let missing_non_copies = small::MISSING_PHOTO_IDS.len() as i64 - small::VIRTUAL_COPIES;
    assert_eq!(
        kept.len() as i64,
        small::PHOTOS - small::VIRTUAL_COPIES - missing_non_copies
    );
    for id in small::MISSING_FILE_IDS {
        assert!(!ids(&kept).contains(&id), "photo {id} should be dropped");
    }
    assert_eq!(report.skipped_count(), small::MISSING_PHOTO_IDS.len());
    assert_eq!(report.read_count(), read_before, "no extra read() calls");
    let missing_reasons = report
        .skipped_items()
        .iter()
        .filter(|s| s.reason.contains("missing"))
        .count();
    assert_eq!(missing_reasons as i64, missing_non_copies);
}

#[test]
fn numbers_add_up_after_conversion() {
    let dir = tempfile::tempdir().unwrap();
    let (photos, mut report) = read_photos(&with_files(dir.path()));
    let kept = drop_missing_originals(photos, &mut report);
    assert_eq!(report.read_count(), kept.len() + report.skipped_count());
    for _ in &kept {
        report.converted();
    }
    assert!(report.is_balanced());
}

#[test]
fn without_files_every_photo_is_dropped_as_missing() {
    let dir = tempfile::tempdir().unwrap();
    let (photos, mut report) = read_photos(&without_files(dir.path()));
    let kept = drop_missing_originals(photos, &mut report);
    assert!(kept.is_empty());
    assert_eq!(report.skipped_count() as i64, small::PHOTOS);
    assert!(report.is_balanced());
}

#[test]
fn dropping_missing_originals_leaves_the_files_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = with_files(dir.path());
    let snapshot = |root: &Path| -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "jpg") {
                    out.push((p.clone(), std::fs::read(&p).unwrap()));
                }
            }
        }
        out.sort();
        out
    };
    let before = snapshot(dir.path());
    let (photos, mut report) = read_photos(&path);
    let _ = drop_missing_originals(photos, &mut report);
    assert_eq!(before, snapshot(dir.path()));
}

#[test]
fn reading_photos_does_not_change_the_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let path = with_files(dir.path());
    let before = checksum(&path).unwrap();
    {
        let (photos, mut report) = read_photos(&path);
        let _ = drop_missing_originals(photos, &mut report);
    }
    assert_eq!(before, checksum(&path).unwrap());
}

#[test]
fn reading_twice_gives_identical_results() {
    let dir = tempfile::tempdir().unwrap();
    let path = with_files(dir.path());
    let (a, _) = read_photos(&path);
    let (b, _) = read_photos(&path);
    assert_eq!(ids(&a), ids(&b));
    let pa: Vec<_> = a.iter().map(|p| p.path.clone()).collect();
    let pb: Vec<_> = b.iter().map(|p| p.path.clone()).collect();
    assert_eq!(pa, pb);
}

// --- Broken rows: edit a copy of the fixture with SQL, then read it. ---

/// Applies `sql` to a fresh fixture and returns the catalog path.
fn broken_catalog(dir: &Path, sql: &str) -> PathBuf {
    let path = with_files(dir);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(sql).unwrap();
    drop(conn);
    path
}

fn assert_one_extra_skip(sql: &str, photo_ids: &[i64], reason_part: &str) {
    let dir = tempfile::tempdir().unwrap();
    let path = broken_catalog(dir.path(), sql);
    let (photos, report) = read_photos(&path);
    assert_eq!(report.read_count() as i64, small::PHOTOS);
    for id in photo_ids {
        assert!(!ids(&photos).contains(id), "photo {id} should be skipped");
    }
    assert_eq!(
        report.skipped_count(),
        small::VIRTUAL_COPIES as usize + photo_ids.len()
    );
    assert_eq!(report.read_count(), photos.len() + report.skipped_count());
    assert!(
        report
            .skipped_items()
            .iter()
            .any(|s| s.reason.to_lowercase().contains(reason_part)),
        "no skip reason mentions {reason_part:?}"
    );
}

#[test]
fn missing_file_row_skips_only_that_photo() {
    // Photo 5 uses file 5; remove the file row.
    assert_one_extra_skip(
        "DELETE FROM AgLibraryFile WHERE id_local = 5;",
        &[5],
        "file",
    );
}

#[test]
fn missing_folder_row_skips_only_that_photo() {
    // File 7 points to a folder that does not exist.
    assert_one_extra_skip(
        "UPDATE AgLibraryFile SET folder = 999 WHERE id_local = 7;",
        &[7],
        "folder",
    );
}

#[test]
fn missing_root_folder_row_skips_photos_in_that_folder() {
    // Folder 6 (long path) loses its root folder; photos 6, 12, 18, 19 use it.
    assert_one_extra_skip(
        "UPDATE AgLibraryFolder SET rootFolder = 999 WHERE id_local = 6;",
        &[6, 12, 18, 19],
        "root",
    );
}

#[test]
fn rating_out_of_range_skips_only_that_photo() {
    assert_one_extra_skip(
        "UPDATE Adobe_images SET rating = 7 WHERE id_local = 8;",
        &[8],
        "rating",
    );
}

#[test]
fn negative_rating_is_skipped_too() {
    assert_one_extra_skip(
        "UPDATE Adobe_images SET rating = -1 WHERE id_local = 9;",
        &[9],
        "rating",
    );
}
