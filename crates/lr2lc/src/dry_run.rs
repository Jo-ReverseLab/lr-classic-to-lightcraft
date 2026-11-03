//! The dry run: read the catalog, apply path mappings, check originals, read
//! keywords for the converted photos and count what would be converted.
//! Nothing is written except the optional report file.

use crate::labels::{self, LabelCounts, LabelMode};
use anyhow::{Context, Result, bail};
use lr_reader::{Catalog, KeywordData, checksum, drop_missing_originals};
use model::{
    Collection, CollectionKind, Flag, Folder, Keyword, MappedPath, PathMappings, Photo,
    QuickCollection, Rating, Report, SkipCode, SmartCollection, Stack,
};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// Result of a dry run.
#[derive(Debug)]
pub struct Outcome {
    pub catalog_version: String,
    /// SHA-256 of the catalog file before the run.
    pub catalog_checksum: [u8; 32],
    pub catalog_unchanged: bool,
    pub folders: Report,
    pub photos: Report,
    /// Photos skipped because they are videos; a subset of the skipped
    /// photos.
    pub videos: usize,
    pub collections: Report,
    /// Kind and name counts over the converted collections.
    pub collection_counts: CollectionCounts,
    /// Members of collections: converted with an album, or skipped.
    pub collection_members: Report,
    pub stacks: Report,
    /// Counts over the converted stacks.
    pub stack_counts: StackCounts,
    /// Members of stacks: converted with a stack, or skipped.
    pub stack_members: Report,
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
    /// Converted photos that carry a caption. The text is never reported.
    pub captions: usize,
    /// Converted photos that carry a copyright notice. The text is never
    /// reported.
    pub copyrights: usize,
    /// Converted photos that carry a creator. The text is never reported.
    pub creators: usize,
    /// Converted photos that carry a title. The text is never reported.
    pub titles: usize,
    /// Converted photos whose title blob cannot be read.
    pub titles_unreadable: usize,
    /// What the writer gets: the converted photos (paths mapped), the
    /// converted keywords and the keywords of each converted photo.
    pub converted: Converted,
}

/// The converted elements, as input for the writer.
#[derive(Debug, Default)]
pub struct Converted {
    pub photos: Vec<Photo>,
    pub keywords: Vec<Keyword>,
    /// Keywords per `Photo::lightroom_id`.
    pub photo_keywords: BTreeMap<i64, Vec<Keyword>>,
    /// Converted albums and folders, ascending by Lightroom id.
    pub collections: Vec<Collection>,
    /// Converted stacks, ascending by Lightroom id.
    pub stacks: Vec<Stack>,
    /// The converted Quick Collection.
    pub quick: Option<QuickCollection>,
    /// Smart collections that can be written as smart albums, ascending by
    /// Lightroom id.
    pub smart: Vec<SmartCollection>,
    /// Lightroom ids of the smart collections read with their rules but not
    /// in `smart` (untranslatable, or their parent is not a collection
    /// set), ascending.
    pub smart_untranslatable_ids: Vec<i64>,
}

/// Counts over the converted stacks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StackCounts {
    /// Stacks collapsed in Lightroom; they are written expanded.
    pub collapsed: usize,
    /// Stacks without a member at position 1; the first by position is
    /// the top.
    pub no_top: usize,
}

/// Counts over the converted collections.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CollectionCounts {
    pub albums: usize,
    pub folders: usize,
    /// Collections that repeat the exact name of an earlier converted
    /// sibling below the same parent (two equal names count as 1).
    pub duplicate_names: usize,
    /// 1 if a Quick Collection was converted, else 0.
    pub quick: usize,
    /// Smart collections converted as smart albums.
    pub smart: usize,
}

impl CollectionCounts {
    fn count(collections: &[Collection]) -> Self {
        let mut siblings: HashSet<(Option<i64>, &str)> = HashSet::new();
        let mut counts = CollectionCounts::default();
        for c in collections {
            match c.kind {
                CollectionKind::Album => counts.albums += 1,
                CollectionKind::Folder => counts.folders += 1,
            }
            if !siblings.insert((c.parent, c.name.as_str())) {
                counts.duplicate_names += 1;
            }
        }
        counts
    }
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
        collection_counts,
        collection_members,
        (stacks, stack_counts, stack_members),
        keywords,
        keyword_attributes,
        keyword_assignments,
        keyword_synonyms,
        ratings,
        flags,
        labels,
        captions,
        copyrights,
        creators,
        titles,
        titles_unreadable,
        converted,
    ) = {
        let catalog = Catalog::open(path)?;
        let mut folders = Report::new();
        let mut photos = Report::new();
        let mut keywords = Report::new();
        let mut keyword_assignments = Report::new();
        let found_folders = catalog.folders(&mut folders)?;
        let photo_data = catalog.photo_data(&mut photos)?;
        let mapped_folders: Vec<Folder> = found_folders
            .into_iter()
            .filter_map(|f| match mappings.classify(&f.path) {
                MappedPath::Mapped(p) | MappedPath::Unchanged(p) => Some(Folder::new(p)),
                MappedPath::Escaped(_) => {
                    folders.skip(
                        SkipCode::PathMappingEscaped,
                        "folder: path mapping would climb out of the new prefix",
                    );
                    None
                }
            })
            .collect();
        for _ in &mapped_folders {
            folders.converted();
        }
        // Map each photo, refusing mappings that would climb out of the new
        // prefix via `..` (reported, path left unchanged and photo dropped).
        let mut mapped_photos: Vec<Photo> = Vec::with_capacity(photo_data.photos.len());
        for mut p in photo_data.photos {
            p.path = match mappings.classify(&p.path) {
                MappedPath::Mapped(new) => new,
                MappedPath::Unchanged(orig) => orig,
                MappedPath::Escaped(_) => {
                    photos.skip(
                        SkipCode::PathMappingEscaped,
                        format!(
                            "photo {}: path mapping would climb out of the new prefix",
                            p.lightroom_id
                        ),
                    );
                    continue;
                }
            };
            mapped_photos.push(p);
        }
        // Two photos landing on the same path after mapping would be a silent
        // merge: keep the first, report each later one. A virtual copy shares
        // its master's file by design, so only masters take part in the
        // check; a copy is never a collision with its own master.
        let master_indices: Vec<usize> = mapped_photos
            .iter()
            .enumerate()
            .filter(|(_, p)| p.master.is_none())
            .map(|(i, _)| i)
            .collect();
        let master_paths: Vec<&Path> = master_indices
            .iter()
            .map(|&i| mapped_photos[i].path.as_path())
            .collect();
        let mut collisions: Vec<usize> = mappings
            .collisions(&master_paths)
            .into_iter()
            .map(|j| master_indices[j])
            .collect();
        collisions.sort_unstable();
        let mut kept_before_originals: Vec<Photo> = Vec::with_capacity(mapped_photos.len());
        for (i, p) in mapped_photos.drain(..).enumerate() {
            if collisions.binary_search(&i).is_ok() {
                photos.skip(
                    SkipCode::PathMappingCollision,
                    format!(
                        "photo {}: path mapping collides with an earlier photo",
                        p.lightroom_id
                    ),
                );
            } else {
                kept_before_originals.push(p);
            }
        }
        let mut kept_photos = drop_missing_originals(kept_before_originals, &mut photos);
        let labels = labels::apply(&mut kept_photos, &photo_data.color_labels, label_mode);
        for _ in &kept_photos {
            photos.converted();
        }
        // Only converted photos are passed on, so assignments to skipped
        // photos are skipped as well.
        let data = catalog.keywords(&kept_photos, &mut keywords, &mut keyword_assignments)?;
        let keyword_attributes = KeywordAttributes::count(&data);
        // Members naming skipped photos are skipped as well.
        let collection_data = catalog.collection_data(&kept_photos)?;
        // The reader already produced the final verdict for every smart
        // collection (converted or skipped with a reason); the caller no
        // longer rewrites the report by reason text. The convertible ones go
        // to the writer; the rest are reported as untranslatable ids.
        let smart = collection_data.convertible_smart.clone();
        let smart_untranslatable_ids: Vec<i64> = collection_data
            .smart_ids
            .iter()
            .copied()
            .filter(|id| !smart.iter().any(|s| s.lightroom_id == *id))
            .collect();
        let stack_data = catalog.stack_data(&kept_photos)?;
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
        // Membership test once per photo instead of a linear scan of the
        // unreadable ids for every photo.
        let title_unreadable: HashSet<i64> = photo_data.title_unreadable.iter().copied().collect();
        (
            catalog.version().to_string(),
            folders,
            photos,
            collection_data.report,
            CollectionCounts {
                quick: usize::from(collection_data.quick.is_some()),
                smart: smart.len(),
                ..CollectionCounts::count(&collection_data.collections)
            },
            collection_data.members,
            (
                stack_data.report,
                StackCounts {
                    collapsed: stack_data.collapsed,
                    no_top: stack_data.no_top,
                },
                stack_data.members,
            ),
            keywords,
            keyword_attributes,
            keyword_assignments,
            keyword_synonyms,
            rating_counts(&kept_photos),
            FlagCounts::count(&kept_photos, &photo_data.unknown_flag),
            labels,
            count_captions(&kept_photos),
            kept_photos.iter().filter(|p| p.copyright.is_some()).count(),
            kept_photos.iter().filter(|p| p.creator.is_some()).count(),
            kept_photos.iter().filter(|p| p.title.is_some()).count(),
            kept_photos
                .iter()
                .filter(|p| title_unreadable.contains(&p.lightroom_id))
                .count(),
            Converted {
                photos: kept_photos,
                keywords: data.keywords,
                photo_keywords: data.assignments,
                collections: collection_data.collections,
                stacks: stack_data.stacks,
                quick: collection_data.quick,
                smart,
                smart_untranslatable_ids,
            },
        )
    };
    let after = checksum(path).context("cannot read catalog file after the run")?;
    for (area, report) in [
        ("folders", &folders),
        ("photos", &photos),
        ("collections", &collections),
        ("collection members", &collection_members),
        ("stacks", &stacks),
        ("stack members", &stack_members),
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
        catalog_checksum: before,
        catalog_unchanged: before == after,
        folders,
        videos: count_videos(&photos),
        photos,
        collections,
        collection_counts,
        collection_members,
        stacks,
        stack_counts,
        stack_members,
        keywords,
        keyword_attributes,
        keyword_assignments,
        keyword_synonyms,
        ratings,
        flags,
        labels,
        captions,
        copyrights,
        creators,
        titles,
        titles_unreadable,
        converted,
    })
}

/// Counts the photos that carry a caption.
fn count_captions(photos: &[Photo]) -> usize {
    photos.iter().filter(|p| p.caption.is_some()).count()
}

/// Counts the photos skipped as videos, by their code.
fn count_videos(photos: &Report) -> usize {
    photos
        .skipped_items()
        .iter()
        .filter(|s| s.code == SkipCode::Video)
        .count()
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
    use lr_reader::VIDEO_SKIP_REASON;

    #[test]
    fn reader_produces_the_smart_verdict() {
        // The reader sets the final verdict for every smart collection at read
        // time: the one translatable collection (101, rating >= 3, top level)
        // is converted and in `convertible_smart`; the rest are skipped with a
        // reason that names the criterion (not the old "first criterion"
        // phrase). The caller never rewrites the report by reason text.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smart.lrcat");
        lrcat_fixtures::write_smart_catalog(&path).unwrap();
        let catalog = Catalog::open(&path).unwrap();
        let photos = catalog.photos(&mut Report::new()).unwrap();
        let data = catalog.collection_data(&photos).unwrap();
        assert_eq!(
            data.convertible_smart
                .iter()
                .map(|s| s.lightroom_id)
                .collect::<Vec<_>>(),
            vec![101]
        );
        assert!(data.report.is_balanced());
        let has_skip = |id: i64| {
            data.report
                .skipped_items()
                .iter()
                .any(|s| s.reason.starts_with(&format!("collection {id}:")))
        };
        assert!(!has_skip(101), "101 is converted, not skipped");
        let r102 = data
            .report
            .skipped_items()
            .iter()
            .find(|s| s.reason.starts_with("collection 102:"))
            .expect("102 is skipped");
        assert!(
            r102.reason
                .contains("criterion \"pick\" has no exact LightCraft equivalent"),
            "{}",
            r102.reason
        );
        assert!(
            !r102.reason.contains("first criterion"),
            "the old phrase is gone: {}",
            r102.reason
        );
    }

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

    fn coll(id: i64, kind: CollectionKind, parent: Option<i64>, name: &str) -> Collection {
        Collection {
            lightroom_id: id,
            name: name.into(),
            kind,
            parent,
            members: vec![],
        }
    }

    #[test]
    fn collection_kinds_and_duplicate_names_are_counted() {
        use CollectionKind::{Album, Folder};
        let cs = [
            coll(1, Folder, None, "A"),
            coll(2, Album, None, "A"),
            coll(3, Album, Some(1), "A"),
            coll(4, Album, Some(1), "A"),
            coll(5, Album, Some(1), "A"),
            coll(6, Album, Some(1), "a"),
            coll(7, Album, Some(1), "A "),
        ];
        assert_eq!(
            CollectionCounts::count(&cs),
            CollectionCounts {
                albums: 6,
                folders: 1,
                duplicate_names: 3,
                quick: 0,
                smart: 0,
            }
        );
        assert_eq!(CollectionCounts::count(&[]), CollectionCounts::default());
    }

    #[test]
    fn rating_counts_cover_all_stars() {
        let photos = [photo(1, 0), photo(2, 5), photo(3, 5), photo(4, 3)];
        assert_eq!(rating_counts(&photos), [1, 0, 0, 1, 0, 2]);
    }

    #[test]
    fn only_photos_with_a_caption_are_counted() {
        let photos = [
            photo(1, 0).with_caption("a"),
            photo(2, 0),
            photo(3, 0).with_caption(" "),
        ];
        assert_eq!(count_captions(&photos), 2);
        assert_eq!(count_captions(&[]), 0);
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
        photos.skip(SkipCode::Video, format!("photo 2: {VIDEO_SKIP_REASON}"));
        photos.skip(SkipCode::Video, format!("photo 4: {VIDEO_SKIP_REASON}"));
        photos.skip(SkipCode::Missing, "photo 3: original file is missing");
        photos.skip(
            SkipCode::VirtualCopy,
            "photo 9: virtual copy of photo 4 is not converted",
        );
        // The text alone does not make a video: the code does.
        photos.skipped(format!("photo 5: {VIDEO_SKIP_REASON}"));
        photos.converted();
        assert_eq!(count_videos(&photos), 2);
        assert_eq!(count_videos(&Report::new()), 0);
    }
}
