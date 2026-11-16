//! What a skip code means for the person reading the log: a short message,
//! whether it is a deliberate skip or a failure, and what to do about it.
//! The texts never contain a name, a path or a value.

use model::{SkipCode, Status};

/// The log view of one skip code.
pub struct Guidance {
    pub status: Status,
    pub message: &'static str,
    pub recommendation: Option<&'static str>,
}

const fn skip(message: &'static str, recommendation: Option<&'static str>) -> Guidance {
    Guidance {
        status: Status::Skipped,
        message,
        recommendation,
    }
}

const fn error(message: &'static str, recommendation: Option<&'static str>) -> Guidance {
    Guidance {
        status: Status::Error,
        message,
        recommendation,
    }
}

const RERUN: Option<&str> = Some(
    "Check that LightCraft is running and not blocked by a dialog, then run again. Finished photos are not imported twice.",
);
const CATALOG_DAMAGED: Option<&str> = Some(
    "The catalog has an inconsistent entry. Lightroom's catalog check (File › Optimize Catalog) may repair it.",
);

/// The guidance for `code`.
pub fn guidance(code: SkipCode) -> Guidance {
    use SkipCode as C;
    match code {
        C::NoRootFolder | C::RootFolderNoPath | C::FileRowNotFound | C::FolderRowNotFound => {
            skip("Catalog entry incomplete", CATALOG_DAMAGED)
        }
        C::RootFolderNotFound => skip(
            "Folder not found on this computer",
            Some(
                "Connect the drive, or add the folder's new location under path mapping, then run again.",
            ),
        ),
        C::VirtualCopy => skip(
            "Virtual copy not created: its master was not transferred",
            Some("Transfer the master photo first; the copy follows on the next run."),
        ),
        C::Video => skip("Video, not supported by LightCraft", None),
        C::Missing => skip(
            "Original missing, photo not transferred",
            Some(
                "Connect the drive or add a path mapping, then run again. lr2lc does not yet transfer photos without an original as offline; LightCraft's own Lightroom import keeps them.",
            ),
        ),
        C::MissingGlobalId | C::DuplicateGlobalId => {
            skip("Photo has no unique id in the catalog", CATALOG_DAMAGED)
        }
        C::InvalidRating => skip("Star rating out of range", CATALOG_DAMAGED),
        C::CollectionKindUnsupported => skip(
            "Collection type not supported",
            Some("Prints, books, slideshows and web galleries are not transferred."),
        ),
        C::CollectionKindMissing => skip("Collection type unknown", CATALOG_DAMAGED),
        C::QuickDuplicate => skip("Second Quick Collection ignored", None),
        C::SmartUnreadable | C::SmartNoCriteria => skip(
            "Smart collection rules cannot be read",
            Some("The collection stays unchanged in Lightroom."),
        ),
        C::SmartUntranslatable => skip(
            "Rule not supported yet",
            Some(
                "So far only rules based on star ratings become smart albums. The collection stays unchanged in Lightroom.",
            ),
        ),
        C::NameInvalid | C::NameEmpty => skip(
            "Name is empty or invalid",
            Some("Rename it in Lightroom, then run again."),
        ),
        C::NameHasBar => skip(
            "Keyword name contains “|”",
            Some(
                "LightCraft uses “|” to separate keyword levels. Rename the keyword in Lightroom, then run again.",
            ),
        ),
        C::ParentInvalid | C::ParentMissing | C::ParentNotSet | C::ParentCycle => {
            skip("Parent cannot be resolved", CATALOG_DAMAGED)
        }
        C::ParentSkipped => skip("Parent was not transferred", None),
        C::CollectionIdInvalid | C::CollectionPhotoInvalid | C::CollectionUnknown => {
            skip("Collection member cannot be resolved", CATALOG_DAMAGED)
        }
        C::CollectionSkipped => skip("Member of a collection that was not transferred", None),
        C::PhotoNotConverted => skip("Photo was not transferred", None),
        C::DuplicateMember => skip("Photo listed twice; added once", None),
        C::CollectionIsSet => skip("A collection set holds no photos", None),
        C::TooFewPhotos => skip("Stack with fewer than two photos", None),
        C::StackUnknown | C::StackIdInvalid | C::IdsInvalid | C::PhotoInvalid => {
            skip("Stack entry cannot be resolved", CATALOG_DAMAGED)
        }
        C::AlreadyInStack => skip("Photo already in another stack", None),
        C::StackSkipped => skip("Member of a stack that was not transferred", None),
        C::AssignmentPhotoSkipped => skip("Keyword of a photo that was not transferred", None),
        C::AssignmentKeywordSkipped | C::KeywordSkipped => {
            skip("Keyword was not transferred", None)
        }
        C::KeywordUnknown | C::KeywordUnresolved => {
            skip("Keyword cannot be resolved", CATALOG_DAMAGED)
        }
        C::KeywordChanged => skip(
            "Keyword attributes changed after it was created",
            Some(
                "lr2lc never changes an existing keyword. Adjust its synonyms or export options in LightCraft if needed.",
            ),
        ),
        C::PathNotUtf8 => skip(
            "Path is not valid text",
            Some("Rename the file or folder, then run again."),
        ),
        C::NoGlobalId => skip("Photo has no id to recognize it again", CATALOG_DAMAGED),
        C::NoLightcraftId => error(
            "Not in the LightCraft library",
            Some("The photo it depends on was not transferred. Run again after fixing that photo."),
        ),
        C::Ambiguous => skip(
            "Several matching albums exist",
            Some("Rename or remove the duplicate album in LightCraft, then run again."),
        ),
        C::ParentNotCreated | C::ParentFailed => {
            error("Parent album folder could not be created", RERUN)
        }
        C::NoIdReturned => error("LightCraft did not return an id", RERUN),
        C::AppliedNotSaved => error(
            "Applied, but LightCraft could not save it yet",
            Some("LightCraft saves it later. Check free disk space."),
        ),
        C::ImportFailed => error(
            "LightCraft did not accept the import",
            Some(
                "Check that LightCraft can read the file, then run again. Finished photos are skipped.",
            ),
        ),
        C::Rejected => error("LightCraft rejected the command", RERUN),
        C::SameContent => error("A copy with the same content could not be created", RERUN),
        C::PathMappingEscaped => skip(
            "Path mapping would leave the new folder",
            Some(
                "Check the path mapping: the rest of the path must not climb out of the new folder.",
            ),
        ),
        C::PathMappingCollision => skip(
            "Two photos map to the same path",
            Some("Check the path mapping; two different photos must not end up at one path."),
        ),
        C::Unspecified => skip("Not transferred", None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_has_a_message_and_no_text_names_a_value() {
        for code in SkipCode::ALL {
            let g = guidance(code);
            assert!(!g.message.is_empty(), "{code:?}");
            assert!(
                !g.message.ends_with('.'),
                "{code:?}: messages are short labels"
            );
            if let Some(r) = g.recommendation {
                assert!(r.ends_with('.'), "{code:?}");
            }
        }
    }
}
