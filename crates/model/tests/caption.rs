//! Acceptance tests for captions in the model (task P5b-7).
//!
//! Switched off with `cfg(any())` until the API exists; the developer removes
//! that line and the `ignore` attributes.
//!
//! Expected API: `model::Photo` gets `pub caption: Option<String>`.
//! `Photo::new` sets `None`; `with_caption(self, impl Into<String>) -> Self`
//! sets `Some(..)` and keeps the text byte for byte.
#![cfg(any())]

use model::{Photo, Rating};

fn photo() -> Photo {
    Photo::new("/Users/example/Pictures/a.jpg", 1, Rating::new(3).unwrap())
}

#[test]
#[ignore = "waiting for P5b-7"]
fn new_photo_has_no_caption() {
    assert_eq!(photo().caption, None);
}

#[test]
#[ignore = "waiting for P5b-7"]
fn with_caption_keeps_text_byte_for_byte() {
    let text = "  Sonnenuntergang \u{fc}ber K\u{f6}ln \u{1f305} \n";
    assert_eq!(photo().with_caption(text).caption.as_deref(), Some(text));
}
