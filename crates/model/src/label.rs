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

    /// The (name, colour) pairs in this set, for collision checks and tests.
    /// The names are the exact stored strings (no normalisation).
    pub fn entries(&self) -> impl Iterator<Item = (&str, ColorLabel)> {
        self.names.iter().map(|(n, l)| (n.as_str(), *l))
    }

    /// Every standard set that `auto` recognises, in the order `auto`
    /// consults them. Today these are the English and German sets only.
    ///
    /// French and Spanish sets are **not** included yet: their names are
    /// pending a citable Adobe source (see `docs/research/label-names.md`).
    /// When a cited set is added here it is automatically covered by the
    /// `no_standard_name_maps_to_two_colors` collision test, and must be
    /// added to the `auto` chain in `crates/lr2lc/src/labels.rs` at the same
    /// time so the two stay in step.
    pub fn standard_sets() -> Vec<LabelSet> {
        vec![Self::en(), Self::de()]
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

    #[test]
    fn no_standard_name_maps_to_two_colors() {
        // A name that belongs to two colours across the standard sets would
        // make `auto` ambiguous. This guard fails the moment such a collision
        // enters the table, so a future French or Spanish set cannot silently
        // clash with an existing name. Matching is exact, so the check is over
        // the precise stored strings with no normalisation.
        let sets = LabelSet::standard_sets();
        let mut seen: HashMap<&str, ColorLabel> = HashMap::new();
        for set in &sets {
            for (name, label) in set.entries() {
                if let Some(prev) = seen.get(name) {
                    assert_eq!(
                        *prev, label,
                        "standard label name {name:?} maps to two colours"
                    );
                } else {
                    seen.insert(name, label);
                }
            }
        }
    }
}
