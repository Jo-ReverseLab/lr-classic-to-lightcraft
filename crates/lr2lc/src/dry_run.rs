//! The dry run: read the catalog, apply path mappings, check originals, read
//! keywords for the converted photos and count what would be converted.
//! Nothing is written except the optional report file.

use anyhow::{Context, Result, bail};
use lr_reader::{Catalog, checksum, drop_missing_originals};
use model::{Folder, PathMappings, Photo, Rating, Report};
use std::path::Path;

/// Result of a dry run.
#[derive(Debug)]
pub struct Outcome {
    pub catalog_version: String,
    pub catalog_unchanged: bool,
    pub folders: Report,
    pub photos: Report,
    pub collections: Report,
    pub keywords: Report,
    pub keyword_assignments: Report,
    /// Synonyms of keywords; converted with their keyword.
    pub keyword_synonyms: Report,
    /// Number of converted photos per star rating, index 0 to 5.
    pub ratings: [usize; 6],
}

/// Runs the dry run on the catalog at `path`.
///
/// Fails if the catalog cannot be opened or read. A changed checksum is not
/// an error here; the caller decides after writing the report.
pub fn run(path: &Path, mappings: &PathMappings) -> Result<Outcome> {
    let before = checksum(path).context("cannot read catalog file")?;
    let (
        catalog_version,
        folders,
        photos,
        collections,
        keywords,
        keyword_assignments,
        keyword_synonyms,
        ratings,
    ) = {
        let catalog = Catalog::open(path)?;
        let mut folders = Report::new();
        let mut photos = Report::new();
        let mut collections = Report::new();
        let mut keywords = Report::new();
        let mut keyword_assignments = Report::new();
        let found_folders = catalog.folders(&mut folders)?;
        let found_photos = catalog.photos(&mut photos)?;
        catalog.collections(&mut collections)?;
        let mapped_folders: Vec<Folder> = found_folders
            .into_iter()
            .map(|f| Folder::new(mappings.map(&f.path)))
            .collect();
        for _ in &mapped_folders {
            folders.converted();
        }
        let mapped_photos = found_photos
            .into_iter()
            .map(|mut p| {
                p.path = mappings.map(&p.path);
                p
            })
            .collect();
        let kept_photos = drop_missing_originals(mapped_photos, &mut photos);
        for _ in &kept_photos {
            photos.converted();
        }
        // Only converted photos are passed on, so assignments to skipped
        // photos are skipped as well.
        let data = catalog.keywords(&kept_photos, &mut keywords, &mut keyword_assignments)?;
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
            keyword_assignments,
            keyword_synonyms,
            rating_counts(&kept_photos),
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
        photos,
        collections,
        keywords,
        keyword_assignments,
        keyword_synonyms,
        ratings,
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

    #[test]
    fn rating_counts_cover_all_stars() {
        let photos = [photo(1, 0), photo(2, 5), photo(3, 5), photo(4, 3)];
        assert_eq!(rating_counts(&photos), [1, 0, 0, 1, 0, 2]);
    }

    #[test]
    fn rating_counts_of_no_photos_are_zero() {
        assert_eq!(rating_counts(&[]), [0; 6]);
    }
}
