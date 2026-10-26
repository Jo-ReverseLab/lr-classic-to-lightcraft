//! Generator for synthetic Lightroom Classic 15 test catalogs.
//!
//! Everything written here is invented. Paths use the user name `example`.
//! Output is deterministic.

use rusqlite::{Connection, Result, params};
use std::path::Path;

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
}

const SCHEMA: &str = "
CREATE TABLE Adobe_variablesTable (id_local INTEGER PRIMARY KEY, id_global NOT NULL, name, type, value NOT NULL);
CREATE TABLE AgLibraryRootFolder (id_local INTEGER PRIMARY KEY, id_global NOT NULL, absolutePath NOT NULL, name NOT NULL, relativePathFromCatalog);
CREATE TABLE AgLibraryFolder (id_local INTEGER PRIMARY KEY, id_global NOT NULL, parentId INTEGER, pathFromRoot NOT NULL, rootFolder INTEGER NOT NULL, visibility INTEGER);
CREATE TABLE AgLibraryFile (id_local INTEGER PRIMARY KEY, id_global NOT NULL, baseName NOT NULL, extension NOT NULL, folder INTEGER NOT NULL, idx_filename NOT NULL, lc_idx_filename NOT NULL, lc_idx_filenameExtension NOT NULL, originalFilename NOT NULL, errorMessage, errorTime, externalModTime, importHash, md5, modTime, sidecarExtensions);
CREATE TABLE Adobe_images (id_local INTEGER PRIMARY KEY, id_global NOT NULL, aspectRatioCache NOT NULL, bitDepth NOT NULL, colorChannels NOT NULL, colorLabels NOT NULL, colorMode NOT NULL, copyCreationTime NOT NULL, captureTime, copyName, copyReason, fileHeight, fileWidth, orientation, rating, editLock INTEGER NOT NULL, fileFormat NOT NULL, pick NOT NULL, positionInFolder NOT NULL, touchCount NOT NULL, touchTime NOT NULL, hasMissingSidecars INTEGER, masterImage INTEGER, originalRootEntity INTEGER, rootFile INTEGER NOT NULL);
CREATE TABLE AgLibraryKeyword (id_local INTEGER PRIMARY KEY, id_global NOT NULL, dateCreated NOT NULL, genealogy NOT NULL, includeOnExport INTEGER NOT NULL, includeParents INTEGER NOT NULL, includeSynonyms INTEGER NOT NULL, imageCountCache, keywordType, lastApplied, lc_name, name, parent INTEGER);
CREATE TABLE AgLibraryKeywordImage (id_local INTEGER PRIMARY KEY, image INTEGER NOT NULL, tag INTEGER NOT NULL);
CREATE TABLE Adobe_AdditionalMetadata (id_local INTEGER PRIMARY KEY, id_global, image INTEGER, xmp, embeddedXmp INTEGER, externalXmpIsDirty INTEGER);
CREATE TABLE Adobe_imageProperties (id_local INTEGER PRIMARY KEY, id_global, image, propertiesString);
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
    let c = create(path)?;
    c.execute_batch("BEGIN")?;
    add_variables(&c)?;
    add_root_folder(&c, 1, "/Users/example/Pictures/", "Pictures")?;
    add_root_folder(&c, 2, "C:\\Users\\example\\Pictures\\", "Pictures")?;
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
    c.execute_batch("COMMIT")?;
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
    c.execute_batch("COMMIT")?;
    Ok(())
}
