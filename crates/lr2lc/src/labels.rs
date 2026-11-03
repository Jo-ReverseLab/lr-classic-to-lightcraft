//! Choosing a colour label set and mapping the catalog's label text to
//! colour labels.
//!
//! Label text is user data: it never appears in messages or reports.

use model::{ColorLabel, LabelSet, Photo};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The `--label-set` value as given on the command line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LabelChoice {
    /// Detect the English or German set; guess nothing otherwise.
    #[default]
    Auto,
    En,
    De,
    /// A JSON file mapping label names to colours.
    File(PathBuf),
}

impl LabelChoice {
    /// Parses the option value; anything but `en`, `de` and `auto` is a file.
    pub fn parse(value: &str) -> LabelChoice {
        match value {
            "auto" => LabelChoice::Auto,
            "en" => LabelChoice::En,
            "de" => LabelChoice::De,
            _ => LabelChoice::File(PathBuf::from(value)),
        }
    }
}

/// A label set ready to use, after loading a file if one was given.
#[derive(Debug, Clone)]
pub enum LabelMode {
    Auto,
    /// A fixed set and its name in the report (`en`, `de` or `file`).
    Fixed(LabelSet, &'static str),
}

impl LabelMode {
    /// Loads the set for `choice`. Errors are usage errors; their text
    /// names the file but never its content.
    pub fn load(choice: &LabelChoice) -> Result<LabelMode, String> {
        Ok(match choice {
            LabelChoice::Auto => LabelMode::Auto,
            LabelChoice::En => LabelMode::Fixed(LabelSet::en(), "en"),
            LabelChoice::De => LabelMode::Fixed(LabelSet::de(), "de"),
            LabelChoice::File(path) => LabelMode::Fixed(load_file(path)?, "file"),
        })
    }

    /// The label set name written to `report.json`: `auto` for the combined
    /// set, otherwise `en`, `de` or `file`.
    pub fn name(&self) -> &'static str {
        match self {
            LabelMode::Auto => "auto",
            LabelMode::Fixed(_, name) => name,
        }
    }
}

/// Reads a label set file: a JSON object `{"<name>": "<colour>"}`.
fn load_file(path: &Path) -> Result<LabelSet, String> {
    let shown = path.display();
    let text = std::fs::read_to_string(path).map_err(|e| {
        format!("--label-set {shown}: not en, de, auto or a readable label set file ({e})")
    })?;
    parse_set(&text).map_err(|e| format!("--label-set {shown}: {e}"))
}

/// Parses the content of a label set file.
fn parse_set(text: &str) -> Result<LabelSet, String> {
    // serde_json errors only give line and column, never the content.
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("not valid JSON ({e})"))?;
    let object = value
        .as_object()
        .ok_or("must be a JSON object of label names and colours")?;
    let mut names = HashMap::new();
    for (index, (name, colour)) in object.iter().enumerate() {
        let label = colour.as_str().and_then(colour_from_str).ok_or_else(|| {
            format!(
                "entry {} must have one of the colours red, yellow, green, blue, purple",
                index + 1
            )
        })?;
        names.insert(name.clone(), label);
    }
    Ok(LabelSet::custom(names))
}

/// The colour named exactly `red`, `yellow`, `green`, `blue` or `purple`.
fn colour_from_str(s: &str) -> Option<ColorLabel> {
    match s {
        "red" => Some(ColorLabel::Red),
        "yellow" => Some(ColorLabel::Yellow),
        "green" => Some(ColorLabel::Green),
        "blue" => Some(ColorLabel::Blue),
        "purple" => Some(ColorLabel::Purple),
        _ => None,
    }
}

/// Label counts over the converted photos. The seven counters add up to
/// the converted photos.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LabelCounts {
    pub red: usize,
    pub yellow: usize,
    pub green: usize,
    pub blue: usize,
    pub purple: usize,
    /// Photos without a label (empty or NULL).
    pub none: usize,
    /// Photos whose label is not in the set; converted without a label.
    pub custom: usize,
    /// `auto`, `en`, `de` or `file`.
    pub set: &'static str,
    /// Ids of the custom photos, ascending.
    pub custom_ids: Vec<i64>,
    /// Under `auto`, the standard sets whose names matched at least one
    /// converted photo, in the order `en`, `de`. Empty for a fixed set.
    pub matched_sets: Vec<&'static str>,
}

/// Sets the label of each photo from `raw` (`(id, text)` pairs of
/// non-empty label text) and counts the result.
///
/// Only labels of the given photos count, so labels of skipped photos
/// never influence the automatic detection.
///
/// With `auto`, each label is matched against the English and German
/// standard names together: a name that equals one of them exactly maps to
/// its colour, whatever the other labels are. The standard names do not
/// overlap, so a name matches at most one set, and `matched_sets` records
/// which set(s) were used. Any other non-empty label is a custom label of
/// its own photo. Nothing else is guessed.
pub fn apply(photos: &mut [Photo], raw: &[(i64, String)], mode: &LabelMode) -> LabelCounts {
    let ids: HashSet<i64> = photos.iter().map(|p| p.lightroom_id).collect();
    let text: HashMap<i64, &str> = raw
        .iter()
        .filter(|(id, t)| ids.contains(id) && !t.is_empty())
        .map(|(id, t)| (*id, t.as_str()))
        .collect();
    let fixed_set = match mode {
        LabelMode::Fixed(set, _) => Some(set.clone()),
        LabelMode::Auto => None,
    };
    let en = LabelSet::en();
    let de = LabelSet::de();
    let mut counts = LabelCounts {
        set: mode.name(),
        ..LabelCounts::default()
    };
    let mut matched_en = false;
    let mut matched_de = false;
    for photo in photos.iter_mut() {
        photo.label = None;
        let Some(t) = text.get(&photo.lightroom_id) else {
            counts.none += 1;
            continue;
        };
        let resolved = match mode {
            LabelMode::Auto => en
                .resolve(t)
                .inspect(|_| matched_en = true)
                .or_else(|| de.resolve(t).inspect(|_| matched_de = true)),
            LabelMode::Fixed(..) => fixed_set.as_ref().and_then(|s| s.resolve(t)),
        };
        match resolved {
            Some(label) => {
                photo.label = Some(label);
                *counts.slot(label) += 1;
            }
            None => {
                counts.custom += 1;
                counts.custom_ids.push(photo.lightroom_id);
            }
        }
    }
    counts.custom_ids.sort_unstable();
    if matches!(mode, LabelMode::Auto) {
        let mut matched = Vec::new();
        if matched_en {
            matched.push("en");
        }
        if matched_de {
            matched.push("de");
        }
        counts.matched_sets = matched;
    }
    counts
}

impl LabelCounts {
    fn slot(&mut self, label: ColorLabel) -> &mut usize {
        match label {
            ColorLabel::Red => &mut self.red,
            ColorLabel::Yellow => &mut self.yellow,
            ColorLabel::Green => &mut self.green,
            ColorLabel::Blue => &mut self.blue,
            ColorLabel::Purple => &mut self.purple,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::Rating;

    fn photos(ids: &[i64]) -> Vec<Photo> {
        ids.iter()
            .map(|&id| Photo::new(format!("p{id}.jpg"), id, Rating::default()))
            .collect()
    }

    fn raw(pairs: &[(i64, &str)]) -> Vec<(i64, String)> {
        pairs.iter().map(|(id, t)| (*id, t.to_string())).collect()
    }

    #[test]
    fn choice_parses_names_and_files() {
        assert_eq!(LabelChoice::parse("auto"), LabelChoice::Auto);
        assert_eq!(LabelChoice::parse("en"), LabelChoice::En);
        assert_eq!(LabelChoice::parse("de"), LabelChoice::De);
        assert_eq!(
            LabelChoice::parse("EN"),
            LabelChoice::File(PathBuf::from("EN"))
        );
    }

    #[test]
    fn auto_maps_german_names_together() {
        let mut p = photos(&[1, 2, 3]);
        let c = apply(&mut p, &raw(&[(1, "Rot"), (2, "Blau")]), &LabelMode::Auto);
        assert_eq!(
            (c.red, c.blue, c.none, c.custom, c.set),
            (1, 1, 1, 0, "auto")
        );
        assert_eq!(c.matched_sets, ["de"]);
        assert_eq!(p[0].label, Some(ColorLabel::Red));
    }

    #[test]
    fn auto_maps_mixed_english_and_german_together() {
        let mut p = photos(&[1, 2]);
        let c = apply(&mut p, &raw(&[(2, "Rot"), (1, "Red")]), &LabelMode::Auto);
        assert_eq!((c.red, c.custom, c.set), (2, 0, "auto"));
        assert_eq!(c.matched_sets, ["en", "de"]);
        assert!(p.iter().all(|p| p.label == Some(ColorLabel::Red)));
    }

    #[test]
    fn auto_without_labels_reports_auto_and_no_match() {
        let mut p = photos(&[1]);
        let c = apply(&mut p, &[], &LabelMode::Auto);
        assert_eq!((c.none, c.set), (1, "auto"));
        assert!(c.matched_sets.is_empty());
    }

    #[test]
    fn auto_treats_unknown_names_as_custom_with_no_match() {
        let mut p = photos(&[1]);
        let c = apply(&mut p, &raw(&[(1, "Pinkish")]), &LabelMode::Auto);
        assert_eq!((c.custom, c.none, c.set), (1, 0, "auto"));
        assert!(c.matched_sets.is_empty());
        assert_eq!(c.custom_ids, [1]);
        assert!(p[0].label.is_none());
    }

    #[test]
    fn auto_does_not_match_nfd_gruen() {
        // The German set stores NFC `Grün`; a decomposed form stays custom.
        let mut p = photos(&[1]);
        let c = apply(&mut p, &raw(&[(1, "Gru\u{308}n")]), &LabelMode::Auto);
        assert_eq!(c.custom, 1);
        assert!(c.matched_sets.is_empty());
    }

    #[test]
    fn labels_of_other_photos_are_ignored() {
        let mut p = photos(&[1]);
        let c = apply(&mut p, &raw(&[(1, "Red"), (9, "Rot")]), &LabelMode::Auto);
        assert_eq!((c.red, c.set), (1, "auto"));
        assert_eq!(c.matched_sets, ["en"]);
    }

    #[test]
    fn fixed_set_has_no_matched_sets() {
        let mut p = photos(&[1]);
        let mode = LabelMode::Fixed(LabelSet::en(), "en");
        let c = apply(&mut p, &raw(&[(1, "Rot")]), &mode);
        assert_eq!((c.custom, c.set), (1, "en"));
        assert!(c.matched_sets.is_empty());
    }

    #[test]
    fn set_file_is_parsed_strictly() {
        let s = parse_set(r#"{"A": "red", "B": "purple"}"#).unwrap();
        assert_eq!(s.resolve("A"), Some(ColorLabel::Red));
        assert_eq!(s.resolve("B"), Some(ColorLabel::Purple));
        assert!(parse_set("{}").is_ok());
        for bad in ["", "[]", "1", r#"{"A": "Red"}"#, r#"{"A": 1}"#, "{"] {
            assert!(parse_set(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn set_file_errors_do_not_show_names() {
        let err = parse_set(r#"{"Secret-Name": "pink"}"#).unwrap_err();
        assert!(!err.contains("Secret"), "{err}");
        assert!(!err.contains("pink"), "{err}");
    }
}
