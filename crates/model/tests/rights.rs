//! Acceptance tests for copyright and creator in the model (task P5b-7b).
//!
//! Switched off with `cfg(any())` until the API exists; the developer removes
//! that line and the `ignore` attributes.
//!
//! Expected API: `model::Photo` gets `pub copyright: Option<String>` and
//! `pub creator: Option<String>`. `Photo::new` sets both to `None`;
//! `with_copyright` and `with_creator` (`impl Into<String>`) set `Some(..)`
//! and keep the text byte for byte, independently of each other and of the
//! caption.

use model::{Photo, Rating};

fn photo() -> Photo {
    Photo::new("/Users/example/Pictures/a.jpg", 1, Rating::new(3).unwrap())
}

#[test]
fn new_photo_has_neither() {
    let p = photo();
    assert_eq!((p.copyright, p.creator), (None, None));
}

#[test]
fn builders_keep_text_byte_for_byte_and_are_independent() {
    let c = "  \u{a9} K\u{f6}ln \u{1f305} \n";
    let a = " Example M\u{fc}ller \u{1f4f7} ";
    let p = photo()
        .with_copyright(c)
        .with_creator(a)
        .with_caption("cap");
    assert_eq!(p.copyright.as_deref(), Some(c));
    assert_eq!(p.creator.as_deref(), Some(a));
    assert_eq!(p.caption.as_deref(), Some("cap"));
    let only = photo().with_creator(a);
    assert_eq!(only.copyright, None);
}
