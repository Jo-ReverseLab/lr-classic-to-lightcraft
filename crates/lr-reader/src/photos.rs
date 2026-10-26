//! Reading photos (`Adobe_images` joined with `AgLibraryFile`,
//! `AgLibraryFolder` and `AgLibraryRootFolder`).

use crate::catalog::{Catalog, ReadError};
use crate::folders::join_folder_path;
use model::{Photo, Rating, Report};
use rusqlite::types::Value;

/// One joined row; `None` means the referenced row does not exist.
struct Row {
    image_id: i64,
    master: Option<i64>,
    rating: Value,
    file: Option<(String, String)>,
    folder: Option<String>,
    root_row: bool,
    root_path: Option<String>,
}

impl Catalog {
    /// Reads all photos, ordered by their catalog id.
    ///
    /// Every `Adobe_images` row is counted with [`Report::read`]. Rows that
    /// cannot become a photo (virtual copies, missing file, folder or root
    /// folder rows, ratings outside 0..=5) are recorded with
    /// [`Report::skipped`] and left out. Paths are assembled from stored
    /// strings only; the file system is never accessed. Nothing is marked
    /// converted here.
    pub fn photos(&self, report: &mut Report) -> Result<Vec<Photo>, ReadError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT i.id_local, i.masterImage, i.rating, \
                        f.id_local, f.baseName, f.extension, \
                        d.id_local, d.pathFromRoot, \
                        r.id_local, r.absolutePath \
                 FROM Adobe_images i \
                 LEFT JOIN AgLibraryFile f ON f.id_local = i.rootFile \
                 LEFT JOIN AgLibraryFolder d ON d.id_local = f.folder \
                 LEFT JOIN AgLibraryRootFolder r ON r.id_local = d.rootFolder \
                 ORDER BY i.id_local",
            )
            .map_err(ReadError::Query)?;
        let rows = stmt
            .query_map([], |row| {
                let file_id: Option<i64> = row.get(3)?;
                let folder_id: Option<i64> = row.get(6)?;
                let root_id: Option<i64> = row.get(8)?;
                Ok(Row {
                    image_id: row.get(0)?,
                    master: row.get(1)?,
                    rating: row.get(2)?,
                    file: match file_id {
                        Some(_) => Some((
                            row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                            row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                        )),
                        None => None,
                    },
                    folder: match folder_id {
                        Some(_) => Some(row.get::<_, Option<String>>(7)?.unwrap_or_default()),
                        None => None,
                    },
                    root_row: root_id.is_some(),
                    root_path: row.get(9)?,
                })
            })
            .map_err(ReadError::Query)?;
        let mut photos = Vec::new();
        for row in rows {
            let row = row.map_err(ReadError::Query)?;
            report.read();
            match photo_from_row(row) {
                Ok(photo) => photos.push(photo),
                Err(reason) => report.skipped(reason),
            }
        }
        Ok(photos)
    }
}

/// Turns a joined row into a photo, or returns the reason it is skipped.
///
/// Reasons name the photo's catalog id but never paths or file names.
fn photo_from_row(row: Row) -> Result<Photo, String> {
    let id = row.image_id;
    if let Some(master) = row.master {
        return Err(format!(
            "photo {id}: virtual copy of photo {master} is not converted"
        ));
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
    let folder = join_folder_path(&root, &rel);
    Ok(Photo::new(
        join_file_path(&folder, &file_name(&base, &ext)),
        id,
        rating,
    ))
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
            rating: Value::Null,
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
    fn valid_row_becomes_photo() {
        let p = photo_from_row(row()).unwrap();
        assert_eq!(p.path, std::path::PathBuf::from("/r/a/IMG.jpg"));
        assert_eq!(p.lightroom_id, 1);
        assert_eq!(p.rating.get(), 0);
    }

    #[test]
    fn root_folder_without_path_is_skipped() {
        let mut r = row();
        r.root_path = None;
        assert!(photo_from_row(r).unwrap_err().contains("root"));
    }

    #[test]
    fn skip_reasons_do_not_leak_names() {
        let mut r = row();
        r.rating = Value::Integer(9);
        let reason = photo_from_row(r).unwrap_err();
        assert!(!reason.contains("IMG") && !reason.contains("/r/"));
    }

    #[test]
    fn virtual_copy_reason_does_not_say_missing() {
        let mut r = row();
        r.master = Some(7);
        let reason = photo_from_row(r).unwrap_err();
        assert!(reason.contains("virtual copy") && !reason.contains("missing"));
    }
}
