//! Acceptance tests for skipping videos in the reader (LightCraft cannot read videos).
//!
//! Expected behaviour (no new API; `Catalog::photos` changes):
//! - A photo is a video if `Adobe_images.fileFormat` is `VIDEO`, or if the
//!   file extension is a video extension, compared case-insensitively (at
//!   least mov, mp4, m4v, avi, mts, m2ts, 3gp, mpg, mpeg, wmv, mkv).
//!   Only the extension column counts, never a dot inside the base name.
//! - `Catalog::photos` still calls `report.read()` once per row and skips each
//!   video with exactly the reason `photo {id}: video files are not supported
//!   by LightCraft` (ids only, no names or paths). Videos do not reach
//!   `drop_missing_originals`, so a missing video is a video, not a missing
//!   original (photo 2). The file system is not accessed by `photos`.
//! - Order of checks: virtual copy first (a virtual copy of a video keeps the
//!   "virtual copy" reason, photo 21), then video, then the other reasons.
//! - Keyword assignments to videos are skipped like assignments to any other
//!   skipped photo (they are not passed to `Catalog::keywords`).
//! - Fixture: `lrcat_fixtures::write_video_catalog_with_files`, counts in
//!   `lrcat_fixtures::video_edge`.

use lr_reader::{Catalog, checksum, drop_missing_originals};
use lrcat_fixtures::{small, video_edge, write_video_catalog_with_files};
use model::{Photo, Report};
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

const VIDEO_REASON: &str = "video files are not supported by LightCraft";

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_video_catalog_with_files(dir.path()).unwrap();
    (dir, cat)
}

fn read_photos(path: &Path) -> (Vec<Photo>, Report) {
    let catalog = Catalog::open(path).unwrap();
    let mut report = Report::new();
    let photos = catalog.photos(&mut report).expect("read photos");
    (photos, report)
}

fn ids(photos: &[Photo]) -> Vec<i64> {
    photos.iter().map(|p| p.lightroom_id).collect()
}

fn reasons(report: &Report) -> Vec<String> {
    report
        .skipped_items()
        .iter()
        .map(|s| s.reason.clone())
        .collect()
}

fn reason_for(report: &Report, id: i64) -> Vec<String> {
    let prefix = format!("photo {id}:");
    reasons(report)
        .into_iter()
        .filter(|r| r.starts_with(&prefix))
        .collect()
}

#[test]
fn fixture_has_the_documented_shape() {
    let (_dir, cat) = fixture();
    let c = Connection::open_with_flags(&cat, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let n: i64 = c
        .query_row("SELECT COUNT(*) FROM Adobe_images", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, video_edge::PHOTOS);
    let fmt = |id: i64| -> (String, String) {
        c.query_row(
            "SELECT i.fileFormat, f.extension FROM Adobe_images i \
             JOIN AgLibraryFile f ON f.id_local = i.rootFile WHERE i.id_local = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    };
    assert_eq!(fmt(4), ("VIDEO".into(), "mov".into()));
    assert_eq!(fmt(5).1, "MP4");
    assert_ne!(fmt(5).0, "VIDEO");
    assert_ne!(fmt(6).0, "VIDEO");
    assert_eq!(fmt(6).1, "mpo");
}

#[test]
fn every_row_is_read_and_each_video_is_skipped_with_its_reason() {
    let (_dir, cat) = fixture();
    let (photos, report) = read_photos(&cat);
    assert_eq!(report.read_count() as i64, video_edge::PHOTOS);
    assert_eq!(report.converted_count(), 0);
    for id in video_edge::VIDEO_IDS {
        assert_eq!(
            reason_for(&report, id),
            vec![format!("photo {id}: {VIDEO_REASON}")],
            "video {id}"
        );
        assert!(
            !ids(&photos).contains(&id),
            "video {id} must not be returned"
        );
    }
}

#[test]
fn video_with_fileformat_video_is_detected() {
    let (_dir, cat) = fixture();
    let (_, report) = read_photos(&cat);
    assert_eq!(reason_for(&report, 4).len(), 1);
}

#[test]
fn video_is_detected_by_upper_case_extension_alone() {
    let (_dir, cat) = fixture();
    let (_, report) = read_photos(&cat);
    let got = reason_for(&report, video_edge::VIDEO_BY_EXTENSION_ID);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].contains(VIDEO_REASON));
}

#[test]
fn missing_video_counts_as_video_not_as_missing_original() {
    let (_dir, cat) = fixture();
    let (photos, report) = read_photos(&cat);
    assert!(!ids(&photos).contains(&video_edge::VIDEO_MISSING_ID));
    let got = reason_for(&report, video_edge::VIDEO_MISSING_ID);
    assert_eq!(got.len(), 1);
    assert!(got[0].contains(VIDEO_REASON) && !got[0].contains("missing"));
    // The later existence check never sees it.
    let mut later = Report::new();
    let kept = drop_missing_originals(photos, &mut later);
    assert!(reason_for(&later, video_edge::VIDEO_MISSING_ID).is_empty());
    assert!(!ids(&kept).contains(&video_edge::VIDEO_MISSING_ID));
}

#[test]
fn lookalikes_are_not_videos() {
    let (_dir, cat) = fixture();
    let (photos, report) = read_photos(&cat);
    for id in video_edge::LOOKALIKE_IDS {
        assert!(ids(&photos).contains(&id), "photo {id} is not a video");
        assert!(reason_for(&report, id).is_empty(), "photo {id}");
    }
}

#[test]
fn virtual_copy_of_a_video_keeps_the_virtual_copy_reason() {
    let (_dir, cat) = fixture();
    let (_, report) = read_photos(&cat);
    let got = reason_for(&report, 21);
    assert_eq!(got.len(), 1);
    assert!(got[0].contains("virtual copy") && !got[0].contains("video files"));
}

#[test]
fn only_the_defined_videos_carry_the_video_reason() {
    let (_dir, cat) = fixture();
    let (photos, report) = read_photos(&cat);
    let video_skips = reasons(&report)
        .iter()
        .filter(|r| r.contains(VIDEO_REASON))
        .count();
    assert_eq!(video_skips, video_edge::VIDEO_IDS.len());
    // The three videos plus the virtual copy 21 of video 4. The virtual copy
    // 20 of photo 3 converts with its master (T03).
    assert_eq!(report.skipped_count(), video_edge::VIDEO_IDS.len() + 1);
    assert_eq!(report.read_count(), photos.len() + report.skipped_count());
}

#[test]
fn reasons_contain_no_names_or_paths() {
    let (_dir, cat) = fixture();
    let (_, report) = read_photos(&cat);
    for r in reasons(&report) {
        for bad in ["IMG_", "clip", ".mov", ".MP4", "/", "\\"] {
            assert!(!r.contains(bad), "reason leaks {bad}: {r}");
        }
    }
}

#[test]
fn full_pipeline_balances_and_missing_original_is_only_photo_three() {
    let (_dir, cat) = fixture();
    let (photos, mut report) = read_photos(&cat);
    let kept = drop_missing_originals(photos, &mut report);
    for _ in &kept {
        report.converted();
    }
    assert_eq!(kept.len() as i64, video_edge::CONVERTED);
    assert_eq!(report.skipped_count() as i64, video_edge::SKIPPED);
    assert!(report.is_balanced());
    let missing: Vec<i64> = video_edge::MISSING_ORIGINAL_IDS.to_vec();
    for id in missing {
        assert!(reason_for(&report, id)[0].contains("missing"));
    }
}

#[test]
fn keyword_assignments_to_videos_are_skipped_like_other_skipped_photos() {
    let (_dir, cat) = fixture();
    let (photos, mut photo_report) = read_photos(&cat);
    let kept = drop_missing_originals(photos, &mut photo_report);
    let c = Connection::open_with_flags(&cat, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let on_videos: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM AgLibraryKeywordImage WHERE image IN (2, 4, 5)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(on_videos > 0, "fixture must assign keywords to videos");
    let catalog = Catalog::open(&cat).unwrap();
    let mut keywords = Report::new();
    let mut assignments = Report::new();
    let data = catalog
        .keywords(&kept, &mut keywords, &mut assignments)
        .unwrap();
    for id in video_edge::VIDEO_IDS {
        assert!(!data.assignments.contains_key(&id), "video {id}");
    }
    let video_skips = assignments
        .skipped_items()
        .iter()
        .filter(|s| {
            video_edge::VIDEO_IDS.iter().any(|id| {
                s.reason.starts_with("assignment ")
                    && s.reason.contains(&format!("photo {id} is skipped"))
            })
        })
        .count() as i64;
    assert_eq!(video_skips, on_videos);
    let total: i64 = c
        .query_row("SELECT COUNT(*) FROM AgLibraryKeywordImage", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(assignments.read_count() as i64, total);
    let _ = small::ASSIGNMENTS;
}

#[test]
fn catalog_is_unchanged_after_reading() {
    let (_dir, cat) = fixture();
    let before = checksum(&cat).unwrap();
    let _ = read_photos(&cat);
    assert_eq!(before, checksum(&cat).unwrap());
}
