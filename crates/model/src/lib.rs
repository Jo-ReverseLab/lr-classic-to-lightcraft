//! Neutral data model shared by the reader and the writer.
//!
//! The model knows nothing about Lightroom's or LightCraft's storage.
//! It only describes folders, photos, keywords, ratings, flags, colour labels,
//! collections and the run report.

use std::fmt;
use std::path::PathBuf;

pub mod develop;
pub mod label;
pub mod mapping;
pub mod skip;
pub mod smart;

pub use develop::{
    BrushStroke, CropGeometry, CurvePoint, Curves, Develop, Mask, MaskComponent, MaskOp, MaskShape,
    MergeValue, Point, WhiteBalance, WhiteBalanceHint,
};
pub use label::{ColorLabel, LabelSet};
pub use mapping::{MappedPath, PathMapping, PathMappings};
pub use skip::{SkipCode, Skipped};
pub use smart::{SmartCollection, SmartCombine, SmartRule, SmartRules, SmartValue};

/// Errors raised when building model values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    /// A star rating outside 0..=5.
    InvalidRating(u8),
    /// A keyword path that is empty or has an empty or whitespace-only part.
    EmptyKeyword,
    /// A path mapping with an empty old prefix.
    EmptyMappingPrefix,
    /// A path mapping spec without `=` between old and new prefix.
    InvalidMappingSpec(String),
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelError::InvalidRating(v) => write!(f, "rating {v} is outside 0..=5"),
            ModelError::EmptyKeyword => {
                write!(f, "keyword path is empty or has an empty part")
            }
            ModelError::EmptyMappingPrefix => {
                write!(f, "path mapping has an empty old prefix")
            }
            ModelError::InvalidMappingSpec(spec) => {
                write!(f, "path mapping {spec:?} is not of the form OLD=NEW")
            }
        }
    }
}

impl std::error::Error for ModelError {}

/// A folder that contains photos.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Folder {
    /// Absolute path of the folder.
    pub path: PathBuf,
}

impl Folder {
    /// Creates a folder from its path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Folder { path: path.into() }
    }
}

/// A star rating from 0 (unrated) to 5.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rating(u8);

impl Rating {
    /// Highest valid rating.
    pub const MAX: u8 = 5;

    /// Creates a rating; values above 5 are rejected.
    pub fn new(v: u8) -> Result<Rating, ModelError> {
        if v > Self::MAX {
            Err(ModelError::InvalidRating(v))
        } else {
            Ok(Rating(v))
        }
    }

    /// Returns the number of stars.
    pub fn get(self) -> u8 {
        self.0
    }
}

/// The pick/reject flag of a photo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Flag {
    /// Neither picked nor rejected.
    #[default]
    None,
    /// Picked.
    Pick,
    /// Rejected.
    Reject,
}

/// A photo, referenced by the path of its original file.
///
/// Equality includes the development settings. `Photo` is not `Eq` because
/// those settings hold floating-point values.
#[derive(Debug, Clone, PartialEq)]
pub struct Photo {
    /// Path of the original file. The file itself is never touched.
    pub path: PathBuf,
    /// Id of the photo in the source catalog (provenance, used for idempotency).
    pub lightroom_id: i64,
    /// Star rating.
    pub rating: Rating,
    /// Catalog-independent identity of the photo (Lightroom's `id_global`),
    /// kept byte for byte. Stable across catalogs that hold the same photo,
    /// unlike `lightroom_id`. Internal identity only, never reported.
    pub global_id: Option<String>,
    /// Pick/reject flag.
    pub flag: Flag,
    /// Colour label, if any.
    pub label: Option<ColorLabel>,
    /// Caption, kept byte for byte. `None` if the photo has none.
    pub caption: Option<String>,
    /// Copyright notice, kept byte for byte. `None` if the photo has none.
    pub copyright: Option<String>,
    /// Creator (author), kept byte for byte. `None` if the photo has none.
    pub creator: Option<String>,
    /// Title, kept byte for byte. `None` if the photo has none.
    pub title: Option<String>,
    /// `lightroom_id` of the master photo this is a virtual copy of, or
    /// `None` for a master. A virtual copy shares the master's file but has
    /// its own rating, label, flag and keywords; the writer creates it once
    /// with `photo.virtualCopy` and records it in the mapping file.
    pub master: Option<i64>,
    /// Development settings from the catalog. `None` if the catalog has no
    /// readable settings for this photo.
    pub develop: Option<Develop>,
}

impl Photo {
    /// Creates a photo.
    pub fn new(path: impl Into<PathBuf>, lightroom_id: i64, rating: Rating) -> Self {
        Photo {
            path: path.into(),
            lightroom_id,
            rating,
            global_id: None,
            flag: Flag::None,
            label: None,
            caption: None,
            copyright: None,
            creator: None,
            title: None,
            master: None,
            develop: None,
        }
    }

    /// Returns the photo with its flag set.
    pub fn with_flag(mut self, flag: Flag) -> Self {
        self.flag = flag;
        self
    }

    /// Returns the photo with its global id set.
    pub fn with_global_id(mut self, global_id: impl Into<String>) -> Self {
        self.global_id = Some(global_id.into());
        self
    }

    /// Returns the photo with its colour label set.
    pub fn with_label(mut self, label: ColorLabel) -> Self {
        self.label = Some(label);
        self
    }

    /// Returns the photo with its caption set, kept byte for byte.
    pub fn with_caption(mut self, caption: impl Into<String>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    /// Returns the photo with its copyright notice set, kept byte for byte.
    pub fn with_copyright(mut self, copyright: impl Into<String>) -> Self {
        self.copyright = Some(copyright.into());
        self
    }

    /// Returns the photo with its creator set, kept byte for byte.
    pub fn with_creator(mut self, creator: impl Into<String>) -> Self {
        self.creator = Some(creator.into());
        self
    }

    /// Returns the photo with its title set, kept byte for byte.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Returns the photo with its master set: the `lightroom_id` of the
    /// photo this is a virtual copy of.
    pub fn with_master(mut self, master: i64) -> Self {
        self.master = Some(master);
        self
    }

    /// Returns the photo with its development settings set.
    pub fn with_develop(mut self, develop: Develop) -> Self {
        self.develop = Some(develop);
        self
    }
}

/// A keyword with its full hierarchy path, root first, its synonyms and its
/// attributes.
///
/// Equality compares the path, the synonyms, the attributes and the
/// global id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Keyword {
    path: Vec<String>,
    /// Alternative names of the keyword, in source order. Each is non-empty.
    pub synonyms: Vec<String>,
    /// The keyword is written into exported photos.
    pub include_on_export: bool,
    /// Its parent keywords are exported with it.
    pub export_containing: bool,
    /// Its synonyms are exported with it.
    pub export_synonyms: bool,
    /// The keyword names a person.
    pub person: bool,
    /// Catalog-independent identity of the keyword (Lightroom's
    /// `id_global`), kept byte for byte. Internal identity only, never
    /// reported.
    pub global_id: Option<String>,
    /// The keyword's id in the Lightroom catalog (`AgLibraryKeyword.id_local`).
    /// Reports name a renamed keyword by this id, never by its name.
    pub lightroom_id: Option<i64>,
}

impl Keyword {
    /// Creates a keyword from its hierarchy path, e.g. `["Travel", "Italy"]`.
    ///
    /// Returns [`ModelError::EmptyKeyword`] if `parts` is empty or any part
    /// is empty or whitespace-only; a keyword and each of its levels always
    /// have a name. The keyword starts without synonyms and with the default
    /// attributes: exported with parents and synonyms, not a person.
    pub fn new(parts: Vec<String>) -> Result<Keyword, ModelError> {
        if parts.is_empty() || parts.iter().any(|p| p.trim().is_empty()) {
            return Err(ModelError::EmptyKeyword);
        }
        Ok(Keyword::with_path(parts))
    }

    /// Builds a keyword from a path already known to be valid.
    fn with_path(path: Vec<String>) -> Keyword {
        Keyword {
            path,
            synonyms: Vec::new(),
            include_on_export: true,
            export_containing: true,
            export_synonyms: true,
            person: false,
            global_id: None,
            lightroom_id: None,
        }
    }

    /// Returns the keyword with its global id set.
    pub fn with_global_id(mut self, global_id: impl Into<String>) -> Self {
        self.global_id = Some(global_id.into());
        self
    }

    /// Returns the keyword with its Lightroom id set.
    pub fn with_lightroom_id(mut self, lightroom_id: i64) -> Self {
        self.lightroom_id = Some(lightroom_id);
        self
    }

    /// The keyword's own name (last part of the path).
    pub fn name(&self) -> &str {
        // The constructor guarantees a non-empty path.
        &self.path[self.path.len() - 1]
    }

    /// The full hierarchy path, root first.
    pub fn path(&self) -> &[String] {
        &self.path
    }

    /// Number of levels; 1 for a root keyword.
    pub fn depth(&self) -> usize {
        self.path.len()
    }

    /// The parent keyword, or `None` for a root keyword. The parent is
    /// built from the path only, so it has no synonyms, the default
    /// attributes and no global id.
    pub fn parent(&self) -> Option<Keyword> {
        if self.path.len() > 1 {
            Some(Keyword::with_path(
                self.path[..self.path.len() - 1].to_vec(),
            ))
        } else {
            None
        }
    }

    /// True if `self` is a direct child of `other`.
    pub fn is_child_of(&self, other: &Keyword) -> bool {
        self.path.len() == other.path.len() + 1 && self.path.starts_with(&other.path)
    }
}

/// What a collection is: an album holds photos, a folder holds other
/// collections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollectionKind {
    /// A collection of photos (Lightroom: collection).
    Album,
    /// A container of albums and folders (Lightroom: collection set).
    Folder,
}

/// A collection or collection set with its place in the tree and its
/// members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    /// Lightroom's `id_local` of the collection (provenance).
    pub lightroom_id: i64,
    /// The name, kept byte for byte. Never reported.
    pub name: String,
    pub kind: CollectionKind,
    /// `lightroom_id` of the parent folder; `None` on top level.
    pub parent: Option<i64>,
    /// `lightroom_id`s of the member photos, in source order. Always empty
    /// for folders.
    pub members: Vec<i64>,
}

/// Lightroom's Quick Collection: a single system collection of photos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickCollection {
    /// Lightroom's `id_local` of the collection (provenance).
    pub lightroom_id: i64,
    /// `lightroom_id`s of the member photos, in source order.
    pub members: Vec<i64>,
}

/// A folder stack: photos grouped behind a top photo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stack {
    /// Lightroom's `id_local` of the stack (provenance).
    pub lightroom_id: i64,
    /// `lightroom_id`s of the member photos, the top photo first. At least
    /// two after reading.
    pub members: Vec<i64>,
    /// Whether the stack is collapsed in Lightroom. Only reported, never
    /// written (stacks are written expanded).
    pub collapsed: bool,
}

/// Counts of a run: every element read is either converted or skipped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    read: usize,
    converted: usize,
    skipped: Vec<Skipped>,
}

impl Report {
    /// Reason recorded when a skip is reported without one.
    pub const UNSPECIFIED_REASON: &'static str = "unspecified";

    /// Creates an empty report.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one element read from the source.
    pub fn read(&mut self) {
        self.read += 1;
    }

    /// Records one element converted.
    pub fn converted(&mut self) {
        self.converted += 1;
    }

    /// Records one element skipped, with a reason but no specific code
    /// ([`SkipCode::Unspecified`]). The reader and the writer use [`Self::skip`].
    /// An empty reason is replaced by "unspecified".
    pub fn skipped(&mut self, reason: impl Into<String>) {
        self.skip(SkipCode::Unspecified, reason);
    }

    /// Records one element skipped, with its code and reason.
    /// An empty reason is replaced by "unspecified".
    pub fn skip(&mut self, code: SkipCode, reason: impl Into<String>) {
        self.skipped.push(Skipped::new(code, reason));
    }

    /// Records one skip entry built elsewhere.
    pub fn push_skipped(&mut self, item: Skipped) {
        self.skipped.push(item);
    }

    /// Counts the skipped elements per code, keyed by the code's text.
    pub fn skipped_by_code(&self) -> std::collections::BTreeMap<&'static str, usize> {
        skip::count_by_code(&self.skipped)
    }

    /// Number of elements read.
    pub fn read_count(&self) -> usize {
        self.read
    }

    /// Number of elements converted.
    pub fn converted_count(&self) -> usize {
        self.converted
    }

    /// Number of elements skipped.
    pub fn skipped_count(&self) -> usize {
        self.skipped.len()
    }

    /// All skipped elements with their reasons.
    pub fn skipped_items(&self) -> &[Skipped] {
        &self.skipped
    }

    /// True if read == converted + skipped.
    pub fn is_balanced(&self) -> bool {
        self.converted
            .checked_add(self.skipped.len())
            .is_some_and(|handled| handled == self.read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_records_the_code_and_counts_by_code() {
        let mut r = Report::new();
        r.skip(SkipCode::Video, "v");
        r.skip(SkipCode::Video, "w");
        r.skipped("x");
        r.push_skipped(Skipped::new(SkipCode::Missing, "m"));
        assert_eq!(r.skipped_items()[0].code, SkipCode::Video);
        assert_eq!(r.skipped_items()[2].code, SkipCode::Unspecified);
        let by = r.skipped_by_code();
        assert_eq!(by.get("video"), Some(&2));
        assert_eq!(by.get("missing"), Some(&1));
        assert_eq!(by.get("unspecified"), Some(&1));
    }

    #[test]
    fn invalid_rating_error_names_value() {
        let err = Rating::new(7).unwrap_err();
        assert_eq!(err, ModelError::InvalidRating(7));
        assert!(err.to_string().contains('7'));
    }

    #[test]
    fn default_rating_is_zero() {
        assert_eq!(Rating::default().get(), 0);
    }

    #[test]
    fn empty_keyword_path_is_rejected() {
        assert_eq!(Keyword::new(Vec::new()), Err(ModelError::EmptyKeyword));
    }

    #[test]
    fn whitespace_part_in_middle_is_rejected() {
        let parts = vec!["A".into(), "\t ".into(), "C".into()];
        assert_eq!(Keyword::new(parts), Err(ModelError::EmptyKeyword));
    }

    #[test]
    fn empty_keyword_error_has_message() {
        assert!(!ModelError::EmptyKeyword.to_string().is_empty());
    }

    #[test]
    fn new_keyword_has_no_synonyms() {
        let k = Keyword::new(vec!["A".into()]).unwrap();
        assert!(k.synonyms.is_empty());
    }

    #[test]
    fn equality_includes_synonyms() {
        let a = Keyword::new(vec!["A".into()]).unwrap();
        let mut b = a.clone();
        assert_eq!(a, b);
        b.synonyms.push("Alias".into());
        assert_ne!(a, b);
    }

    #[test]
    fn parent_has_no_synonyms() {
        let mut k = Keyword::new(vec!["A".into(), "B".into()]).unwrap();
        k.synonyms.push("Alias".into());
        assert!(k.parent().unwrap().synonyms.is_empty());
    }

    #[test]
    fn new_keyword_has_default_attributes() {
        let k = Keyword::new(vec!["A".into()]).unwrap();
        assert!(k.include_on_export && k.export_containing && k.export_synonyms);
        assert!(!k.person);
    }

    #[test]
    fn equality_includes_attributes() {
        let a = Keyword::new(vec!["A".into()]).unwrap();
        let mut b = a.clone();
        b.person = true;
        assert_ne!(a, b);
        let mut c = a.clone();
        c.include_on_export = false;
        assert_ne!(a, c);
    }

    #[test]
    fn parent_has_default_attributes() {
        let mut k = Keyword::new(vec!["A".into(), "B".into()]).unwrap();
        k.person = true;
        k.export_synonyms = false;
        let p = k.parent().unwrap();
        assert!(!p.person && p.export_synonyms);
    }

    #[test]
    fn parent_has_no_global_id() {
        let k = Keyword::new(vec!["A".into(), "B".into()])
            .unwrap()
            .with_global_id("g");
        assert_eq!(k.parent().unwrap().global_id, None);
    }

    #[test]
    fn photo_equality_includes_global_id() {
        let p = Photo::new("/a.jpg", 1, Rating::default());
        assert_ne!(p.clone().with_global_id("x"), p);
    }

    #[test]
    fn photo_equality_includes_flag() {
        let p = Photo::new("/a.jpg", 1, Rating::default());
        assert_eq!(p.flag, Flag::None);
        assert_ne!(p.clone().with_flag(Flag::Reject), p);
    }

    #[test]
    fn keyword_is_not_child_of_itself() {
        let k = Keyword::new(vec!["A".into()]).unwrap();
        assert!(!k.is_child_of(&k));
    }

    #[test]
    fn non_empty_reason_is_kept() {
        let mut r = Report::new();
        r.skipped("file missing");
        assert_eq!(r.skipped_items()[0].reason, "file missing");
    }

    #[test]
    fn empty_reason_becomes_unspecified() {
        let mut r = Report::new();
        r.skipped(" ");
        assert_eq!(r.skipped_items()[0].reason, Report::UNSPECIFIED_REASON);
    }
}
