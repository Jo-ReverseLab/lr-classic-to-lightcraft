//! Neutral data model shared by the reader and the writer.
//!
//! The model knows nothing about Lightroom's or LightCraft's storage.
//! It only describes folders, photos, keywords, ratings and the run report.

use std::fmt;
use std::path::PathBuf;

/// Errors raised when building model values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    /// A star rating outside 0..=5.
    InvalidRating(u8),
    /// A keyword path that is empty or has an empty or whitespace-only part.
    EmptyKeyword,
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelError::InvalidRating(v) => write!(f, "rating {v} is outside 0..=5"),
            ModelError::EmptyKeyword => {
                write!(f, "keyword path is empty or has an empty part")
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

/// A photo, referenced by the path of its original file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Photo {
    /// Path of the original file. The file itself is never touched.
    pub path: PathBuf,
    /// Id of the photo in the source catalog (provenance, used for idempotency).
    pub lightroom_id: i64,
    /// Star rating.
    pub rating: Rating,
}

impl Photo {
    /// Creates a photo.
    pub fn new(path: impl Into<PathBuf>, lightroom_id: i64, rating: Rating) -> Self {
        Photo {
            path: path.into(),
            lightroom_id,
            rating,
        }
    }
}

/// A keyword with its full hierarchy path, root first.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Keyword {
    path: Vec<String>,
}

impl Keyword {
    /// Creates a keyword from its hierarchy path, e.g. `["Travel", "Italy"]`.
    ///
    /// Returns [`ModelError::EmptyKeyword`] if `parts` is empty or any part
    /// is empty or whitespace-only; a keyword and each of its levels always
    /// have a name.
    pub fn new(parts: Vec<String>) -> Result<Keyword, ModelError> {
        if parts.is_empty() || parts.iter().any(|p| p.trim().is_empty()) {
            return Err(ModelError::EmptyKeyword);
        }
        Ok(Keyword { path: parts })
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

    /// The parent keyword, or `None` for a root keyword.
    pub fn parent(&self) -> Option<Keyword> {
        if self.path.len() > 1 {
            Some(Keyword {
                path: self.path[..self.path.len() - 1].to_vec(),
            })
        } else {
            None
        }
    }

    /// True if `self` is a direct child of `other`.
    pub fn is_child_of(&self, other: &Keyword) -> bool {
        self.path.len() == other.path.len() + 1 && self.path.starts_with(&other.path)
    }
}

/// An element that could not be converted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// Why the element was skipped. Never empty.
    pub reason: String,
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

    /// Records one element skipped, with a reason.
    /// An empty reason is replaced by "unspecified".
    pub fn skipped(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        let reason = if reason.trim().is_empty() {
            Self::UNSPECIFIED_REASON.to_string()
        } else {
            reason
        };
        self.skipped.push(Skipped { reason });
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
