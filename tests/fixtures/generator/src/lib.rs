//! Generator for synthetic Lightroom Classic 15 test catalogs.
//!
//! Everything written here is invented. Paths use the user name `example`.
//! Output is deterministic.

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
    /// Assignment ids that cannot be converted: 20 (virtual copy photo 20),
    /// 31 (pipe keyword), 32 (empty keyword), 33 (keyword under empty one),
    /// 34 (photo 999 does not exist), 35 (virtual copy photo 20),
    /// 36 (empty keyword 12).
    pub const ASSIGNMENTS_SKIPPED: [i64; 7] = [20, 31, 32, 33, 34, 35, 36];
    pub const ASSIGNMENTS_CONVERTED: i64 = 30;
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
    /// Virtual copies: 20 (of photo 3) and 21 (of video 4). They keep the
    /// virtual copy reason and are not counted as videos.
    pub const VIRTUAL_COPY_IDS: [i64; 2] = [20, 21];
    /// Photos with a missing original that are not videos: only photo 3.
    pub const MISSING_ORIGINAL_IDS: [i64; 1] = [3];
    pub const CONVERTED: i64 = 15;
    /// Skipped photos: 3 videos + 1 missing + 2 virtual copies.
    pub const SKIPPED: i64 = 6;
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
        let genealogy = match parent {
            Some(p) => format!("/{p}/{id}"),
            None => format!("/{id}"),
        };
        let count = images.iter().filter(|(c, _)| *c == id).count() as i64;
        c.execute(
            "INSERT INTO AgLibraryCollection (id_local, creationId, genealogy, imageCount, name, parent, systemOnly) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, creation, genealogy, count, name, parent, system],
        )?;
    }
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

/// Creates a placeholder file for every original except the missing ones.
fn create_original_files(catalog: &Path) -> Result<()> {
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
        if !small::MISSING_FILE_IDS.contains(&id) {
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

/// Sets `AgLibraryKeyword.id_global` of keyword `id` (NOT NULL, see above).
pub fn set_keyword_global_id(path: &Path, id: i64, global: &str) -> Result<()> {
    let c = Connection::open(path)?;
    c.execute(
        "UPDATE AgLibraryKeyword SET id_global = ?2 WHERE id_local = ?1",
        params![id, global],
    )?;
    Ok(())
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
