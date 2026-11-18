//! Checking that the original files of photos exist, and which of them are
//! only in the cloud.

use model::{Photo, Report, SkipCode, Skipped, Subject};
use std::fs::Metadata;

/// Originals that are only in the cloud: on macOS, files iCloud Drive (or
/// another file provider) keeps as "dataless" placeholders. Their bytes are
/// downloaded when the file is first read, so LightCraft's import has to
/// wait for the download and may give up. Only stat data is read; nothing
/// is downloaded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudOnly {
    /// Lightroom id and file size in bytes of each such photo, in input
    /// order.
    pub photos: Vec<(i64, u64)>,
}

impl CloudOnly {
    /// The number of such photos.
    pub fn count(&self) -> usize {
        self.photos.len()
    }

    /// Their size together, in bytes.
    pub fn bytes(&self) -> u64 {
        self.photos.iter().map(|(_, b)| b).sum()
    }
}

/// macOS's `SF_DATALESS` file flag (`sys/stat.h`): the file's data is not
/// on this computer.
pub const SF_DATALESS: u32 = 0x4000_0000;

/// Whether BSD file flags (`st_flags`) mark a dataless placeholder.
pub fn flags_dataless(flags: u32) -> bool {
    flags & SF_DATALESS != 0
}

/// Whether the file is a dataless placeholder (only on macOS; elsewhere
/// always false).
pub fn is_dataless(metadata: &Metadata) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        flags_dataless(metadata.st_flags())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = metadata;
        false
    }
}

/// Removes photos whose original file does not exist and records each one
/// with [`Report::skip`].
///
/// Files are only checked for existence; nothing is opened or changed.
/// If the check itself fails (e.g. no permission), the photo counts as
/// missing with its own reason. Reasons name the photo's catalog id but
/// not its path. Does not call [`Report::read`].
pub fn drop_missing_originals(photos: Vec<Photo>, report: &mut Report) -> Vec<Photo> {
    check_originals(photos, report).0
}

/// Like [`drop_missing_originals`], and also lists the originals that are
/// only in the cloud ([`is_dataless`]).
pub fn check_originals(photos: Vec<Photo>, report: &mut Report) -> (Vec<Photo>, CloudOnly) {
    check_originals_with(photos, report, is_dataless)
}

/// Like [`check_originals`], with `dataless` deciding which existing files
/// are only in the cloud.
pub fn check_originals_with(
    photos: Vec<Photo>,
    report: &mut Report,
    dataless: impl Fn(&Metadata) -> bool,
) -> (Vec<Photo>, CloudOnly) {
    let mut cloud = CloudOnly::default();
    let kept = photos
        .into_iter()
        .filter(|photo| match std::fs::metadata(&photo.path) {
            Ok(metadata) => {
                if dataless(&metadata) {
                    cloud.photos.push((photo.lightroom_id, metadata.len()));
                }
                true
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                report.push_skipped(
                    Skipped::new(
                        SkipCode::Missing,
                        format!("photo {}: original file is missing", photo.lightroom_id),
                    )
                    .about(Subject::Photo(photo.lightroom_id)),
                );
                false
            }
            Err(e) => {
                report.push_skipped(
                    Skipped::new(
                        SkipCode::Missing,
                        format!(
                            "photo {}: original file is missing (cannot check it: {})",
                            photo.lightroom_id,
                            e.kind()
                        ),
                    )
                    .about(Subject::Photo(photo.lightroom_id)),
                );
                false
            }
        })
        .collect();
    (kept, cloud)
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

    #[test]
    fn cloud_only_originals_are_kept_and_listed_with_their_size() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("a.jpg");
        let cloud = dir.path().join("b.cr3");
        std::fs::write(&local, b"x").unwrap();
        std::fs::write(&cloud, b"12345").unwrap();
        let photos = vec![
            Photo::new(&local, 1, Rating::default()),
            Photo::new(&cloud, 2, Rating::default()),
        ];
        let mut report = Report::new();
        // Stand-in for the dataless flag, which only iCloud can set.
        let (kept, found) = check_originals_with(photos, &mut report, |m| m.len() == 5);
        assert_eq!(kept.len(), 2);
        assert_eq!(found.photos, vec![(2, 5)]);
        assert_eq!((found.count(), found.bytes()), (1, 5));
        assert!(report.skipped_items().is_empty());
    }

    #[test]
    fn the_dataless_flag_is_read_from_st_flags() {
        // `ls -lO` shows such a file as `compressed,dataless`.
        assert!(flags_dataless(0x4000_0020));
        assert!(flags_dataless(SF_DATALESS));
        assert!(!flags_dataless(0x20));
        assert!(!flags_dataless(0));
    }

    #[test]
    fn a_plain_local_file_is_not_dataless() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        std::fs::write(&path, b"x").unwrap();
        assert!(!is_dataless(&std::fs::metadata(&path).unwrap()));
    }
}
