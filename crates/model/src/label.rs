//! Colour labels and the name sets that map label text to them.

use std::collections::HashMap;

/// One of the five colour labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorLabel {
    /// Red.
    Red,
    /// Yellow.
    Yellow,
    /// Green.
    Green,
    /// Blue.
    Blue,
    /// Purple.
    Purple,
}

impl ColorLabel {
    /// All labels in their usual order.
    pub const ALL: [ColorLabel; 5] = [
        ColorLabel::Red,
        ColorLabel::Yellow,
        ColorLabel::Green,
        ColorLabel::Blue,
        ColorLabel::Purple,
    ];
}

/// Maps label names (as stored in a catalog) to colour labels.
///
/// The lookup is exact, byte for byte: no case folding, no trimming, no
/// Unicode normalisation and no fallback to another set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelSet {
    names: HashMap<String, ColorLabel>,
}

impl LabelSet {
    fn from_names(names: [&str; 5]) -> LabelSet {
        LabelSet {
            names: names
                .iter()
                .zip(ColorLabel::ALL)
                .map(|(n, l)| (n.to_string(), l))
                .collect(),
        }
    }

    /// The English names: Red, Yellow, Green, Blue, Purple.
    pub fn en() -> LabelSet {
        Self::from_names(["Red", "Yellow", "Green", "Blue", "Purple"])
    }

    /// The German names: Rot, Gelb, Grün (NFC), Blau, Lila.
    pub fn de() -> LabelSet {
        Self::from_names(["Rot", "Gelb", "Gr\u{fc}n", "Blau", "Lila"])
    }

    /// A set with user-defined names.
    pub fn custom(names: HashMap<String, ColorLabel>) -> LabelSet {
        LabelSet { names }
    }

    /// The label for `name`, or `None` if the set does not contain it.
    pub fn resolve(&self, name: &str) -> Option<ColorLabel> {
        self.names.get(name).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_name_is_not_resolved() {
        assert_eq!(LabelSet::en().resolve(""), None);
        assert_eq!(LabelSet::de().resolve(""), None);
    }

    #[test]
    fn german_green_is_nfc() {
        assert_eq!(LabelSet::de().resolve("Gr\u{fc}n"), Some(ColorLabel::Green));
        assert_eq!(LabelSet::de().resolve("Gru\u{308}n"), None);
    }
}
