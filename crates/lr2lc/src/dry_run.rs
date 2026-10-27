//! The dry run: read the catalog, apply path mappings, check originals, read
//! keywords for the converted photos and count what would be converted.
//! Nothing is written except the optional report file.

use crate::labels::{self, LabelCounts, LabelMode};
use anyhow::{Context, Result, bail};
use lr_reader::{Catalog, KeywordData, VIDEO_SKIP_REASON, checksum, drop_missing_originals};
use model::{Flag, Folder, Keyword, PathMappings, Photo, Rating, Report};
use std::collections::HashSet;
use std::path::Path;

/// Result of a dry run.
#[derive(Debug)]
pub struct Outcome {
    pub catalog_version: String,
    pub catalog_unchanged: bool,
    pub folders: Report,
    pub photos: Report,
    /// Photos skipped because they are videos; a subset of the skipped
    /// photos.
    pub videos: usize,
    pub collections: Report,
    pub keywords: Report,
    /// Attribute counts over the converted keywords.
    pub keyword_attributes: KeywordAttributes,
    pub keyword_assignments: Report,
    /// Synonyms of keywords; converted with their keyword.
    pub keyword_synonyms: Report,
    /// Number of converted photos per star rating, index 0 to 5.
    pub ratings: [usize; 6],
    /// Pick/reject counts over the converted photos.
    pub flags: FlagCounts,
    /// Colour label counts over the converted photos.
    pub labels: LabelCounts,
}

/// Number of converted photos per flag. `none` does not include `unknown`,
/// so the four fields add up to the converted photos.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlagCounts {
    pub pick: usize,
    pub reject: usize,
    pub none: usize,
    /// Photos whose stored flag value is not known; converted without flag.
    pub unknown: usize,
}

impl FlagCounts {
    /// Counts the flags of `photos`; ids in `unknown` (sorted ascending)
    /// count as unknown instead of their flag.
    fn count(photos: &[Photo], unknown: &[i64]) -> Self {
        let mut counts = FlagCounts::default();
        for p in photos {
            if unknown.binary_search(&p.lightroom_id).is_ok() {
                counts.unknown += 1;
                continue;
            }
            match p.flag {
                Flag::Pick => counts.pick += 1,
                Flag::Reject => counts.reject += 1,
                Flag::None => counts.none += 1,
            }
        }
        counts
    }
}

/// Counts of keyword attributes, over converted keywords only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeywordAttributes {
    /// Keywords that name a person.
    pub persons: usize,
    /// Keywords not written into exported photos.
    pub not_exported: usize,
    /// Keywords exported without their parents.
    pub no_parents_on_export: usize,
    /// Keywords exported without their synonyms.
    pub no_synonyms_on_export: usize,
    /// Keywords with a type that is neither a plain keyword nor a person.
    pub unknown_type: usize,
    /// Keywords without any converted assignment.
    pub unused: usize,
}

impl KeywordAttributes {
    /// Counts the attributes of the converted keywords in `data`.
    fn count(data: &KeywordData) -> Self {
        let used: HashSet<&Keyword> = data.assignments.values().flatten().collect();
        let n = |f: &dyn Fn(&Keyword) -> bool| data.keywords.iter().filter(|k| f(k)).count();
        KeywordAttributes {
            persons: n(&|k| k.person),
            not_exported: n(&|k| !k.include_on_export),
            no_parents_on_export: n(&|k| !k.export_containing),
            no_synonyms_on_export: n(&|k| !k.export_synonyms),
            unknown_type: data.unknown_type,
            unused: n(&|k| !used.contains(k)),
        }
    }
}

/// Runs the dry run on the catalog at `path`.
///
/// Fails if the catalog cannot be opened or read. A changed checksum is not
/// an error here; the caller decides after writing the report.
pub fn run(path: &Path, mappings: &PathMappings, label_mode: &LabelMode) -> Result<Outcome> {
    let before = checksum(path).context("cannot read catalog file")?;
    let (
        catalog_version,
        folders,
        photos,
        collections,
        keywords,
        keyword_attributes,
        keyword_assignments,
        keyword_synonyms,
        ratings,
        flags,
        labels,
    ) = {
        let catalog = Catalog::open(path)?;
        let mut folders = Report::new();
        let mut photos = Report::new();
        let mut collections = Report::new();
        let mut keywords = Report::new();
        let mut keyword_assignments = Report::new();
        let found_folders = catalog.folders(&mut folders)?;
        let photo_data = catalog.photo_data(&mut photos)?;
        catalog.collections(&mut collections)?;
        let mapped_folders: Vec<Folder> = found_folders
            .into_iter()
            .map(|f| Folder::new(mappings.map(&f.path)))
            .collect();
        for _ in &mapped_folders {
            folders.converted();
        }
        let mapped_photos = photo_data
            .photos
            .into_iter()
            .map(|mut p| {
                p.path = mappings.map(&p.path);
                p
            })
            .collect();
        let mut kept_photos = drop_missing_originals(mapped_photos, &mut photos);
        let labels = labels::apply(&mut kept_photos, &photo_data.color_labels, label_mode);
        for _ in &kept_photos {
            photos.converted();
        }
        // Only converted photos are passed on, so assignments to skipped
        // photos are skipped as well.
        let data = catalog.keywords(&kept_photos, &mut keywords, &mut keyword_assignments)?;
        let keyword_attributes = KeywordAttributes::count(&data);
        let mut keyword_synonyms = data.synonym_report;
        for keyword in &data.keywords {
            keywords.converted();
            for _ in &keyword.synonyms {
                keyword_synonyms.converted();
            }
        }
        for _ in data.assignments.values().flatten() {
            keyword_assignments.converted();
        }
        (
            catalog.version().to_string(),
            folders,
            photos,
            collections,
            keywords,
            keyword_attributes,
            keyword_assignments,
            keyword_synonyms,
            rating_counts(&kept_photos),
            FlagCounts::count(&kept_photos, &photo_data.unknown_flag),
            labels,
        )
    };
    let after = checksum(path).context("cannot read catalog file after the run")?;
    for (area, report) in [
        ("folders", &folders),
        ("photos", &photos),
        ("collections", &collections),
        ("keywords", &keywords),
        ("keyword assignments", &keyword_assignments),
        ("keyword synonyms", &keyword_synonyms),
    ] {
        if !report.is_balanced() {
            bail!("internal error: {area} do not add up (read != converted + skipped)");
        }
    }
    Ok(Outcome {
        catalog_version,
        catalog_unchanged: before == after,
        folders,
        videos: count_videos(&photos),
        photos,
        collections,
        keywords,
        keyword_attributes,
        keyword_assignments,
        keyword_synonyms,
        ratings,
        flags,
        labels,
    })
}

/// Counts the photos skipped as videos.
fn count_videos(photos: &Report) -> usize {
    photos
        .skipped_items()
        .iter()
        .filter(|s| is_video_reason(&s.reason))
        .count()
}

/// True for the reader's video reason `photo {id}: <VIDEO_SKIP_REASON>`.
fn is_video_reason(reason: &str) -> bool {
    reason
        .strip_prefix("photo ")
        .and_then(|rest| rest.split_once(": "))
        .is_some_and(|(id, text)| {
            !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && text == VIDEO_SKIP_REASON
        })
}

/// Counts photos per star rating; index `n` holds the photos with `n` stars.
fn rating_counts(photos: &[Photo]) -> [usize; 6] {
    let mut counts = [0; Rating::MAX as usize + 1];
    for p in photos {
        counts[usize::from(p.rating.get())] += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(id: i64, stars: u8) -> Photo {
        Photo::new(format!("p{id}.jpg"), id, Rating::new(stars).unwrap())
    }

    fn kw(name: &str) -> Keyword {
        Keyword::new(vec![name.into()]).unwrap()
    }

    #[test]
    fn keyword_attributes_are_counted() {
        let mut person = kw("P");
        person.person = true;
        person.include_on_export = false;
        let mut quiet = kw("Q");
        quiet.export_containing = false;
        quiet.export_synonyms = false;
        let plain = kw("R");
        let data = KeywordData {
            keywords: vec![person.clone(), quiet, plain],
            assignments: [(1, vec![person.clone()]), (2, vec![person])]
                .into_iter()
                .collect(),
            synonym_report: Report::new(),
            unknown_type: 1,
        };
        assert_eq!(
            KeywordAttributes::count(&data),
            KeywordAttributes {
                persons: 1,
                not_exported: 1,
                no_parents_on_export: 1,
                no_synonyms_on_export: 1,
                unknown_type: 1,
                unused: 2,
            }
        );
    }

    #[test]
    fn flags_are_counted_with_unknown_apart() {
        let photos = [
            photo(1, 0).with_flag(Flag::Pick),
            photo(2, 0).with_flag(Flag::Reject),
            photo(3, 0),
            photo(4, 0),
            photo(5, 0).with_flag(Flag::Pick),
        ];
        assert_eq!(
            FlagCounts::count(&photos, &[4, 9]),
            FlagCounts {
                pick: 2,
                reject: 1,
                none: 1,
                unknown: 1,
            }
        );
        assert_eq!(FlagCounts::count(&[], &[1]), FlagCounts::default());
    }

    #[test]
    fn rating_counts_cover_all_stars() {
        let photos = [photo(1, 0), photo(2, 5), photo(3, 5), photo(4, 3)];
        assert_eq!(rating_counts(&photos), [1, 0, 0, 1, 0, 2]);
    }

    #[test]
    fn rating_counts_of_no_photos_are_zero() {
        assert_eq!(rating_counts(&[]), [0; 6]);
    }

    #[test]
    fn only_video_reasons_are_counted_as_videos() {
        let mut photos = Report::new();
        for _ in 0..5 {
            photos.read();
        }
        photos.skipped(format!("photo 2: {VIDEO_SKIP_REASON}"));
        photos.skipped(format!("photo 4: {VIDEO_SKIP_REASON}"));
        photos.skipped("photo 3: original file is missing");
        photos.skipped("photo 9: virtual copy of photo 4 is not converted");
        photos.converted();
        assert_eq!(count_videos(&photos), 2);
        assert_eq!(count_videos(&Report::new()), 0);
    }

    #[test]
    fn video_reason_needs_a_photo_id() {
        assert!(is_video_reason(&format!("photo 12: {VIDEO_SKIP_REASON}")));
        assert!(!is_video_reason(&format!("photo x: {VIDEO_SKIP_REASON}")));
        assert!(!is_video_reason(&format!("photo : {VIDEO_SKIP_REASON}")));
        assert!(!is_video_reason(&format!("keyword 1: {VIDEO_SKIP_REASON}")));
        assert!(!is_video_reason(VIDEO_SKIP_REASON));
        assert!(!is_video_reason("photo 1: video files"));
    }
}
