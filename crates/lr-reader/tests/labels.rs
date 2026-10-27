//! Acceptance tests for reading colour labels (task P5b-2).
//!
//! API under test: `PhotoData::color_labels: Vec<(i64, String)>`, filled by
//! `Catalog::photo_data`:
//! - one `(catalog id, raw text of Adobe_images.colorLabels)` entry for every
//!   returned photo whose value is non-empty text, ascending by id. The text
//!   is exactly the stored string (no trimming, no normalisation, no case
//!   change).
//! - the empty string, NULL and non-text values give no entry (no label).
//! - photos that are skipped (virtual copy, missing file row, ...) get no
//!   entry, even if their label is set.
//! - Labels do not change `photos`: same list as before, same `read` /
//!   `skipped` counts, no extra skip reason. The `Photo::label` field stays
//!   `None` here; mapping to `ColorLabel` is the CLI's job (it needs the
//!   label set).
//! - Nothing in `Report`, `Debug` of errors etc. contains the label text.
//! - `Adobe_images.colorLabels` is part of the required schema:
//!   `ReadError::MissingSchema { table: "Adobe_images", column:
//!   Some("colorLabels") }`.
//! - Fixtures: `lrcat_fixtures::write_label_catalog_with_files`, see
//!   `lrcat_fixtures::label_edge`.

use lr_reader::{Catalog, checksum};
use lrcat_fixtures::label_edge::{self, Fixture};
use lrcat_fixtures::{small, write_label_catalog_with_files};
use model::Report;
use std::path::{Path, PathBuf};

fn fixture(f: Fixture) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_label_catalog_with_files(dir.path(), f).unwrap();
    (dir, cat)
}

fn read(path: &Path) -> (lr_reader::PhotoData, Report) {
    let catalog = Catalog::open(path).unwrap();
    let mut report = Report::new();
    let data = catalog.photo_data(&mut report).expect("read photos");
    (data, report)
}

fn labels(data: &lr_reader::PhotoData) -> Vec<(i64, &str)> {
    data.color_labels
        .iter()
        .map(|(id, s)| (*id, s.as_str()))
        .collect()
}

#[test]
fn english_labels_are_returned_raw() {
    let (_d, cat) = fixture(Fixture::En);
    let (data, _) = read(&cat);
    assert_eq!(
        labels(&data),
        [
            (2, "Red"),
            (4, "Red"),
            (5, "Yellow"),
            (6, "Green"),
            (7, "Blue"),
            (8, "Purple"),
            (9, "Red")
        ]
    );
}

#[test]
fn german_labels_are_returned_raw_in_nfc() {
    let (_d, cat) = fixture(Fixture::De);
    let (data, _) = read(&cat);
    assert_eq!(
        labels(&data),
        [
            (2, "Rot"),
            (4, "Rot"),
            (5, "Gelb"),
            (6, "Gr\u{fc}n"),
            (7, "Blau"),
            (8, "Lila"),
            (9, "Rot")
        ]
    );
}

#[test]
fn odd_labels_are_not_trimmed_or_normalised() {
    let (_d, cat) = fixture(Fixture::Odd);
    let (data, _) = read(&cat);
    assert_eq!(
        labels(&data),
        [
            (2, "Red"),
            (4, "Gru\u{308}n"),
            (5, "rot"),
            (6, "Red "),
            (7, label_edge::CUSTOM_NAME),
            (8, "Green"),
            (9, "Red")
        ]
    );
}

#[test]
fn empty_string_and_null_mean_no_label() {
    let (_d, cat) = fixture(Fixture::Empty);
    let (data, _) = read(&cat);
    assert!(data.color_labels.is_empty());
    // Photo 10 stores NULL in every fixture and gets no entry either.
    let (_d, cat) = fixture(Fixture::En);
    let (data, _) = read(&cat);
    assert!(
        data.color_labels
            .iter()
            .all(|(id, _)| *id != label_edge::NULL_ID)
    );
}

#[test]
fn skipped_photos_get_no_label_entry() {
    // Photo 2 (missing original) is still returned by the reader (the CLI
    // drops it later); photo 20 is a virtual copy and is not returned.
    for f in [Fixture::En, Fixture::De, Fixture::Mixed, Fixture::Odd] {
        let (_d, cat) = fixture(f);
        let (data, _) = read(&cat);
        assert!(data.color_labels.iter().all(|(id, _)| *id != 20), "{f:?}");
        let returned: Vec<i64> = data.photos.iter().map(|p| p.lightroom_id).collect();
        for (id, _) in &data.color_labels {
            assert!(returned.contains(id), "{f:?}: entry {id} has no photo");
        }
        // The label of photo 2 is read with its (returned) photo.
        assert!(returned.contains(&2));
        assert!(data.color_labels.iter().any(|(id, _)| *id == 2), "{f:?}");
    }
}

#[test]
fn labels_do_not_change_photos_or_counts() {
    // Each fixture lives in its own temporary directory, so photo paths are
    // compared relative to it.
    fn relative(dir: &Path, photos: &[model::Photo]) -> Vec<model::Photo> {
        photos
            .iter()
            .map(|p| {
                let mut p = p.clone();
                p.path = p.path.strip_prefix(dir).unwrap().to_path_buf();
                p
            })
            .collect()
    }
    let (plain_dir, plain_cat) = fixture(Fixture::Empty);
    let (plain, plain_report) = read(&plain_cat);
    assert!(plain.color_labels.is_empty());
    let plain_photos = relative(plain_dir.path(), &plain.photos);
    for f in [Fixture::En, Fixture::Odd] {
        let (d, cat) = fixture(f);
        let (data, report) = read(&cat);
        assert_eq!(report.read_count() as i64, small::PHOTOS);
        assert_eq!(report.skipped_count(), plain_report.skipped_count());
        assert_eq!(relative(d.path(), &data.photos), plain_photos, "{f:?}");
        assert!(data.photos.iter().all(|p| p.label.is_none()));
    }
}

#[test]
fn labels_stay_out_of_the_report() {
    let (_d, cat) = fixture(Fixture::Odd);
    let (_, report) = read(&cat);
    for item in report.skipped_items() {
        assert!(!item.reason.contains(label_edge::CUSTOM_NAME));
        assert!(!item.reason.contains("Gru"));
    }
}

#[test]
fn second_read_is_identical_and_catalog_unchanged() {
    let (_d, cat) = fixture(Fixture::Odd);
    let before = checksum(&cat).unwrap();
    let (a, _) = read(&cat);
    let (b, _) = read(&cat);
    assert_eq!(a, b);
    assert_eq!(checksum(&cat).unwrap(), before);
}
