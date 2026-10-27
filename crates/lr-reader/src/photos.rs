//! Reading photos (`Adobe_images` joined with `AgLibraryFile`,
//! `AgLibraryFolder` and `AgLibraryRootFolder`).

use crate::catalog::{Catalog, ReadError};
use crate::folders::join_folder_path;
use model::{Flag, Photo, Rating, Report};
use rusqlite::types::Value;
use std::collections::HashSet;

/// One joined row; `None` means the referenced row does not exist.
struct Row {
    image_id: i64,
    master: Option<i64>,
    file_format: Option<String>,
    /// `Adobe_images.id_global` if it is text.
    global_id: Option<String>,
    rating: Value,
    /// `Adobe_images.pick`, as stored.
    pick: Value,
    /// `Adobe_images.colorLabels` if it is non-empty text.
    color_label: Option<String>,
    file: Option<(String, String)>,
    folder: Option<String>,
    root_row: bool,
    root_path: Option<String>,
}

/// Photos read from a catalog, plus facts about them that the photos
/// themselves cannot carry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhotoData {
    /// Photos, ordered by their catalog id (see [`Catalog::photos`]).
    pub photos: Vec<Photo>,
    /// Catalog ids of the returned photos whose stored `pick` value is
    /// neither NULL, 0, 1 nor -1. These photos carry [`Flag::None`].
    /// Ascending.
    pub unknown_flag: Vec<i64>,
    /// `(catalog id, raw colorLabels text)` of the returned photos whose
    /// stored label is non-empty text, kept byte for byte. Ascending by id.
    /// The photos themselves carry no label; mapping the text needs a
    /// label set and is left to the caller.
    pub color_labels: Vec<(i64, String)>,
}

impl Catalog {
    /// Reads all photos, ordered by their catalog id.
    ///
    /// Same as [`Catalog::photo_data`] without the extra facts.
    pub fn photos(&self, report: &mut Report) -> Result<Vec<Photo>, ReadError> {
        self.photo_data(report).map(|data| data.photos)
    }

    /// Reads all photos, ordered by their catalog id, and the ids of the
    /// photos with an unknown flag value.
    ///
    /// Every `Adobe_images` row is counted with [`Report::read`]. Rows that
    /// cannot become a photo (virtual copies, videos, missing or duplicate
    /// global id, missing file, folder or root folder rows, ratings outside
    /// 0..=5) are recorded with
    /// [`Report::skipped`] and left out. Paths are assembled from stored
    /// strings only; the file system is never accessed. Nothing is marked
    /// converted here.
    ///
    /// The flag comes from `Adobe_images.pick`: 1 is [`Flag::Pick`], -1 is
    /// [`Flag::Reject`], 0 or NULL is [`Flag::None`] (whole-number reals
    /// count the same). Any other value also gives [`Flag::None`]; the photo
    /// is kept and its id is listed in [`PhotoData::unknown_flag`].
    pub fn photo_data(&self, report: &mut Report) -> Result<PhotoData, ReadError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT i.id_local, i.masterImage, i.rating, i.fileFormat, \
                        f.id_local, f.baseName, f.extension, \
                        d.id_local, d.pathFromRoot, \
                        r.id_local, r.absolutePath, i.id_global, i.pick, i.colorLabels \
                 FROM Adobe_images i \
                 LEFT JOIN AgLibraryFile f ON f.id_local = i.rootFile \
                 LEFT JOIN AgLibraryFolder d ON d.id_local = f.folder \
                 LEFT JOIN AgLibraryRootFolder r ON r.id_local = d.rootFolder \
                 ORDER BY i.id_local",
            )
            .map_err(ReadError::Query)?;
        let rows = stmt
            .query_map([], |row| {
                let file_id: Option<i64> = row.get(4)?;
                let folder_id: Option<i64> = row.get(7)?;
                let root_id: Option<i64> = row.get(9)?;
                Ok(Row {
                    image_id: row.get(0)?,
                    master: row.get(1)?,
                    rating: row.get(2)?,
                    pick: row.get(12)?,
                    color_label: text_value(row.get(13)?).filter(|t| !t.is_empty()),
                    file_format: text_value(row.get(3)?),
                    global_id: text_value(row.get(11)?),
                    file: match file_id {
                        Some(_) => Some((
                            row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                            row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                        )),
                        None => None,
                    },
                    folder: match folder_id {
                        Some(_) => Some(row.get::<_, Option<String>>(8)?.unwrap_or_default()),
                        None => None,
                    },
                    root_row: root_id.is_some(),
                    root_path: row.get(10)?,
                })
            })
            .map_err(ReadError::Query)?;
        let mut data = PhotoData::default();
        let mut used = HashSet::new();
        for row in rows {
            let row = row.map_err(ReadError::Query)?;
            report.read();
            let unknown_flag = flag_from_value(&row.pick).is_none();
            let color_label = row.color_label.clone();
            match photo_from_row(row, &mut used) {
                Ok(photo) => {
                    if unknown_flag {
                        data.unknown_flag.push(photo.lightroom_id);
                    }
                    if let Some(label) = color_label {
                        data.color_labels.push((photo.lightroom_id, label));
                    }
                    data.photos.push(photo);
                }
                Err(reason) => report.skipped(reason),
            }
        }
        Ok(data)
    }
}

/// Turns a joined row into a photo, or returns the reason it is skipped.
///
/// Reasons name the photo's catalog id but never paths, file names or the
/// global id. The checks run in this order: virtual copy, video, missing
/// or duplicate global id, file, folder and root folder rows, rating.
///
/// `used` holds the global ids of the photos converted so far (rows are
/// read in `id_local` order). Only a converted photo adds its global id, so
/// a row skipped for any reason does not block a later row with the same
/// global id.
fn photo_from_row(row: Row, used: &mut HashSet<String>) -> Result<Photo, String> {
    let id = row.image_id;
    if let Some(master) = row.master {
        return Err(format!(
            "photo {id}: virtual copy of photo {master} is not converted"
        ));
    }
    let ext = row.file.as_ref().map(|(_, ext)| ext.as_str());
    if is_video(row.file_format.as_deref(), ext) {
        return Err(format!("photo {id}: {VIDEO_SKIP_REASON}"));
    }
    let global_id = match row.global_id {
        Some(g) if !g.trim().is_empty() => g,
        _ => return Err(format!("photo {id}: missing global id")),
    };
    if used.contains(&global_id) {
        return Err(format!("photo {id}: duplicate global id"));
    }
    let (base, ext) = row
        .file
        .ok_or_else(|| format!("photo {id}: file row not found in catalog"))?;
    let rel = row
        .folder
        .ok_or_else(|| format!("photo {id}: folder row not found in catalog"))?;
    if !row.root_row {
        return Err(format!("photo {id}: root folder row not found in catalog"));
    }
    let root = row
        .root_path
        .ok_or_else(|| format!("photo {id}: root folder has no path"))?;
    let rating = rating_from_value(&row.rating)
        .ok_or_else(|| format!("photo {id}: rating is not a whole number in 0..=5"))?;
    let flag = flag_from_value(&row.pick).unwrap_or_default();
    let folder = join_folder_path(&root, &rel);
    used.insert(global_id.clone());
    Ok(
        Photo::new(join_file_path(&folder, &file_name(&base, &ext)), id, rating)
            .with_global_id(global_id)
            .with_flag(flag),
    )
}

/// Skip reason of a video, after the `photo {id}: ` prefix.
pub const VIDEO_SKIP_REASON: &str = "video files are not supported by LightCraft";

/// File extensions (lower case, without dot) that mark a video.
const VIDEO_EXTENSIONS: [&str; 11] = [
    "mov", "mp4", "m4v", "avi", "mts", "m2ts", "3gp", "mpg", "mpeg", "wmv", "mkv",
];

/// True if `Adobe_images.fileFormat` is `VIDEO` or the extension column
/// (compared without regard to case) is a video extension. Dots inside the
/// base name are never looked at.
fn is_video(file_format: Option<&str>, extension: Option<&str>) -> bool {
    file_format == Some("VIDEO")
        || extension.is_some_and(|ext| VIDEO_EXTENSIONS.iter().any(|v| v.eq_ignore_ascii_case(ext)))
}

/// A stored value as text if it is text; any other type is `None`.
/// A global id that is not text therefore counts as missing.
fn text_value(value: Value) -> Option<String> {
    match value {
        Value::Text(t) => Some(t),
        _ => None,
    }
}

/// Maps a stored rating to stars: NULL is 0; integers (or integral reals)
/// 0..=5 are kept; anything else is `None`.
fn rating_from_value(value: &Value) -> Option<Rating> {
    let stars = match value {
        Value::Null => 0,
        Value::Integer(v) => *v,
        Value::Real(v) if v.fract() == 0.0 && (0.0..=5.0).contains(v) => *v as i64,
        _ => return None,
    };
    u8::try_from(stars).ok().and_then(|s| Rating::new(s).ok())
}

/// Maps a stored `pick` value to a flag: NULL or 0 is none, 1 is pick, -1
/// is reject (integers or whole-number reals); anything else is `None`
/// (unknown).
fn flag_from_value(value: &Value) -> Option<Flag> {
    match value {
        Value::Null | Value::Integer(0) => Some(Flag::None),
        Value::Integer(1) => Some(Flag::Pick),
        Value::Integer(-1) => Some(Flag::Reject),
        Value::Real(v) if *v == 0.0 => Some(Flag::None),
        Value::Real(v) if *v == 1.0 => Some(Flag::Pick),
        Value::Real(v) if *v == -1.0 => Some(Flag::Reject),
        _ => None,
    }
}

/// `baseName` plus `.extension`, without a dot when the extension is empty.
fn file_name(base: &str, ext: &str) -> String {
    if ext.is_empty() {
        base.to_string()
    } else {
        format!("{base}.{ext}")
    }
}

/// Appends a file name to a folder path built by [`join_folder_path`],
/// using the folder's separator style.
fn join_file_path(folder: &str, name: &str) -> String {
    let windows = folder.contains('\\') && !folder.contains('/');
    let mut path = folder.to_string();
    if !path.ends_with(['/', '\\']) {
        path.push(if windows { '\\' } else { '/' });
    }
    path.push_str(name);
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> Row {
        Row {
            image_id: 1,
            master: None,
            file_format: Some("JPG".into()),
            global_id: Some("g-1".into()),
            rating: Value::Null,
            pick: Value::Null,
            color_label: None,
            file: Some(("IMG".into(), "jpg".into())),
            folder: Some("a/".into()),
            root_row: true,
            root_path: Some("/r/".into()),
        }
    }

    #[test]
    fn file_name_without_extension_has_no_dot() {
        assert_eq!(file_name("IMG_1", ""), "IMG_1");
        assert_eq!(file_name("IMG_1", "dng"), "IMG_1.dng");
    }

    #[test]
    fn file_path_uses_folder_separator_style() {
        assert_eq!(join_file_path("/a/b", "x.jpg"), "/a/b/x.jpg");
        assert_eq!(join_file_path("C:\\a", "x.jpg"), "C:\\a\\x.jpg");
        assert_eq!(join_file_path("C:\\", "x.jpg"), "C:\\x.jpg");
        assert_eq!(join_file_path("/", "x.jpg"), "/x.jpg");
    }

    #[test]
    fn ratings_map_to_stars() {
        let r = |v| rating_from_value(&v).map(Rating::get);
        assert_eq!(r(Value::Null), Some(0));
        assert_eq!(r(Value::Integer(5)), Some(5));
        assert_eq!(r(Value::Real(3.0)), Some(3));
        assert_eq!(r(Value::Real(2.5)), None);
        assert_eq!(r(Value::Integer(6)), None);
        assert_eq!(r(Value::Integer(-1)), None);
        assert_eq!(r(Value::Text("3".into())), None);
    }

    #[test]
    fn pick_values_map_to_flags() {
        let f = |v| flag_from_value(&v);
        assert_eq!(f(Value::Null), Some(Flag::None));
        assert_eq!(f(Value::Integer(0)), Some(Flag::None));
        assert_eq!(f(Value::Real(0.0)), Some(Flag::None));
        assert_eq!(f(Value::Real(-0.0)), Some(Flag::None));
        assert_eq!(f(Value::Integer(1)), Some(Flag::Pick));
        assert_eq!(f(Value::Real(1.0)), Some(Flag::Pick));
        assert_eq!(f(Value::Integer(-1)), Some(Flag::Reject));
        assert_eq!(f(Value::Real(-1.0)), Some(Flag::Reject));
        assert_eq!(f(Value::Integer(2)), None);
        assert_eq!(f(Value::Integer(i64::MAX)), None);
        assert_eq!(f(Value::Real(0.5)), None);
        assert_eq!(f(Value::Real(f64::NAN)), None);
        assert_eq!(f(Value::Text("1".into())), None);
        assert_eq!(f(Value::Blob(vec![1])), None);
    }

    #[test]
    fn row_flag_is_set_and_unknown_becomes_none() {
        let mut r = row();
        r.pick = Value::Integer(-1);
        assert_eq!(
            photo_from_row(r, &mut HashSet::new()).unwrap().flag,
            Flag::Reject
        );
        let mut r = row();
        r.pick = Value::Integer(2);
        assert_eq!(
            photo_from_row(r, &mut HashSet::new()).unwrap().flag,
            Flag::None
        );
    }

    #[test]
    fn valid_row_becomes_photo() {
        let p = photo_from_row(row(), &mut HashSet::new()).unwrap();
        assert_eq!(p.path, std::path::PathBuf::from("/r/a/IMG.jpg"));
        assert_eq!(p.lightroom_id, 1);
        assert_eq!(p.rating.get(), 0);
    }

    #[test]
    fn global_id_is_kept_byte_for_byte() {
        let mut r = row();
        r.global_id = Some(" a\u{1F305} ".into());
        let p = photo_from_row(r, &mut HashSet::new()).unwrap();
        assert_eq!(p.global_id.as_deref(), Some(" a\u{1F305} "));
    }

    #[test]
    fn missing_or_blank_global_id_is_skipped() {
        for g in [None, Some(""), Some(" \t\n")] {
            let mut r = row();
            r.global_id = g.map(str::to_owned);
            assert_eq!(
                photo_from_row(r, &mut HashSet::new()).unwrap_err(),
                "photo 1: missing global id"
            );
        }
    }

    #[test]
    fn duplicate_global_id_is_skipped_without_naming_it() {
        let mut used = HashSet::new();
        assert!(photo_from_row(row(), &mut used).is_ok());
        let mut r = row();
        r.image_id = 2;
        assert_eq!(
            photo_from_row(r, &mut used).unwrap_err(),
            "photo 2: duplicate global id"
        );
    }

    #[test]
    fn skipped_photo_does_not_use_its_global_id() {
        // Lead decision: only a converted photo claims its global id, so a
        // later row with the same id is converted instead.
        let mut used = HashSet::new();
        let mut bad = row();
        bad.rating = Value::Integer(9);
        assert!(photo_from_row(bad, &mut used).is_err());
        let mut copy = row();
        copy.master = Some(5);
        assert!(photo_from_row(copy, &mut used).is_err());
        assert!(used.is_empty());
        let mut later = row();
        later.image_id = 2;
        assert_eq!(photo_from_row(later, &mut used).unwrap().lightroom_id, 2);
    }

    #[test]
    fn global_id_check_order() {
        // Virtual copy and video come before the global id check ...
        let mut r = row();
        r.global_id = None;
        r.master = Some(3);
        assert!(
            photo_from_row(r, &mut HashSet::new())
                .unwrap_err()
                .contains("virtual copy")
        );
        let mut r = row();
        r.global_id = None;
        r.file_format = Some("VIDEO".into());
        assert!(
            photo_from_row(r, &mut HashSet::new())
                .unwrap_err()
                .contains(VIDEO_SKIP_REASON)
        );
        // ... missing rows and the rating after it.
        let mut r = row();
        r.global_id = None;
        r.file = None;
        r.rating = Value::Integer(9);
        assert_eq!(
            photo_from_row(r, &mut HashSet::new()).unwrap_err(),
            "photo 1: missing global id"
        );
    }

    #[test]
    fn root_folder_without_path_is_skipped() {
        let mut r = row();
        r.root_path = None;
        assert!(
            photo_from_row(r, &mut HashSet::new())
                .unwrap_err()
                .contains("root")
        );
    }

    #[test]
    fn skip_reasons_do_not_leak_names() {
        let mut r = row();
        r.rating = Value::Integer(9);
        let reason = photo_from_row(r, &mut HashSet::new()).unwrap_err();
        assert!(!reason.contains("IMG") && !reason.contains("/r/"));
    }

    #[test]
    fn virtual_copy_reason_does_not_say_missing() {
        let mut r = row();
        r.master = Some(7);
        let reason = photo_from_row(r, &mut HashSet::new()).unwrap_err();
        assert!(reason.contains("virtual copy") && !reason.contains("missing"));
    }

    #[test]
    fn video_is_detected_by_file_format_or_extension() {
        assert!(is_video(Some("VIDEO"), Some("jpg")));
        assert!(is_video(Some("VIDEO"), None));
        assert!(is_video(Some("JPG"), Some("MP4")));
        assert!(is_video(None, Some("m2ts")));
        for ext in VIDEO_EXTENSIONS {
            assert!(is_video(None, Some(&ext.to_uppercase())), "{ext}");
        }
    }

    #[test]
    fn similar_formats_are_not_videos() {
        assert!(!is_video(Some("JPG"), Some("mpo")));
        assert!(!is_video(Some("video"), Some("jpg")));
        assert!(!is_video(Some("RAW"), Some("dng")));
        assert!(!is_video(None, Some("")));
        assert!(!is_video(None, Some(".mp4")));
        assert!(!is_video(None, None));
    }

    #[test]
    fn video_row_is_skipped_with_video_reason() {
        let mut r = row();
        r.image_id = 4;
        r.file = Some(("clip".into(), "MOV".into()));
        assert_eq!(
            photo_from_row(r, &mut HashSet::new()).unwrap_err(),
            format!("photo 4: {VIDEO_SKIP_REASON}")
        );
    }

    #[test]
    fn video_check_comes_before_missing_rows_and_rating() {
        let mut r = row();
        r.file_format = Some("VIDEO".into());
        r.file = None;
        r.rating = Value::Integer(9);
        assert!(
            photo_from_row(r, &mut HashSet::new())
                .unwrap_err()
                .contains(VIDEO_SKIP_REASON)
        );
    }

    #[test]
    fn virtual_copy_of_video_keeps_virtual_copy_reason() {
        let mut r = row();
        r.file_format = Some("VIDEO".into());
        r.master = Some(4);
        let reason = photo_from_row(r, &mut HashSet::new()).unwrap_err();
        assert!(reason.contains("virtual copy") && !reason.contains("video"));
    }

    #[test]
    fn base_name_with_video_suffix_is_not_a_video() {
        let mut r = row();
        r.file = Some(("clip.mp4".into(), "jpg".into()));
        assert!(photo_from_row(r, &mut HashSet::new()).is_ok());
    }
}
