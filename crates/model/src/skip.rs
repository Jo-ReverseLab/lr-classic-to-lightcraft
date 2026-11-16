//! Machine-readable codes for skipped elements.
//!
//! Every skipped (or incomplete) element carries a free-text reason for
//! people and a stable code for tools. The code is set where the reason
//! arises (reader or writer), never derived from the text afterwards. A code
//! is a fixed snake_case key and never contains a value from the catalog.

use crate::entry::Subject;
use std::collections::BTreeMap;
use std::fmt;

/// Why an element was skipped, as a stable machine-readable key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SkipCode {
    // Reader: folders (and photos).
    NoRootFolder,
    RootFolderNotFound,
    RootFolderNoPath,
    // Reader: photos.
    /// A virtual copy whose master was not converted (reader), or whose master
    /// has no LightCraft id yet when the copy is to be created (writer). Emitted
    /// by both the reader and the writer; never created blindly.
    VirtualCopy,
    Video,
    Missing,
    MissingGlobalId,
    DuplicateGlobalId,
    FileRowNotFound,
    FolderRowNotFound,
    InvalidRating,
    // Reader: collections.
    CollectionKindUnsupported,
    CollectionKindMissing,
    QuickDuplicate,
    SmartUnreadable,
    SmartNoCriteria,
    SmartUntranslatable,
    NameInvalid,
    ParentInvalid,
    ParentMissing,
    ParentSkipped,
    ParentNotSet,
    ParentCycle,
    // Reader: collection members.
    CollectionIdInvalid,
    CollectionPhotoInvalid,
    CollectionSkipped,
    CollectionUnknown,
    PhotoNotConverted,
    DuplicateMember,
    CollectionIsSet,
    // Reader: stacks and stack members.
    TooFewPhotos,
    StackUnknown,
    StackIdInvalid,
    IdsInvalid,
    PhotoInvalid,
    AlreadyInStack,
    StackSkipped,
    // Reader: keywords, assignments and synonyms.
    NameEmpty,
    NameHasBar,
    AssignmentPhotoSkipped,
    AssignmentKeywordSkipped,
    KeywordSkipped,
    KeywordUnknown,
    /// A keyword whose resolution hit a broken internal invariant (its
    /// global-id result or its resolution entry was missing). Emitted only
    /// as a defensive skip so a broken invariant is reported instead of
    /// aborting the run; not reachable through normal catalog input.
    KeywordUnresolved,
    // Writer.
    KeywordChanged,
    /// A path or file name cannot be used as text: a catalog path or file
    /// name read by the reader (bytes that are not valid UTF-8, or a
    /// non-text value), or a model path the writer would send (not valid
    /// UTF-8). Emitted by both; the element is skipped instead of being
    /// lossily rewritten.
    PathNotUtf8,
    NoGlobalId,
    NoLightcraftId,
    Ambiguous,
    ParentNotCreated,
    ParentFailed,
    NoIdReturned,
    AppliedNotSaved,
    ImportFailed,
    Rejected,
    /// A photo whose file has the same bytes as another photo of the run:
    /// the separate LightCraft record (a copy linked to its own file) could
    /// not be made.
    SameContent,
    // Dry run: path mapping.
    /// A path mapping would climb out of the new prefix via `..`; the path is
    /// left unchanged and the element is not converted.
    PathMappingEscaped,
    /// Two photos resolve to the same path after mapping; the later one is not
    /// converted to avoid a silent merge.
    PathMappingCollision,
    /// A skip recorded without a specific code. Not used by the reader or
    /// the writer; kept for callers of [`crate::Report::skipped`].
    Unspecified,
}

impl SkipCode {
    /// Every code, in declaration order.
    pub const ALL: [SkipCode; 59] = [
        SkipCode::NoRootFolder,
        SkipCode::RootFolderNotFound,
        SkipCode::RootFolderNoPath,
        SkipCode::VirtualCopy,
        SkipCode::Video,
        SkipCode::Missing,
        SkipCode::MissingGlobalId,
        SkipCode::DuplicateGlobalId,
        SkipCode::FileRowNotFound,
        SkipCode::FolderRowNotFound,
        SkipCode::InvalidRating,
        SkipCode::CollectionKindUnsupported,
        SkipCode::CollectionKindMissing,
        SkipCode::QuickDuplicate,
        SkipCode::SmartUnreadable,
        SkipCode::SmartNoCriteria,
        SkipCode::SmartUntranslatable,
        SkipCode::NameInvalid,
        SkipCode::ParentInvalid,
        SkipCode::ParentMissing,
        SkipCode::ParentSkipped,
        SkipCode::ParentNotSet,
        SkipCode::ParentCycle,
        SkipCode::CollectionIdInvalid,
        SkipCode::CollectionPhotoInvalid,
        SkipCode::CollectionSkipped,
        SkipCode::CollectionUnknown,
        SkipCode::PhotoNotConverted,
        SkipCode::DuplicateMember,
        SkipCode::CollectionIsSet,
        SkipCode::TooFewPhotos,
        SkipCode::StackUnknown,
        SkipCode::StackIdInvalid,
        SkipCode::IdsInvalid,
        SkipCode::PhotoInvalid,
        SkipCode::AlreadyInStack,
        SkipCode::StackSkipped,
        SkipCode::NameEmpty,
        SkipCode::NameHasBar,
        SkipCode::AssignmentPhotoSkipped,
        SkipCode::AssignmentKeywordSkipped,
        SkipCode::KeywordSkipped,
        SkipCode::KeywordUnknown,
        SkipCode::KeywordUnresolved,
        SkipCode::KeywordChanged,
        SkipCode::PathNotUtf8,
        SkipCode::NoGlobalId,
        SkipCode::NoLightcraftId,
        SkipCode::Ambiguous,
        SkipCode::ParentNotCreated,
        SkipCode::ParentFailed,
        SkipCode::NoIdReturned,
        SkipCode::AppliedNotSaved,
        SkipCode::ImportFailed,
        SkipCode::Rejected,
        SkipCode::SameContent,
        SkipCode::PathMappingEscaped,
        SkipCode::PathMappingCollision,
        SkipCode::Unspecified,
    ];

    /// The snake_case key written to the report.
    pub fn as_str(self) -> &'static str {
        match self {
            SkipCode::NoRootFolder => "no_root_folder",
            SkipCode::RootFolderNotFound => "root_folder_not_found",
            SkipCode::RootFolderNoPath => "root_folder_no_path",
            SkipCode::VirtualCopy => "virtual_copy",
            SkipCode::Video => "video",
            SkipCode::Missing => "missing",
            SkipCode::MissingGlobalId => "missing_global_id",
            SkipCode::DuplicateGlobalId => "duplicate_global_id",
            SkipCode::FileRowNotFound => "file_row_not_found",
            SkipCode::FolderRowNotFound => "folder_row_not_found",
            SkipCode::InvalidRating => "invalid_rating",
            SkipCode::CollectionKindUnsupported => "collection_kind_unsupported",
            SkipCode::CollectionKindMissing => "collection_kind_missing",
            SkipCode::QuickDuplicate => "quick_duplicate",
            SkipCode::SmartUnreadable => "smart_unreadable",
            SkipCode::SmartNoCriteria => "smart_no_criteria",
            SkipCode::SmartUntranslatable => "smart_untranslatable",
            SkipCode::NameInvalid => "name_invalid",
            SkipCode::ParentInvalid => "parent_invalid",
            SkipCode::ParentMissing => "parent_missing",
            SkipCode::ParentSkipped => "parent_skipped",
            SkipCode::ParentNotSet => "parent_not_set",
            SkipCode::ParentCycle => "parent_cycle",
            SkipCode::CollectionIdInvalid => "collection_id_invalid",
            SkipCode::CollectionPhotoInvalid => "collection_photo_invalid",
            SkipCode::CollectionSkipped => "collection_skipped",
            SkipCode::CollectionUnknown => "collection_unknown",
            SkipCode::PhotoNotConverted => "photo_not_converted",
            SkipCode::DuplicateMember => "duplicate_member",
            SkipCode::CollectionIsSet => "collection_is_set",
            SkipCode::TooFewPhotos => "too_few_photos",
            SkipCode::StackUnknown => "stack_unknown",
            SkipCode::StackIdInvalid => "stack_id_invalid",
            SkipCode::IdsInvalid => "ids_invalid",
            SkipCode::PhotoInvalid => "photo_invalid",
            SkipCode::AlreadyInStack => "already_in_stack",
            SkipCode::StackSkipped => "stack_skipped",
            SkipCode::NameEmpty => "name_empty",
            SkipCode::NameHasBar => "name_has_bar",
            SkipCode::AssignmentPhotoSkipped => "assignment_photo_skipped",
            SkipCode::AssignmentKeywordSkipped => "assignment_keyword_skipped",
            SkipCode::KeywordSkipped => "keyword_skipped",
            SkipCode::KeywordUnknown => "keyword_unknown",
            SkipCode::KeywordUnresolved => "keyword_unresolved",
            SkipCode::KeywordChanged => "keyword_changed",
            SkipCode::PathNotUtf8 => "path_not_utf8",
            SkipCode::NoGlobalId => "no_global_id",
            SkipCode::NoLightcraftId => "no_lightcraft_id",
            SkipCode::Ambiguous => "ambiguous",
            SkipCode::ParentNotCreated => "parent_not_created",
            SkipCode::ParentFailed => "parent_failed",
            SkipCode::NoIdReturned => "no_id_returned",
            SkipCode::AppliedNotSaved => "applied_not_saved",
            SkipCode::ImportFailed => "import_failed",
            SkipCode::Rejected => "rejected",
            SkipCode::SameContent => "same_content",
            SkipCode::PathMappingEscaped => "path_mapping_escaped",
            SkipCode::PathMappingCollision => "path_mapping_collision",
            SkipCode::Unspecified => "unspecified",
        }
    }
}

impl fmt::Display for SkipCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An element that could not be converted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// Why the element was skipped, for people. Never empty.
    pub reason: String,
    /// Why the element was skipped, for tools.
    pub code: SkipCode,
    /// What was skipped, by id; [`Subject::None`] where the skip site does
    /// not name it.
    pub subject: Subject,
}

impl Skipped {
    /// Creates a skip entry. An empty reason is replaced by "unspecified".
    pub fn new(code: SkipCode, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        let reason = if reason.trim().is_empty() {
            crate::Report::UNSPECIFIED_REASON.to_string()
        } else {
            reason
        };
        Skipped {
            reason,
            code,
            subject: Subject::None,
        }
    }

    /// Names what was skipped.
    pub fn about(mut self, subject: Subject) -> Self {
        self.subject = subject;
        self
    }
}

/// Counts the entries per code, keyed by the code's text.
pub fn count_by_code(items: &[Skipped]) -> BTreeMap<&'static str, usize> {
    let mut out = BTreeMap::new();
    for s in items {
        *out.entry(s.code.as_str()).or_insert(0) += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn codes_are_unique_snake_case() {
        let set: BTreeSet<_> = SkipCode::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(set.len(), SkipCode::ALL.len());
        for c in set {
            let mut chars = c.chars();
            assert!(
                chars.next().is_some_and(|ch| ch.is_ascii_lowercase()),
                "{c}"
            );
            assert!(
                c.chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_'),
                "{c}"
            );
        }
    }

    #[test]
    fn display_is_the_key() {
        assert_eq!(
            SkipCode::RootFolderNoPath.to_string(),
            "root_folder_no_path"
        );
    }

    #[test]
    fn new_replaces_an_empty_reason() {
        let s = Skipped::new(SkipCode::Video, " ");
        assert_eq!(s.reason, "unspecified");
        assert_eq!(s.code, SkipCode::Video);
    }

    #[test]
    fn count_by_code_counts_each_code() {
        let items = [
            Skipped::new(SkipCode::Video, "a"),
            Skipped::new(SkipCode::Missing, "b"),
            Skipped::new(SkipCode::Video, "c"),
        ];
        let m = count_by_code(&items);
        assert_eq!(m.get("video"), Some(&2));
        assert_eq!(m.get("missing"), Some(&1));
        assert_eq!(m.len(), 2);
        assert!(count_by_code(&[]).is_empty());
    }
}
