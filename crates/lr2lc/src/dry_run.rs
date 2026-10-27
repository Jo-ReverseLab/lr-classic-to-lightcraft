//! The dry run: read the catalog, apply path mappings, check originals and
//! count what would be converted. Nothing is written except the optional
//! report file.

use anyhow::{Context, Result, bail};
use lr_reader::{Catalog, checksum, drop_missing_originals};
use model::{Folder, PathMappings, Report};
use std::path::Path;

/// Result of a dry run.
#[derive(Debug)]
pub struct Outcome {
    pub catalog_version: String,
    pub catalog_unchanged: bool,
    pub folders: Report,
    pub photos: Report,
    pub collections: Report,
}

/// Runs the dry run on the catalog at `path`.
///
/// Fails if the catalog cannot be opened or read. A changed checksum is not
/// an error here; the caller decides after writing the report.
pub fn run(path: &Path, mappings: &PathMappings) -> Result<Outcome> {
    let before = checksum(path).context("cannot read catalog file")?;
    let (catalog_version, folders, photos, collections) = {
        let catalog = Catalog::open(path)?;
        let mut folders = Report::new();
        let mut photos = Report::new();
        let mut collections = Report::new();
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
        for _ in drop_missing_originals(mapped_photos, &mut photos) {
            photos.converted();
        }
        (catalog.version().to_string(), folders, photos, collections)
    };
    let after = checksum(path).context("cannot read catalog file after the run")?;
    for (area, report) in [
        ("folders", &folders),
        ("photos", &photos),
        ("collections", &collections),
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
    })
}
