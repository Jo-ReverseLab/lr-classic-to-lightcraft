//! Acceptance tests for videos in the CLI dry run (task P5-6, user decision
//! F-013: LightCraft cannot read videos, so they are skipped with their own
//! reason and counted separately).
//!
//! Expected behaviour of `lr2lc <CATALOG> [--report FILE]` on the video
//! fixture (`lrcat_fixtures::write_video_catalog_with_files`):
//! - Photos: read 21, would convert 15, skipped 6 (3 videos, 1 missing
//!   original, 2 virtual copies). Read = converted + skipped.
//! - Reason of each video: `photo {id}: video files are not supported by
//!   LightCraft` (ids only). A missing video is a video; a virtual copy of a
//!   video keeps the virtual copy reason.
//! - report.json: `photos.videos` is the number of skipped videos (an
//!   integer, 3 here, 0 when there are none). Skipped videos are also listed
//!   in `photos.skipped`; `videos` is a subset of it, so the balance
//!   `read == converted + len(skipped)` is unchanged.
//! - stdout: a line `videos: skipped N` (always printed, also with N = 0) and
//!   the usual grouped reason line `N x video files are not supported by
//!   LightCraft` under `skipped photos:`.
//! - Keyword assignments to videos count as skipped assignments.
//! - Without `--apply` nothing is written; the catalog is unchanged; a second
//!   run gives identical output.

use lrcat_fixtures::{
    assignments_to_photo, small, video_edge, write_small_catalog_with_files,
    write_video_catalog_with_files,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const VIDEO_REASON: &str = "video files are not supported by LightCraft";

fn run_report(cat: &Path) -> (String, serde_json::Value, String) {
    let cwd = tempfile::tempdir().unwrap();
    let report = cwd.path().join("report.json");
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd.path())
        .arg(cat)
        .arg("--report")
        .arg(&report)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let raw = fs::read_to_string(&report).unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        serde_json::from_str(&raw).unwrap(),
        raw,
    )
}

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_video_catalog_with_files(dir.path()).unwrap();
    (dir, cat)
}

fn skipped_reasons(v: &serde_json::Value, area: &str) -> Vec<String> {
    v[area]["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["reason"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn photo_counts_add_up_with_videos() {
    let (_d, cat) = fixture();
    let (text, v, _) = run_report(&cat);
    let want = format!(
        "photos: read {}, would convert {}, skipped {}",
        video_edge::PHOTOS,
        video_edge::CONVERTED,
        video_edge::SKIPPED
    );
    assert!(text.lines().any(|l| l.trim() == want), "stdout: {text}");
    assert_eq!(v["photos"]["read"], video_edge::PHOTOS);
    assert_eq!(v["photos"]["converted"], video_edge::CONVERTED);
    assert_eq!(
        v["photos"]["skipped"].as_array().unwrap().len() as i64,
        video_edge::SKIPPED
    );
}

#[test]
fn report_json_counts_videos_separately() {
    let (_d, cat) = fixture();
    let (_, v, _) = run_report(&cat);
    assert_eq!(v["photos"]["videos"], video_edge::VIDEO_IDS.len());
    let reasons = skipped_reasons(&v, "photos");
    let video_reasons = reasons.iter().filter(|r| r.contains(VIDEO_REASON)).count();
    assert_eq!(video_reasons, video_edge::VIDEO_IDS.len());
}

#[test]
fn stdout_has_video_line_and_grouped_reason() {
    let (_d, cat) = fixture();
    let (text, _, _) = run_report(&cat);
    let want = format!("videos: skipped {}", video_edge::VIDEO_IDS.len());
    assert!(text.lines().any(|l| l.trim() == want), "stdout: {text}");
    let group = format!("{} x {VIDEO_REASON}", video_edge::VIDEO_IDS.len());
    assert!(text.lines().any(|l| l.trim() == group), "stdout: {text}");
}

#[test]
fn each_video_reason_names_the_id_only() {
    let (_d, cat) = fixture();
    let (text, v, raw) = run_report(&cat);
    let reasons = skipped_reasons(&v, "photos");
    for id in video_edge::VIDEO_IDS {
        assert!(
            reasons.contains(&format!("photo {id}: {VIDEO_REASON}")),
            "video {id}: {reasons:?}"
        );
    }
    for bad in [
        "IMG_0004",
        "IMG_0005",
        "clip",
        ".mov",
        ".MP4",
        "videos.lrcat",
    ] {
        assert!(!raw.contains(bad), "report.json leaks {bad}");
        assert!(!text.contains(bad), "stdout leaks {bad}");
    }
}

#[test]
fn missing_video_is_a_video_and_virtual_copy_keeps_its_reason() {
    let (_d, cat) = fixture();
    let (_, v, _) = run_report(&cat);
    let reasons = skipped_reasons(&v, "photos");
    let for_id = |id: i64| -> Vec<&String> {
        reasons
            .iter()
            .filter(|r| r.starts_with(&format!("photo {id}:")))
            .collect()
    };
    let missing_video = for_id(video_edge::VIDEO_MISSING_ID);
    assert_eq!(missing_video.len(), 1);
    assert!(missing_video[0].contains(VIDEO_REASON));
    let copy = for_id(21);
    assert_eq!(copy.len(), 1);
    assert!(copy[0].contains("virtual copy") && !copy[0].contains("video files"));
    let missing = for_id(3);
    assert_eq!(missing.len(), 1);
    assert!(missing[0].contains("missing"));
    let missing_reasons = reasons.iter().filter(|r| r.contains("missing")).count();
    assert_eq!(missing_reasons, video_edge::MISSING_ORIGINAL_IDS.len());
}

#[test]
fn lookalike_files_are_converted() {
    let (_d, cat) = fixture();
    let (_, v, _) = run_report(&cat);
    let reasons = skipped_reasons(&v, "photos");
    for id in video_edge::LOOKALIKE_IDS {
        assert!(
            !reasons
                .iter()
                .any(|r| r.starts_with(&format!("photo {id}:"))),
            "photo {id} must be converted"
        );
    }
}

#[test]
fn keyword_assignments_to_videos_are_skipped_and_balance_holds() {
    let (_d, cat) = fixture();
    let (_, v, _) = run_report(&cat);
    let on_videos: i64 = video_edge::VIDEO_IDS
        .iter()
        .map(|id| assignments_to_photo(&cat, *id).unwrap())
        .sum();
    assert!(on_videos > 0);
    let reasons = skipped_reasons(&v, "keyword_assignments");
    for id in video_edge::VIDEO_IDS {
        let n = reasons
            .iter()
            .filter(|r| r.contains(&format!("photo {id} is skipped")))
            .count() as i64;
        let want = assignments_to_photo(&cat, id).unwrap();
        assert_eq!(n, want, "assignments to video {id}");
    }
    let a = &v["keyword_assignments"];
    assert_eq!(
        a["read"].as_i64().unwrap(),
        a["converted"].as_i64().unwrap() + reasons.len() as i64
    );
}

#[test]
fn small_catalog_without_videos_reports_zero() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_small_catalog_with_files(dir.path()).unwrap();
    let (text, v, _) = run_report(&cat);
    assert_eq!(v["photos"]["videos"], 0);
    assert!(
        text.lines().any(|l| l.trim() == "videos: skipped 0"),
        "{text}"
    );
    assert!(!text.contains(VIDEO_REASON));
    assert_eq!(v["photos"]["read"], small::PHOTOS);
}

#[test]
fn catalog_unchanged_second_run_identical_and_nothing_written() {
    let (_d, cat) = fixture();
    let before = fs::read(&cat).unwrap();
    let dir = cat.parent().unwrap().to_path_buf();
    let listing = |p: &Path| {
        let mut v: Vec<_> = fs::read_dir(p)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        v.sort();
        v
    };
    let files_before = listing(&dir);
    let (text1, v1, _) = run_report(&cat);
    let (text2, v2, _) = run_report(&cat);
    assert_eq!(text1, text2);
    assert_eq!(v1, v2);
    assert_eq!(v1["catalog_unchanged"], true);
    assert_eq!(before, fs::read(&cat).unwrap());
    assert_eq!(files_before, listing(&dir));
}

#[test]
fn apply_is_still_refused_and_writes_nothing() {
    let (_d, cat) = fixture();
    let before = fs::read(&cat).unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd.path())
        .arg(&cat)
        .arg("--apply")
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    assert_eq!(before, fs::read(&cat).unwrap());
    assert_eq!(fs::read_dir(cwd.path()).unwrap().count(), 0);
}
