//! Acceptance tests for the title in the model (task P5b-7c).
//!
//! Switched off with `cfg(any())` until the API exists; the developer removes
//! that line.
//!
//! Expected API: `model::Photo` gets `pub title: Option<String>`.
//! `Photo::new` sets it to `None`; `with_title` (`impl Into<String>`) sets
//! `Some(..)`, keeps the text byte for byte and is independent of caption,
//! copyright and creator.

use model::{Photo, Rating};

fn photo() -> Photo {
    Photo::new("/Users/example/Pictures/a.jpg", 1, Rating::new(3).unwrap())
}

#[test]
fn new_photo_has_no_title() {
    assert_eq!(photo().title, None);
}

#[test]
fn title_is_kept_byte_for_byte_and_independent() {
    let t = " K\u{f6}ln \u{1f305} & <b> \n";
    let p = photo()
        .with_title(t)
        .with_caption("cap")
        .with_copyright("c")
        .with_creator("a");
    assert_eq!(p.title.as_deref(), Some(t));
    assert_eq!(p.caption.as_deref(), Some("cap"));
    assert_eq!(p.copyright.as_deref(), Some("c"));
    assert_eq!(p.creator.as_deref(), Some("a"));
    let only = photo().with_caption("cap");
    assert_eq!(only.title, None);
}
