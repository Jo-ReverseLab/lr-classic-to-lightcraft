//! Acceptance tests for the pick/reject flag in the model.
//!
//! Expected API:
//! - `pub enum Flag { None, Pick, Reject }` in `model`, deriving at least
//!   `Debug, Clone, Copy, PartialEq, Eq, Default`; the default is `None`.
//! - `model::Photo` gets `pub flag: Flag`. `Photo::new` sets `Flag::None`;
//!   `with_flag(self, Flag) -> Self` sets it. `PartialEq` stays derived, so
//!   photos with different flags differ.

use model::{Flag, Photo, Rating};

fn photo() -> Photo {
    Photo::new("/Users/example/Pictures/a.jpg", 1, Rating::new(3).unwrap())
}

#[test]
fn default_flag_is_none() {
    assert_eq!(Flag::default(), Flag::None);
}

#[test]
fn new_photo_has_no_flag() {
    assert_eq!(photo().flag, Flag::None);
}

#[test]
fn with_flag_sets_the_flag_and_keeps_the_rest() {
    let base = photo().with_global_id("g-1");
    for flag in [Flag::None, Flag::Pick, Flag::Reject] {
        let p = base.clone().with_flag(flag);
        assert_eq!(p.flag, flag);
        assert_eq!(p.lightroom_id, 1);
        assert_eq!(p.rating, Rating::new(3).unwrap());
        assert_eq!(p.global_id.as_deref(), Some("g-1"));
    }
}

#[test]
fn photos_with_different_flags_differ() {
    assert_ne!(photo().with_flag(Flag::Pick), photo());
    assert_ne!(
        photo().with_flag(Flag::Reject),
        photo().with_flag(Flag::Pick)
    );
    assert_eq!(photo().with_flag(Flag::Pick), photo().with_flag(Flag::Pick));
}
