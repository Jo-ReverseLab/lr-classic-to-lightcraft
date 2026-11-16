//! Reading photos (`Adobe_images` joined with `AgLibraryFile`,
//! `AgLibraryFolder` and `AgLibraryRootFolder`).

use crate::catalog::{Catalog, ReadError, TextCell, text_cell, value_or_blob};
use crate::develop::{StoredGeometry, parse_develop_with_geometry};
use crate::folders::join_folder_path;
use crate::xmp::{Unreadable, title_from_blob};
use model::{Develop, Flag, Photo, Rating, Report, SkipCode, Skipped, Subject};
use rusqlite::types::Value;
use std::collections::{BTreeMap, HashSet};

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
    /// `AgLibraryIPTC.caption` of the row with the smallest `id_local` for
    /// this image, if it is non-empty text.
    caption: Option<String>,
    /// `AgLibraryIPTC.copyright` of the same row as the caption, if it is
    /// non-empty text.
    copyright: Option<String>,
    /// `AgInternedIptcCreator.value` referenced by the
    /// `AgHarvestedIptcMetadata` row with the smallest `id_local` for this
    /// image, if it is non-empty text.
    creator: Option<String>,
    /// `Adobe_AdditionalMetadata.xmp` of the row with the smallest
    /// `id_local` for this image, as stored.
    xmp: Value,
    /// `Adobe_images.orientation` if it is text; `None` when the column is
    /// missing.
    orientation: Option<String>,
    /// `Adobe_images.fileWidth` if it is a number; `None` when the column is
    /// missing.
    file_width: Option<f64>,
    /// `Adobe_images.fileHeight` if it is a number; `None` when the column
    /// is missing.
    file_height: Option<f64>,
    file: Option<(TextCell, TextCell)>,
    folder: Option<TextCell>,
    root_row: bool,
    root_path: TextCell,
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
    /// Catalog ids of the returned photos whose XMP blob (the one that
    /// decides the title) cannot be read: too short, not zlib, too large or
    /// not UTF-8. These photos have no title. Ascending.
    pub title_unreadable: Vec<i64>,
    /// Catalog ids of the returned photos whose development-settings text is
    /// unreadable: broken, of an unexpected type or larger than the reader
    /// accepts. A row without text is not listed. These photos are still
    /// returned, with `develop = None` (no settings, no hash). Ascending. They
    /// are not skipped and do not change the report's balance.
    pub develop_invalid: Vec<i64>,
    /// Catalog ids of the returned photos whose settings text contained the
    /// `-999999` sentinel. They keep their settings, with `deferred` set.
    /// Ascending.
    pub develop_deferred: Vec<i64>,
}

impl Catalog {
    /// Whether the optional creator tables `AgHarvestedIptcMetadata`
    /// (`id_local`, `image`, `creatorRef`) and `AgInternedIptcCreator`
    /// (`id_local`, `value`) exist with the columns the reader uses.
    /// Without them every photo has no creator.
    fn has_creator_tables(&self) -> Result<bool, ReadError> {
        let present: i64 = self
            .conn
            .query_row(
                "SELECT (SELECT count(*) FROM pragma_table_info('AgHarvestedIptcMetadata') \
                         WHERE name IN ('id_local', 'image', 'creatorRef')) \
                      + (SELECT count(*) FROM pragma_table_info('AgInternedIptcCreator') \
                         WHERE name IN ('id_local', 'value'))",
                [],
                |row| row.get(0),
            )
            .map_err(ReadError::Query)?;
        Ok(present == 5)
    }

    /// Whether the optional table `Adobe_AdditionalMetadata` exists with the
    /// columns `id_local`, `image` and `xmp`. Without it no photo has a
    /// title.
    fn has_xmp_table(&self) -> Result<bool, ReadError> {
        let present: i64 = self
            .conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('Adobe_AdditionalMetadata') \
                 WHERE name IN ('id_local', 'image', 'xmp')",
                [],
                |row| row.get(0),
            )
            .map_err(ReadError::Query)?;
        Ok(present == 3)
    }

    /// Whether the optional table `Adobe_imageDevelopSettings` exists with
    /// the columns `id_local`, `image` and `text`. Without it no photo has
    /// development settings.
    fn has_develop_table(&self) -> Result<bool, ReadError> {
        let present: i64 = self
            .conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('Adobe_imageDevelopSettings') \
                 WHERE name IN ('id_local', 'image', 'text')",
                [],
                |row| row.get(0),
            )
            .map_err(ReadError::Query)?;
        Ok(present == 3)
    }

    /// Whether `Adobe_images` has the optional orientation and pixel-size
    /// columns. Without all three every photo counts as upright with no known
    /// stored size.
    fn has_image_geometry(&self) -> Result<bool, ReadError> {
        let present: i64 = self
            .conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('Adobe_images') \
                 WHERE name IN ('orientation', 'fileWidth', 'fileHeight')",
                [],
                |row| row.get(0),
            )
            .map_err(ReadError::Query)?;
        Ok(present == 3)
    }

    /// The stored settings text per image, grouped once by image.
    ///
    /// One row per image is expected; if several exist, the one with the
    /// smallest `id_local` wins, like the other optional tables. A NULL
    /// `text` counts as no settings. The rows are read without touching the
    /// report: a settings row is not a photo.
    fn develop_texts(&self) -> Result<BTreeMap<i64, Value>, ReadError> {
        let mut texts = BTreeMap::new();
        if !self.has_develop_table()? {
            return Ok(texts);
        }
        let mut stmt = self
            .conn
            .prepare("SELECT image, text FROM Adobe_imageDevelopSettings ORDER BY id_local")
            .map_err(ReadError::Query)?;
        let rows = stmt
            .query_map([], |row| {
                let image: Option<i64> = row.get(0)?;
                Ok((image, value_or_blob(row.get_ref(1)?)))
            })
            .map_err(ReadError::Query)?;
        for row in rows {
            let (image, text) = row.map_err(ReadError::Query)?;
            if let Some(image) = image {
                texts.entry(image).or_insert(text);
            }
        }
        Ok(texts)
    }

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
        // Read the optional settings table once, grouped by image, instead of
        // querying it per photo.
        let mut develops = self.develop_texts()?;
        // The optional tables are joined only when present. Each one is
        // grouped once (`image`, smallest `id_local`) instead of being
        // queried per photo: a correlated scalar subquery would scan the
        // whole table for every image, which is quadratic on a catalog
        // without indexes.
        let mut joins = String::new();
        let creator = if self.has_creator_tables()? {
            joins.push_str(
                " LEFT JOIN (SELECT image AS harvested_image, min(id_local) AS harvested_row \
                              FROM AgHarvestedIptcMetadata GROUP BY image) hc \
                         ON hc.harvested_image = i.id_local \
                 LEFT JOIN AgHarvestedIptcMetadata h ON h.id_local = hc.harvested_row \
                 LEFT JOIN AgInternedIptcCreator n ON n.id_local = h.creatorRef",
            );
            "n.value"
        } else {
            "NULL"
        };
        let xmp = if self.has_xmp_table()? {
            joins.push_str(
                " LEFT JOIN (SELECT image AS xmp_image, min(id_local) AS xmp_row \
                              FROM Adobe_AdditionalMetadata GROUP BY image) xa \
                         ON xa.xmp_image = i.id_local \
                 LEFT JOIN Adobe_AdditionalMetadata a ON a.id_local = xa.xmp_row",
            );
            "a.xmp"
        } else {
            "NULL"
        };
        // The orientation and pixel-size columns are optional in a minimal
        // catalog; they are selected as NULL there.
        let (orientation, file_width, file_height) = if self.has_image_geometry()? {
            ("i.orientation", "i.fileWidth", "i.fileHeight")
        } else {
            ("NULL", "NULL", "NULL")
        };
        // Both `AgLibraryIPTC` columns come from the same row: the one with
        // the smallest `id_local` for the image.
        let sql = format!(
            "SELECT i.id_local, i.masterImage, i.rating, i.fileFormat, \
                    f.id_local, f.baseName, f.extension, \
                    d.id_local, d.pathFromRoot, \
                    r.id_local, r.absolutePath, i.id_global, i.pick, i.colorLabels, \
                    c.caption, c.copyright, {creator}, {xmp}, \
                    {orientation}, {file_width}, {file_height} \
             FROM Adobe_images i \
             LEFT JOIN (SELECT image AS iptc_image, min(id_local) AS iptc_row \
                          FROM AgLibraryIPTC GROUP BY image) ic \
                    ON ic.iptc_image = i.id_local \
             LEFT JOIN AgLibraryIPTC c ON c.id_local = ic.iptc_row \
             LEFT JOIN AgLibraryFile f ON f.id_local = i.rootFile \
             LEFT JOIN AgLibraryFolder d ON d.id_local = f.folder \
             LEFT JOIN AgLibraryRootFolder r ON r.id_local = d.rootFolder \
             {joins} \
             ORDER BY i.id_local"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(ReadError::Query)?;
        let rows = stmt
            .query_map([], |row| {
                let file_id: Option<i64> = row.get(4)?;
                let folder_id: Option<i64> = row.get(7)?;
                let root_id: Option<i64> = row.get(9)?;
                Ok(Row {
                    image_id: row.get(0)?,
                    master: row.get(1)?,
                    rating: value_or_blob(row.get_ref(2)?),
                    pick: value_or_blob(row.get_ref(12)?),
                    color_label: text_value(value_or_blob(row.get_ref(13)?))
                        .filter(|t| !t.is_empty()),
                    caption: text_value(value_or_blob(row.get_ref(14)?)).filter(|t| !t.is_empty()),
                    copyright: text_value(value_or_blob(row.get_ref(15)?))
                        .filter(|t| !t.is_empty()),
                    creator: text_value(value_or_blob(row.get_ref(16)?)).filter(|t| !t.is_empty()),
                    xmp: value_or_blob(row.get_ref(17)?),
                    orientation: text_value(value_or_blob(row.get_ref(18)?)),
                    file_width: number_value(value_or_blob(row.get_ref(19)?)),
                    file_height: number_value(value_or_blob(row.get_ref(20)?)),
                    file_format: text_value(value_or_blob(row.get_ref(3)?)),
                    global_id: text_value(value_or_blob(row.get_ref(11)?)),
                    file: match file_id {
                        Some(_) => Some((text_cell(row.get_ref(5)?), text_cell(row.get_ref(6)?))),
                        None => None,
                    },
                    folder: match folder_id {
                        Some(_) => Some(text_cell(row.get_ref(8)?)),
                        None => None,
                    },
                    root_row: root_id.is_some(),
                    root_path: text_cell(row.get_ref(10)?),
                })
            })
            .map_err(ReadError::Query)?;
        let mut data = PhotoData::default();
        let mut used = HashSet::new();
        let mut converted: HashSet<i64> = HashSet::new();
        for row in rows {
            let row = row.map_err(ReadError::Query)?;
            report.read();
            let unknown_flag = flag_from_value(&row.pick).is_none();
            let color_label = row.color_label.clone();
            let title = title_from_value(&row.xmp);
            // The crop is stored on the unoriented image; the settings read
            // needs the stored orientation and size to express it in the
            // displayed frame.
            let geometry =
                StoredGeometry::new(row.orientation.as_deref(), row.file_width, row.file_height);
            match photo_from_row(row, &mut used, &mut converted) {
                Ok(photo) => {
                    let mut photo = match title {
                        Ok(Some(title)) => photo.with_title(title),
                        Ok(None) => photo,
                        Err(Unreadable) => {
                            data.title_unreadable.push(photo.lightroom_id);
                            photo
                        }
                    };
                    if let Some(value) = develops.remove(&photo.lightroom_id) {
                        match develop_from_value(value, geometry) {
                            Ok(Some(develop)) => {
                                if develop.deferred {
                                    data.develop_deferred.push(photo.lightroom_id);
                                }
                                photo = photo.with_develop(develop);
                            }
                            Ok(None) => {}
                            Err(()) => data.develop_invalid.push(photo.lightroom_id),
                        }
                    }
                    if unknown_flag {
                        data.unknown_flag.push(photo.lightroom_id);
                    }
                    if let Some(label) = color_label {
                        data.color_labels.push((photo.lightroom_id, label));
                    }
                    data.photos.push(photo);
                }
                Err(skip) => report.push_skipped(skip),
            }
        }
        Ok(data)
    }
}

/// Parses one stored settings value.
///
/// `Ok(None)` means the column is NULL, so the photo has no settings row
/// value. Any other non-text value, and any text the parser rejects, is an
/// error and makes the photo unreadable, not skipped.
fn develop_from_value(value: Value, geometry: StoredGeometry) -> Result<Option<Develop>, ()> {
    match value {
        Value::Null => Ok(None),
        Value::Text(text) => parse_develop_with_geometry(&text, geometry)
            .map(Some)
            .map_err(|_| ()),
        _ => Err(()),
    }
}

/// The title stored in an XMP column value. NULL gives no title; a blob
/// (or text) is unpacked; any other type cannot be read.
fn title_from_value(value: &Value) -> Result<Option<String>, Unreadable> {
    match value {
        Value::Null => Ok(None),
        Value::Blob(b) => title_from_blob(b),
        Value::Text(t) => title_from_blob(t.as_bytes()),
        Value::Integer(_) | Value::Real(_) => Err(Unreadable),
    }
}

/// Turns a joined row into a photo, or returns the reason it is skipped.
///
/// Reasons name the photo's catalog id but never paths, file names or the
/// global id. The checks run in this order: virtual copy whose master was not
/// converted, video, missing or duplicate global id, file, folder and root
/// folder rows, rating. A virtual copy whose master was converted is itself
/// converted with its `master` set; the writer then creates it once with
/// `photo.virtualCopy`. A copy whose master was skipped (a video, a missing
/// original, ...) cannot be created and is skipped here with the reason
/// `virtual_copy`.
///
/// `used` holds the global ids of the photos converted so far (rows are
/// read in `id_local` order). Only a converted photo adds its global id, so
/// a row skipped for any reason does not block a later row with the same
/// global id. `converted` holds the `lightroom_id` of every photo converted
/// so far, so a copy can tell whether its master was converted (masters are
/// read before their copies).
fn photo_from_row(
    row: Row,
    used: &mut HashSet<String>,
    converted: &mut HashSet<i64>,
) -> Result<Photo, Skipped> {
    let id = row.image_id;
    let skip = |code, reason: String| Skipped::new(code, reason).about(Subject::Photo(id));
    let master = row.master;
    if let Some(master) = master
        && !converted.contains(&master)
    {
        return Err(skip(
            SkipCode::VirtualCopy,
            format!(
                "photo {id}: virtual copy of photo {master} cannot be created, \
                 the master was not converted"
            ),
        ));
    }
    // An unusable extension is not a video extension; the path check below
    // skips the row for it.
    let ext = row.file.as_ref().and_then(|(_, ext)| match ext {
        TextCell::Text(name) => Some(name.as_str()),
        _ => None,
    });
    if is_video(row.file_format.as_deref(), ext) {
        return Err(skip(
            SkipCode::Video,
            format!("photo {id}: {VIDEO_SKIP_REASON}"),
        ));
    }
    let global_id = match row.global_id {
        Some(g) if !g.trim().is_empty() => g,
        _ => {
            return Err(skip(
                SkipCode::MissingGlobalId,
                format!("photo {id}: missing global id"),
            ));
        }
    };
    if used.contains(&global_id) {
        return Err(skip(
            SkipCode::DuplicateGlobalId,
            format!("photo {id}: duplicate global id"),
        ));
    }
    let (base, ext) = row.file.ok_or_else(|| {
        skip(
            SkipCode::FileRowNotFound,
            format!("photo {id}: file row not found in catalog"),
        )
    })?;
    let base = match base {
        TextCell::Text(name) => name,
        TextCell::Null => String::new(),
        cell => return Err(unreadable_path(id, "file name", &cell)),
    };
    let ext = match ext {
        TextCell::Text(name) => name,
        TextCell::Null => String::new(),
        cell => return Err(unreadable_path(id, "file extension", &cell)),
    };
    let rel = match row.folder.ok_or_else(|| {
        skip(
            SkipCode::FolderRowNotFound,
            format!("photo {id}: folder row not found in catalog"),
        )
    })? {
        TextCell::Text(path) => path,
        TextCell::Null => String::new(),
        cell => return Err(unreadable_path(id, "folder path", &cell)),
    };
    if !row.root_row {
        return Err(skip(
            SkipCode::RootFolderNotFound,
            format!("photo {id}: root folder row not found in catalog"),
        ));
    }
    let root = match row.root_path {
        TextCell::Text(path) => path,
        TextCell::Null => {
            return Err(skip(
                SkipCode::RootFolderNoPath,
                format!("photo {id}: root folder has no path"),
            ));
        }
        cell => return Err(unreadable_path(id, "root folder path", &cell)),
    };
    let rating = rating_from_value(&row.rating).ok_or_else(|| {
        skip(
            SkipCode::InvalidRating,
            format!("photo {id}: rating is not a whole number in 0..=5"),
        )
    })?;
    let flag = flag_from_value(&row.pick).unwrap_or_default();
    let folder = join_folder_path(&root, &rel);
    used.insert(global_id.clone());
    converted.insert(id);
    let photo = Photo::new(join_file_path(&folder, &file_name(&base, &ext)), id, rating)
        .with_global_id(global_id)
        .with_flag(flag);
    let photo = match master {
        Some(master) => photo.with_master(master),
        None => photo,
    };
    let photo = match row.caption {
        Some(caption) => photo.with_caption(caption),
        None => photo,
    };
    let photo = match row.copyright {
        Some(copyright) => photo.with_copyright(copyright),
        None => photo,
    };
    Ok(match row.creator {
        Some(creator) => photo.with_creator(creator),
        None => photo,
    })
}

/// Skip reason of a video, after the `photo {id}: ` prefix.
pub const VIDEO_SKIP_REASON: &str = "video files are not supported by LightCraft";

/// The skip entry of a photo whose path component is unusable. The reason
/// names the photo id and the component, never the stored bytes.
fn unreadable_path(id: i64, component: &str, cell: &TextCell) -> Skipped {
    Skipped::new(
        SkipCode::PathNotUtf8,
        format!(
            "photo {id}: {component} {}",
            cell.problem().unwrap_or("is not usable text")
        ),
    )
}

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

/// A stored value as a finite number if it is one; integer columns count.
fn number_value(value: Value) -> Option<f64> {
    match value {
        Value::Integer(v) => Some(v as f64),
        Value::Real(v) if v.is_finite() => Some(v),
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

    fn text(s: &str) -> TextCell {
        TextCell::Text(s.to_owned())
    }

    fn row() -> Row {
        Row {
            image_id: 1,
            master: None,
            file_format: Some("JPG".into()),
            global_id: Some("g-1".into()),
            rating: Value::Null,
            pick: Value::Null,
            color_label: None,
            caption: None,
            copyright: None,
            creator: None,
            xmp: Value::Null,
            orientation: None,
            file_width: None,
            file_height: None,
            file: Some((text("IMG"), text("jpg"))),
            folder: Some(text("a/")),
            root_row: true,
            root_path: text("/r/"),
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
    fn caption_of_the_row_is_carried_by_the_photo() {
        let mut r = row();
        r.caption = Some(" a ".into());
        let p = photo_from_row(r, &mut HashSet::new(), &mut HashSet::new()).unwrap();
        assert_eq!(p.caption.as_deref(), Some(" a "));
        let p = photo_from_row(row(), &mut HashSet::new(), &mut HashSet::new()).unwrap();
        assert_eq!(p.caption, None);
    }

    #[test]
    fn copyright_and_creator_of_the_row_are_carried_by_the_photo() {
        let mut r = row();
        r.copyright = Some(" c ".into());
        r.creator = Some(" a ".into());
        let p = photo_from_row(r, &mut HashSet::new(), &mut HashSet::new()).unwrap();
        assert_eq!(p.copyright.as_deref(), Some(" c "));
        assert_eq!(p.creator.as_deref(), Some(" a "));
        let p = photo_from_row(row(), &mut HashSet::new(), &mut HashSet::new()).unwrap();
        assert_eq!((p.copyright, p.creator), (None, None));
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
            photo_from_row(r, &mut HashSet::new(), &mut HashSet::new())
                .unwrap()
                .flag,
            Flag::Reject
        );
        let mut r = row();
        r.pick = Value::Integer(2);
        assert_eq!(
            photo_from_row(r, &mut HashSet::new(), &mut HashSet::new())
                .unwrap()
                .flag,
            Flag::None
        );
    }

    #[test]
    fn valid_row_becomes_photo() {
        let p = photo_from_row(row(), &mut HashSet::new(), &mut HashSet::new()).unwrap();
        assert_eq!(p.path, std::path::PathBuf::from("/r/a/IMG.jpg"));
        assert_eq!(p.lightroom_id, 1);
        assert_eq!(p.rating.get(), 0);
    }

    #[test]
    fn global_id_is_kept_byte_for_byte() {
        let mut r = row();
        r.global_id = Some(" a\u{1F305} ".into());
        let p = photo_from_row(r, &mut HashSet::new(), &mut HashSet::new()).unwrap();
        assert_eq!(p.global_id.as_deref(), Some(" a\u{1F305} "));
    }

    #[test]
    fn missing_or_blank_global_id_is_skipped() {
        for g in [None, Some(""), Some(" \t\n")] {
            let mut r = row();
            r.global_id = g.map(str::to_owned);
            assert_eq!(
                photo_from_row(r, &mut HashSet::new(), &mut HashSet::new())
                    .unwrap_err()
                    .reason,
                "photo 1: missing global id"
            );
        }
    }

    #[test]
    fn duplicate_global_id_is_skipped_without_naming_it() {
        let mut used = HashSet::new();
        let mut converted = HashSet::new();
        assert!(photo_from_row(row(), &mut used, &mut converted).is_ok());
        let mut r = row();
        r.image_id = 2;
        assert_eq!(
            photo_from_row(r, &mut used, &mut converted)
                .unwrap_err()
                .reason,
            "photo 2: duplicate global id"
        );
    }

    #[test]
    fn skipped_photo_does_not_use_its_global_id() {
        // Lead decision: only a converted photo claims its global id, so a
        // later row with the same id is converted instead.
        let mut used = HashSet::new();
        let mut converted = HashSet::new();
        let mut bad = row();
        bad.rating = Value::Integer(9);
        assert!(photo_from_row(bad, &mut used, &mut converted).is_err());
        let mut copy = row();
        copy.master = Some(5);
        assert!(photo_from_row(copy, &mut used, &mut converted).is_err());
        assert!(used.is_empty());
        let mut later = row();
        later.image_id = 2;
        assert_eq!(
            photo_from_row(later, &mut used, &mut converted)
                .unwrap()
                .lightroom_id,
            2
        );
    }

    #[test]
    fn virtual_copy_of_a_converted_master_is_converted_with_master() {
        // A copy whose master was converted is itself converted and carries
        // the master's Lightroom id; the writer then creates it once.
        let mut used = HashSet::new();
        let mut converted = HashSet::new();
        let mut master = row();
        master.image_id = 1;
        master.global_id = Some("g-1".into());
        let master_photo = photo_from_row(master, &mut used, &mut converted).unwrap();
        assert_eq!(master_photo.master, None);
        let mut copy = row();
        copy.image_id = 2;
        copy.global_id = Some("g-2".into());
        copy.master = Some(1);
        let copy_photo = photo_from_row(copy, &mut used, &mut converted).unwrap();
        assert_eq!(copy_photo.lightroom_id, 2);
        assert_eq!(copy_photo.master, Some(1));
    }

    #[test]
    fn global_id_check_order() {
        // Virtual copy and video come before the global id check ...
        let mut r = row();
        r.global_id = None;
        r.master = Some(3);
        assert!(
            photo_from_row(r, &mut HashSet::new(), &mut HashSet::new())
                .unwrap_err()
                .reason
                .contains("virtual copy")
        );
        let mut r = row();
        r.global_id = None;
        r.file_format = Some("VIDEO".into());
        assert!(
            photo_from_row(r, &mut HashSet::new(), &mut HashSet::new())
                .unwrap_err()
                .reason
                .contains(VIDEO_SKIP_REASON)
        );
        // ... missing rows and the rating after it.
        let mut r = row();
        r.global_id = None;
        r.file = None;
        r.rating = Value::Integer(9);
        assert_eq!(
            photo_from_row(r, &mut HashSet::new(), &mut HashSet::new())
                .unwrap_err()
                .reason,
            "photo 1: missing global id"
        );
    }

    #[test]
    fn every_skip_has_its_code() {
        let code = |r: Row| {
            photo_from_row(r, &mut HashSet::new(), &mut HashSet::new())
                .unwrap_err()
                .code
        };
        let mut r = row();
        r.master = Some(3);
        assert_eq!(code(r), SkipCode::VirtualCopy);
        let mut r = row();
        r.file_format = Some("VIDEO".into());
        assert_eq!(code(r), SkipCode::Video);
        let mut r = row();
        r.global_id = None;
        assert_eq!(code(r), SkipCode::MissingGlobalId);
        let mut used = HashSet::new();
        let mut converted = HashSet::new();
        assert!(photo_from_row(row(), &mut used, &mut converted).is_ok());
        assert_eq!(
            photo_from_row(row(), &mut used, &mut converted)
                .unwrap_err()
                .code,
            SkipCode::DuplicateGlobalId
        );
        let mut r = row();
        r.file = None;
        assert_eq!(code(r), SkipCode::FileRowNotFound);
        let mut r = row();
        r.folder = None;
        assert_eq!(code(r), SkipCode::FolderRowNotFound);
        let mut r = row();
        r.root_row = false;
        assert_eq!(code(r), SkipCode::RootFolderNotFound);
        let mut r = row();
        r.root_path = TextCell::Null;
        assert_eq!(code(r), SkipCode::RootFolderNoPath);
        let mut r = row();
        r.rating = Value::Integer(9);
        assert_eq!(code(r), SkipCode::InvalidRating);
    }

    #[test]
    fn root_folder_without_path_is_skipped() {
        let mut r = row();
        r.root_path = TextCell::Null;
        assert!(
            photo_from_row(r, &mut HashSet::new(), &mut HashSet::new())
                .unwrap_err()
                .reason
                .contains("root")
        );
    }

    #[test]
    fn skip_reasons_do_not_leak_names() {
        let mut r = row();
        r.rating = Value::Integer(9);
        let reason = photo_from_row(r, &mut HashSet::new(), &mut HashSet::new())
            .unwrap_err()
            .reason;
        assert!(!reason.contains("IMG") && !reason.contains("/r/"));
    }

    #[test]
    fn virtual_copy_reason_does_not_say_missing() {
        let mut r = row();
        r.master = Some(7);
        let reason = photo_from_row(r, &mut HashSet::new(), &mut HashSet::new())
            .unwrap_err()
            .reason;
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
        r.file = Some((text("clip"), text("MOV")));
        assert_eq!(
            photo_from_row(r, &mut HashSet::new(), &mut HashSet::new())
                .unwrap_err()
                .reason,
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
            photo_from_row(r, &mut HashSet::new(), &mut HashSet::new())
                .unwrap_err()
                .reason
                .contains(VIDEO_SKIP_REASON)
        );
    }

    #[test]
    fn virtual_copy_of_video_keeps_virtual_copy_reason() {
        let mut r = row();
        r.file_format = Some("VIDEO".into());
        r.master = Some(4);
        let reason = photo_from_row(r, &mut HashSet::new(), &mut HashSet::new())
            .unwrap_err()
            .reason;
        assert!(reason.contains("virtual copy") && !reason.contains("video"));
    }

    #[test]
    fn base_name_with_video_suffix_is_not_a_video() {
        let mut r = row();
        r.file = Some((text("clip.mp4"), text("jpg")));
        assert!(photo_from_row(r, &mut HashSet::new(), &mut HashSet::new()).is_ok());
    }

    /// Reads a generated catalog and returns the photo with `id`.
    fn photo_with(catalog: &Catalog, id: i64) -> Photo {
        let mut report = Report::new();
        let data = catalog.photo_data(&mut report).unwrap();
        data.photos
            .iter()
            .find(|p| p.lightroom_id == id)
            .cloned()
            .unwrap_or_else(|| panic!("photo {id} is not converted"))
    }

    #[test]
    fn caption_of_one_image_comes_from_its_smallest_id() {
        use lrcat_fixtures::caption_edge as e;
        let dir = tempfile::tempdir().unwrap();
        let path = lrcat_fixtures::write_caption_catalog_with_files(dir.path()).unwrap();
        let catalog = Catalog::open(&path).unwrap();
        assert_eq!(
            photo_with(&catalog, e::DUPLICATE_ID).caption.as_deref(),
            Some(e::DUPLICATE_TEXT)
        );
        assert_eq!(
            photo_with(&catalog, e::DUPLICATE_NULL_FIRST_ID).caption,
            None
        );
        assert_eq!(photo_with(&catalog, e::NON_TEXT_ID).caption, None);
    }

    #[test]
    fn copyright_and_creator_of_one_image_come_from_its_smallest_id() {
        use lrcat_fixtures::rights_edge as e;
        let dir = tempfile::tempdir().unwrap();
        let path = lrcat_fixtures::write_rights_catalog_with_files(dir.path()).unwrap();
        let catalog = Catalog::open(&path).unwrap();
        let duplicate = photo_with(&catalog, e::DUPLICATE_ID);
        assert_eq!(duplicate.copyright.as_deref(), Some(e::DUPLICATE_COPYRIGHT));
        // The smallest harvested row has no creator, even though a later one
        // names one.
        assert_eq!(duplicate.creator, None);
        assert_eq!(
            photo_with(&catalog, e::TWO_HARVESTED_ID).creator.as_deref(),
            Some(e::FIRST_CREATOR)
        );
        let null_first = photo_with(&catalog, e::DUPLICATE_NULL_FIRST_ID);
        assert_eq!(null_first.copyright, None);
        assert_eq!(null_first.creator, None);
        let non_text = photo_with(&catalog, e::NON_TEXT_ID);
        assert_eq!(non_text.copyright, None);
        assert_eq!(non_text.creator, None);
    }

    #[test]
    fn creator_is_absent_without_the_optional_tables() {
        use lrcat_fixtures::rights_edge as e;
        let dir = tempfile::tempdir().unwrap();
        let path = lrcat_fixtures::write_copyright_only_catalog_with_files(dir.path()).unwrap();
        let catalog = Catalog::open(&path).unwrap();
        let photo = photo_with(&catalog, e::UMLAUT_ID);
        assert_eq!(photo.copyright.as_deref(), Some(e::UMLAUT_COPYRIGHT));
        assert_eq!(photo.creator, None);
    }

    #[test]
    fn title_comes_from_the_smallest_xmp_row() {
        use lrcat_fixtures::title_edge as e;
        let dir = tempfile::tempdir().unwrap();
        let path = lrcat_fixtures::write_title_catalog_with_files(dir.path()).unwrap();
        let catalog = Catalog::open(&path).unwrap();
        assert_eq!(
            photo_with(&catalog, e::DUPLICATE_ID).title.as_deref(),
            Some(e::DUPLICATE_TITLE)
        );
        // A NULL xmp in the smallest row wins over a later valid title.
        assert_eq!(photo_with(&catalog, e::NULL_FIRST_ID).title, None);
        // A broken blob in the smallest row wins over a later valid title.
        assert_eq!(photo_with(&catalog, e::BROKEN_FIRST_ID).title, None);
    }

    #[test]
    fn invalid_utf8_or_non_text_path_components_are_skipped() {
        let skip =
            |file: Option<(TextCell, TextCell)>, folder: Option<TextCell>, root_path: TextCell| {
                let mut r = row();
                r.file = file;
                r.folder = folder;
                r.root_path = root_path;
                photo_from_row(r, &mut HashSet::new(), &mut HashSet::new()).unwrap_err()
            };
        let base = skip(
            Some((TextCell::InvalidUtf8, text("jpg"))),
            Some(text("a/")),
            text("/r/"),
        );
        assert_eq!(base.code, SkipCode::PathNotUtf8);
        assert_eq!(base.reason, "photo 1: file name is not valid UTF-8");
        let ext = skip(
            Some((text("IMG"), TextCell::NotText)),
            Some(text("a/")),
            text("/r/"),
        );
        assert_eq!(ext.code, SkipCode::PathNotUtf8);
        assert_eq!(ext.reason, "photo 1: file extension is not text");
        let folder = skip(
            Some((text("IMG"), text("jpg"))),
            Some(TextCell::InvalidUtf8),
            text("/r/"),
        );
        assert_eq!(folder.reason, "photo 1: folder path is not valid UTF-8");
        let root = skip(
            Some((text("IMG"), text("jpg"))),
            Some(text("a/")),
            TextCell::NotText,
        );
        assert_eq!(root.reason, "photo 1: root folder path is not text");
    }

    /// Writes the small fixture and stores a raw value in one column.
    fn catalog_with_sql(sql: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photos.lrcat");
        lrcat_fixtures::write_small_catalog(&path).expect("generate fixture");
        let conn = rusqlite::Connection::open(&path).expect("open for setup");
        conn.execute_batch(sql).expect("setup");
        drop(conn);
        (dir, path)
    }

    fn read_photos(path: &std::path::Path) -> (PhotoData, Report) {
        let catalog = Catalog::open(path).expect("open catalog");
        let mut report = Report::new();
        let data = catalog.photo_data(&mut report).expect("read photos");
        (data, report)
    }

    #[test]
    fn invalid_utf8_text_photo_file_name_is_skipped() {
        let (_dir, path) = catalog_with_sql(
            "UPDATE AgLibraryFile SET baseName = CAST(x'FFFE' AS TEXT) WHERE id_local = 1;",
        );
        let (data, report) = read_photos(&path);
        let skipped = report
            .skipped_items()
            .iter()
            .find(|s| s.reason.starts_with("photo 1: "))
            .expect("photo 1 is skipped");
        assert_eq!(skipped.code, SkipCode::PathNotUtf8);
        assert_eq!(skipped.reason, "photo 1: file name is not valid UTF-8");
        assert!(data.photos.iter().all(|p| p.lightroom_id != 1));
        assert_eq!(
            report.read_count(),
            data.photos.len() + report.skipped_count()
        );
    }

    #[test]
    fn invalid_utf8_text_photo_extension_is_skipped() {
        let (_dir, path) = catalog_with_sql(
            "UPDATE AgLibraryFile SET extension = CAST(x'FFFE' AS TEXT) WHERE id_local = 1;",
        );
        let (data, report) = read_photos(&path);
        let skipped = report
            .skipped_items()
            .iter()
            .find(|s| s.reason.starts_with("photo 1: "))
            .expect("photo 1 is skipped");
        assert_eq!(skipped.code, SkipCode::PathNotUtf8);
        assert_eq!(skipped.reason, "photo 1: file extension is not valid UTF-8");
        assert!(
            data.photos.iter().all(|p| p.lightroom_id != 1),
            "photo 1 is not converted"
        );
        assert_eq!(
            report.read_count(),
            data.photos.len() + report.skipped_count()
        );
    }

    #[test]
    fn invalid_utf8_text_photo_folder_path_is_skipped() {
        let (_dir, path) = catalog_with_sql(
            "UPDATE AgLibraryFolder SET pathFromRoot = CAST(x'FFFE' AS TEXT) WHERE id_local = 2;",
        );
        let (data, report) = read_photos(&path);
        let skipped: Vec<_> = report
            .skipped_items()
            .iter()
            .filter(|s| s.code == SkipCode::PathNotUtf8)
            .collect();
        assert_eq!(skipped.len(), 3);
        assert!(
            skipped
                .iter()
                .any(|s| s.reason == "photo 1: folder path is not valid UTF-8")
        );
        for id in [1, 8, 14] {
            assert!(data.photos.iter().all(|p| p.lightroom_id != id));
        }
        assert_eq!(
            report.read_count(),
            data.photos.len() + report.skipped_count()
        );
    }

    #[test]
    fn invalid_utf8_text_photo_root_path_is_skipped() {
        let (_dir, path) = catalog_with_sql(
            "UPDATE AgLibraryRootFolder SET absolutePath = CAST(x'FFFE' AS TEXT) \
             WHERE id_local = 1;",
        );
        let (data, report) = read_photos(&path);
        let skipped: Vec<_> = report
            .skipped_items()
            .iter()
            .filter(|s| s.code == SkipCode::PathNotUtf8)
            .collect();
        assert!(!skipped.is_empty());
        assert!(
            skipped
                .iter()
                .any(|s| s.reason == "photo 1: root folder path is not valid UTF-8")
        );
        assert!(data.photos.iter().all(|p| p.lightroom_id != 1));
        assert_eq!(
            report.read_count(),
            data.photos.len() + report.skipped_count()
        );
    }
}
