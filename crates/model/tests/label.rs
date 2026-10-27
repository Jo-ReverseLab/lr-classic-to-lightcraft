//! Acceptance tests for colour labels in the model (task P5b-2).
//!
//! This file does not compile before the API exists, so it is switched off
//! with the `cfg(any())` line below. The developer removes that line (and the
//! `ignore` attributes) once the API is in place.
//!
//! Expected API in `model`:
//! - `pub enum ColorLabel { Red, Yellow, Green, Blue, Purple }`, deriving at
//!   least `Debug, Clone, Copy, PartialEq, Eq, Hash`.
//! - `model::Photo` gets `pub label: Option<ColorLabel>`. `Photo::new` sets
//!   `None`; `with_label(self, ColorLabel) -> Self` sets `Some(..)`.
//! - `pub struct LabelSet` with
//!   - `LabelSet::en()`: Red, Yellow, Green, Blue, Purple;
//!   - `LabelSet::de()`: Rot, Gelb, Grün (NFC, U+00FC), Blau, Lila;
//!   - `LabelSet::custom(map: HashMap<String, ColorLabel>) -> LabelSet`;
//!   - `resolve(&self, name: &str) -> Option<ColorLabel>`: exact lookup, byte
//!     for byte. No case folding, no trimming, no Unicode normalisation, no
//!     fallback to another set. Unknown names and the empty string give None.

#![cfg(any())]

use model::{ColorLabel, LabelSet, Photo, Rating};
use std::collections::HashMap;

const ALL: [ColorLabel; 5] = [
    ColorLabel::Red,
    ColorLabel::Yellow,
    ColorLabel::Green,
    ColorLabel::Blue,
    ColorLabel::Purple,
];

fn photo() -> Photo {
    Photo::new("/Users/example/Pictures/a.jpg", 1, Rating::new(3).unwrap())
}

#[test]
#[ignore = "waiting for P5b-2"]
fn new_photo_has_no_label() {
    assert_eq!(photo().label, None);
}

#[test]
#[ignore = "waiting for P5b-2"]
fn with_label_sets_the_label_and_keeps_the_rest() {
    let base = photo().with_global_id("g-1");
    for label in ALL {
        let p = base.clone().with_label(label);
        assert_eq!(p.label, Some(label));
        assert_eq!(p.lightroom_id, 1);
        assert_eq!(p.global_id.as_deref(), Some("g-1"));
    }
}

#[test]
#[ignore = "waiting for P5b-2"]
fn photos_with_different_labels_differ() {
    assert_ne!(photo().with_label(ColorLabel::Red), photo());
    assert_ne!(
        photo().with_label(ColorLabel::Red),
        photo().with_label(ColorLabel::Blue)
    );
    assert_eq!(
        photo().with_label(ColorLabel::Red),
        photo().with_label(ColorLabel::Red)
    );
}

#[test]
#[ignore = "waiting for P5b-2"]
fn english_set_resolves_exact_names() {
    let s = LabelSet::en();
    for (name, want) in ["Red", "Yellow", "Green", "Blue", "Purple"].iter().zip(ALL) {
        assert_eq!(s.resolve(name), Some(want), "{name}");
    }
}

#[test]
#[ignore = "waiting for P5b-2"]
fn german_set_resolves_exact_names() {
    let s = LabelSet::de();
    for (name, want) in ["Rot", "Gelb", "Gr\u{fc}n", "Blau", "Lila"].iter().zip(ALL) {
        assert_eq!(s.resolve(name), Some(want), "{name}");
    }
}

#[test]
#[ignore = "waiting for P5b-2"]
fn sets_do_not_fall_back_to_each_other() {
    assert_eq!(LabelSet::en().resolve("Rot"), None);
    assert_eq!(LabelSet::de().resolve("Red"), None);
    assert_eq!(LabelSet::de().resolve("Purple"), None);
}

#[test]
#[ignore = "waiting for P5b-2"]
fn lookup_is_byte_exact() {
    let en = LabelSet::en();
    for name in ["red", "RED", "Red ", " Red", "Red\n", "", "Rd", "red "] {
        assert_eq!(en.resolve(name), None, "{name:?}");
    }
    let de = LabelSet::de();
    // NFD u + combining diaeresis is not the NFC name.
    assert_eq!(de.resolve("Gru\u{308}n"), None);
    assert_eq!(de.resolve("gr\u{fc}n"), None);
    assert_eq!(de.resolve("Gr\u{fc}n"), Some(ColorLabel::Green));
    assert_eq!(de.resolve("Gruen"), None);
}

#[test]
#[ignore = "waiting for P5b-2"]
fn custom_set_uses_only_its_own_names() {
    let mut m = HashMap::new();
    m.insert("Zyx-Label-Custom".to_string(), ColorLabel::Purple);
    m.insert("Red".to_string(), ColorLabel::Blue);
    m.insert("Gru\u{308}n".to_string(), ColorLabel::Green);
    let s = LabelSet::custom(m);
    assert_eq!(s.resolve("Zyx-Label-Custom"), Some(ColorLabel::Purple));
    // Exact lookup in the given map; the built-in meaning of "Red" is gone.
    assert_eq!(s.resolve("Red"), Some(ColorLabel::Blue));
    assert_eq!(s.resolve("Yellow"), None);
    assert_eq!(s.resolve("Gru\u{308}n"), Some(ColorLabel::Green));
    assert_eq!(s.resolve("Gr\u{fc}n"), None);
    assert_eq!(s.resolve(""), None);
}

#[test]
#[ignore = "waiting for P5b-2"]
fn empty_custom_set_resolves_nothing() {
    let s = LabelSet::custom(HashMap::new());
    assert_eq!(s.resolve("Red"), None);
}
