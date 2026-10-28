//! Checking that the original files of photos exist.

use model::{Photo, Report, SkipCode};

/// Removes photos whose original file does not exist and records each one
/// with [`Report::skip`].
///
/// Files are only checked for existence; nothing is opened or changed.
/// If the check itself fails (e.g. no permission), the photo counts as
/// missing with its own reason. Reasons name the photo's catalog id but
/// not its path. Does not call [`Report::read`].
pub fn drop_missing_originals(photos: Vec<Photo>, report: &mut Report) -> Vec<Photo> {
    photos
        .into_iter()
        .filter(|photo| match photo.path.try_exists() {
            Ok(true) => true,
            Ok(false) => {
                report.skip(
                    SkipCode::Missing,
                    format!("photo {}: original file is missing", photo.lightroom_id),
                );
                false
            }
            Err(e) => {
                report.skip(
                    SkipCode::Missing,
                    format!(
                        "photo {}: original file is missing (cannot check it: {})",
                        photo.lightroom_id,
                        e.kind()
                    ),
                );
                false
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::Rating;

    #[test]
    fn keeps_existing_and_drops_missing() {
        let dir = tempfile::tempdir().unwrap();
        let present = dir.path().join("a.jpg");
        std::fs::write(&present, b"x").unwrap();
        let photos = vec![
            Photo::new(&present, 1, Rating::default()),
            Photo::new(dir.path().join("b.jpg"), 2, Rating::default()),
        ];
        let mut report = Report::new();
        let kept = drop_missing_originals(photos, &mut report);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].lightroom_id, 1);
        assert_eq!(report.skipped_items()[0].code, SkipCode::Missing);
        let reason = &report.skipped_items()[0].reason;
        assert!(reason.contains("missing") && reason.contains('2'));
        assert!(!reason.contains("b.jpg"));
        assert_eq!(report.read_count(), 0);
    }
}
