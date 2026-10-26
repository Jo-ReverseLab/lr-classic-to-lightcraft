//! Acceptance tests for the neutral data model (crate `model`).
//!
//! # Expected API
//!
//! ```ignore
//! pub struct Folder { pub path: std::path::PathBuf }
//! impl Folder { pub fn new(path: impl Into<PathBuf>) -> Self }
//!
//! pub struct Rating(u8);                       // 0..=5
//! impl Rating {
//!     pub fn new(v: u8) -> Result<Rating, ModelError>; // Err for v > 5
//!     pub fn get(self) -> u8;
//! }
//!
//! pub struct Photo {
//!     pub path: PathBuf,
//!     pub lightroom_id: i64,   // provenance
//!     pub rating: Rating,
//! }
//! impl Photo { pub fn new(path: impl Into<PathBuf>, lightroom_id: i64, rating: Rating) -> Self }
//!
//! pub struct Keyword { /* hierarchy path, e.g. ["Travel","Italy","Rome"] */ }
//! impl Keyword {
//!     pub fn new(path: Vec<String>) -> Self;       // non-empty path
//!     pub fn name(&self) -> &str;                  // last part
//!     pub fn path(&self) -> &[String];
//!     pub fn depth(&self) -> usize;                // 1 for a root keyword
//!     pub fn parent(&self) -> Option<Keyword>;     // None for a root keyword
//!     pub fn is_child_of(&self, other: &Keyword) -> bool; // direct child only
//! }
//!
//! pub struct Report { /* counters and entries */ }
//! impl Report {
//!     pub fn new() -> Self;
//!     pub fn read(&mut self);                      // one element read
//!     pub fn converted(&mut self);                 // one element converted
//!     pub fn skipped(&mut self, reason: impl Into<String>); // one element skipped
//!     pub fn read_count(&self) -> usize;
//!     pub fn converted_count(&self) -> usize;
//!     pub fn skipped_count(&self) -> usize;
//!     pub fn skipped_items(&self) -> &[Skipped];   // Skipped { pub reason: String }
//!     pub fn is_balanced(&self) -> bool;           // read == converted + skipped
//! }
//! ```
//!
//! `Report::skipped` with an empty or whitespace-only reason must still
//! record a non-empty reason (e.g. "unspecified"), so nothing is lost silently.

use model::{Folder, Keyword, Photo, Rating, Report};
use std::path::PathBuf;

fn kw(parts: &[&str]) -> Keyword {
    Keyword::new(parts.iter().map(|s| s.to_string()).collect())
}

#[test]
fn rating_accepts_zero_to_five() {
    for v in 0..=5u8 {
        assert_eq!(Rating::new(v).expect("valid rating").get(), v);
    }
}

#[test]
fn rating_rejects_above_five() {
    assert!(Rating::new(6).is_err());
    assert!(Rating::new(255).is_err());
}

#[test]
fn photo_keeps_lightroom_id_and_rating() {
    let p = Photo::new("/Users/example/Pictures/a.jpg", 42, Rating::new(3).unwrap());
    assert_eq!(p.lightroom_id, 42);
    assert_eq!(p.rating.get(), 3);
    assert_eq!(p.path, PathBuf::from("/Users/example/Pictures/a.jpg"));
}

#[test]
fn photo_without_rating_is_zero() {
    let p = Photo::new("a.jpg", 1, Rating::new(0).unwrap());
    assert_eq!(p.rating.get(), 0);
}

#[test]
fn folder_keeps_path() {
    let f = Folder::new("/Users/example/Pictures");
    assert_eq!(f.path, PathBuf::from("/Users/example/Pictures"));
}

#[test]
fn keyword_depth_and_name() {
    let k = kw(&["Travel", "Italy", "Rome"]);
    assert_eq!(k.depth(), 3);
    assert_eq!(k.name(), "Rome");
    assert_eq!(k.path(), ["Travel", "Italy", "Rome"]);
    assert_eq!(kw(&["Travel"]).depth(), 1);
}

#[test]
fn keyword_parent_chain_ends_at_root() {
    let k = kw(&["Travel", "Italy", "Rome"]);
    let parent = k.parent().expect("has parent");
    assert_eq!(parent, kw(&["Travel", "Italy"]));
    let root = parent.parent().expect("has parent");
    assert_eq!(root, kw(&["Travel"]));
    assert!(root.parent().is_none());
}

#[test]
fn keyword_child_relation_is_direct_only() {
    let root = kw(&["Travel"]);
    let child = kw(&["Travel", "Italy"]);
    let grandchild = kw(&["Travel", "Italy", "Rome"]);
    assert!(child.is_child_of(&root));
    assert!(grandchild.is_child_of(&child));
    assert!(!grandchild.is_child_of(&root));
    assert!(!root.is_child_of(&child));
    assert!(!kw(&["Food", "Italy"]).is_child_of(&root));
}

#[test]
fn keyword_supports_umlauts_and_emoji() {
    let k = kw(&["Städte", "Köln 🎉"]);
    assert_eq!(k.name(), "Köln 🎉");
    assert_eq!(k.depth(), 2);
}

#[test]
fn keyword_supports_deep_hierarchy() {
    let parts: Vec<String> = (0..50).map(|i| format!("level{i}")).collect();
    let k = Keyword::new(parts);
    assert_eq!(k.depth(), 50);
}

#[test]
fn report_balanced_when_read_equals_converted_plus_skipped() {
    let mut r = Report::new();
    for _ in 0..5 {
        r.read();
    }
    r.converted();
    r.converted();
    r.converted();
    r.skipped("file missing");
    r.skipped("unsupported rating");
    assert_eq!(r.read_count(), 5);
    assert_eq!(r.converted_count(), 3);
    assert_eq!(r.skipped_count(), 2);
    assert!(r.is_balanced());
}

#[test]
fn empty_report_is_balanced() {
    assert!(Report::new().is_balanced());
}

#[test]
fn report_unbalanced_when_element_is_lost() {
    let mut r = Report::new();
    r.read();
    r.read();
    r.converted();
    assert!(!r.is_balanced());
}

#[test]
fn report_unbalanced_when_more_handled_than_read() {
    let mut r = Report::new();
    r.read();
    r.converted();
    r.converted();
    assert!(!r.is_balanced());
}

#[test]
fn every_skipped_item_has_non_empty_reason() {
    let mut r = Report::new();
    for _ in 0..3 {
        r.read();
    }
    r.skipped("file missing");
    r.skipped("");
    r.skipped("   ");
    assert_eq!(r.skipped_items().len(), 3);
    assert_eq!(r.skipped_count(), 3);
    for s in r.skipped_items() {
        assert!(!s.reason.trim().is_empty());
    }
    assert!(r.is_balanced());
}
