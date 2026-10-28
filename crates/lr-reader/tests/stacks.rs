//! Acceptance tests for reading folder stacks into the model (task P5b-5),
//! on the stack fixture (`lrcat_fixtures::stack_edge`).
//!
//! Expected API (the first line below, `#![cfg(any())]`, switches these
//! tests off until the developer removes it):
//!
//! ```ignore
//! // model
//! pub struct Stack { pub lightroom_id: i64, pub members: Vec<i64>, pub collapsed: bool }
//! // lr-reader (re-exported from lib.rs)
//! pub struct StackData {
//!     pub stacks: Vec<model::Stack>,   // converted, ascending by lightroom_id
//!     pub report: model::Report,       // one entry per AgLibraryFolderStack row
//!     pub members: model::Report,      // one entry per AgLibraryFolderStackImage row
//!     pub collapsed: usize,            // converted stacks with collapsed != 0
//!     pub no_top: usize,               // converted stacks without a member at position 1
//! }
//! impl Catalog {
//!     /// `photos` are the converted photos; other members are skipped.
//!     pub fn stack_data(&self, photos: &[model::Photo]) -> Result<StackData, ReadError>;
//! }
//! ```
//!
//! Rules under test:
//! - `members` are Lightroom photo ids: the photo at `position` 1 first,
//!   then by `position` (REAL or INTEGER), then by row id.
//! - A member whose photo is not converted (virtual copy, video, unknown id)
//!   is skipped; the reason names the photo id.
//! - Fewer than two converted members: the stack is skipped, reason
//!   "fewer than two converted photos" with the stack id. Its member rows
//!   count as skipped as well (members counts only rows of converted stacks).
//! - A photo in two stacks stays in the one with the smaller id; in the
//!   other the row is skipped, reason "already in stack N" with the photo id.
//! - No member at position 1: the first by position is the top, counted in
//!   `no_top`.
//! - `collapsed` counts converted stacks whose `collapsed` is not 0; it is
//!   not part of the stacks' identity or reasons (F-019 point 9).
//! - Missing stack tables: no stacks, no error, empty reports.
//! - Reasons name ids, never anything else. The catalog stays unchanged.
//!   Both reports balance: read == converted + skipped.

#![cfg(any())]

use lr_reader::{Catalog, StackData, checksum};
use lrcat_fixtures::stack_edge as e;
use model::{Photo, Report};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn fixture(dir: &Path) -> PathBuf {
    lrcat_fixtures::write_stack_catalog_with_files(dir).expect("generate fixture")
}

fn read(path: &Path) -> StackData {
    let catalog = Catalog::open(path).unwrap();
    let photos: Vec<Photo> = catalog.photos(&mut Report::new()).unwrap();
    catalog.stack_data(&photos).expect("read stacks")
}

fn numbers(text: &str) -> BTreeSet<i64> {
    text.split(|c: char| !c.is_ascii_digit())
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect()
}

fn reasons(report: &Report) -> Vec<String> {
    report
        .skipped_items()
        .iter()
        .map(|s| s.reason.to_lowercase())
        .collect()
}

fn has_reason(report: &Report, phrase: &str, id: i64) -> bool {
    reasons(report)
        .iter()
        .any(|r| r.contains(phrase) && numbers(r).contains(&id))
}

fn stack(d: &StackData, id: i64) -> &model::Stack {
    d.stacks
        .iter()
        .find(|s| s.lightroom_id == id)
        .unwrap_or_else(|| panic!("stack {id} not converted"))
}

#[test]
fn counts_balance_and_stacks_are_ascending() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    assert_eq!(d.report.read_count() as i64, e::STACKS);
    assert_eq!(d.stacks.len() as i64, e::CONVERTED);
    assert_eq!(d.report.skipped_count() as i64, e::SKIPPED);
    assert_eq!(
        d.report.read_count(),
        d.stacks.len() + d.report.skipped_count()
    );
    assert_eq!(d.members.read_count() as i64, e::MEMBER_ROWS);
    let kept: usize = d.stacks.iter().map(|s| s.members.len()).sum();
    assert_eq!(kept as i64, e::MEMBERS_CONVERTED);
    assert_eq!(d.members.skipped_count() as i64, e::MEMBERS_SKIPPED);
    assert_eq!(
        d.members.read_count(),
        kept + d.members.skipped_count(),
        "members balance"
    );
    let ids: Vec<i64> = d.stacks.iter().map(|s| s.lightroom_id).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(ids, sorted, "ascending, no stack twice");
}

#[test]
fn members_are_in_order_top_first() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    for (id, members) in e::STACK_MEMBERS {
        assert_eq!(stack(&d, id).members, members, "stack {id}");
    }
}

#[test]
fn real_and_integer_positions_both_sort() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    // Stack 1: REAL positions, rows inserted as (7, 6). Stack 2: INTEGER.
    assert_eq!(stack(&d, 1).members, [6, 7]);
    assert_eq!(stack(&d, 2).members, [9, 10, 8]);
}

#[test]
fn every_photo_is_in_at_most_one_stack() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    let mut seen = BTreeSet::new();
    for s in &d.stacks {
        assert!(s.members.len() >= 2, "stack {}", s.lightroom_id);
        for m in &s.members {
            assert!(seen.insert(*m), "photo {m} twice");
        }
    }
}

#[test]
fn virtual_copy_member_is_skipped_and_the_small_stack_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    assert!(has_reason(&d.members, "", e::VIRTUAL_COPY_MEMBER));
    for id in e::SKIPPED_STACKS {
        assert!(
            has_reason(&d.report, "fewer than two converted photos", id),
            "stack {id}: {:?}",
            reasons(&d.report)
        );
        assert!(d.stacks.iter().all(|s| s.lightroom_id != id));
    }
}

#[test]
fn video_members_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    for video in e::VIDEO_MEMBERS {
        assert!(has_reason(&d.members, "", video), "video {video}");
        assert!(
            d.stacks.iter().all(|s| !s.members.contains(&video)),
            "video {video} in a stack"
        );
    }
    // Stack 5 keeps its two photos although its third member is a video.
    assert_eq!(stack(&d, 5).members, [13, 14]);
}

#[test]
fn unknown_photo_member_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    assert!(has_reason(&d.members, "", e::UNKNOWN_MEMBER));
    assert_eq!(stack(&d, 8).members, [19, 1]);
}

#[test]
fn photo_in_two_stacks_stays_in_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    assert!(
        stack(&d, e::DOUBLE_FIRST)
            .members
            .contains(&e::DOUBLE_PHOTO)
    );
    assert!(
        !stack(&d, e::DOUBLE_SECOND)
            .members
            .contains(&e::DOUBLE_PHOTO)
    );
    let r = reasons(&d.members);
    assert!(
        r.iter().any(|r| r.contains("already in stack")
            && numbers(r).contains(&e::DOUBLE_FIRST)
            && numbers(r).contains(&e::DOUBLE_PHOTO)),
        "{r:?}"
    );
}

#[test]
fn stack_without_position_one_takes_the_first_by_position() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    assert_eq!(stack(&d, 8).members[0], 19);
    assert_eq!(d.no_top as i64, e::NO_TOP);
}

#[test]
fn collapsed_is_only_counted() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    assert_eq!(d.collapsed as i64, e::COLLAPSED);
    assert!(stack(&d, 2).collapsed);
    assert!(!stack(&d, 1).collapsed);
}

#[test]
fn equal_positions_fall_back_to_row_order() {
    let dir = tempfile::tempdir().unwrap();
    let cat = fixture(dir.path());
    // Stack 1: both photos at position 1 (the top is ambiguous).
    lrcat_fixtures::set_stack_member_position(&cat, 1, 7, 1.0).unwrap();
    let d = read(&cat);
    // Row ids: photo 7 was inserted first, so it comes first.
    assert_eq!(stack(&d, 1).members, [7, 6]);
}

#[test]
fn reasons_carry_ids_only() {
    let dir = tempfile::tempdir().unwrap();
    let d = read(&fixture(dir.path()));
    for r in reasons(&d.report).iter().chain(&reasons(&d.members)) {
        assert!(
            !r.contains(".jpg") && !r.contains('/') && !r.contains('\\'),
            "{r}"
        );
        assert!(!numbers(r).is_empty(), "{r}");
    }
}

#[test]
fn missing_stack_tables_mean_no_stacks() {
    let dir = tempfile::tempdir().unwrap();
    let cat = fixture(dir.path());
    lrcat_fixtures::drop_stack_tables(&cat).unwrap();
    let d = read(&cat);
    assert!(d.stacks.is_empty());
    assert_eq!(d.report.read_count(), 0);
    assert_eq!(d.members.read_count(), 0);
    assert_eq!((d.collapsed, d.no_top), (0, 0));
}

#[test]
fn small_fixture_without_stack_tables_has_no_stacks() {
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_small_catalog_with_files(dir.path()).unwrap();
    let d = read(&cat);
    assert!(d.stacks.is_empty());
    assert_eq!(d.report.read_count(), 0);
}

#[test]
fn empty_stack_tables_mean_no_stacks() {
    let dir = tempfile::tempdir().unwrap();
    let cat = fixture(dir.path());
    let c = rusqlite::Connection::open(&cat).unwrap();
    c.execute_batch(
        "DELETE FROM AgLibraryFolderStackData; DELETE FROM AgLibraryFolderStackImage; DELETE FROM AgLibraryFolderStack;",
    )
    .unwrap();
    drop(c);
    let d = read(&cat);
    assert!(d.stacks.is_empty());
    assert_eq!(d.report.read_count(), 0);
}

#[test]
fn reading_leaves_the_catalog_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let cat = fixture(dir.path());
    let before = checksum(&cat).unwrap();
    let first = read(&cat);
    let second = read(&cat);
    assert_eq!(checksum(&cat).unwrap(), before);
    assert_eq!(first.stacks, second.stacks, "second read gives the same");
}
