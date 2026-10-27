//! Acceptance tests for the collection model (task P5b-3a).
//!
//! Expected API (new in `crates/model/src/lib.rs`):
//!
//! ```ignore
//! #[derive(Debug, Clone, PartialEq)]
//! pub struct Collection {
//!     pub lightroom_id: i64,
//!     pub name: String,
//!     pub kind: CollectionKind,
//!     pub parent: Option<i64>,   // lightroom_id of the parent folder
//!     pub members: Vec<i64>,     // lightroom_ids of photos, in order
//! }
//! #[derive(Debug, Clone, Copy, PartialEq, Eq)]
//! pub enum CollectionKind { Album, Folder }
//! ```

use model::{Collection, CollectionKind};

fn album(id: i64, name: &str, parent: Option<i64>, members: Vec<i64>) -> Collection {
    Collection {
        lightroom_id: id,
        name: name.to_string(),
        kind: CollectionKind::Album,
        parent,
        members,
    }
}

#[test]
fn collection_keeps_its_fields() {
    let c = album(7, "Trip", Some(3), vec![5, 1, 9]);
    assert_eq!(c.lightroom_id, 7);
    assert_eq!(c.name, "Trip");
    assert_eq!(c.kind, CollectionKind::Album);
    assert_eq!(c.parent, Some(3));
    assert_eq!(c.members, vec![5, 1, 9], "member order is kept");
}

#[test]
fn kinds_differ_and_collections_compare_by_value() {
    assert_ne!(CollectionKind::Album, CollectionKind::Folder);
    let a = album(1, "A", None, vec![]);
    assert_eq!(a, a.clone());
    let mut b = a.clone();
    b.kind = CollectionKind::Folder;
    assert_ne!(a, b);
}

#[test]
fn names_are_kept_byte_for_byte() {
    for name in [
        "S\u{fc}\u{df}e",
        "Gru\u{308}n ",
        "\u{1f305} Sunrise",
        "",
        " x ",
    ] {
        let c = album(1, name, None, vec![]);
        assert_eq!(c.name.as_bytes(), name.as_bytes());
    }
}
