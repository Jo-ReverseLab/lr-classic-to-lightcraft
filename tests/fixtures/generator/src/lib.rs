//! Generator for synthetic Lightroom Classic 15 test catalogs.
//!
//! Everything written here is invented. Paths use the user name `example`.
//! Output is deterministic.

pub mod hostile;

use rusqlite::{Connection, Result, params};
use std::path::{Path, PathBuf};

/// Fixed counts of the small fixture; tests assert exactly these numbers.
pub mod small {
    pub const ROOT_FOLDERS: i64 = 2;
    pub const FOLDERS: i64 = 6;
    pub const PHOTOS: i64 = 20;
    pub const VIRTUAL_COPIES: i64 = 1;
    /// Keywords excluding the hidden root keyword row.
    pub const KEYWORDS: i64 = 12;
    pub const KEYWORD_MAX_DEPTH: i64 = 4;
    pub const ASSIGNMENTS: i64 = 30;
    pub const RATING_NULL: i64 = 5;
    pub const RATING_ZERO: i64 = 3;
    /// Photos with 1, 2, 3, 4 and 5 stars.
    pub const RATING_STARS: [i64; 5] = [3, 2, 3, 2, 2];
    /// Original files that are absent on disk in `write_small_catalog_with_files`.
    pub const MISSING_FILES: i64 = 2;
    /// `AgLibraryFile.id_local` of the missing originals. File 2 is `IMG_0001` in
    /// the Windows-style root (the other `IMG_0001` exists); file 3 has a
    /// virtual copy (photo 20) that shares it.
    pub const MISSING_FILE_IDS: [i64; 2] = [2, 3];
    /// `Adobe_images.id_local` of photos whose original is missing (incl. the
    /// virtual copy).
    pub const MISSING_PHOTO_IDS: [i64; 3] = [2, 3, 20];
    /// Rows in `AgLibraryCollection` (all kinds together).
    pub const COLLECTIONS: i64 = 9;
    /// Collections with `systemOnly != 0` (ids 7 and 8; id 8 also carries the
    /// plain collection `creationId`, so the system rule must win).
    pub const COLLECTIONS_SYSTEM: i64 = 2;
    /// Plain collections (ids 2, 3, 4; id 2 lies inside collection set 1).
    pub const COLLECTIONS_PLAIN: i64 = 3;
    /// Collection sets (id 1).
    pub const COLLECTION_SETS: i64 = 1;
    /// Smart collections (ids 5, 6).
    pub const COLLECTIONS_SMART: i64 = 2;
    /// Collections with an unknown `creationId` (id 9).
    pub const COLLECTIONS_UNKNOWN: i64 = 1;
    /// Rows in `AgLibraryCollectionImage`.
    pub const COLLECTION_IMAGES: i64 = 7;
    /// Directory names of the two root folders below the target directory.
    pub const MACOS_ROOT_DIR: &str = "macos-pictures";
    pub const WINDOWS_ROOT_DIR: &str = "windows-pictures";
}

/// Counts of the keyword edge-case catalog ([`write_keyword_edge_catalog`]).
/// It is the small fixture plus the extra rows described there.
pub mod keyword_edge {
    /// Keyword rows excluding the hidden root row (small: 12, plus ids 14, 16-19).
    pub const KEYWORDS: i64 = 17;
    /// Keywords that can be converted (all except ids 12, 14, 16, 17, 18).
    pub const KEYWORDS_CONVERTED: i64 = 12;
    /// Name contains `|` (id 14, no children).
    pub const KEYWORD_PIPE_ID: i64 = 14;
    /// Empty names: id 12 (no children) and id 16 (children 17, 18 below it).
    pub const KEYWORD_EMPTY_IDS: [i64; 2] = [12, 16];
    /// Keywords below the empty keyword 16 (17 is its child, 18 the child of 17).
    pub const KEYWORD_UNDER_EMPTY_IDS: [i64; 2] = [17, 18];
    /// Person keyword with `includeOnExport` = 0 (id 19); converted normally.
    pub const KEYWORD_PERSON_ID: i64 = 19;
    /// Rows in `AgLibraryKeywordImage` (small: ids 1-30, extra: ids 31-37).
    pub const ASSIGNMENTS: i64 = 37;
    /// Assignment ids that cannot be converted: 31 (pipe keyword), 32 (empty
    /// keyword), 33 (keyword under empty one), 34 (photo 999 does not exist),
    /// 36 (empty keyword 12). The virtual copy 20 converts with its master
    /// (T03), so assignments 20 and 35 convert too.
    pub const ASSIGNMENTS_SKIPPED: [i64; 5] = [31, 32, 33, 34, 36];
    pub const ASSIGNMENTS_CONVERTED: i64 = 32;
    /// Rows in `AgLibraryKeywordSynonym` (small fixture: none). Ids 1 and 2
    /// belong to keyword 3 (id 1 sorts after id 2 alphabetically, so id order
    /// matters), id 3 to the skipped keyword 16, id 4 to the non-existent
    /// keyword 999, ids 5 (empty text) and 6 (NULL text) to keyword 2.
    pub const SYNONYMS: i64 = 6;
    /// Synonyms of converted keywords with a text (ids 1 and 2).
    pub const SYNONYMS_CONVERTED: i64 = 2;
    /// `(synonym id, keyword id, name)` of the rows above.
    pub const SYNONYM_ROWS: [(i64, i64, Option<&str>); 6] = [
        (1, 3, Some("Zyx-Synonym-Bär")),
        (2, 3, Some("Zyx-Synonym-Alpha")),
        (3, 16, Some("Zyx-Synonym-Gamma")),
        (4, 999, Some("Zyx-Synonym-Orphan")),
        (5, 2, Some("")),
        (6, 2, None),
    ];
    /// Synonyms of keyword 3, in `id_local` order.
    pub const KEYWORD_3_SYNONYMS: [&str; 2] = ["Zyx-Synonym-Bär", "Zyx-Synonym-Alpha"];
    /// All non-empty synonym texts; they must never appear in any output.
    pub const SYNONYM_TEXTS: [&str; 4] = [
        "Zyx-Synonym-Bär",
        "Zyx-Synonym-Alpha",
        "Zyx-Synonym-Gamma",
        "Zyx-Synonym-Orphan",
    ];
}

/// Keyword attribute fixture ([`write_keyword_attr_catalog`]): the small
/// fixture plus keywords 14-28. Names are invented (`Zyx-Attr-<id>`).
pub mod keyword_attr {
    /// One added keyword: `(id, keywordType, includeOnExport, includeParents,
    /// includeSynonyms, photo)`. `photo` is the photo the keyword is assigned
    /// to; photo 20 is a skipped virtual copy, 0 means no assignment.
    pub type Row = (i64, Option<&'static str>, i64, i64, i64, i64);
    /// Ids 14-21: all 8 flag combinations, type NULL. For `n = id - 14`:
    /// export = bit 2, parents = bit 1, synonyms = bit 0 (14 = all off,
    /// 21 = all on). 20 is unused. Then: 22 person all on, 23 person all off
    /// (unused), 24 type `Person` (wrong case, unknown), 25 type `zyx-kind`
    /// (unknown; export 0, parents 1, synonyms 0), 26 NULL type with the
    /// non-zero values 2, -1, 7, 27 assigned only to the skipped photo 20
    /// (unused), 28 empty name with type `weird` (skipped, not counted).
    pub const ROWS: [Row; 15] = [
        (14, None, 0, 0, 0, 1),
        (15, None, 0, 0, 1, 4),
        (16, None, 0, 1, 0, 5),
        (17, None, 0, 1, 1, 6),
        (18, None, 1, 0, 0, 7),
        (19, None, 1, 0, 1, 8),
        (20, None, 1, 1, 0, 0),
        (21, None, 1, 1, 1, 9),
        (22, Some("person"), 1, 1, 1, 10),
        (23, Some("person"), 0, 0, 0, 0),
        (24, Some("Person"), 1, 1, 1, 11),
        (25, Some("zyx-kind"), 0, 1, 0, 12),
        (26, None, 2, -1, 7, 13),
        (27, None, 1, 1, 1, 20),
        (28, Some("weird"), 1, 1, 1, 0),
    ];
    /// Keyword rows read (small: 12, plus ids 14-28).
    pub const KEYWORDS: i64 = 27;
    /// Converted: all but the empty names 12 and 28.
    pub const KEYWORDS_CONVERTED: i64 = 25;
    /// Ids of the skipped keywords.
    pub const SKIPPED_IDS: [i64; 2] = [12, 28];
    /// Converted keywords with `person` set (22, 23).
    pub const PERSONS: i64 = 2;
    /// Converted keywords with `include_on_export` false
    /// (14-17, 23, 25).
    pub const NOT_EXPORTED: i64 = 6;
    /// Converted keywords with `export_containing` false (14, 15, 18, 19, 23).
    pub const NO_PARENTS: i64 = 5;
    /// Converted keywords with `export_synonyms` false
    /// (14, 16, 18, 20, 23, 25).
    pub const NO_SYNONYMS: i64 = 6;
    /// Converted keywords with an unknown type (24, 25; not 28, it is skipped).
    pub const UNKNOWN_TYPE: i64 = 2;
    /// Converted keywords without a converted assignment
    /// (small keyword 11, plus 20, 23, 27).
    pub const UNUSED: i64 = 4;
    /// Ids of the unused converted keywords.
    pub const UNUSED_IDS: [i64; 4] = [11, 20, 23, 27];
    /// Reader level: photo 20 (virtual copy) converts with its master (T03),
    /// so keyword 27 is used there; the CLI still drops photo 20.
    pub const UNUSED_READER: i64 = 3;
    /// Rows in `AgLibraryKeywordImage` (small: 30, plus 12 added).
    pub const ASSIGNMENTS: i64 = 42;
    /// Converted assignments: the 12 added ones minus the one on photo 20.
    /// With placeholder originals the small fixture's skipped photos stay
    /// as before; the added assignments avoid photos 2 and 3.
    pub const ADDED_ASSIGNMENTS_CONVERTED: i64 = 11;
    /// Names that must never appear in any output.
    pub const NAMES: [&str; 3] = ["Zyx-Attr-14", "Zyx-Attr-22", "Zyx-Attr-27"];
}

/// Counts and ids of the flag fixture ([`write_flag_catalog_with_files`]).
///
/// It is the small fixture with `Adobe_images.pick` set as listed in
/// [`PICK_VALUES`]. Unlike a real catalog (where `pick` is NOT NULL), the
/// column of this fixture is nullable so that NULL can be tested. Every
/// `AgLibraryCollectionImage.pick` is 1, which must never be read.
pub mod flag_edge {
    /// `(photo id, stored value, description)`. Photos not listed store the
    /// INTEGER 0. Photos 2 and 3 have a missing original, photo 20 is a
    /// virtual copy.
    pub const PICK_VALUES: [(i64, &str, &str); 12] = [
        (4, "1", "integer 1: pick"),
        (5, "-1", "integer -1: reject"),
        (6, "0", "integer 0: none"),
        (7, "NULL", "NULL: none"),
        (8, "1.0", "real 1.0: pick"),
        (9, "-1.0", "real -1.0: reject"),
        (10, "2", "integer 2: unknown"),
        (11, "'flagged'", "text: unknown"),
        (12, "0.5", "real 0.5: unknown"),
        (2, "-1", "reject, but original missing (skipped by the CLI)"),
        (3, "2", "unknown, but original missing (skipped by the CLI)"),
        (20, "1", "pick, but virtual copy (skipped by the reader)"),
    ];
    pub const PICK_IDS: [i64; 2] = [4, 8];
    pub const REJECT_IDS: [i64; 2] = [5, 9];
    /// Reader level: the virtual copy 20 converts with its master (T03), so
    /// its pick is returned too.
    pub const PICK_IDS_READER: [i64; 3] = [4, 8, 20];
    pub const REJECT_IDS_READER: [i64; 3] = [2, 5, 9];
    pub const UNKNOWN_IDS: [i64; 3] = [10, 11, 12];
    /// Photos 6 and 7 plus every photo not listed above.
    pub const PHOTOS: i64 = 20;
    /// Converted photos in the CLI: 20 rows - 1 virtual copy - 2 missing.
    pub const CONVERTED: i64 = 17;
    pub const SKIPPED: i64 = 3;
    pub const PICK: i64 = 2;
    pub const REJECT: i64 = 2;
    pub const UNKNOWN: i64 = 3;
    /// Converted photos with no flag (0, NULL and all unlisted photos).
    pub const NONE: i64 = 10;
}

/// Counts and ids of the colour label fixtures
/// ([`write_label_catalog_with_files`]).
///
/// Each fixture is the small fixture with `Adobe_images.colorLabels` set as
/// listed in [`label_edge::values`]. Photos not listed store the empty string,
/// except photo 10, which stores NULL (a real catalog has `NOT NULL`; the
/// fixture column is nullable so that NULL can be tested). Photos 2 and 3
/// have a missing original and photo 20 is a virtual copy, so the CLI
/// converts 17 photos: ids 1 and 4..=19. Photos 2 and 20 carry a label as
/// well; it must not be counted anywhere.
pub mod label_edge {
    /// Label fixtures. All names are invented.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Fixture {
        /// English names. Photos 4..=9: Red, Yellow, Green, Blue, Purple, Red.
        En,
        /// German names, `Grün` in NFC. Photos 4..=9: Rot, Gelb, Grün, Blau,
        /// Lila, Rot.
        De,
        /// Both sets mixed. Photos 4..=9: Red, Rot, Green, Blau, Purple, Gelb.
        Mixed,
        /// Photos 4..=9: `Grün` in NFD, `rot`, `Red ` (trailing space),
        /// `Zyx-Label-Custom`, `Green`, `Red`.
        Odd,
        /// No labels at all (empty strings; photo 10 NULL).
        Empty,
        /// Both standard sets plus a custom name. Photos 4..=9: Rot, Red,
        /// `Zyx-Label-Custom`, Gelb, Purple, Lila.
        MixedCustom,
        /// Custom names only. Photos 4..=9: six invented names, none of them
        /// a standard name.
        OnlyCustom,
        /// German names that differ from the standard ones in one detail.
        /// Photos 4..=9: `Grün` in NFD, `rot`, `Gelb ` (trailing space),
        /// Blau, `LILA`, Rot.
        OddDe,
    }

    /// Converted photos (17 = 20 rows - virtual copy - 2 missing originals).
    pub const CONVERTED: i64 = 17;
    pub const PHOTOS: i64 = 20;
    pub const SKIPPED: i64 = 3;
    /// Converted photos that carry a non-empty label in `En`, `De`, `Mixed`
    /// and `Odd` (ids 4..=9).
    pub const LABELLED: i64 = 6;
    /// Converted photos with an empty or NULL label (17 - 6).
    pub const UNLABELLED: i64 = 11;
    /// Photo 10 stores NULL in every label fixture.
    pub const NULL_ID: i64 = 10;
    /// Ids of the photos that carry a label (all fixtures but `Empty`).
    pub const LABELLED_IDS: [i64; 6] = [4, 5, 6, 7, 8, 9];
    /// The custom name in `Odd`.
    pub const CUSTOM_NAME: &str = "Zyx-Label-Custom";

    /// `(photo id, stored label)` rows of a fixture, without the NULL of
    /// photo 10. Photos 2 (missing original) and 20 (virtual copy) are
    /// included.
    pub fn values(f: Fixture) -> Vec<(i64, &'static str)> {
        let six: [&'static str; 6] = match f {
            Fixture::En => ["Red", "Yellow", "Green", "Blue", "Purple", "Red"],
            Fixture::De => ["Rot", "Gelb", "Gr\u{fc}n", "Blau", "Lila", "Rot"],
            Fixture::Mixed => ["Red", "Rot", "Green", "Blau", "Purple", "Gelb"],
            Fixture::Odd => ["Gru\u{308}n", "rot", "Red ", CUSTOM_NAME, "Green", "Red"],
            Fixture::MixedCustom => ["Rot", "Red", CUSTOM_NAME, "Gelb", "Purple", "Lila"],
            Fixture::OnlyCustom => [
                CUSTOM_NAME,
                "Alpha-Custom",
                "Beta-Custom",
                "Gamma-Custom",
                "Delta-Custom",
                "Epsilon-Custom",
            ],
            Fixture::OddDe => ["Gru\u{308}n", "rot", "Gelb ", "Blau", "LILA", "Rot"],
            Fixture::Empty => return Vec::new(),
        };
        let mut v: Vec<(i64, &'static str)> = LABELLED_IDS.into_iter().zip(six).collect();
        // Skipped photos: missing original (2) and virtual copy (20).
        let skipped: [&'static str; 2] = match f {
            Fixture::En | Fixture::Mixed | Fixture::Odd | Fixture::MixedCustom => ["Red", "Green"],
            _ => ["Rot", "Gr\u{fc}n"],
        };
        v.push((2, skipped[0]));
        v.push((20, skipped[1]));
        v
    }
}

/// Counts and ids of the caption fixture ([`write_caption_catalog_with_files`]).
///
/// The small fixture plus rows in `AgLibraryIPTC` (all other fixtures have
/// that table, empty). The CLI converts photos 1 and 4..=19 (17 photos);
/// photos 2 and 3 (missing original) and 20 (virtual copy) are skipped.
/// Photos 2 and 20 carry a caption that must not be counted anywhere.
pub mod caption_edge {
    pub const CONVERTED: i64 = 17;
    pub const SKIPPED: i64 = 3;
    /// Converted photos with a caption: [`UMLAUT_ID`], [`PADDED_ID`],
    /// [`DUPLICATE_ID`].
    pub const CAPTIONS: i64 = 3;
    pub const CAPTIONED_IDS: [i64; 3] = [4, 5, 9];
    /// Photo 4: umlaut and emoji.
    pub const UMLAUT_ID: i64 = 4;
    pub const UMLAUT_TEXT: &str = "Sonnenuntergang \u{fc}ber K\u{f6}ln \u{1f305}";
    /// Photo 5: leading and trailing spaces and a trailing newline.
    pub const PADDED_ID: i64 = 5;
    pub const PADDED_TEXT: &str = "  padded caption \n";
    /// Photo 6: empty string.
    pub const EMPTY_ID: i64 = 6;
    /// Photo 7: NULL.
    pub const NULL_ID: i64 = 7;
    /// Photo 8: no row at all.
    pub const NO_ROW_ID: i64 = 8;
    /// Photo 9: rows `id_local` 101 ("second", inserted first) and 100
    /// ("first"). The smallest `id_local` wins.
    pub const DUPLICATE_ID: i64 = 9;
    pub const DUPLICATE_TEXT: &str = "first";
    /// Photo 10: rows 200 (NULL) and 201 ("later"). The smallest `id_local`
    /// wins, so there is no caption.
    pub const DUPLICATE_NULL_FIRST_ID: i64 = 10;
    /// Photo 11: an integer 42 instead of text: no caption.
    pub const NON_TEXT_ID: i64 = 11;
    /// Skipped photos with a caption: 2 (missing original), 20 (virtual copy).
    pub const SKIPPED_CAPTIONED_IDS: [i64; 2] = [2, 20];
    pub const SKIPPED_TEXTS: [&str; 2] = ["skipped-missing-caption", "skipped-virtual-caption"];
    /// Every caption text of the fixture that is not empty; none of them may
    /// appear in any output of the tool.
    pub const ALL_TEXTS: [&str; 7] = [
        "Sonnenuntergang",
        "padded caption",
        "first",
        "second",
        "later",
        "skipped-missing-caption",
        "skipped-virtual-caption",
    ];
}

/// Counts and ids of the rights fixture ([`write_rights_catalog_with_files`]).
///
/// The small fixture plus `AgLibraryIPTC.copyright` rows and the tables
/// `AgHarvestedIptcMetadata` (`image`, `creatorRef`) and
/// `AgInternedIptcCreator` (`id_local`, `value`). The CLI converts photos 1
/// and 4..=19 (17 photos); photos 2 and 3 (missing original) and 20 (virtual
/// copy) are skipped. Photos 2 and 20 carry copyright and creator values that
/// must not be counted or printed anywhere.
/// [`write_copyright_only_catalog_with_files`] has the same copyright rows but
/// neither creator table.
pub mod rights_edge {
    pub const CONVERTED: i64 = 17;
    pub const SKIPPED: i64 = 3;
    /// Converted photos with a copyright: 4, 5, 9.
    pub const COPYRIGHTS: i64 = 3;
    pub const COPYRIGHT_IDS: [i64; 3] = [4, 5, 9];
    /// Converted photos with a creator: 4, 7, 12, 13.
    pub const CREATORS: i64 = 4;
    pub const CREATOR_IDS: [i64; 4] = [4, 7, 12, 13];
    /// Photo 4: umlaut and emoji (copyright and creator).
    pub const UMLAUT_ID: i64 = 4;
    pub const UMLAUT_COPYRIGHT: &str = "\u{a9} 2026 Example Creator K\u{f6}ln \u{1f305}";
    pub const UMLAUT_CREATOR: &str = "Example Creator M\u{fc}ller \u{1f4f7}";
    /// Photo 5: copyright with padding and a trailing newline; creatorRef NULL.
    pub const PADDED_ID: i64 = 5;
    pub const PADDED_COPYRIGHT: &str = "  padded copyright \n";
    /// Photo 6: empty copyright; creatorRef points at a missing row (999).
    pub const EMPTY_ID: i64 = 6;
    /// Photo 7: copyright NULL; two harvested rows, 301 (ref 502, "Second
    /// Creator") inserted first and 300 (ref 501, "First Creator"). The
    /// smallest `id_local` wins.
    pub const TWO_HARVESTED_ID: i64 = 7;
    pub const FIRST_CREATOR: &str = "First Creator";
    /// Photo 8: no rows at all.
    pub const NO_ROW_ID: i64 = 8;
    /// Photo 9: copyright rows 101 ("second", inserted first) and 100
    /// ("first"); harvested rows 400 (creatorRef NULL) and 401 (ref 501): the
    /// smallest `id_local` wins, so no creator.
    pub const DUPLICATE_ID: i64 = 9;
    pub const DUPLICATE_COPYRIGHT: &str = "first";
    /// Photo 10: copyright rows 200 (NULL) and 201 ("later"): none. Creator
    /// value is the empty string: none.
    pub const DUPLICATE_NULL_FIRST_ID: i64 = 10;
    /// Photo 11: integer 42 instead of text for copyright and creator: none.
    pub const NON_TEXT_ID: i64 = 11;
    /// Photos 12 and 13: creator "First Creator" (shared interned row 501).
    pub const SHARED_CREATOR_IDS: [i64; 2] = [12, 13];
    /// Skipped photos (2 missing original, 20 virtual copy) with values.
    pub const SKIPPED_IDS: [i64; 2] = [2, 20];
    /// Every non-empty text of the fixture; none may appear in any output.
    pub const ALL_TEXTS: [&str; 11] = [
        "Example Creator",
        "padded copyright",
        "first",
        "second",
        "later",
        "First Creator",
        "Second Creator",
        "skipped-missing-copyright",
        "skipped-virtual-copyright",
        "skipped-missing-creator",
        "skipped-virtual-creator",
    ];
}

/// Counts and ids of the video fixture ([`write_video_catalog_with_files`]).
pub mod video_edge {
    /// `Adobe_images` rows: the 20 of the small fixture plus virtual copy 21.
    pub const PHOTOS: i64 = 21;
    /// Videos: 2 (`fileFormat` VIDEO, original missing), 4 (`fileFormat`
    /// VIDEO, `.mov`), 5 (`fileFormat` JPG but extension `MP4`, upper case).
    pub const VIDEO_IDS: [i64; 3] = [2, 4, 5];
    /// Photo 2 is a video whose original does not exist; it counts as a video.
    pub const VIDEO_MISSING_ID: i64 = 2;
    /// Photo 5 is recognised by its upper-case extension only.
    pub const VIDEO_BY_EXTENSION_ID: i64 = 5;
    /// Not videos although they look similar: 6 (extension `mpo`, a still
    /// image format) and 7 (base name `clip.mp4`, extension `jpg`).
    pub const LOOKALIKE_IDS: [i64; 2] = [6, 7];
    /// Virtual copies: 20 (of photo 3) and 21 (of video 4). At reader level
    /// 20 converts with its master (T03); 21 keeps the virtual-copy reason
    /// because its master is a video. Neither is counted as a video.
    pub const VIRTUAL_COPY_IDS: [i64; 2] = [20, 21];
    /// Photos with a missing original that are not videos: only photo 3.
    pub const MISSING_ORIGINAL_IDS: [i64; 1] = [3];
    pub const CONVERTED: i64 = 15;
    /// Skipped photos: 3 videos + 1 missing + 2 virtual copies.
    pub const SKIPPED: i64 = 6;
}

/// Counts and ids of the stack fixture ([`write_stack_catalog_with_files`]).
///
/// It is the video fixture ([`video_edge`]) plus folder stacks. Photos 1, 3
/// and 6..=19 are converted by `Catalog::photos`; 2, 4, 5 are videos, 20 and
/// 21 virtual copies. Photo 3 (missing original) is not used by any stack so
/// that reader and CLI see the same members.
pub mod stack_edge {
    /// Rows in `AgLibraryFolderStack`.
    pub const STACKS: i64 = 8;
    /// Rows in `AgLibraryFolderStackImage`.
    pub const MEMBER_ROWS: i64 = 20;
    /// Converted stacks and their members, top first.
    ///
    /// - 1: two photos, `position` REAL (rows inserted out of order)
    /// - 2: three photos, `position` INTEGER, `collapsed` 1
    /// - 5: photos 13 and 14; member 5 is a video (skipped)
    /// - 6: photos 15 and 16
    /// - 7: photos 17 and 18; member 16 is already in stack 6 (skipped)
    /// - 8: photos 19 and 1; no member has position 1 (`no_top`); member 999
    ///   does not exist (skipped)
    pub const STACK_MEMBERS: [(i64, &[i64]); 6] = [
        (1, &[6, 7]),
        (2, &[9, 10, 8]),
        (5, &[13, 14]),
        (6, &[15, 16]),
        (7, &[17, 18]),
        (8, &[19, 1]),
    ];
    pub const CONVERTED: i64 = 6;
    /// Stack 3 (original 11 plus virtual copy 20) and stack 4 (photo 12 plus
    /// video 4) keep fewer than two photos (CLI: photo 20's original is
    /// missing, so it is dropped first).
    pub const SKIPPED_STACKS: [i64; 2] = [3, 4];
    pub const SKIPPED: i64 = 2;
    pub const MEMBERS_CONVERTED: i64 = 13;
    /// Member rows skipped: stack 3 (2 rows), stack 4 (2), video 5, photo
    /// 16 in stack 7, photo 999.
    pub const MEMBERS_SKIPPED: i64 = 7;
    /// Reader level (before missing originals are dropped): virtual copy 20
    /// converts with its master (T03), so stack 3 also converts.
    pub const CONVERTED_READER: i64 = 7;
    pub const SKIPPED_READER: i64 = 1;
    pub const SKIPPED_STACKS_READER: [i64; 1] = [4];
    pub const MEMBERS_CONVERTED_READER: i64 = 15;
    pub const MEMBERS_SKIPPED_READER: i64 = 5;
    pub const STACK_MEMBERS_READER: [(i64, &[i64]); 7] = [
        (1, &[6, 7]),
        (2, &[9, 10, 8]),
        (3, &[11, 20]),
        (5, &[13, 14]),
        (6, &[15, 16]),
        (7, &[17, 18]),
        (8, &[19, 1]),
    ];
    /// Converted stacks with `collapsed` 1 (stack 2).
    pub const COLLAPSED: i64 = 1;
    /// Converted stacks without a member at position 1 (stack 8).
    pub const NO_TOP: i64 = 1;
    /// Member photo of the skipped virtual copy / video / unknown rows.
    pub const VIRTUAL_COPY_MEMBER: i64 = 20;
    pub const VIDEO_MEMBERS: [i64; 2] = [4, 5];
    pub const UNKNOWN_MEMBER: i64 = 999;
    /// Photo in two stacks: kept in the first (6), skipped in the second (7).
    pub const DOUBLE_PHOTO: i64 = 16;
    pub const DOUBLE_FIRST: i64 = 6;
    pub const DOUBLE_SECOND: i64 = 7;
}

/// Counts and ids of the collection fixture ([`write_collection_catalog`]).
///
/// It is the small fixture with all collection rows replaced. Photos used
/// as members are 1..=19 except 2 and 3 (missing originals in the CLI),
/// 20 (virtual copy) and 999 (does not exist), so reader and CLI see the same
/// converted members. All names are invented; the ones that must never be
/// printed are in [`NAMES`].
pub mod collection_edge {
    /// Rows in `AgLibraryCollection`.
    pub const COLLECTIONS: i64 = 25;
    /// Converted: folders 1, 2, 3 and albums 4..=12.
    pub const CONVERTED: i64 = 12;
    pub const FOLDERS: i64 = 3;
    pub const ALBUMS: i64 = 9;
    pub const SKIPPED: i64 = 13;
    /// Albums that repeat the name of an earlier sibling (ids 11 and 12 are
    /// both "Twin" below set 2: one repeat).
    pub const DUPLICATE_NAMES: i64 = 1;
    /// Rows in `AgLibraryCollectionImage`.
    pub const MEMBER_ROWS: i64 = 26;
    pub const MEMBERS_CONVERTED: i64 = 16;
    pub const MEMBERS_SKIPPED: i64 = 10;
    /// Reader level (before missing originals are dropped): virtual copy 20
    /// converts with its master (T03), so one more member row converts and
    /// one fewer is skipped. `MEMBERS_SKIPPED_READER` still counts the quick
    /// collection's row, which converts too.
    pub const MEMBERS_CONVERTED_READER: i64 = 17;
    pub const MEMBERS_SKIPPED_READER: i64 = 9;
    /// [`MEMBERS`] at reader level; album 8 also holds the converted virtual
    /// copy 20.
    pub const MEMBERS_READER: [(i64, &[i64]); 9] = [
        (4, &[4, 5, 1]),
        (5, &[7, 6, 8]),
        (6, &[11, 10, 9, 12]),
        (7, &[15, 16]),
        (8, &[20, 17]),
        (9, &[18]),
        (10, &[]),
        (11, &[13]),
        (12, &[14]),
    ];
    /// Folder ids and the ids of the folders' (converted) albums.
    pub const FOLDER_IDS: [i64; 3] = [1, 2, 3];
    pub const ALBUM_IDS: [i64; 9] = [4, 5, 6, 7, 8, 9, 10, 11, 12];
    /// Smart collection ids (13 has a member row).
    pub const SMART_IDS: [i64; 1] = [13];
    /// Quick collection ids (`systemOnly` REAL 1.0 and INTEGER 1).
    pub const QUICK_IDS: [i64; 2] = [14, 15];
    /// Web (16) and print (17) collections: other kinds.
    pub const OTHER_KIND_IDS: [i64; 2] = [16, 17];
    /// Skipped because of the parent: missing (18), album (19), skipped
    /// (20), set below a skipped set (21), album below that (22), cycle
    /// (23, 24), own parent (25).
    pub const PARENT_IDS: [i64; 8] = [18, 19, 20, 21, 22, 23, 24, 25];
    /// `(id, name, parent)` of the converted collections.
    pub const NAMES: [(i64, &str, Option<i64>); 12] = [
        (1, "Set Top", None),
        (2, "Set Mid", Some(1)),
        (3, "Set Deep", Some(2)),
        (4, "Album Deep", Some(3)),
        (5, "Album Mid", Some(2)),
        (6, "Album Top", None),
        (7, "S\u{fc}\u{df}e \u{c4}pfel", Some(1)),
        (8, "\u{1f305} Sunrise \u{1f304}", None),
        (9, "Gru\u{308}n ", None),
        (10, "Empty", Some(1)),
        (11, "Twin", Some(2)),
        (12, "Twin", Some(2)),
    ];
    /// Expected member photo ids per converted album, in order.
    pub const MEMBERS: [(i64, &[i64]); 9] = [
        (4, &[4, 5, 1]),
        (5, &[7, 6, 8]),
        (6, &[11, 10, 9, 12]),
        (7, &[15, 16]),
        (8, &[17]),
        (9, &[18]),
        (10, &[]),
        (11, &[13]),
        (12, &[14]),
    ];
    /// Names of skipped collections; they must never be printed.
    pub const SKIPPED_NAMES: [&str; 13] = [
        "Zyx-Smart",
        "Zyx-Quick-Real",
        "Zyx-Quick-Int",
        "Zyx-Web",
        "Zyx-Print",
        "Zyx-Lost-Parent",
        "Zyx-Album-Parent",
        "Zyx-Smart-Child",
        "Zyx-Set-Below",
        "Zyx-Album-Below",
        "Zyx-Cycle-A",
        "Zyx-Cycle-B",
        "Zyx-Self",
    ];
    /// Member photos that are skipped inside converted albums: unknown 999
    /// (album 8), duplicate of 15 (album 7). Virtual copy 20 converts with
    /// its master (T03).
    pub const NOT_CONVERTED_PHOTOS: [i64; 1] = [999];
    /// Album 7 stores the pair (7, 15) twice.
    pub const DUPLICATE_MEMBER: (i64, i64) = (7, 15);
    /// Folder 3 has a member row (photo 19).
    pub const FOLDER_WITH_MEMBER: i64 = 3;
}

const SCHEMA: &str = "
CREATE TABLE Adobe_variablesTable (id_local INTEGER PRIMARY KEY, id_global NOT NULL, name, type, value NOT NULL);
CREATE TABLE AgLibraryRootFolder (id_local INTEGER PRIMARY KEY, id_global NOT NULL, absolutePath NOT NULL, name NOT NULL, relativePathFromCatalog);
CREATE TABLE AgLibraryFolder (id_local INTEGER PRIMARY KEY, id_global NOT NULL, parentId INTEGER, pathFromRoot NOT NULL, rootFolder INTEGER NOT NULL, visibility INTEGER);
CREATE TABLE AgLibraryFile (id_local INTEGER PRIMARY KEY, id_global NOT NULL, baseName NOT NULL, extension NOT NULL, folder INTEGER NOT NULL, idx_filename NOT NULL, lc_idx_filename NOT NULL, lc_idx_filenameExtension NOT NULL, originalFilename NOT NULL, errorMessage, errorTime, externalModTime, importHash, md5, modTime, sidecarExtensions);
CREATE TABLE Adobe_images (id_local INTEGER PRIMARY KEY, id_global NOT NULL, aspectRatioCache NOT NULL, bitDepth NOT NULL, colorChannels NOT NULL, colorLabels NOT NULL, colorMode NOT NULL, copyCreationTime NOT NULL, captureTime, copyName, copyReason, fileHeight, fileWidth, orientation, rating, editLock INTEGER NOT NULL, fileFormat NOT NULL, pick NOT NULL, positionInFolder NOT NULL, touchCount NOT NULL, touchTime NOT NULL, hasMissingSidecars INTEGER, masterImage INTEGER, originalRootEntity INTEGER, rootFile INTEGER NOT NULL);
CREATE TABLE AgLibraryKeyword (id_local INTEGER PRIMARY KEY, id_global NOT NULL, dateCreated NOT NULL, genealogy NOT NULL, includeOnExport INTEGER NOT NULL, includeParents INTEGER NOT NULL, includeSynonyms INTEGER NOT NULL, imageCountCache, keywordType, lastApplied, lc_name, name, parent INTEGER);
CREATE TABLE AgLibraryKeywordImage (id_local INTEGER PRIMARY KEY, image INTEGER NOT NULL, tag INTEGER NOT NULL);
CREATE TABLE AgLibraryKeywordSynonym (id_local INTEGER PRIMARY KEY, keyword INTEGER NOT NULL DEFAULT 0, lc_name, name);
CREATE TABLE AgLibraryIPTC (id_local INTEGER PRIMARY KEY, altTextAccessibility, caption, copyright, extDescrAccessibility, image INTEGER NOT NULL DEFAULT 0);
CREATE TABLE Adobe_AdditionalMetadata (id_local INTEGER PRIMARY KEY, id_global, image INTEGER, xmp, embeddedXmp INTEGER, externalXmpIsDirty INTEGER);
CREATE TABLE Adobe_imageProperties (id_local INTEGER PRIMARY KEY, id_global, image, propertiesString);
CREATE TABLE AgLibraryCollection (id_local INTEGER PRIMARY KEY, creationId NOT NULL, genealogy NOT NULL, imageCount, name NOT NULL, parent INTEGER, systemOnly NOT NULL);
CREATE TABLE AgLibraryCollectionImage (id_local INTEGER PRIMARY KEY, collection INTEGER NOT NULL, image INTEGER NOT NULL, pick NOT NULL, positionInCollection);
";

fn gid(kind: &str, id: i64) -> String {
    format!("{kind}-{id:08}")
}

fn create(path: &Path) -> Result<Connection> {
    if path.exists() {
        std::fs::remove_file(path)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

fn add_root_folder(c: &Connection, id: i64, abs: &str, name: &str) -> Result<()> {
    c.execute(
        "INSERT INTO AgLibraryRootFolder VALUES (?1, ?2, ?3, ?4, NULL)",
        params![id, gid("root", id), abs, name],
    )?;
    Ok(())
}

fn add_folder(c: &Connection, id: i64, parent: Option<i64>, rel: &str, root: i64) -> Result<()> {
    c.execute(
        "INSERT INTO AgLibraryFolder VALUES (?1, ?2, ?3, ?4, ?5, 0)",
        params![id, gid("folder", id), parent, rel, root],
    )?;
    Ok(())
}

fn add_photo(
    c: &Connection,
    id: i64,
    base: &str,
    folder: i64,
    rating: Option<i64>,
    master: Option<i64>,
    file_id: i64,
) -> Result<()> {
    if master.is_none() {
        c.execute(
            "INSERT INTO AgLibraryFile (id_local, id_global, baseName, extension, folder, idx_filename, lc_idx_filename, lc_idx_filenameExtension, originalFilename) VALUES (?1, ?2, ?3, 'jpg', ?4, ?5, ?6, 'jpg', ?5)",
            params![
                file_id,
                gid("file", file_id),
                base,
                folder,
                format!("{base}.jpg"),
                format!("{base}.jpg").to_lowercase()
            ],
        )?;
    }
    c.execute(
        "INSERT INTO Adobe_images (id_local, id_global, aspectRatioCache, bitDepth, colorChannels, colorLabels, colorMode, copyCreationTime, rating, editLock, fileFormat, pick, positionInFolder, touchCount, touchTime, masterImage, rootFile) VALUES (?1, ?2, 1.5, 8, 3, '', 'RGB', 0, ?3, 0, 'JPG', 0, ?4, 0, 0, ?5, ?6)",
        params![id, gid("image", id), rating, format!("{id}"), master, file_id],
    )?;
    c.execute(
        "INSERT INTO Adobe_AdditionalMetadata (id_local, id_global, image, embeddedXmp, externalXmpIsDirty) VALUES (?1, ?2, ?1, 0, 0)",
        params![id, gid("meta", id)],
    )?;
    c.execute(
        "INSERT INTO Adobe_imageProperties (id_local, id_global, image, propertiesString) VALUES (?1, ?2, ?1, '')",
        params![id, gid("props", id)],
    )?;
    Ok(())
}

fn add_keyword(
    c: &Connection,
    id: i64,
    name: Option<&str>,
    parent: i64,
    genealogy: &str,
) -> Result<()> {
    c.execute(
        "INSERT INTO AgLibraryKeyword VALUES (?1, ?2, '2020-01-01T00:00:00', ?3, 1, 1, 1, NULL, NULL, NULL, ?4, ?5, ?6)",
        params![
            id,
            gid("kw", id),
            genealogy,
            name.map(str::to_lowercase),
            name,
            // Parent 0 marks the root keyword (stored as NULL).
            if parent == 0 { None } else { Some(parent) }
        ],
    )?;
    Ok(())
}

/// Collections of the small fixture: (id, creationId, name, parent, systemOnly).
/// Names are invented placeholders.
const COLLECTIONS: [(i64, &str, &str, Option<i64>, i64); 9] = [
    (1, "com.adobe.ag.library.group", "Set Alpha", None, 0),
    (
        2,
        "com.adobe.ag.library.collection",
        "Coll Beta",
        Some(1),
        0,
    ),
    (3, "com.adobe.ag.library.collection", "Coll Gamma", None, 0),
    (4, "com.adobe.ag.library.collection", "Coll Delta", None, 0),
    (
        5,
        "com.adobe.ag.library.smart_collection",
        "Smart Epsilon",
        None,
        0,
    ),
    (
        6,
        "com.adobe.ag.library.smart_collection",
        "Smart Zeta",
        None,
        0,
    ),
    (
        7,
        "com.adobe.ag.library.quick_collection",
        "Sys Eta",
        None,
        1,
    ),
    (8, "com.adobe.ag.library.collection", "Sys Theta", None, 1),
    (9, "com.example.invented", "Odd Iota", None, 0),
];

/// (collection, image) pairs of the small fixture.
const COLLECTION_IMAGES: [(i64, i64); 7] = [(2, 1), (2, 2), (3, 3), (3, 4), (4, 5), (7, 1), (8, 6)];

/// Collections of large catalogs (same row shape, no assignments of their own
/// beyond the ones written in [`write_large_catalog`]).
pub const LARGE_COLLECTIONS: i64 = 4;

fn add_collections(
    c: &Connection,
    collections: &[(i64, &str, &str, Option<i64>, i64)],
    images: &[(i64, i64)],
) -> Result<()> {
    for &(id, creation, name, parent, system) in collections {
        let count = images.iter().filter(|(c, _)| *c == id).count() as i64;
        add_collection_row(c, id, creation, name, parent, system, count)?;
    }
    add_collection_images(c, images)?;
    Ok(())
}

/// Inserts one `AgLibraryCollection` row with the given `imageCount`.
fn add_collection_row(
    c: &Connection,
    id: i64,
    creation: &str,
    name: &str,
    parent: Option<i64>,
    system: i64,
    image_count: i64,
) -> Result<()> {
    let genealogy = match parent {
        Some(p) => format!("/{p}/{id}"),
        None => format!("/{id}"),
    };
    c.execute(
        "INSERT INTO AgLibraryCollection (id_local, creationId, genealogy, imageCount, name, parent, systemOnly) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, creation, genealogy, image_count, name, parent, system],
    )?;
    Ok(())
}

/// Inserts `(collection, image)` membership rows.
fn add_collection_images(c: &Connection, images: &[(i64, i64)]) -> Result<()> {
    for (pos, (coll, image)) in images.iter().enumerate() {
        c.execute(
            "INSERT INTO AgLibraryCollectionImage (collection, image, pick, positionInCollection) VALUES (?1, ?2, 0, ?3)",
            params![coll, image, pos as f64],
        )?;
    }
    Ok(())
}

fn add_variables(c: &Connection) -> Result<()> {
    c.execute(
        "INSERT INTO Adobe_variablesTable (id_global, name, type, value) VALUES ('var-1', 'Adobe_DBVersion', 'string', '1500000')",
        [],
    )?;
    c.execute(
        "INSERT INTO Adobe_variablesTable (id_global, name, type, value) VALUES ('var-2', 'AgLibraryKeyword_rootTagID', 'int', '1')",
        [],
    )?;
    Ok(())
}

/// The storage type the `Adobe_DBVersion` value is written as by
/// [`set_dbversion_type`]. Lightroom stores it as text, but a catalog built
/// by another tool (or copied through a driver that re-typed the column) may
/// store the same logical value `1500000` as a real number or a blob. The
/// reader accepts all three; these variants let the generator exercise that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbVersionType {
    /// `value` stored as SQLite `TEXT` (the Lightroom layout).
    Text,
    /// `value` stored as SQLite `REAL`, e.g. `1500000.0`.
    Real,
    /// `value` stored as SQLite `BLOB`, e.g. the bytes `b"1500000"`.
    Blob,
}

/// Rewrites the `Adobe_DBVersion` value so it is stored with the given
/// SQLite storage type, while keeping the logical value `1500000`. The
/// `type` column text is left as the generator wrote it (it is not read by
/// the reader). Use after one of the `write_*_catalog` functions.
pub fn set_dbversion_type(path: &Path, kind: DbVersionType) -> Result<()> {
    let c = Connection::open(path)?;
    let changed = match kind {
        DbVersionType::Text => c.execute(
            "UPDATE Adobe_variablesTable SET value = '1500000' WHERE name = 'Adobe_DBVersion'",
            [],
        )?,
        DbVersionType::Real => c.execute(
            "UPDATE Adobe_variablesTable SET value = ?1 WHERE name = 'Adobe_DBVersion'",
            params![1500000.0_f64],
        )?,
        DbVersionType::Blob => c.execute(
            "UPDATE Adobe_variablesTable SET value = ?1 WHERE name = 'Adobe_DBVersion'",
            params![b"1500000".to_vec()],
        )?,
    };
    assert_eq!(changed, 1, "Adobe_DBVersion row missing");
    Ok(())
}

/// Keyword table of the small fixture: (id, name, parent id).
const KEYWORDS: [(i64, Option<&str>, i64); 12] = [
    (2, Some("Travel"), 1),
    (3, Some("Italy"), 2),
    (4, Some("Rome"), 3),
    (5, Some("Colosseum"), 4),
    (6, Some("Köln"), 2),
    (7, Some("Süßwasser"), 1),
    (8, Some("🌅 Sunrise"), 1),
    (9, Some("Sea"), 1),
    (10, Some("Ocean"), 1),
    (11, Some("Unused"), 1),
    (12, Some(""), 1),
    (13, Some("Family"), 1),
];

/// Writes the small fixture with the fixed counts in [`small`].
pub fn write_small_catalog(path: &Path) -> Result<()> {
    write_small(
        path,
        "/Users/example/Pictures/",
        "C:\\Users\\example\\Pictures\\",
    )
}

/// Writes the small fixture plus keyword edge cases (counts in
/// [`keyword_edge`]):
/// - keyword 14 `Bad|Name` (parent 1), assigned to photo 1 (assignment 31)
/// - keyword 16 with an empty name, child 17 and grandchild 18 below it;
///   assignment 32: photo 1 -> 16, assignment 33: photo 2 -> 18
/// - assignment 36: photo 4 -> the already existing empty keyword 12
/// - keyword 19 `Alice Example` (person, `includeOnExport` = 0), photo 5 (37)
/// - assignment 34: photo 999 (does not exist), keyword 2
/// - assignment 35: photo 20 (virtual copy), keyword 2
///
/// No assignment hits two problems at once.
pub fn write_keyword_edge_catalog(path: &Path) -> Result<()> {
    write_small_catalog(path)?;
    add_keyword_edges(path)
}

/// Like [`write_keyword_edge_catalog`], but as `dir/keywords.lrcat` with
/// placeholder originals as in [`write_small_catalog_with_files`] (photos 2, 3
/// and 20 have missing originals). Returns the catalog path.
pub fn write_keyword_edge_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let (mac, win) = prepare_root_dirs(dir)?;
    let catalog = dir.join("keywords.lrcat");
    write_small(&catalog, &mac, &win)?;
    add_keyword_edges(&catalog)?;
    create_original_files(&catalog)?;
    Ok(catalog)
}

fn add_keyword_edges(path: &Path) -> Result<()> {
    let c = Connection::open(path)?;
    c.execute_batch("BEGIN")?;
    add_keyword(&c, 14, Some("Bad|Name"), 1, "/1/14")?;
    add_keyword(&c, 16, Some(""), 1, "/1/16")?;
    add_keyword(&c, 17, Some("Under Empty"), 16, "/1/16/17")?;
    add_keyword(&c, 18, Some("Deep Under Empty"), 17, "/1/16/17/18")?;
    add_keyword(&c, 19, Some("Alice Example"), 1, "/1/19")?;
    c.execute_batch(
        "UPDATE AgLibraryKeyword SET includeOnExport = 0, keywordType = 'person' WHERE id_local = 19;",
    )?;
    // Ids 31..=37 in this order.
    for (image, tag) in [
        (1, 14),
        (1, 16),
        (2, 18),
        (999, 2),
        (20, 2),
        (4, 12),
        (5, 19),
    ] {
        c.execute(
            "INSERT INTO AgLibraryKeywordImage (image, tag) VALUES (?1, ?2)",
            params![image, tag],
        )?;
    }
    for (id, keyword, name) in keyword_edge::SYNONYM_ROWS {
        c.execute(
            "INSERT INTO AgLibraryKeywordSynonym (id_local, keyword, lc_name, name) VALUES (?1, ?2, ?3, ?4)",
            params![id, keyword, name.map(str::to_lowercase), name],
        )?;
    }
    c.execute_batch("COMMIT")?;
    Ok(())
}

/// Writes the small fixture plus the keywords of [`keyword_attr`] (all
/// 8 flag combinations, person and unknown types, non-zero flag values,
/// unused keywords, a skipped keyword with an unknown type).
/// `includeOnExport`, `includeParents` and `includeSynonyms` are
/// `NOT NULL` in this schema, so a NULL flag cannot be generated.
pub fn write_keyword_attr_catalog(path: &Path) -> Result<()> {
    write_small_catalog(path)?;
    add_keyword_attrs(path)
}

/// Like [`write_keyword_attr_catalog`], but as `dir/attrs.lrcat` with
/// placeholder originals. Returns the catalog path.
pub fn write_keyword_attr_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let (mac, win) = prepare_root_dirs(dir)?;
    let catalog = dir.join("attrs.lrcat");
    write_small(&catalog, &mac, &win)?;
    add_keyword_attrs(&catalog)?;
    create_original_files(&catalog)?;
    Ok(catalog)
}

fn add_keyword_attrs(path: &Path) -> Result<()> {
    let c = Connection::open(path)?;
    c.execute_batch("BEGIN")?;
    for (id, kind, export, parents, synonyms, _) in keyword_attr::ROWS {
        let name = if id == 28 {
            String::new()
        } else {
            format!("Zyx-Attr-{id}")
        };
        add_keyword(&c, id, Some(&name), 1, &format!("/1/{id}"))?;
        c.execute(
            "UPDATE AgLibraryKeyword SET keywordType = ?2, includeOnExport = ?3, includeParents = ?4, includeSynonyms = ?5 WHERE id_local = ?1",
            params![id, kind, export, parents, synonyms],
        )?;
    }
    // Assignments 31..=42; photo 0 means none. Photo 20 is a virtual copy.
    for (id, _, _, _, _, photo) in keyword_attr::ROWS {
        if photo != 0 {
            c.execute(
                "INSERT INTO AgLibraryKeywordImage (image, tag) VALUES (?1, ?2)",
                params![photo, id],
            )?;
        }
    }
    c.execute_batch("COMMIT")?;
    Ok(())
}

/// Writes the small fixture plus videos as `dir/videos.lrcat` with placeholder
/// originals (counts in [`video_edge`]). Returns the catalog path.
///
/// - photo 2: `fileFormat` VIDEO, extension `mov`, original missing
/// - photo 4: `fileFormat` VIDEO, extension `mov`
/// - photo 5: `fileFormat` JPG, extension `MP4` (video by extension only)
/// - photo 6: `fileFormat` JPG, extension `mpo` (not a video)
/// - photo 7: base name `clip.mp4`, extension `jpg` (not a video)
/// - photo 21: virtual copy of video 4, `fileFormat` VIDEO
///
/// Photo 3 stays a non-video with a missing original.
pub fn write_video_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let (mac, win) = prepare_root_dirs(dir)?;
    let catalog = dir.join("videos.lrcat");
    write_small(&catalog, &mac, &win)?;
    add_video_edges(&catalog)?;
    create_original_files(&catalog)?;
    Ok(catalog)
}

/// Writes the small fixture with `Adobe_images.pick` values as listed in
/// [`flag_edge::PICK_VALUES`] as `dir/flags.lrcat` with placeholder
/// originals. Returns the catalog path.
pub fn write_flag_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let (mac, win) = prepare_root_dirs(dir)?;
    let catalog = dir.join("flags.lrcat");
    write_small(&catalog, &mac, &win)?;
    add_flag_edges(&catalog)?;
    create_original_files(&catalog)?;
    Ok(catalog)
}

fn add_flag_edges(path: &Path) -> Result<()> {
    let c = Connection::open(path)?;
    // Rebuild `Adobe_images` with a nullable, untyped `pick` column.
    let sql: String = c.query_row(
        "SELECT sql FROM sqlite_master WHERE name = 'Adobe_images'",
        [],
        |r| r.get(0),
    )?;
    assert!(sql.contains("pick NOT NULL,"));
    c.execute_batch("BEGIN")?;
    c.execute_batch("ALTER TABLE Adobe_images RENAME TO Adobe_images_old")?;
    c.execute_batch(&sql.replace("pick NOT NULL,", "pick,"))?;
    c.execute_batch("INSERT INTO Adobe_images SELECT * FROM Adobe_images_old")?;
    c.execute_batch("DROP TABLE Adobe_images_old")?;
    for (id, value, _) in flag_edge::PICK_VALUES {
        c.execute_batch(&format!(
            "UPDATE Adobe_images SET pick = {value} WHERE id_local = {id}"
        ))?;
    }
    // A pick in a collection is not the flag of the photo.
    c.execute_batch("UPDATE AgLibraryCollectionImage SET pick = 1")?;
    c.execute_batch("COMMIT")?;
    Ok(())
}

/// Writes the small fixture with `Adobe_images.colorLabels` as listed in
/// [`label_edge::values`] as `dir/labels.lrcat` with placeholder originals.
/// Returns the catalog path.
pub fn write_label_catalog_with_files(dir: &Path, fixture: label_edge::Fixture) -> Result<PathBuf> {
    let (mac, win) = prepare_root_dirs(dir)?;
    let catalog = dir.join("labels.lrcat");
    write_small(&catalog, &mac, &win)?;
    add_label_edges(&catalog, fixture)?;
    create_original_files(&catalog)?;
    Ok(catalog)
}

fn add_label_edges(path: &Path, fixture: label_edge::Fixture) -> Result<()> {
    let c = Connection::open(path)?;
    // Rebuild `Adobe_images` with a nullable `colorLabels` column.
    let sql: String = c.query_row(
        "SELECT sql FROM sqlite_master WHERE name = 'Adobe_images'",
        [],
        |r| r.get(0),
    )?;
    assert!(sql.contains("colorLabels NOT NULL,"));
    c.execute_batch("BEGIN")?;
    c.execute_batch("ALTER TABLE Adobe_images RENAME TO Adobe_images_old")?;
    c.execute_batch(&sql.replace("colorLabels NOT NULL,", "colorLabels,"))?;
    c.execute_batch("INSERT INTO Adobe_images SELECT * FROM Adobe_images_old")?;
    c.execute_batch("DROP TABLE Adobe_images_old")?;
    for (id, label) in label_edge::values(fixture) {
        c.execute(
            "UPDATE Adobe_images SET colorLabels = ?1 WHERE id_local = ?2",
            params![label, id],
        )?;
    }
    c.execute(
        "UPDATE Adobe_images SET colorLabels = NULL WHERE id_local = ?1",
        [label_edge::NULL_ID],
    )?;
    c.execute_batch("COMMIT")?;
    Ok(())
}

/// Writes the small fixture with the `AgLibraryIPTC` rows of
/// [`caption_edge`] as `dir/captions.lrcat` with placeholder originals.
/// Photos not named in [`caption_edge`] (1, 12..=19) have one row with a
/// NULL caption. Returns the catalog path.
pub fn write_caption_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let (mac, win) = prepare_root_dirs(dir)?;
    let catalog = dir.join("captions.lrcat");
    write_small(&catalog, &mac, &win)?;
    add_caption_edges(&catalog)?;
    create_original_files(&catalog)?;
    Ok(catalog)
}

fn add_caption_edges(path: &Path) -> Result<()> {
    use caption_edge as e;
    let c = Connection::open(path)?;
    c.execute_batch("BEGIN")?;
    let put = |row: i64, image: i64, caption: rusqlite::types::Value| {
        c.execute(
            "INSERT INTO AgLibraryIPTC (id_local, caption, image) VALUES (?1, ?2, ?3)",
            params![row, caption, image],
        )
    };
    use rusqlite::types::Value as V;
    let text = |s: &str| V::Text(s.to_string());
    put(1001, 1, V::Null)?;
    for id in 12..=19 {
        put(1000 + id, id, V::Null)?;
    }
    put(1004, e::UMLAUT_ID, text(e::UMLAUT_TEXT))?;
    put(1005, e::PADDED_ID, text(e::PADDED_TEXT))?;
    put(1006, e::EMPTY_ID, text(""))?;
    put(1007, e::NULL_ID, V::Null)?;
    put(101, e::DUPLICATE_ID, text("second"))?;
    put(100, e::DUPLICATE_ID, text(e::DUPLICATE_TEXT))?;
    put(200, e::DUPLICATE_NULL_FIRST_ID, V::Null)?;
    put(201, e::DUPLICATE_NULL_FIRST_ID, text("later"))?;
    put(1011, e::NON_TEXT_ID, V::Integer(42))?;
    put(1002, 2, text(e::SKIPPED_TEXTS[0]))?;
    put(1020, 20, text(e::SKIPPED_TEXTS[1]))?;
    c.execute_batch("COMMIT")?;
    Ok(())
}

/// Writes the small fixture with the rights rows of [`rights_edge`] as
/// `dir/rights.lrcat` with placeholder originals. Returns the catalog path.
pub fn write_rights_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    write_rights(dir, true)
}

/// Like [`write_rights_catalog_with_files`] but without the tables
/// `AgHarvestedIptcMetadata` and `AgInternedIptcCreator`.
pub fn write_copyright_only_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    write_rights(dir, false)
}

fn write_rights(dir: &Path, creators: bool) -> Result<PathBuf> {
    let (mac, win) = prepare_root_dirs(dir)?;
    let catalog = dir.join("rights.lrcat");
    write_small(&catalog, &mac, &win)?;
    add_rights_edges(&catalog, creators)?;
    create_original_files(&catalog)?;
    Ok(catalog)
}

fn add_rights_edges(path: &Path, creators: bool) -> Result<()> {
    use rights_edge as e;
    use rusqlite::types::Value as V;
    let text = |s: &str| V::Text(s.to_string());
    let c = Connection::open(path)?;
    c.execute_batch("BEGIN")?;
    let copy = |row: i64, image: i64, v: V| {
        c.execute(
            "INSERT INTO AgLibraryIPTC (id_local, copyright, image) VALUES (?1, ?2, ?3)",
            params![row, v, image],
        )
    };
    for id in [1, 12, 13, 14, 15, 16, 17, 18, 19] {
        copy(1000 + id, id, V::Null)?;
    }
    copy(1004, e::UMLAUT_ID, text(e::UMLAUT_COPYRIGHT))?;
    copy(1005, e::PADDED_ID, text(e::PADDED_COPYRIGHT))?;
    copy(1006, e::EMPTY_ID, text(""))?;
    copy(1007, e::TWO_HARVESTED_ID, V::Null)?;
    copy(101, e::DUPLICATE_ID, text("second"))?;
    copy(100, e::DUPLICATE_ID, text(e::DUPLICATE_COPYRIGHT))?;
    copy(200, e::DUPLICATE_NULL_FIRST_ID, V::Null)?;
    copy(201, e::DUPLICATE_NULL_FIRST_ID, text("later"))?;
    copy(1011, e::NON_TEXT_ID, V::Integer(42))?;
    copy(1002, 2, text("skipped-missing-copyright"))?;
    copy(1020, 20, text("skipped-virtual-copyright"))?;
    if creators {
        c.execute_batch(
            "CREATE TABLE AgInternedIptcCreator (id_local INTEGER PRIMARY KEY, searchIndex, value);
             CREATE TABLE AgHarvestedIptcMetadata (id_local INTEGER PRIMARY KEY, image INTEGER, creatorRef INTEGER);",
        )?;
        let interned = |id: i64, v: V| {
            c.execute(
                "INSERT INTO AgInternedIptcCreator (id_local, value) VALUES (?1, ?2)",
                params![id, v],
            )
        };
        interned(501, text(e::FIRST_CREATOR))?;
        interned(502, text("Second Creator"))?;
        interned(503, text(e::UMLAUT_CREATOR))?;
        interned(504, text(""))?;
        interned(505, V::Integer(42))?;
        interned(506, text("skipped-missing-creator"))?;
        interned(507, text("skipped-virtual-creator"))?;
        let harvest = |row: i64, image: i64, r: Option<i64>| {
            c.execute(
                "INSERT INTO AgHarvestedIptcMetadata (id_local, image, creatorRef) VALUES (?1, ?2, ?3)",
                params![row, image, r],
            )
        };
        harvest(1, e::UMLAUT_ID, Some(503))?;
        harvest(2, e::PADDED_ID, None)?;
        harvest(3, e::EMPTY_ID, Some(999))?;
        harvest(301, e::TWO_HARVESTED_ID, Some(502))?;
        harvest(300, e::TWO_HARVESTED_ID, Some(501))?;
        harvest(400, e::DUPLICATE_ID, None)?;
        harvest(401, e::DUPLICATE_ID, Some(501))?;
        harvest(10, e::DUPLICATE_NULL_FIRST_ID, Some(504))?;
        harvest(11, e::NON_TEXT_ID, Some(505))?;
        harvest(12, 12, Some(501))?;
        harvest(13, 13, Some(501))?;
        harvest(102, 2, Some(506))?;
        harvest(120, 20, Some(507))?;
    }
    c.execute_batch("COMMIT")?;
    Ok(())
}

/// Combined IPTC edges for [`write_full_catalog_with_files`]: caption and
/// copyright in one `AgLibraryIPTC` row per photo, plus the creator tables of
/// [`rights_edge`]. The reader reads caption and copyright from the same row
/// (the one with the smallest `id_local`, see `crates/lr-reader/src/photos.rs`),
/// so the two independent fixtures [`caption_edge`] and [`rights_edge`] — which
/// share the same photo ids — are merged here into one row each. The edge
/// cases (umlaut, padding, empty, NULL, no row, duplicates, non-text) match the
/// two fixtures; skipped photos 2 and 20 carry values that must not be counted.
fn add_iptc_combined(path: &Path) -> Result<()> {
    use caption_edge as cap;
    use rights_edge as right;
    use rusqlite::types::Value as V;
    let text = |s: &str| V::Text(s.to_string());
    let c = Connection::open(path)?;
    c.execute_batch("BEGIN")?;
    let put = |row: i64, image: i64, caption: V, copyright: V| {
        c.execute(
            "INSERT INTO AgLibraryIPTC (id_local, caption, copyright, image) VALUES (?1, ?2, ?3, ?4)",
            params![row, caption, copyright, image],
        )
    };
    // Photos without edge values get one NULL row.
    put(1001, 1, V::Null, V::Null)?;
    for id in 12..=19 {
        put(1000 + id, id, V::Null, V::Null)?;
    }
    // Photo 4: umlaut caption and copyright (a video in the full catalog, so
    // neither counts, but the row is realistic).
    put(
        1004,
        cap::UMLAUT_ID,
        text(cap::UMLAUT_TEXT),
        text(right::UMLAUT_COPYRIGHT),
    )?;
    // Photo 5: padded caption and copyright (also a video here).
    put(
        1005,
        cap::PADDED_ID,
        text(cap::PADDED_TEXT),
        text(right::PADDED_COPYRIGHT),
    )?;
    // Photo 6: empty caption and copyright.
    put(1006, cap::EMPTY_ID, text(""), text(""))?;
    // Photo 7: NULL caption and copyright.
    put(1007, cap::NULL_ID, V::Null, V::Null)?;
    // Photo 9: duplicate rows; the smallest id_local (100) wins for both.
    put(101, cap::DUPLICATE_ID, text("second"), text("second"))?;
    put(
        100,
        cap::DUPLICATE_ID,
        text(cap::DUPLICATE_TEXT),
        text(right::DUPLICATE_COPYRIGHT),
    )?;
    // Photo 10: NULL first for both.
    put(200, cap::DUPLICATE_NULL_FIRST_ID, V::Null, V::Null)?;
    put(
        201,
        cap::DUPLICATE_NULL_FIRST_ID,
        text("later"),
        text("later"),
    )?;
    // Photo 11: a non-text value for both.
    put(1011, cap::NON_TEXT_ID, V::Integer(42), V::Integer(42))?;
    // Skipped photos carry values that must never be counted.
    put(
        1002,
        2,
        text(cap::SKIPPED_TEXTS[0]),
        text("skipped-missing-copyright"),
    )?;
    put(
        1020,
        20,
        text(cap::SKIPPED_TEXTS[1]),
        text("skipped-virtual-copyright"),
    )?;
    // Creator tables, mirroring add_rights_edges with creators = true.
    c.execute_batch(
        "CREATE TABLE AgInternedIptcCreator (id_local INTEGER PRIMARY KEY, searchIndex, value);
         CREATE TABLE AgHarvestedIptcMetadata (id_local INTEGER PRIMARY KEY, image INTEGER, creatorRef INTEGER);",
    )?;
    let interned = |id: i64, v: V| {
        c.execute(
            "INSERT INTO AgInternedIptcCreator (id_local, value) VALUES (?1, ?2)",
            params![id, v],
        )
    };
    interned(501, text(right::FIRST_CREATOR))?;
    interned(502, text("Second Creator"))?;
    interned(503, text(right::UMLAUT_CREATOR))?;
    interned(504, text(""))?;
    interned(505, V::Integer(42))?;
    interned(506, text("skipped-missing-creator"))?;
    interned(507, text("skipped-virtual-creator"))?;
    let harvest = |row: i64, image: i64, r: Option<i64>| {
        c.execute(
            "INSERT INTO AgHarvestedIptcMetadata (id_local, image, creatorRef) VALUES (?1, ?2, ?3)",
            params![row, image, r],
        )
    };
    harvest(1, right::UMLAUT_ID, Some(503))?;
    harvest(2, right::PADDED_ID, None)?;
    harvest(3, right::EMPTY_ID, Some(999))?;
    harvest(301, right::TWO_HARVESTED_ID, Some(502))?;
    harvest(300, right::TWO_HARVESTED_ID, Some(501))?;
    harvest(400, right::DUPLICATE_ID, None)?;
    harvest(401, right::DUPLICATE_ID, Some(501))?;
    harvest(10, right::DUPLICATE_NULL_FIRST_ID, Some(504))?;
    harvest(11, right::NON_TEXT_ID, Some(505))?;
    harvest(12, 12, Some(501))?;
    harvest(13, 13, Some(501))?;
    harvest(102, 2, Some(506))?;
    harvest(120, 20, Some(507))?;
    c.execute_batch("COMMIT")?;
    Ok(())
}

/// Number of keyword assignments (`AgLibraryKeywordImage` rows) of `photo`
/// in the catalog at `path`. Opened read-only.
pub fn assignments_to_photo(path: &Path, photo: i64) -> Result<i64> {
    let c = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    c.query_row(
        "SELECT COUNT(*) FROM AgLibraryKeywordImage WHERE image = ?1",
        [photo],
        |r| r.get(0),
    )
}

/// Writes the small fixture with its collections replaced by the ones
/// described in [`collection_edge`]. Order of the member rows follows
/// `AgLibraryCollectionImage.id_local`, which is the insertion order below.
pub fn write_collection_catalog(path: &Path) -> Result<()> {
    write_small_catalog(path)?;
    add_collection_edges(path)
}

/// Like [`write_collection_catalog`], but as `dir/collections.lrcat` with
/// placeholder originals as in [`write_small_catalog_with_files`].
pub fn write_collection_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let (mac, win) = prepare_root_dirs(dir)?;
    let catalog = dir.join("collections.lrcat");
    write_small(&catalog, &mac, &win)?;
    add_collection_edges(&catalog)?;
    create_original_files(&catalog)?;
    Ok(catalog)
}

/// Ids of the quick collection fixture ([`write_quick_catalog_with_files`]):
/// the collection fixture with exactly one quick collection (id 14,
/// `systemOnly` REAL 1.0) whose members are given in [`quick_edge::ROWS`].
pub mod quick_edge {
    pub const QUICK_ID: i64 = 14;
    /// `(photo, position)` rows of the quick collection, in row order. 20 is a
    /// virtual copy and 999 does not exist: both are skipped.
    pub const ROWS: [(i64, f64); 5] = [(1, 3.0), (9, 1.0), (7, 2.0), (20, 4.0), (999, 5.0)];
    /// The converted members in Lightroom order (CLI: photo 20's original is
    /// missing, so it is dropped before collections are read).
    pub const MEMBERS: [i64; 3] = [9, 7, 1];
    pub const MEMBERS_SKIPPED: i64 = 2;
    /// Reader level: the virtual copy 20 converts with its master (T03).
    pub const MEMBERS_READER: [i64; 4] = [9, 7, 1, 20];
    pub const MEMBERS_SKIPPED_READER: i64 = 1;
}

/// Writes the collection fixture as `dir/quick.lrcat` with placeholder
/// originals, but with a single quick collection (id 14) that has the member
/// rows of [`quick_edge::ROWS`]; quick collection 15 and the member rows of
/// 14 and 15 of the original fixture are removed.
pub fn write_quick_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let (mac, win) = prepare_root_dirs(dir)?;
    let catalog = dir.join("quick.lrcat");
    write_small(&catalog, &mac, &win)?;
    add_collection_edges(&catalog)?;
    let c = Connection::open(&catalog)?;
    c.execute_batch(
        "DELETE FROM AgLibraryCollectionImage WHERE collection IN (14, 15); \
         DELETE FROM AgLibraryCollection WHERE id_local = 15;",
    )?;
    for (photo, pos) in quick_edge::ROWS {
        c.execute(
            "INSERT INTO AgLibraryCollectionImage (collection, image, pick, positionInCollection) VALUES (14, ?1, 0, ?2)",
            params![photo, pos],
        )?;
    }
    drop(c);
    create_original_files(&catalog)?;
    Ok(catalog)
}

/// Smart collection fixture ([`write_smart_catalog`]): the small fixture
/// plus smart collections 101.. with rule text in
/// `AgLibraryCollectionContent`. All values are invented; the marker
/// [`SECRET`] appears in every value and name so that tests can prove that
/// reports never contain them.
pub mod smart_edge {
    /// Substring of every rule value and of every collection name added here.
    pub const SECRET: &str = "ZyxSecret";
    /// `(collection id, owningModule, content)`. Content `None` is SQL NULL.
    /// Collection ids 5 and 6 of the small fixture have no content row at
    /// all; the table itself exists in this fixture.
    pub type Row = (i64, &'static str, Option<&'static str>);
    pub const SMART: &str = "ag.library.smart_collection";
    /// Readable rule texts, in id order. See the expectations in
    /// `crates/lr-reader/tests/smart_rules.rs`.
    pub const READABLE: [(i64, &str); 14] = [
        (
            101,
            "s = {\n\t{\n\t\tcriteria = \"rating\",\n\t\toperation = \">=\",\n\t\tvalue = 3,\n\t},\n\tcombine = \"intersect\",\n}",
        ),
        (
            102,
            "s = {\n\tcombine = \"union\",\n\t{\n\t\tcriteria = \"pick\",\n\t\toperation = \"==\",\n\t\tvalue = -1,\n\t},\n}",
        ),
        (
            103,
            "s = {\n\t{\n\t\tcriteria = \"labelColor\",\n\t\toperation = \"==\",\n\t\tvalue = \"red ZyxSecret\",\n\t},\n\tcombine = \"intersect\",\n}",
        ),
        (
            104,
            "s = {\n\t{\n\t\tcriteria = \"keywords\",\n\t\toperation = \"any\",\n\t\tvalue = \"Z\u{fc}\u{df}e \u{1f305} ZyxSecret\",\n\t},\n\tcombine = \"intersect\",\n}",
        ),
        (
            105,
            "s = {\n\t{\n\t\tcriteria = \"keywords\",\n\t\toperation = \"empty\",\n\t},\n\tcombine = \"intersect\",\n}",
        ),
        (
            106,
            "s = {\n\t{\n\t\tcriteria = \"captureTime\",\n\t\toperation = \"inLast\",\n\t\tvalue = 30,\n\t\tvalue_units = \"days\",\n\t},\n\tcombine = \"intersect\",\n}",
        ),
        (
            107,
            "s = {\n\t{\n\t\tcriteria = \"touchTime\",\n\t\toperation = \"inLast\",\n\t\tvalue = 2,\n\t\tvalue_units = \"months\",\n\t},\n\tcombine = \"intersect\",\n}",
        ),
        (
            108,
            "s = {\n\t{\n\t\tcriteria = \"fileFormat\",\n\t\toperation = \"==\",\n\t\tvalue = \"DNG ZyxSecret\",\n\t},\n\tcombine = \"intersect\",\n}",
        ),
        // Several rules, union, a string and a number as value, the same
        // criterion twice.
        (
            109,
            "s = {\n\t{\n\t\tcriteria = \"rating\",\n\t\toperation = \"==\",\n\t\tvalue = 5,\n\t},\n\t{\n\t\tcriteria = \"labelColor\",\n\t\toperation = \"==\",\n\t\tvalue = \"blue ZyxSecret\",\n\t},\n\t{\n\t\tcriteria = \"rating\",\n\t\toperation = \"==\",\n\t\tvalue = 4,\n\t},\n\tcombine = \"union\",\n}",
        ),
        // Unknown criterion, operation and combine are kept verbatim.
        (
            110,
            "s = {\n\t{\n\t\tcriteria = \"zyxCriterion\",\n\t\toperation = \"zyxOp\",\n\t\tvalue = \"ZyxSecret\",\n\t},\n\tcombine = \"zyxCombine\",\n}",
        ),
        // Negative and fractional numbers, a number written as text.
        (
            111,
            "s = {\n\t{\n\t\tcriteria = \"rating\",\n\t\toperation = \"<\",\n\t\tvalue = 2.5,\n\t\tvalue2 = -1,\n\t},\n\t{\n\t\tcriteria = \"rating\",\n\t\toperation = \">\",\n\t\tvalue = \"3\",\n\t},\n\tcombine = \"intersect\",\n}",
        ),
        // Escapes inside strings, an unknown extra key holding an escaped
        // nested table (as in the synthetic sample), a boolean extra value.
        (
            112,
            "s = {\n\t{\n\t\tcriteria = \"keywords\",\n\t\toperation = \"any\",\n\t\tvalue = \"say \\\"ZyxSecret\\\" \\\\ ok\",\n\t\tsomeOther = \"anObject = {\\\nkey = \\\"lr\\\",\\\n}\\\n\",\n\t\tflag = true,\n\t},\n\tcombine = \"intersect\",\n}",
        ),
        // No criteria at all.
        (113, "s = {\n\tcombine = \"intersect\",\n}"),
        // Compact formatting without newlines or tabs.
        (
            114,
            "s={{criteria=\"pick\",operation=\"==\",value=1,},combine=\"union\",}",
        ),
    ];
    /// Unreadable rule texts: truncated, malformed, empty, NULL.
    pub const UNREADABLE: [(i64, Option<&str>); 8] = [
        (
            121,
            Some("s = {\n\t{\n\t\tcriteria = \"rating\",\n\t\toperation = \">=\",\n\t\tvalue = 3,"),
        ),
        (
            122,
            Some(
                "s = {\n\t{\n\t\tcriteria = \"rating\",\n\t\toperation = \">=\",\n\t\tvalue = \"ZyxSecr",
            ),
        ),
        (123, Some("this is not a table ZyxSecret")),
        (124, Some("")),
        (125, None),
        (
            126,
            Some("s = {\n\t{\n\t\tcriteria = ,\n\t},\n\tcombine = \"intersect\",\n}"),
        ),
        (
            127,
            Some("s = { { criteria = \"rating\" } } }}}} ZyxSecret"),
        ),
        (
            128,
            Some("s = \u{0}\u{1}\u{ff}{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{"),
        ),
    ];
    /// Smart collections whose only content row has another `owningModule`
    /// (and so no rules).
    pub const OTHER_MODULE_ONLY: i64 = 129;
    /// A collection of the small fixture's kind "plain" (id 4) gets a
    /// smart-module content row too; it must be ignored.
    pub const PLAIN_WITH_RULES: i64 = 4;
    /// Rows added to `AgLibraryCollection` (ids 101..=114, 121..=128, 129).
    pub const ADDED: i64 = 23;
    /// Smart collections in the whole fixture: the 2 of the small fixture
    /// (5, 6; no content row) plus the 23 added.
    pub const SMART_TOTAL: i64 = 25;
    /// Smart collections that give readable rules.
    pub const READABLE_COUNT: i64 = 14;
    /// Smart collections without readable rules: 5, 6, 121..=128, 129.
    pub const UNREADABLE_COUNT: i64 = 11;
}

/// Writes the small fixture plus the smart collections of [`smart_edge`].
/// Smart collection 101 also has a view-settings content row (other
/// `owningModule`) with garbage text that must be ignored.
pub fn write_smart_catalog(path: &Path) -> Result<()> {
    write_small_catalog(path)?;
    add_smart_edges(path)
}

/// Like [`write_smart_catalog`], but as `dir/smart.lrcat` with placeholder
/// originals.
pub fn write_smart_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let (mac, win) = prepare_root_dirs(dir)?;
    let catalog = dir.join("smart.lrcat");
    write_small(&catalog, &mac, &win)?;
    add_smart_edges(&catalog)?;
    create_original_files(&catalog)?;
    Ok(catalog)
}

/// Smart collections for the translation tests ([`write_smart_translate_catalog_with_files`]),
/// on top of [`smart_edge`]. Rule values are plain star counts; names carry
/// [`smart_edge::SECRET`].
pub mod smart_translate {
    /// Smart collections that translate: 101 (from [`super::smart_edge`]:
    /// rating >= 3, top level), 201 (rating == 5, top level), 202
    /// (rating >= 4, `union`, inside collection set 1), 203 (rating >= 1
    /// and rating == 5, `intersect`, top level).
    pub const TRANSLATABLE: [i64; 4] = [101, 201, 202, 203];
    /// The collection set (id 1 of the small fixture) that holds 202.
    pub const PARENT_SET: i64 = 1;
    /// Smart collections that are skipped although their rules are
    /// readable: 204 (rating and pick), 205 (rating `>`), 206 (rating >= 3
    /// inside the plain collection 2, which cannot be a parent).
    pub const NEW_SKIPPED: [i64; 3] = [204, 205, 206];
    /// Rows added by this fixture.
    pub const ADDED: i64 = 6;
    /// The expected `rules` parameters, by collection id, as JSON text.
    pub const RULES: [(i64, &str); 4] = [
        (
            101,
            r#"{"ruleSet":{"match":"all","rules":[{"field":"rating","op":"gte","value":3}]}}"#,
        ),
        (
            201,
            r#"{"ruleSet":{"match":"all","rules":[{"field":"rating","op":"is","value":5}]}}"#,
        ),
        (
            202,
            r#"{"ruleSet":{"match":"any","rules":[{"field":"rating","op":"gte","value":4}]}}"#,
        ),
        (
            203,
            r#"{"ruleSet":{"match":"all","rules":[{"field":"rating","op":"gte","value":1},{"field":"rating","op":"is","value":5}]}}"#,
        ),
    ];
}

/// Like [`write_smart_catalog_with_files`], plus the collections of
/// [`smart_translate`].
pub fn write_smart_translate_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let catalog = write_smart_catalog_with_files(dir)?;
    let c = Connection::open(&catalog)?;
    const R: &str = "criteria = \"rating\"";
    // (id, parent, rule text)
    let rows: [(i64, Option<i64>, String); 6] = [
        (
            201,
            None,
            format!(
                "s = {{\n\t{{\n\t\t{R},\n\t\toperation = \"==\",\n\t\tvalue = 5,\n\t}},\n\tcombine = \"intersect\",\n}}"
            ),
        ),
        (
            202,
            Some(smart_translate::PARENT_SET),
            format!(
                "s = {{\n\t{{\n\t\t{R},\n\t\toperation = \">=\",\n\t\tvalue = 4,\n\t}},\n\tcombine = \"union\",\n}}"
            ),
        ),
        (
            203,
            None,
            format!(
                "s = {{\n\t{{\n\t\t{R},\n\t\toperation = \">=\",\n\t\tvalue = 1,\n\t}},\n\t{{\n\t\t{R},\n\t\toperation = \"==\",\n\t\tvalue = 5,\n\t}},\n\tcombine = \"intersect\",\n}}"
            ),
        ),
        (
            204,
            None,
            format!(
                "s = {{\n\t{{\n\t\t{R},\n\t\toperation = \">=\",\n\t\tvalue = 2,\n\t}},\n\t{{\n\t\tcriteria = \"pick\",\n\t\toperation = \"==\",\n\t\tvalue = 1,\n\t}},\n\tcombine = \"intersect\",\n}}"
            ),
        ),
        (
            205,
            None,
            format!(
                "s = {{\n\t{{\n\t\t{R},\n\t\toperation = \">\",\n\t\tvalue = 2,\n\t}},\n\tcombine = \"intersect\",\n}}"
            ),
        ),
        (
            206,
            Some(2),
            format!(
                "s = {{\n\t{{\n\t\t{R},\n\t\toperation = \">=\",\n\t\tvalue = 3,\n\t}},\n\tcombine = \"intersect\",\n}}"
            ),
        ),
    ];
    for (id, parent, text) in rows {
        c.execute(
            "INSERT INTO AgLibraryCollection (id_local, creationId, genealogy, imageCount, name, parent, systemOnly) VALUES (?1, 'com.adobe.ag.library.smart_collection', ?2, 0, ?3, ?4, 0)",
            params![id, format!("/{id}"), format!("Smart {} {id}", smart_edge::SECRET), parent],
        )?;
        c.execute(
            "INSERT INTO AgLibraryCollectionContent (collection, content, owningModule) VALUES (?1, ?2, ?3)",
            params![id, text, smart_edge::SMART],
        )?;
    }
    Ok(catalog)
}

fn add_smart_edges(path: &Path) -> Result<()> {
    let c = Connection::open(path)?;
    c.execute_batch(
        "CREATE TABLE AgLibraryCollectionContent (id_local INTEGER PRIMARY KEY, collection INTEGER, content, owningModule);",
    )?;
    let ids: Vec<i64> = smart_edge::READABLE
        .iter()
        .map(|(id, _)| *id)
        .chain(smart_edge::UNREADABLE.iter().map(|(id, _)| *id))
        .chain([smart_edge::OTHER_MODULE_ONLY])
        .collect();
    for id in ids {
        c.execute(
            "INSERT INTO AgLibraryCollection (id_local, creationId, genealogy, imageCount, name, parent, systemOnly) VALUES (?1, 'com.adobe.ag.library.smart_collection', ?2, 0, ?3, NULL, 0)",
            params![id, format!("/{id}"), format!("Smart {} {id}", smart_edge::SECRET)],
        )?;
    }
    let insert = |coll: i64, module: &str, content: Option<&str>| {
        c.execute(
            "INSERT INTO AgLibraryCollectionContent (collection, content, owningModule) VALUES (?1, ?2, ?3)",
            params![coll, content, module],
        )
    };
    for (id, text) in smart_edge::READABLE {
        insert(id, smart_edge::SMART, Some(text))?;
    }
    for (id, text) in smart_edge::UNREADABLE {
        insert(id, smart_edge::SMART, text)?;
    }
    insert(
        101,
        "com.adobe.ag.library.sortType",
        Some("s = {{{ not rules ZyxSecret"),
    )?;
    insert(
        smart_edge::OTHER_MODULE_ONLY,
        "com.adobe.ag.library.sortDirection",
        Some("s = { combine = \"intersect\", }"),
    )?;
    insert(
        smart_edge::PLAIN_WITH_RULES,
        smart_edge::SMART,
        Some(smart_edge::READABLE[0].1),
    )?;
    Ok(())
}

/// Smart collections with boolean rule values ([`write_smart_bool_catalog`]).
pub mod smart_bool {
    /// `(collection id, rule text)`: 301 `value = true`, 302 `value2 =
    /// false`, 303 `value_units = true`, 304 `value = { 1 }` (a table: still
    /// unreadable).
    pub const ROWS: [(i64, &str); 4] = [
        (
            301,
            "s = {\n\t{\n\t\tcriteria = \"hasGps\",\n\t\toperation = \"==\",\n\t\tvalue = true,\n\t},\n\tcombine = \"intersect\",\n}",
        ),
        (
            302,
            "s = {\n\t{\n\t\tcriteria = \"rating\",\n\t\toperation = \">=\",\n\t\tvalue = 3,\n\t\tvalue2 = false,\n\t},\n\tcombine = \"intersect\",\n}",
        ),
        (
            303,
            "s = {\n\t{\n\t\tcriteria = \"captureTime\",\n\t\toperation = \"inLast\",\n\t\tvalue = 30,\n\t\tvalue_units = true,\n\t},\n\tcombine = \"intersect\",\n}",
        ),
        (
            304,
            "s = {\n\t{\n\t\tcriteria = \"pick\",\n\t\toperation = \"==\",\n\t\tvalue = { 1 },\n\t},\n\tcombine = \"intersect\",\n}",
        ),
    ];
}

/// Like [`write_smart_catalog`], plus the top-level smart collections of
/// [`smart_bool`].
pub fn write_smart_bool_catalog(path: &Path) -> Result<()> {
    write_smart_catalog(path)?;
    let c = Connection::open(path)?;
    for (id, text) in smart_bool::ROWS {
        c.execute(
            "INSERT INTO AgLibraryCollection (id_local, creationId, genealogy, imageCount, name, parent, systemOnly) VALUES (?1, 'com.adobe.ag.library.smart_collection', ?2, 0, ?3, NULL, 0)",
            params![id, format!("/{id}"), format!("Smart {} {id}", smart_edge::SECRET)],
        )?;
        c.execute(
            "INSERT INTO AgLibraryCollectionContent (collection, content, owningModule) VALUES (?1, ?2, ?3)",
            params![id, text, smart_edge::SMART],
        )?;
    }
    Ok(())
}

/// Removes every collection and every collection member from the catalog at
/// `path`.
pub fn clear_collections(path: &Path) -> Result<()> {
    let c = Connection::open(path)?;
    c.execute_batch("DELETE FROM AgLibraryCollectionImage; DELETE FROM AgLibraryCollection;")
}

/// Replaces the rules of smart collection `id` with a rule that cannot be
/// translated (`pick`), so a collection written earlier is still in the
/// catalog but no longer translatable.
pub fn make_smart_untranslatable(path: &Path, id: i64) -> Result<()> {
    let c = Connection::open(path)?;
    let text = "s = {\n\t{\n\t\tcriteria = \"pick\",\n\t\toperation = \"==\",\n\t\tvalue = 1,\n\t},\n\tcombine = \"intersect\",\n}";
    let changed = c.execute(
        "UPDATE AgLibraryCollectionContent SET content = ?1 WHERE collection = ?2 AND owningModule = ?3",
        params![text, id, smart_edge::SMART],
    )?;
    assert_eq!(changed, 1, "smart collection {id} has no smart content row");
    Ok(())
}

/// How [`break_smart_rules`] destroys the rules of a smart collection.
#[derive(Debug, Clone, Copy)]
pub enum RuleDamage {
    /// The rule text is cut off in the middle of a rule.
    Truncated,
    /// The content row is deleted.
    RowMissing,
    /// The content is SQL NULL.
    NullContent,
    /// The whole `AgLibraryCollectionContent` table is dropped.
    TableMissing,
}

/// Makes the rules of smart collection `id` unreadable while the
/// collection row itself stays.
pub fn break_smart_rules(path: &Path, id: i64, damage: RuleDamage) -> Result<()> {
    let c = Connection::open(path)?;
    match damage {
        RuleDamage::Truncated => {
            c.execute(
                "UPDATE AgLibraryCollectionContent SET content = 's = {\n\t{\n\t\tcriteria = \"rating\",' WHERE collection = ?1 AND owningModule = ?2",
                params![id, smart_edge::SMART],
            )?;
        }
        RuleDamage::RowMissing => {
            c.execute(
                "DELETE FROM AgLibraryCollectionContent WHERE collection = ?1 AND owningModule = ?2",
                params![id, smart_edge::SMART],
            )?;
        }
        RuleDamage::NullContent => {
            c.execute(
                "UPDATE AgLibraryCollectionContent SET content = NULL WHERE collection = ?1 AND owningModule = ?2",
                params![id, smart_edge::SMART],
            )?;
        }
        RuleDamage::TableMissing => {
            c.execute_batch("DROP TABLE AgLibraryCollectionContent;")?;
        }
    }
    Ok(())
}

fn add_collection_edges(path: &Path) -> Result<()> {
    use rusqlite::types::Value as V;
    const COLL: &str = "com.adobe.ag.library.collection";
    const SET: &str = "com.adobe.ag.library.group";
    let c = Connection::open(path)?;
    // `systemOnly` is nullable here (NOT NULL in a real catalog) so that NULL
    // can be tested.
    c.execute_batch(
        "BEGIN; DELETE FROM AgLibraryCollectionImage; DROP TABLE AgLibraryCollection; \
         CREATE TABLE AgLibraryCollection (id_local INTEGER PRIMARY KEY, creationId NOT NULL, genealogy NOT NULL, imageCount, name NOT NULL, parent INTEGER, systemOnly);",
    )?;
    let zero = || V::Integer(0);
    // (id, creationId, name, parent, systemOnly)
    let rows: Vec<(i64, &str, &str, Option<i64>, V)> = vec![
        (1, SET, "Set Top", None, zero()),
        (2, SET, "Set Mid", Some(1), V::Real(0.0)),
        (3, SET, "Set Deep", Some(2), V::Null),
        (4, COLL, "Album Deep", Some(3), zero()),
        (5, COLL, "Album Mid", Some(2), V::Null),
        (6, COLL, "Album Top", None, zero()),
        (7, COLL, "S\u{fc}\u{df}e \u{c4}pfel", Some(1), zero()),
        (8, COLL, "\u{1f305} Sunrise \u{1f304}", None, zero()),
        (9, COLL, "Gru\u{308}n ", None, zero()),
        (10, COLL, "Empty", Some(1), zero()),
        (11, COLL, "Twin", Some(2), zero()),
        (12, COLL, "Twin", Some(2), zero()),
        (
            13,
            "com.adobe.ag.library.smart_collection",
            "Zyx-Smart",
            None,
            zero(),
        ),
        (
            14,
            "com.adobe.ag.library.quick_collection",
            "Zyx-Quick-Real",
            None,
            V::Real(1.0),
        ),
        (15, COLL, "Zyx-Quick-Int", None, V::Integer(1)),
        (16, "com.adobe.ag.webGallery", "Zyx-Web", None, zero()),
        (17, "com.adobe.ag.print", "Zyx-Print", None, zero()),
        (18, COLL, "Zyx-Lost-Parent", Some(9999), zero()),
        (19, COLL, "Zyx-Album-Parent", Some(4), zero()),
        (20, COLL, "Zyx-Smart-Child", Some(13), zero()),
        (21, SET, "Zyx-Set-Below", Some(18), zero()),
        (22, COLL, "Zyx-Album-Below", Some(21), zero()),
        (23, SET, "Zyx-Cycle-A", Some(24), zero()),
        (24, SET, "Zyx-Cycle-B", Some(23), zero()),
        (25, SET, "Zyx-Self", Some(25), zero()),
    ];
    for (id, creation, name, parent, system) in rows {
        c.execute(
            "INSERT INTO AgLibraryCollection (id_local, creationId, genealogy, imageCount, name, parent, systemOnly) VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6)",
            params![id, creation, format!("/{id}"), name, parent, system],
        )?;
    }
    // (collection, image, position); the order is the row order.
    let n = V::Null;
    let r = |x: f64| V::Real(x);
    let members: Vec<(i64, i64, V)> = vec![
        (4, 1, r(3.0)),
        (4, 4, r(1.0)),
        (4, 5, r(2.0)),
        (5, 7, n.clone()),
        (5, 6, n.clone()),
        (5, 8, n.clone()),
        (6, 9, n.clone()),
        (6, 10, r(2.5)),
        (6, 11, r(1.5)),
        (6, 12, n.clone()),
        (7, 15, r(1.0)),
        (7, 15, r(2.0)),
        (7, 16, r(3.0)),
        (8, 20, r(1.0)),
        (8, 999, r(2.0)),
        (8, 17, r(3.0)),
        (9, 18, r(1.0)),
        (11, 13, r(1.0)),
        (12, 14, r(1.0)),
        (3, 19, r(1.0)),
        (13, 1, r(1.0)),
        (14, 2, r(1.0)),
        (15, 3, r(1.0)),
        (16, 4, r(1.0)),
        (18, 5, r(1.0)),
        (22, 15, r(1.0)),
    ];
    for (coll, image, pos) in members {
        c.execute(
            "INSERT INTO AgLibraryCollectionImage (collection, image, pick, positionInCollection) VALUES (?1, ?2, 0, ?3)",
            params![coll, image, pos],
        )?;
    }
    c.execute_batch("COMMIT")?;
    Ok(())
}

/// Writes the video fixture plus the folder stacks described in
/// [`stack_edge`] as `dir/stacks.lrcat` with placeholder originals.
pub fn write_stack_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let (mac, win) = prepare_root_dirs(dir)?;
    let catalog = dir.join("stacks.lrcat");
    write_small(&catalog, &mac, &win)?;
    add_video_edges(&catalog)?;
    add_stack_edges(&catalog)?;
    create_original_files(&catalog)?;
    Ok(catalog)
}

/// Removes the three folder stack tables from the catalog at `path`.
pub fn drop_stack_tables(path: &Path) -> Result<()> {
    let c = Connection::open(path)?;
    c.execute_batch(
        "DROP TABLE IF EXISTS AgLibraryFolderStackData; \
         DROP TABLE IF EXISTS AgLibraryFolderStackImage; \
         DROP TABLE IF EXISTS AgLibraryFolderStack;",
    )
}

/// Moves photo `image` to position `position` (REAL) of the stack member
/// rows in the catalog at `path`.
pub fn set_stack_member_position(path: &Path, stack: i64, image: i64, position: f64) -> Result<()> {
    let c = Connection::open(path)?;
    c.execute(
        "UPDATE AgLibraryFolderStackImage SET position = ?3 WHERE stack = ?1 AND image = ?2",
        params![stack, image, position],
    )?;
    Ok(())
}

fn add_stack_edges(path: &Path) -> Result<()> {
    use rusqlite::types::Value as V;
    let c = Connection::open(path)?;
    c.execute_batch(
        "BEGIN; \
         CREATE TABLE AgLibraryFolderStack (id_local INTEGER PRIMARY KEY, id_global, collapsed INTEGER, text); \
         CREATE TABLE AgLibraryFolderStackImage (id_local INTEGER PRIMARY KEY, collapsed INTEGER, image INTEGER, position, stack INTEGER); \
         CREATE TABLE AgLibraryFolderStackData (stack INTEGER, stackCount INTEGER, stackParent INTEGER);",
    )?;
    // (stack, collapsed, [(image, position)]) in row insertion order.
    let r = |x: f64| V::Real(x);
    let i = |x: i64| V::Integer(x);
    type Stack = (i64, i64, Vec<(i64, V)>);
    let stacks: Vec<Stack> = vec![
        (1, 0, vec![(7, r(2.0)), (6, r(1.0))]),
        (2, 1, vec![(10, i(2)), (8, i(3)), (9, i(1))]),
        (3, 0, vec![(11, r(1.0)), (20, r(2.0))]),
        (4, 0, vec![(12, r(1.0)), (4, r(2.0))]),
        (5, 0, vec![(13, r(1.0)), (14, r(2.0)), (5, r(3.0))]),
        (6, 0, vec![(15, r(1.0)), (16, r(2.0))]),
        (7, 0, vec![(17, r(1.0)), (18, r(2.0)), (16, r(3.0))]),
        (8, 0, vec![(19, r(2.0)), (1, r(3.0)), (999, r(4.0))]),
    ];
    let mut row = 0;
    for (stack, collapsed, members) in stacks {
        c.execute(
            "INSERT INTO AgLibraryFolderStack (id_local, id_global, collapsed, text) VALUES (?1, ?2, ?3, '')",
            params![stack, gid("stack", stack), collapsed],
        )?;
        c.execute(
            "INSERT INTO AgLibraryFolderStackData (stack, stackCount, stackParent) VALUES (?1, ?2, ?3)",
            params![stack, members.len() as i64, members[0].0],
        )?;
        for (image, position) in members {
            row += 1;
            c.execute(
                "INSERT INTO AgLibraryFolderStackImage (id_local, collapsed, image, position, stack) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![row, collapsed, image, position, stack],
            )?;
        }
    }
    c.execute_batch("COMMIT")?;
    Ok(())
}

fn add_video_edges(path: &Path) -> Result<()> {
    let c = Connection::open(path)?;
    c.execute_batch("BEGIN")?;
    // (photo id, base name, extension, fileFormat). Photo ids equal file ids
    // for photos 1..=19 in the small fixture.
    for (id, base, ext, format) in [
        (2, "IMG_0001", "mov", "VIDEO"),
        (4, "IMG_0004", "mov", "VIDEO"),
        (5, "IMG_0005", "MP4", "JPG"),
        (6, "IMG_0006", "mpo", "JPG"),
        (7, "clip.mp4", "jpg", "JPG"),
    ] {
        let name = format!("{base}.{ext}");
        c.execute(
            "UPDATE AgLibraryFile SET baseName = ?2, extension = ?3, idx_filename = ?4, \
             lc_idx_filename = ?5, lc_idx_filenameExtension = ?6, originalFilename = ?4 \
             WHERE id_local = ?1",
            params![id, base, ext, name, name.to_lowercase(), ext.to_lowercase()],
        )?;
        c.execute(
            "UPDATE Adobe_images SET fileFormat = ?2 WHERE id_local = ?1",
            params![id, format],
        )?;
    }
    // Virtual copy of video 4; it shares the file of photo 4.
    add_photo(&c, 21, "IMG_0004", 4, Some(2), Some(4), 4)?;
    c.execute_batch("UPDATE Adobe_images SET fileFormat = 'VIDEO' WHERE id_local = 21;")?;
    c.execute_batch("COMMIT")?;
    Ok(())
}

/// Writes the small fixture, then removes `table` (when `column` is `None`)
/// or just `column` from `table`. Used to test schema checks.
pub fn write_small_catalog_without(path: &Path, table: &str, column: Option<&str>) -> Result<()> {
    write_small_catalog(path)?;
    let conn = Connection::open(path)?;
    match column {
        None => conn.execute_batch(&format!("DROP TABLE \"{table}\";"))?,
        Some(c) => conn.execute_batch(&format!("ALTER TABLE \"{table}\" DROP COLUMN \"{c}\";"))?,
    }
    drop(conn);
    Ok(())
}

fn io_err(e: std::io::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(e))
}

/// Root folder path string below `dir` with a trailing separator.
fn root_below(dir: &Path, name: &str) -> String {
    let mut s = dir.join(name).to_string_lossy().into_owned();
    s.push(std::path::MAIN_SEPARATOR);
    s
}

/// Writes the small fixture as `dir/small.lrcat` and returns that path.
///
/// Both root folders lie below `dir` (`small::MACOS_ROOT_DIR`,
/// `small::WINDOWS_ROOT_DIR`), so the stored paths are valid on the test
/// machine. Every original gets a tiny placeholder file (not a real image)
/// except the files in `small::MISSING_FILE_IDS`. Counts are the same as in
/// `write_small_catalog`.
pub fn write_small_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let (mac, win) = prepare_root_dirs(dir)?;
    let catalog = dir.join("small.lrcat");
    write_small(&catalog, &mac, &win)?;
    create_original_files(&catalog)?;
    Ok(catalog)
}

fn prepare_root_dirs(dir: &Path) -> Result<(String, String)> {
    std::fs::create_dir_all(dir).map_err(io_err)?;
    Ok((
        root_below(dir, small::MACOS_ROOT_DIR),
        root_below(dir, small::WINDOWS_ROOT_DIR),
    ))
}

/// Creates a placeholder file for every original except the missing ones of
/// the small fixture (`small::MISSING_FILE_IDS`).
fn create_original_files(catalog: &Path) -> Result<()> {
    create_original_files_except(catalog, &small::MISSING_FILE_IDS)
}

/// Creates a placeholder file for every original.
fn create_all_original_files(catalog: &Path) -> Result<()> {
    create_original_files_except(catalog, &[])
}

/// Creates a placeholder file for every original not in `missing_file_ids`.
fn create_original_files_except(catalog: &Path, missing_file_ids: &[i64]) -> Result<()> {
    let c = Connection::open_with_flags(catalog, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut st = c.prepare(
        "SELECT f.id_local, r.absolutePath, d.pathFromRoot, f.idx_filename FROM AgLibraryFile f \
         JOIN AgLibraryFolder d ON d.id_local = f.folder \
         JOIN AgLibraryRootFolder r ON r.id_local = d.rootFolder ORDER BY f.id_local",
    )?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (id, root, rel, name) = row?;
        let mut p = PathBuf::from(root);
        for part in rel.split('/').filter(|x| !x.is_empty()) {
            p.push(part);
        }
        std::fs::create_dir_all(&p).map_err(io_err)?;
        if !missing_file_ids.contains(&id) {
            std::fs::write(p.join(name), b"placeholder\n").map_err(io_err)?;
        }
    }
    Ok(())
}

fn write_small(path: &Path, mac_root: &str, win_root: &str) -> Result<()> {
    let c = create(path)?;
    c.execute_batch("BEGIN")?;
    add_variables(&c)?;
    add_root_folder(&c, 1, mac_root, "Pictures")?;
    add_root_folder(&c, 2, win_root, "Pictures")?;
    let long = format!("{}/", vec!["very-long-folder-name"; 13].join("/"));
    let folders: [(i64, Option<i64>, String, i64); 6] = [
        (1, None, String::new(), 1),
        (2, Some(1), "2024/".into(), 1),
        (3, Some(2), "2024/Rome/".into(), 1),
        (4, None, String::new(), 2),
        (5, Some(4), "Trips/".into(), 2),
        (6, Some(5), format!("Trips/{long}"), 2),
    ];
    for (id, parent, rel, root) in &folders {
        add_folder(&c, *id, *parent, rel, *root)?;
    }
    // Ratings by photo id 1..=20: 5 NULL, 3 zero, stars 1..5 as in `small`.
    let ratings: [Option<i64>; 20] = [
        None,
        None,
        None,
        None,
        None,
        Some(0),
        Some(0),
        Some(0),
        Some(1),
        Some(1),
        Some(1),
        Some(2),
        Some(2),
        Some(3),
        Some(3),
        Some(3),
        Some(4),
        Some(4),
        Some(5),
        Some(5),
    ];
    let mut file_id = 0;
    for id in 1..=20_i64 {
        // Photos 1 and 2 share a file name in different folders.
        let (base, folder) = match id {
            1 => ("IMG_0001".to_string(), 2),
            2 => ("IMG_0001".to_string(), 5),
            19 => (format!("IMG_{id:04}"), 6),
            _ => (format!("IMG_{id:04}"), (id - 1) % 6 + 1),
        };
        // Photo 20 is a virtual copy of photo 3 and shares its file.
        if id == 20 {
            add_photo(&c, id, &base, folder, ratings[19], Some(3), 3)?;
        } else {
            file_id += 1;
            add_photo(
                &c,
                id,
                &base,
                folder,
                ratings[(id - 1) as usize],
                None,
                file_id,
            )?;
        }
    }
    add_keyword(&c, 1, None, 0, "/1")?;
    let mut genealogy = std::collections::HashMap::new();
    genealogy.insert(1, "/1".to_string());
    for (id, name, parent) in KEYWORDS {
        let g = format!("{}/{id}", genealogy[&parent]);
        add_keyword(&c, id, name, parent, &g)?;
        genealogy.insert(id, g);
    }
    // Keywords 11 (unused) and 12 (empty name) get no photos.
    let tags = [2, 3, 4, 5, 6, 7, 8, 9, 10, 13];
    for k in 0..30_i64 {
        let image = k % 20 + 1;
        let tag = tags[((k * 3 + k / 20) % 10) as usize];
        c.execute(
            "INSERT INTO AgLibraryKeywordImage (image, tag) VALUES (?1, ?2)",
            params![image, tag],
        )?;
    }
    add_collections(&c, &COLLECTIONS, &COLLECTION_IMAGES)?;
    c.execute_batch("COMMIT")?;
    Ok(())
}

/// Identity fixture ([`write_identity_catalog`]): the same logical content
/// stored twice with different `id_local` values, row order and root path.
pub mod identity {
    /// `(photo global id, folder below the root, base name, rating)`.
    pub const PHOTOS: [(&str, &str, &str, i64); 5] = [
        ("ph-0a1b", "", "IMG_0001", 3),
        ("ph-2c3d", "2024/", "IMG_0001", 0),
        ("ph-4e5f", "2024/Rome/", "IMG_0003", 5),
        ("ph-6a7b", "2024/Rome/", "IMG_0004", 1),
        ("ph-8c9d", "", "IMG_0005", 2),
    ];
    /// `(keyword global id, name, parent index into this array or None)`.
    /// A parent always comes before its children.
    pub const KEYWORDS: [(&str, &str, Option<usize>); 4] = [
        ("kw-0a1b", "Travel", None),
        ("kw-2c3d", "Italy", Some(0)),
        ("kw-4e5f", "Rome", Some(1)),
        ("kw-6a7b", "Sea", None),
    ];
    /// `(photo index, keyword index)` assignments.
    pub const ASSIGNMENTS: [(usize, usize); 6] = [(0, 0), (0, 3), (1, 2), (3, 1), (3, 3), (4, 3)];
    /// Root folder path of the two variants (index 0 and 1).
    pub const ROOTS: [&str; 2] = ["/Users/example/Pictures/", "/Users/example/Archive/2026/"];
}

/// Writes the identity fixture, variant 0 or 1. Both variants hold the same
/// photos and keywords with the same `id_global`, but variant 1 uses other
/// `id_local` values (photos 100+, keywords 500+, root keyword 400), inserts
/// the rows in reverse order and has another root folder path
/// ([`identity::ROOTS`]). No photo is missing, no virtual copies.
pub fn write_identity_catalog(path: &Path, variant: usize) -> Result<()> {
    assert!(variant < 2, "variant must be 0 or 1");
    let c = create(path)?;
    c.execute_batch("BEGIN")?;
    add_variables(&c)?;
    let root_kw = if variant == 0 { 1 } else { 400 };
    c.execute(
        "UPDATE Adobe_variablesTable SET value = ?1 WHERE name = 'AgLibraryKeyword_rootTagID'",
        [root_kw.to_string()],
    )?;
    add_root_folder(&c, 1, identity::ROOTS[variant], "Pictures")?;
    let folders = ["", "2024/", "2024/Rome/"];
    let order = |n: usize| -> Vec<usize> {
        if variant == 0 {
            (0..n).collect()
        } else {
            (0..n).rev().collect()
        }
    };
    let folder_id = |i: usize| -> i64 {
        if variant == 0 {
            i as i64 + 1
        } else {
            70 - i as i64
        }
    };
    for i in order(folders.len()) {
        add_folder(&c, folder_id(i), None, folders[i], 1)?;
    }
    let photo_id = |i: usize| -> i64 {
        if variant == 0 {
            i as i64 + 1
        } else {
            100 + 7 * (4 - i as i64)
        }
    };
    for i in order(identity::PHOTOS.len()) {
        let (global, folder, base, rating) = identity::PHOTOS[i];
        let fi = folders.iter().position(|f| *f == folder).unwrap();
        let id = photo_id(i);
        add_photo(&c, id, base, folder_id(fi), Some(rating), None, id + 1000)?;
        c.execute(
            "UPDATE Adobe_images SET id_global = ?2 WHERE id_local = ?1",
            params![id, global],
        )?;
    }
    add_keyword(&c, root_kw, None, 0, &format!("/{root_kw}"))?;
    let kw_id = |i: usize| -> i64 {
        if variant == 0 {
            i as i64 + 2
        } else {
            500 + 3 * (3 - i as i64)
        }
    };
    let mut genealogy: Vec<String> = vec![String::new(); identity::KEYWORDS.len()];
    for (i, (_, _, parent)) in identity::KEYWORDS.iter().enumerate() {
        let base = match parent {
            Some(p) => genealogy[*p].clone(),
            None => format!("/{root_kw}"),
        };
        genealogy[i] = format!("{base}/{}", kw_id(i));
    }
    for i in order(identity::KEYWORDS.len()) {
        let (global, name, parent) = identity::KEYWORDS[i];
        let parent_id = parent.map_or(root_kw, kw_id);
        add_keyword(&c, kw_id(i), Some(name), parent_id, &genealogy[i])?;
        c.execute(
            "UPDATE AgLibraryKeyword SET id_global = ?2 WHERE id_local = ?1",
            params![kw_id(i), global],
        )?;
    }
    let mut assignments = identity::ASSIGNMENTS.to_vec();
    if variant == 1 {
        assignments.reverse();
    }
    for (p, k) in assignments {
        c.execute(
            "INSERT INTO AgLibraryKeywordImage (image, tag) VALUES (?1, ?2)",
            params![photo_id(p), kw_id(k)],
        )?;
    }
    c.execute_batch("COMMIT")?;
    Ok(())
}

/// Sets `Adobe_images.id_global` of photo `id` (e.g. to `""` or `"  "`).
/// The column is `NOT NULL` in this schema, so NULL cannot be generated.
pub fn set_photo_global_id(path: &Path, id: i64, global: &str) -> Result<()> {
    let c = Connection::open(path)?;
    c.execute(
        "UPDATE Adobe_images SET id_global = ?2 WHERE id_local = ?1",
        params![id, global],
    )?;
    Ok(())
}

/// Sets `Adobe_images.rating` of photo `id`, as if the user had changed the
/// stars in Lightroom. `None` writes SQL NULL, which the reader treats like
/// zero stars.
pub fn set_photo_rating(path: &Path, id: i64, rating: Option<i64>) -> Result<()> {
    let c = Connection::open(path)?;
    c.execute(
        "UPDATE Adobe_images SET rating = ?2 WHERE id_local = ?1",
        params![id, rating],
    )?;
    Ok(())
}

/// Sets `AgLibraryKeyword.id_global` of keyword `id` (NOT NULL, see above).
pub fn set_keyword_global_id(path: &Path, id: i64, global: &str) -> Result<()> {
    let c = Connection::open(path)?;
    c.execute(
        "UPDATE AgLibraryKeyword SET id_global = ?2 WHERE id_local = ?1",
        params![id, global],
    )?;
    Ok(())
}

/// Removes photo `id` from the catalog, as if the user had deleted it in
/// Lightroom: its image row and every row that points at it go. The file row
/// stays; the reader only looks at files through `Adobe_images`, so an
/// orphaned file row is invisible.
pub fn remove_photo(path: &Path, id: i64) -> Result<()> {
    let c = Connection::open(path)?;
    c.execute_batch("BEGIN")?;
    for sql in [
        "DELETE FROM AgLibraryKeywordImage WHERE image = ?1",
        "DELETE FROM AgLibraryCollectionImage WHERE image = ?1",
        "DELETE FROM Adobe_AdditionalMetadata WHERE image = ?1",
        "DELETE FROM Adobe_imageProperties WHERE image = ?1",
        "DELETE FROM Adobe_images WHERE id_local = ?1",
    ] {
        c.execute(sql, params![id])?;
    }
    c.execute_batch("COMMIT")?;
    Ok(())
}

/// Renames keyword `id` in place, keeping its `id_global`, as if the user had
/// renamed it in Lightroom.
pub fn rename_keyword(path: &Path, id: i64, name: &str) -> Result<()> {
    let c = Connection::open(path)?;
    let changed = c.execute(
        "UPDATE AgLibraryKeyword SET name = ?2, lc_name = ?3 WHERE id_local = ?1",
        params![id, name, name.to_lowercase()],
    )?;
    assert_eq!(changed, 1, "no keyword {id} to rename");
    Ok(())
}

/// Moves the original file of photo `id` into another folder of the same
/// catalog (the file name stays), as if the user had moved it in Lightroom.
/// `id_global` is unchanged. The caller creates the file at its new place
/// when the reader must see it.
pub fn move_photo_file(path: &Path, id: i64, folder: i64) -> Result<()> {
    let c = Connection::open(path)?;
    let changed = c.execute(
        "UPDATE AgLibraryFile SET folder = ?2 \
         WHERE id_local = (SELECT rootFile FROM Adobe_images WHERE id_local = ?1)",
        params![id, folder],
    )?;
    assert_eq!(changed, 1, "no file row for photo {id} to move");
    Ok(())
}

/// The absolute path of photo `id`'s original as the catalog resolves it.
pub fn photo_file_path(path: &Path, id: i64) -> Result<PathBuf> {
    let c = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let (root, rel, name): (String, String, String) = c.query_row(
        "SELECT r.absolutePath, d.pathFromRoot, f.idx_filename \
         FROM Adobe_images i \
         JOIN AgLibraryFile f ON f.id_local = i.rootFile \
         JOIN AgLibraryFolder d ON d.id_local = f.folder \
         JOIN AgLibraryRootFolder r ON r.id_local = d.rootFolder \
         WHERE i.id_local = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let mut p = PathBuf::from(root);
    for part in rel.split('/').filter(|x| !x.is_empty()) {
        p.push(part);
    }
    p.push(name);
    Ok(p)
}

/// Writes a large catalog with exactly `images` photos for load tests.
pub fn write_large_catalog(path: &Path, images: usize) -> Result<()> {
    let c = create(path)?;
    c.execute_batch("BEGIN")?;
    add_variables(&c)?;
    add_root_folder(&c, 1, "/Users/example/Pictures/", "Pictures")?;
    let folder_count = 100_i64;
    for f in 1..=folder_count {
        add_folder(&c, f, None, &format!("folder{f:03}/"), 1)?;
    }
    add_keyword(&c, 1, None, 0, "/1")?;
    let keyword_count = 200_i64;
    for k in 2..=keyword_count + 1 {
        let parent = if k > 2 && k % 3 == 0 { k - 1 } else { 1 };
        add_keyword(&c, k, Some(&format!("kw{k}")), parent, &format!("/1/{k}"))?;
    }
    let mut state: u64 = 1;
    for id in 1..=images as i64 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let rating = match (state >> 33) % 7 {
            6 => None,
            r => Some(r as i64),
        };
        let folder = id % folder_count + 1;
        add_photo(&c, id, &format!("IMG_{id:06}"), folder, rating, None, id)?;
        let tag = ((state >> 40) as i64) % keyword_count + 2;
        c.execute(
            "INSERT INTO AgLibraryKeywordImage (image, tag) VALUES (?1, ?2)",
            params![id, tag],
        )?;
    }
    let large_images: Vec<(i64, i64)> = (1..=images.min(50) as i64).map(|i| (2, i)).collect();
    add_collections(
        &c,
        &[
            (1, "com.adobe.ag.library.group", "Set Large", None, 0),
            (
                2,
                "com.adobe.ag.library.collection",
                "Coll Large",
                Some(1),
                0,
            ),
            (
                3,
                "com.adobe.ag.library.smart_collection",
                "Smart Large",
                None,
                0,
            ),
            (
                4,
                "com.adobe.ag.library.quick_collection",
                "Sys Large",
                None,
                1,
            ),
        ],
        &large_images,
    )?;
    c.execute_batch("COMMIT")?;
    Ok(())
}

/// Proportions of the performance catalogs written by
/// [`write_performance_catalog`] and
/// [`write_performance_catalog_with_files`].
///
/// The proportions are fixed constants, so every test that asks for the same
/// photo count gets exactly the same catalog shape.
pub mod performance {
    /// One folder per this many photos (at least one folder).
    pub const PHOTOS_PER_FOLDER: usize = 20;
    /// One keyword per this many photos (at least one keyword).
    pub const PHOTOS_PER_KEYWORD: usize = 10;
    /// One collection per this many photos (at least one collection).
    pub const PHOTOS_PER_COLLECTION: usize = 500;
    /// Upper bounds that keep the largest catalog bounded. They only take
    /// effect above 200 000, 500 000 and 500 000 photos.
    pub const MAX_FOLDERS: usize = 10_000;
    pub const MAX_KEYWORDS: usize = 50_000;
    pub const MAX_COLLECTIONS: usize = 1_000;
    /// Every tenth photo is put into a plain album. With several albums the
    /// photos are spread over them round-robin.
    pub const MEMBER_EVERY: usize = 10;
    /// File name of the catalog written into a directory.
    pub const CATALOG_FILE: &str = "performance.lrcat";
    /// Sub-directory below `dir` that holds the placeholder originals.
    pub const PICTURES_DIR: &str = "pictures";
}

/// Shape of a generated performance catalog.
///
/// `photos` originals in `folders` folders, `keywords` visible keyword rows
/// (plus the hidden root row) and `collections` collection rows. The first
/// collection is a set, the second an album inside it, the third an
/// unreadable smart collection and the fourth the Quick Collection; every
/// further collection is a top-level album.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogShape {
    pub photos: usize,
    pub folders: usize,
    pub keywords: usize,
    pub collections: usize,
}

impl CatalogShape {
    /// One folder per [`performance::PHOTOS_PER_FOLDER`] photos, one keyword
    /// per [`performance::PHOTOS_PER_KEYWORD`] photos and one collection per
    /// [`performance::PHOTOS_PER_COLLECTION`] photos, each at least one and
    /// capped by the `MAX_*` constants.
    pub fn proportional(photos: usize) -> Self {
        let photos = photos.max(1);
        CatalogShape {
            photos,
            folders: (photos / performance::PHOTOS_PER_FOLDER).clamp(1, performance::MAX_FOLDERS),
            keywords: (photos / performance::PHOTOS_PER_KEYWORD)
                .clamp(1, performance::MAX_KEYWORDS),
            collections: (photos / performance::PHOTOS_PER_COLLECTION)
                .clamp(1, performance::MAX_COLLECTIONS),
        }
    }
}

/// Writes a performance catalog of the given shape with the neutral macOS
/// path `/Users/example/Pictures/` as its root folder (no originals on disk).
///
/// Output is deterministic: the same shape gives a byte-identical catalog.
/// Use [`write_performance_catalog_with_files`] when a dry run should convert
/// the photos instead of skipping them as missing.
pub fn write_performance_catalog(path: &Path, shape: CatalogShape) -> Result<()> {
    write_performance_catalog_at(path, shape, "/Users/example/Pictures/")
}

/// Writes a performance catalog as `dir/performance.lrcat` and returns that
/// path. The root folder points at `dir/pictures/`, and every original gets a
/// tiny placeholder file (not a real image), so a dry run converts all photos.
///
/// The catalog bytes embed the absolute `dir` path and are therefore only
/// identical for the same directory; the row contents are deterministic.
pub fn write_performance_catalog_with_files(dir: &Path, shape: CatalogShape) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).map_err(io_err)?;
    let root = root_below(dir, performance::PICTURES_DIR);
    let catalog = dir.join(performance::CATALOG_FILE);
    write_performance_catalog_at(&catalog, shape, &root)?;
    create_all_original_files(&catalog)?;
    Ok(catalog)
}

/// [`write_performance_catalog_with_files`] with the proportional shape of
/// [`CatalogShape::proportional`]: the convenience entry point for the
/// performance tests, which only care about the photo count.
pub fn write_proportional_catalog_with_files(dir: &Path, photos: usize) -> Result<PathBuf> {
    write_performance_catalog_with_files(dir, CatalogShape::proportional(photos))
}

fn write_performance_catalog_at(path: &Path, shape: CatalogShape, root: &str) -> Result<()> {
    let c = create(path)?;
    c.execute_batch("BEGIN")?;
    add_variables(&c)?;
    add_root_folder(&c, 1, root, "Pictures")?;
    let folders = shape.folders.max(1) as i64;
    for f in 1..=folders {
        add_folder(&c, f, None, &format!("folder{f:05}/"), 1)?;
    }
    add_keyword(&c, 1, None, 0, "/1")?;
    let keywords = shape.keywords.max(1) as i64;
    for k in 2..=keywords + 1 {
        let parent = if k > 2 && k % 3 == 0 { k - 1 } else { 1 };
        add_keyword(&c, k, Some(&format!("kw{k}")), parent, &format!("/1/{k}"))?;
    }
    // Fixed linear congruential generator (the same constants as
    // `write_large_catalog`): ratings and keyword assignments are repeatable.
    let mut state: u64 = 1;
    for id in 1..=shape.photos as i64 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let rating = match (state >> 33) % 7 {
            6 => None,
            r => Some(r as i64),
        };
        let folder = id % folders + 1;
        add_photo(&c, id, &format!("IMG_{id:07}"), folder, rating, None, id)?;
        let tag = ((state >> 40) as i64) % keywords + 2;
        c.execute(
            "INSERT INTO AgLibraryKeywordImage (image, tag) VALUES (?1, ?2)",
            params![id, tag],
        )?;
    }
    add_performance_collections(&c, shape)?;
    c.execute_batch("COMMIT")?;
    Ok(())
}

/// Adds the collection rows and members of a performance catalog.
fn add_performance_collections(c: &Connection, shape: CatalogShape) -> Result<()> {
    const GROUP: &str = "com.adobe.ag.library.group";
    const ALBUM: &str = "com.adobe.ag.library.collection";
    const SMART: &str = "com.adobe.ag.library.smart_collection";
    const QUICK: &str = "com.adobe.ag.library.quick_collection";

    let total = shape.collections.max(1) as i64;
    // Plain albums are id 2 (inside the set) and every id from 5 on.
    let plain: Vec<i64> = (1..=total).filter(|id| *id == 2 || *id >= 5).collect();
    let members: Vec<(i64, i64)> = if plain.is_empty() {
        Vec::new()
    } else {
        (1..=shape.photos as i64)
            .step_by(performance::MEMBER_EVERY)
            .map(|photo| {
                let slot = (photo as usize / performance::MEMBER_EVERY) % plain.len();
                (plain[slot], photo)
            })
            .collect()
    };
    let mut image_count = vec![0_i64; total as usize + 1];
    for &(collection, _) in &members {
        image_count[collection as usize] += 1;
    }
    for id in 1..=total {
        let (creation, name, parent, system) = match id {
            1 => (GROUP, "Set Performance".to_string(), None, 0),
            2 => (ALBUM, "Coll Performance".to_string(), Some(1), 0),
            3 => (SMART, "Smart Performance".to_string(), None, 0),
            4 => (QUICK, "Sys Performance".to_string(), None, 1),
            _ => (ALBUM, format!("Coll Performance {id}"), None, 0),
        };
        add_collection_row(
            c,
            id,
            creation,
            &name,
            parent,
            system,
            image_count[id as usize],
        )?;
    }
    add_collection_images(c, &members)?;
    Ok(())
}

/// Counts and ids of the title fixture ([`write_title_catalog_with_files`]).
///
/// The small fixture with `Adobe_AdditionalMetadata.xmp` blobs: 4 header bytes
/// (big-endian length of the XMP text) followed by the zlib-compressed XMP
/// packet. The CLI converts photos 1 and 4..=19 (17 photos); photos 2 and 3
/// (missing original) and 20 (virtual copy) are skipped. Photos 2 and 20
/// carry blobs (2 a valid title, 20 a broken blob) that must be counted
/// nowhere. The rows of the small fixture for the listed photos are replaced.
pub mod title_edge {
    pub const CONVERTED: i64 = 17;
    pub const SKIPPED: i64 = 3;
    /// Converted photos with a title: 4, 5, 6, 7, 14, 17, 18.
    pub const TITLES: i64 = 7;
    pub const TITLED_IDS: [i64; 7] = [4, 5, 6, 7, 14, 17, 18];
    /// Converted photos with a broken blob: 11, 12, 13, 16.
    pub const UNREADABLE: i64 = 4;
    pub const UNREADABLE_IDS: [i64; 4] = [11, 12, 13, 16];
    /// Photo 4: `x-default` (umlaut, emoji) listed after a `de` entry.
    pub const UMLAUT_ID: i64 = 4;
    pub const UMLAUT_TITLE: &str = "Sonnenaufgang \u{fc}ber K\u{f6}ln \u{1f305}";
    /// Photo 5: no `x-default`, `de-DE` first, `en-US` second: first wins.
    pub const OTHER_LANG_ID: i64 = 5;
    pub const OTHER_LANG_TITLE: &str = "Nur Deutsch";
    /// Photo 6: three entries without `x-default` and without `xml:lang`
    /// on the first one: the first `rdf:li` wins.
    pub const MULTI_ID: i64 = 6;
    pub const MULTI_TITLE: &str = "First entry";
    /// Photo 7: XML entities, named and numeric.
    pub const ENTITY_ID: i64 = 7;
    pub const ENTITY_TITLE: &str = "Tom & Jerry <b> \"q\" 's' AB\u{1f305}";
    /// Photo 8: `x-default` entry is empty. Photo 9: only spaces: none.
    pub const EMPTY_ID: i64 = 8;
    pub const BLANK_ID: i64 = 9;
    /// Photo 10: valid XMP without `dc:title`: none, not unreadable.
    pub const NO_TITLE_ID: i64 = 10;
    /// Photo 11: blob shorter than the 4 header bytes. Photo 12: header
    /// followed by bytes that are not zlib. Photo 13: valid zlib whose content
    /// is not UTF-8. All three: no title, photo kept, counted unreadable.
    pub const SHORT_ID: i64 = 11;
    pub const NOT_ZLIB_ID: i64 = 12;
    pub const NOT_UTF8_ID: i64 = 13;
    /// Photo 14: rows 101 ("second", inserted first) and 100 ("first").
    pub const DUPLICATE_ID: i64 = 14;
    pub const DUPLICATE_TITLE: &str = "first";
    /// Photo 15: rows 200 (`xmp` NULL) and 201 (valid title): none.
    pub const NULL_FIRST_ID: i64 = 15;
    /// Photo 16: rows 300 (broken blob) and 301 (valid title): none, counted
    /// once as unreadable.
    pub const BROKEN_FIRST_ID: i64 = 16;
    /// Photo 17: plain `x-default` title. Photo 18: the same plus a
    /// `dc:description` and `dc:subject` that must not be mistaken for it.
    pub const PLAIN_ID: i64 = 17;
    pub const PLAIN_TITLE: &str = "Plain title";
    pub const WITH_DESCRIPTION_ID: i64 = 18;
    pub const WITH_DESCRIPTION_TITLE: &str = "Real title";
    /// Photos with `xmp` NULL (19) or no row at all (1).
    pub const NULL_XMP_ID: i64 = 19;
    pub const NO_ROW_ID: i64 = 1;
    /// Skipped photos: 2 (valid title), 20 (broken blob).
    pub const SKIPPED_IDS: [i64; 2] = [2, 20];
    /// Every non-empty text of the fixture; none may appear in any output.
    pub const ALL_TEXTS: [&str; 15] = [
        "Sonnenaufgang",
        "Nur Deutsch",
        "Only English",
        "First entry",
        "Second entry",
        "Jerry",
        "first",
        "second",
        "later",
        "Plain title",
        "Real title",
        "desc text",
        "skipped-missing-title",
        "Third entry",
        "keyword-like",
    ];
    /// Variants of [`super::write_title_catalog_with_files_variant`].
    #[derive(Clone, Copy, Debug)]
    pub enum Variant {
        Full,
        /// `Adobe_AdditionalMetadata` dropped.
        NoTable,
        /// Column `xmp` dropped.
        NoColumn,
    }
}

/// A title blob: 4 header bytes (big-endian length) plus zlib data.
fn xmp_blob(xmp: &[u8]) -> Vec<u8> {
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(xmp).expect("in-memory write");
    let mut blob = (xmp.len() as u32).to_be_bytes().to_vec();
    blob.extend(enc.finish().expect("in-memory finish"));
    blob
}

/// XMP packet with `inner` inside `rdf:Description`.
fn xmp_packet(inner: &str) -> String {
    format!(
        "<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         <rdf:Description xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmp:Rating=\"3\">\n\
         {inner}\n</rdf:Description></rdf:RDF></x:xmpmeta>\n<?xpacket end=\"w\"?>"
    )
}

fn alt(tag: &str, items: &[(Option<&str>, &str)]) -> String {
    let mut s = format!("<{tag}><rdf:Alt>");
    for (lang, text) in items {
        match lang {
            Some(l) => s.push_str(&format!("<rdf:li xml:lang=\"{l}\">{text}</rdf:li>")),
            None => s.push_str(&format!("<rdf:li>{text}</rdf:li>")),
        }
    }
    s.push_str(&format!("</rdf:Alt></{tag}>"));
    s
}

fn title_blob(items: &[(Option<&str>, &str)]) -> Vec<u8> {
    xmp_blob(xmp_packet(&alt("dc:title", items)).as_bytes())
}

/// Writes the small fixture with the XMP blobs of [`title_edge`] as
/// `dir/titles.lrcat` with placeholder originals. Returns the catalog path.
pub fn write_title_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    write_title_catalog_with_files_variant(dir, title_edge::Variant::Full)
}

/// Like [`write_title_catalog_with_files`] with the table or the `xmp`
/// column of `Adobe_AdditionalMetadata` removed for the other variants.
pub fn write_title_catalog_with_files_variant(
    dir: &Path,
    variant: title_edge::Variant,
) -> Result<PathBuf> {
    let (mac, win) = prepare_root_dirs(dir)?;
    let catalog = dir.join("titles.lrcat");
    write_small(&catalog, &mac, &win)?;
    add_title_blobs(&catalog)?;
    match variant {
        title_edge::Variant::Full => {}
        title_edge::Variant::NoTable => {
            Connection::open(&catalog)?.execute_batch("DROP TABLE Adobe_AdditionalMetadata;")?;
        }
        title_edge::Variant::NoColumn => {
            Connection::open(&catalog)?
                .execute_batch("ALTER TABLE Adobe_AdditionalMetadata DROP COLUMN xmp;")?;
        }
    }
    create_original_files(&catalog)?;
    Ok(catalog)
}

fn add_title_blobs(path: &Path) -> Result<()> {
    use rusqlite::types::Value as V;
    use title_edge as e;
    let c = Connection::open(path)?;
    c.execute_batch("BEGIN")?;
    let x = Some("x-default");
    let blob = |b: Vec<u8>| V::Blob(b);
    // Replace the small fixture's row of every photo handled here.
    c.execute_batch("DELETE FROM Adobe_AdditionalMetadata WHERE image BETWEEN 1 AND 20")?;
    let put = |row: i64, image: i64, v: V| {
        c.execute(
            "INSERT INTO Adobe_AdditionalMetadata (id_local, id_global, image, xmp, embeddedXmp, externalXmpIsDirty) VALUES (?1, ?2, ?3, ?4, 0, 0)",
            params![row, format!("TITLE-META-{row}"), image, v],
        )
    };
    for id in 3..=20 {
        // Defaults: NULL xmp (photo 3 is skipped anyway).
        if id != e::NO_ROW_ID {
            put(1000 + id, id, V::Null)?;
        }
    }
    let replace = |row: i64, image: i64, v: V| -> Result<()> {
        c.execute(
            "UPDATE Adobe_AdditionalMetadata SET xmp = ?1 WHERE id_local = ?2 AND image = ?3",
            params![v, row, image],
        )?;
        Ok(())
    };
    let own = |image: i64, v: V| replace(1000 + image, image, v);
    own(
        e::UMLAUT_ID,
        blob(title_blob(&[
            (Some("de"), "Deutsch"),
            (x, "Sonnenaufgang \u{fc}ber K\u{f6}ln \u{1f305}"),
        ])),
    )?;
    own(
        e::OTHER_LANG_ID,
        blob(title_blob(&[
            (Some("de-DE"), "Nur Deutsch"),
            (Some("en-US"), "Only English"),
        ])),
    )?;
    own(
        e::MULTI_ID,
        blob(title_blob(&[
            (None, "First entry"),
            (Some("fr"), "Second entry"),
            (Some("it"), "Third entry"),
        ])),
    )?;
    own(
        e::ENTITY_ID,
        blob(title_blob(&[(
            x,
            "Tom &amp; Jerry &lt;b&gt; &quot;q&quot; &apos;s&apos; &#65;&#x42;&#x1F305;",
        )])),
    )?;
    own(e::EMPTY_ID, blob(title_blob(&[(x, "")])))?;
    own(e::BLANK_ID, blob(title_blob(&[(x, "   \n ")])))?;
    own(
        e::NO_TITLE_ID,
        blob(xmp_blob(
            xmp_packet(&alt("dc:description", &[(x, "desc text")])).as_bytes(),
        )),
    )?;
    own(e::SHORT_ID, V::Blob(vec![0, 0, 1]))?;
    let mut not_zlib = vec![0, 0, 0, 40];
    not_zlib.extend_from_slice(b"this is not zlib data at all, sorry");
    own(e::NOT_ZLIB_ID, V::Blob(not_zlib))?;
    own(
        e::NOT_UTF8_ID,
        blob(xmp_blob(&[b'<', 0xff, 0xfe, 0xc0, 0x80, b'>'])),
    )?;
    // Duplicates: replace the default row by two explicit ones.
    let dup = |image: i64, rows: [(i64, V); 2]| -> Result<()> {
        c.execute(
            "DELETE FROM Adobe_AdditionalMetadata WHERE image = ?1",
            [image],
        )?;
        for (row, v) in rows {
            put(row, image, v)?;
        }
        Ok(())
    };
    dup(
        e::DUPLICATE_ID,
        [
            (101, blob(title_blob(&[(x, "second")]))),
            (100, blob(title_blob(&[(x, e::DUPLICATE_TITLE)]))),
        ],
    )?;
    dup(
        e::NULL_FIRST_ID,
        [(200, V::Null), (201, blob(title_blob(&[(x, "later")])))],
    )?;
    dup(
        e::BROKEN_FIRST_ID,
        [
            (300, V::Blob(vec![1, 2])),
            (301, blob(title_blob(&[(x, "later")]))),
        ],
    )?;
    own(e::PLAIN_ID, blob(title_blob(&[(x, e::PLAIN_TITLE)])))?;
    let inner = format!(
        "{}\n{}\n<dc:subject><rdf:Bag><rdf:li>keyword-like</rdf:li></rdf:Bag></dc:subject>",
        alt("dc:description", &[(x, "desc text")]),
        alt("dc:title", &[(x, e::WITH_DESCRIPTION_TITLE)]),
    );
    own(
        e::WITH_DESCRIPTION_ID,
        blob(xmp_blob(xmp_packet(&inner).as_bytes())),
    )?;
    // Skipped photos: 2 gets a valid title, 20 a broken blob.
    put(1002, 2, blob(title_blob(&[(x, "skipped-missing-title")])))?;
    own(20, V::Blob(vec![9]))?;
    c.execute_batch("COMMIT")?;
    Ok(())
}

/// The full feature catalog: the small fixture with every supported edge
/// applied, so a single catalog exercises folders, photos (incl. videos,
/// missing files and a virtual copy), keywords with hierarchy/synonyms and
/// attributes, ratings, flags, color labels (label set `En`), collections and
/// collection sets, smart collections (translatable and not), stacks, the quick
/// collection, titles, captions, copyright and creator.
///
/// Composition order matters: the keyword, flag and label adders rebuild
/// `Adobe_images` but copy every row with `SELECT *`, so earlier edges survive.
/// The combined IPTC adder ([`add_iptc_combined`]) replaces the separate
/// caption/rights adders because the reader reads caption and copyright from
/// the same `AgLibraryIPTC` row. `add_video_edges` runs before stacks (stacks
/// need the video rows). `add_collection_edges` drops and recreates the
/// collection tables, so the quick-collection adjustment (collection 14 is the
/// quick collection, 15 is removed) runs after it. `add_smart_edges` creates
/// `AgLibraryCollectionContent` and inserts smart collections 101.. plus a
/// content row for the plain collection 4. Titles are written last. Placeholder
/// originals are created for every file except the missing ones.
///
/// Returns `dir/full.lrcat`.
pub fn write_full_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let (mac, win) = prepare_root_dirs(dir)?;
    let catalog = dir.join("full.lrcat");
    write_small(&catalog, &mac, &win)?;
    add_keyword_edges(&catalog)?;
    add_flag_edges(&catalog)?;
    add_label_edges(&catalog, label_edge::Fixture::En)?;
    add_iptc_combined(&catalog)?;
    add_video_edges(&catalog)?;
    add_stack_edges(&catalog)?;
    add_collection_edges(&catalog)?;
    // The quick collection is collection 14; collection 15 is removed. This
    // mirrors write_quick_catalog_with_files but runs in the full catalog.
    {
        let c = Connection::open(&catalog)?;
        c.execute_batch(
            "DELETE FROM AgLibraryCollectionImage WHERE collection IN (14, 15); \
             DELETE FROM AgLibraryCollection WHERE id_local = 15;",
        )?;
        for (photo, pos) in quick_edge::ROWS {
            c.execute(
                "INSERT INTO AgLibraryCollectionImage (collection, image, pick, positionInCollection) VALUES (14, ?1, 0, ?2)",
                params![photo, pos],
            )?;
        }
    }
    add_smart_edges(&catalog)?;
    add_title_blobs(&catalog)?;
    create_original_files(&catalog)?;
    Ok(catalog)
}
