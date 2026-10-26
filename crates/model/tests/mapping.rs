//! Acceptance tests for path mapping (principle 6, task P4-4).
//!
//! # Expected API (crate `model`, module `mapping`, re-exported at the root)
//!
//! ```ignore
//! pub struct PathMapping { /* from, to */ }
//! impl PathMapping {
//!     pub fn new(from: impl Into<String>, to: impl Into<String>)
//!         -> Result<PathMapping, ModelError>;   // empty `from` is an error
//!     pub fn parse(spec: &str) -> Result<PathMapping, ModelError>; // "OLD=NEW"
//! }
//! pub struct PathMappings { /* list */ }
//! impl PathMappings {
//!     pub fn new(rules: Vec<PathMapping>) -> PathMappings;
//!     pub fn apply(&self, path: &Path) -> Option<PathBuf>; // None = no rule matched
//!     pub fn map(&self, path: &Path) -> PathBuf;           // original if no match
//! }
//! ```
//!
//! Matching is purely string based, so results are identical on every OS.
//! All comparisons go through `to_str()` strings.

use model::{PathMapping, PathMappings, Photo, Rating};
use std::path::Path;

fn rules(pairs: &[(&str, &str)]) -> PathMappings {
    PathMappings::new(
        pairs
            .iter()
            .map(|(a, b)| PathMapping::new(*a, *b).unwrap())
            .collect(),
    )
}

fn applied(m: &PathMappings, p: &str) -> Option<String> {
    m.apply(Path::new(p))
        .map(|r| r.to_str().unwrap().to_string())
}

fn mapped(m: &PathMappings, p: &str) -> String {
    m.map(Path::new(p)).to_str().unwrap().to_string()
}

#[test]
fn macos_to_linux() {
    let m = rules(&[("/Users/example/Pictures", "/mnt/photos")]);
    assert_eq!(
        applied(&m, "/Users/example/Pictures/2024/IMG_0001.CR3").as_deref(),
        Some("/mnt/photos/2024/IMG_0001.CR3")
    );
}

#[test]
fn windows_to_linux_uses_forward_slashes_in_rest() {
    let m = rules(&[("C:\\data\\Pictures", "/srv/photos")]);
    assert_eq!(
        applied(&m, "C:\\data\\Pictures\\2024\\IMG_0001.CR3").as_deref(),
        Some("/srv/photos/2024/IMG_0001.CR3")
    );
}

#[test]
fn macos_to_windows_uses_backslashes_in_rest() {
    let m = rules(&[("/Users/example/Pictures", "D:\\photos")]);
    assert_eq!(
        applied(&m, "/Users/example/Pictures/2024/IMG_0001.CR3").as_deref(),
        Some("D:\\photos\\2024\\IMG_0001.CR3")
    );
}

#[test]
fn windows_to_other_drive() {
    let m = rules(&[("C:\\data\\photos", "E:\\archive")]);
    assert_eq!(
        applied(&m, "C:\\data\\photos\\2024\\a.jpg").as_deref(),
        Some("E:\\archive\\2024\\a.jpg")
    );
}

#[test]
fn slash_styles_are_equivalent_when_matching() {
    let m = rules(&[("C:\\data\\photos", "/srv/photos")]);
    assert_eq!(
        applied(&m, "C:/data/photos/2024/a.jpg").as_deref(),
        Some("/srv/photos/2024/a.jpg")
    );
    let m = rules(&[("/data/Pictures", "/mnt/photos")]);
    assert_eq!(
        applied(&m, "\\data\\Pictures\\a.jpg").as_deref(),
        Some("/mnt/photos/a.jpg")
    );
}

#[test]
fn matches_only_at_component_boundaries() {
    let m = rules(&[("/Users/example/Pictures", "/mnt/e")]);
    assert_eq!(applied(&m, "/Users/example/Pictures2/x.jpg"), None);
    assert_eq!(
        applied(&m, "/Users/example/Pictures/x.jpg").as_deref(),
        Some("/mnt/e/x.jpg")
    );
}

#[test]
fn prefix_itself_matches() {
    let m = rules(&[("/Users/example/Pictures", "/mnt/e")]);
    assert_eq!(
        applied(&m, "/Users/example/Pictures").as_deref(),
        Some("/mnt/e")
    );
}

#[test]
fn drive_letter_case_is_ignored_but_rest_is_exact() {
    let m = rules(&[("c:\\data", "/srv/data")]);
    assert_eq!(
        applied(&m, "C:\\data\\a.jpg").as_deref(),
        Some("/srv/data/a.jpg")
    );
    let m = rules(&[("C:\\data", "/srv/data")]);
    assert_eq!(
        applied(&m, "c:\\data\\a.jpg").as_deref(),
        Some("/srv/data/a.jpg")
    );
    // Case elsewhere matters.
    assert_eq!(applied(&m, "C:\\Data\\a.jpg"), None);
}

#[test]
fn trailing_separator_is_irrelevant() {
    for (from, to) in [
        ("/Users/example/Pictures/", "/mnt/photos"),
        ("/Users/example/Pictures", "/mnt/photos/"),
        ("/Users/example/Pictures/", "/mnt/photos/"),
    ] {
        let m = rules(&[(from, to)]);
        assert_eq!(
            applied(&m, "/Users/example/Pictures/2024/a.jpg").as_deref(),
            Some("/mnt/photos/2024/a.jpg"),
            "rule {from:?} -> {to:?}"
        );
    }
}

#[test]
fn longest_prefix_wins_regardless_of_order() {
    let short = ("/Users/example/Pictures", "/mnt/photos");
    let long = ("/Users/example/Pictures/2024", "/mnt/new2024");
    for order in [[short, long], [long, short]] {
        let m = rules(&order);
        assert_eq!(
            applied(&m, "/Users/example/Pictures/2024/a.jpg").as_deref(),
            Some("/mnt/new2024/a.jpg")
        );
        assert_eq!(
            applied(&m, "/Users/example/Pictures/2023/a.jpg").as_deref(),
            Some("/mnt/photos/2023/a.jpg")
        );
    }
}

#[test]
fn no_matching_rule_returns_none_and_map_keeps_original() {
    let m = rules(&[("/Users/example/Pictures", "/mnt/photos")]);
    assert_eq!(applied(&m, "/Volumes/other/a.jpg"), None);
    assert_eq!(mapped(&m, "/Volumes/other/a.jpg"), "/Volumes/other/a.jpg");
    let empty = PathMappings::new(vec![]);
    assert_eq!(applied(&empty, "/a/b.jpg"), None);
    assert_eq!(mapped(&empty, "C:\\a\\b.jpg"), "C:\\a\\b.jpg");
}

#[test]
fn map_applies_matching_rule() {
    let m = rules(&[("/Users/example/Pictures", "/mnt/photos")]);
    assert_eq!(
        mapped(&m, "/Users/example/Pictures/a.jpg"),
        "/mnt/photos/a.jpg"
    );
}

#[test]
fn parse_splits_at_first_equals_sign() {
    let m = PathMapping::parse("/a=/b=c").unwrap();
    assert_eq!(m, PathMapping::new("/a", "/b=c").unwrap());
    let ms = PathMappings::new(vec![m]);
    assert_eq!(applied(&ms, "/a/x.jpg").as_deref(), Some("/b=c/x.jpg"));
}

#[test]
fn parse_windows_spec() {
    let m = PathMapping::parse("C:\\data\\Pictures=/srv/photos").unwrap();
    assert_eq!(
        m,
        PathMapping::new("C:\\data\\Pictures", "/srv/photos").unwrap()
    );
}

#[test]
fn parse_rejects_bad_specs() {
    assert!(PathMapping::parse("/only/old").is_err(), "missing '='");
    assert!(PathMapping::parse("=/new").is_err(), "empty old part");
    assert!(PathMapping::parse("").is_err());
}

#[test]
fn new_rejects_empty_from() {
    assert!(PathMapping::new("", "/mnt/photos").is_err());
}

#[test]
fn mapping_photos_keeps_id_and_rating() {
    let m = rules(&[("/Users/example/Pictures", "/mnt/photos")]);
    let photos = [
        Photo::new("/Users/example/Pictures/a.jpg", 7, Rating::new(3).unwrap()),
        Photo::new("/Volumes/other/b.jpg", 8, Rating::new(0).unwrap()),
    ];
    let out: Vec<Photo> = photos
        .iter()
        .map(|p| Photo::new(m.map(&p.path), p.lightroom_id, p.rating))
        .collect();
    assert_eq!(out.len(), photos.len());
    assert_eq!(out[0].path.to_str().unwrap(), "/mnt/photos/a.jpg");
    assert_eq!(out[0].lightroom_id, 7);
    assert_eq!(out[0].rating.get(), 3);
    assert_eq!(out[1].path.to_str().unwrap(), "/Volumes/other/b.jpg");
    assert_eq!(out[1].lightroom_id, 8);
    assert_eq!(out[1].rating.get(), 0);
}

#[test]
fn mapping_is_pure_and_deterministic() {
    // No file system access: the paths do not exist anywhere.
    let m = rules(&[("/Users/example/Pictures", "/mnt/photos")]);
    let a = mapped(&m, "/Users/example/Pictures/nope/none.jpg");
    let b = mapped(&m, "/Users/example/Pictures/nope/none.jpg");
    assert_eq!(a, b);
    assert_eq!(a, "/mnt/photos/nope/none.jpg");
}
