//! Deliberately damaged catalogs, for hostile-input tests.
//!
//! Each [`HostileCase`] is a small, named mutation applied on top of a normal
//! generated fixture (the small catalog with placeholder originals, or the
//! stack fixture where stack tables are needed). Everything is invented; no
//! real catalog is touched. The mutations use only SQLite `UPDATE`/`ALTER`
//! statements, so they are deterministic and need no file system access beyond
//! the catalog file itself.
//!
//! The cases are grouped by the kind of damage. A case never combines two
//! damages, so a failing test points at exactly one problem.

use rusqlite::{Connection, Result};
use std::path::Path;

/// One deliberately damaged catalog. Display name and group are exposed for
/// test reports; the name is a Windows-safe kebab-case slug.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostileCase {
    // --- NULL where a value is expected ---
    /// `Adobe_images.rating` is NULL on photo 1 (accepted as 0).
    NullPhotoRating,
    /// `Adobe_images.id_global` is NULL on photo 1 (missing global id).
    NullPhotoGlobalId,
    /// `Adobe_images.pick` is NULL on photo 1 (column rebuilt nullable).
    NullPhotoPick,
    /// `Adobe_images.colorLabels` is NULL on photo 1 (column rebuilt nullable).
    NullPhotoColorLabel,
    /// `AgLibraryKeyword.name` is NULL on keyword 2.
    NullKeywordName,
    /// `AgLibraryKeyword.id_global` is NULL on keyword 2 (column rebuilt
    /// nullable; missing global id).
    NullKeywordGlobalId,
    /// `AgLibraryRootFolder.absolutePath` is NULL on root 1 (column rebuilt
    /// nullable; root folder has no path).
    NullRootAbsolutePath,
    /// `AgLibraryCollection.name` is NULL on collection 4 (column rebuilt
    /// nullable; name invalid).
    NullCollectionName,

    // --- an id that points at a row that does not exist ---
    /// `AgLibraryFolder.rootFolder` points at a missing root folder.
    DanglingFolderRoot,
    /// `AgLibraryKeyword.parent` points at a missing keyword.
    DanglingKeywordParent,
    /// `AgLibraryCollection.parent` points at a missing collection.
    DanglingCollectionParent,
    /// `AgLibraryCollectionImage.image` points at a missing photo.
    DanglingCollectionImage,
    /// `AgLibraryFolderStackImage.image` points at a missing photo.
    DanglingStackMember,

    // --- cycles ---
    /// Two keywords whose parents point at each other.
    KeywordCycle,
    /// Two collection sets whose parents point at each other.
    CollectionCycle,

    // --- duplicates and odd names ---
    /// Two photos share the same `id_global`.
    DuplicatePhotoGlobalId,
    /// Two folder rows resolve to the same path.
    DuplicateFolderPath,
    /// A keyword name with an embedded NUL byte.
    EmbeddedNulName,
    /// A keyword name stored as a blob (a lone surrogate-like byte sequence).
    BlobKeywordName,
    /// A keyword with a very long name.
    LongKeywordName,

    // --- numbers out of range ---
    /// `Adobe_images.rating` is -1.
    RatingBelowRange,
    /// `Adobe_images.rating` is 9.
    RatingAboveRange,
    /// `Adobe_images.pick` is 2 (not -1, 0 or 1).
    BadPickValue,

    // --- structural damage ---
    /// Every table is present but empty.
    EmptyCatalog,
    /// An unknown extra table is present.
    ExtraTable,
    /// An optional table (`Adobe_AdditionalMetadata`, used for titles) is
    /// missing; titles fall back to absent.
    MissingOptionalTable,
    /// An optional column (`AgLibraryKeyword.lc_name`) is missing.
    MissingOptionalColumn,
    /// A required table (`Adobe_images`) is missing.
    MissingRequiredTable,
    /// A required column (`Adobe_images.rating`) is missing.
    MissingRequiredColumn,
}

impl HostileCase {
    /// Windows-safe slug used in file names and reports.
    pub fn slug(self) -> &'static str {
        use HostileCase::*;
        match self {
            NullPhotoRating => "null-photo-rating",
            NullPhotoGlobalId => "null-photo-global-id",
            NullPhotoPick => "null-photo-pick",
            NullPhotoColorLabel => "null-photo-color-label",
            NullKeywordName => "null-keyword-name",
            NullKeywordGlobalId => "null-keyword-global-id",
            NullRootAbsolutePath => "null-root-absolute-path",
            NullCollectionName => "null-collection-name",
            DanglingFolderRoot => "dangling-folder-root",
            DanglingKeywordParent => "dangling-keyword-parent",
            DanglingCollectionParent => "dangling-collection-parent",
            DanglingCollectionImage => "dangling-collection-image",
            DanglingStackMember => "dangling-stack-member",
            KeywordCycle => "keyword-cycle",
            CollectionCycle => "collection-cycle",
            DuplicatePhotoGlobalId => "duplicate-photo-global-id",
            DuplicateFolderPath => "duplicate-folder-path",
            EmbeddedNulName => "embedded-nul-name",
            BlobKeywordName => "blob-keyword-name",
            LongKeywordName => "long-keyword-name",
            RatingBelowRange => "rating-below-range",
            RatingAboveRange => "rating-above-range",
            BadPickValue => "bad-pick-value",
            EmptyCatalog => "empty-catalog",
            ExtraTable => "extra-table",
            MissingOptionalTable => "missing-optional-table",
            MissingOptionalColumn => "missing-optional-column",
            MissingRequiredTable => "missing-required-table",
            MissingRequiredColumn => "missing-required-column",
        }
    }

    /// The damage group, for the report table.
    pub fn group(self) -> &'static str {
        use HostileCase::*;
        match self {
            NullPhotoRating | NullPhotoGlobalId | NullPhotoPick | NullPhotoColorLabel
            | NullKeywordName | NullKeywordGlobalId | NullRootAbsolutePath | NullCollectionName => {
                "null"
            }
            DanglingFolderRoot
            | DanglingKeywordParent
            | DanglingCollectionParent
            | DanglingCollectionImage
            | DanglingStackMember => "dangling id",
            KeywordCycle | CollectionCycle => "cycle",
            DuplicatePhotoGlobalId
            | DuplicateFolderPath
            | EmbeddedNulName
            | BlobKeywordName
            | LongKeywordName => "duplicate or odd name",
            RatingBelowRange | RatingAboveRange | BadPickValue => "number out of range",
            EmptyCatalog
            | ExtraTable
            | MissingOptionalTable
            | MissingOptionalColumn
            | MissingRequiredTable
            | MissingRequiredColumn => "structural",
        }
    }

    /// All cases, in declaration order. Tests iterate over this.
    pub fn all() -> &'static [HostileCase] {
        &[
            HostileCase::NullPhotoRating,
            HostileCase::NullPhotoGlobalId,
            HostileCase::NullPhotoPick,
            HostileCase::NullPhotoColorLabel,
            HostileCase::NullKeywordName,
            HostileCase::NullKeywordGlobalId,
            HostileCase::NullRootAbsolutePath,
            HostileCase::NullCollectionName,
            HostileCase::DanglingFolderRoot,
            HostileCase::DanglingKeywordParent,
            HostileCase::DanglingCollectionParent,
            HostileCase::DanglingCollectionImage,
            HostileCase::DanglingStackMember,
            HostileCase::KeywordCycle,
            HostileCase::CollectionCycle,
            HostileCase::DuplicatePhotoGlobalId,
            HostileCase::DuplicateFolderPath,
            HostileCase::EmbeddedNulName,
            HostileCase::BlobKeywordName,
            HostileCase::LongKeywordName,
            HostileCase::RatingBelowRange,
            HostileCase::RatingAboveRange,
            HostileCase::BadPickValue,
            HostileCase::EmptyCatalog,
            HostileCase::ExtraTable,
            HostileCase::MissingOptionalTable,
            HostileCase::MissingOptionalColumn,
            HostileCase::MissingRequiredTable,
            HostileCase::MissingRequiredColumn,
        ]
    }

    /// True when the case needs the stack fixture (stack tables present).
    fn needs_stacks(self) -> bool {
        matches!(self, HostileCase::DanglingStackMember)
    }
}

/// Writes the damaged catalog for `case` as `dir/<slug>.lrcat` and returns
/// that path. Placeholder original files are created as in the small fixture
/// (or the stack fixture for stack cases).
pub fn write_hostile_catalog(dir: &Path, case: HostileCase) -> Result<std::path::PathBuf> {
    let catalog = dir.join(format!("{}.lrcat", case.slug()));
    if case.needs_stacks() {
        // The stack fixture writes its own files under a sub-directory.
        let stack_dir = dir.join("stack-base");
        std::fs::create_dir_all(&stack_dir).map_err(io_err)?;
        let base = crate::write_stack_catalog_with_files(&stack_dir)?;
        std::fs::copy(&base, &catalog).map_err(io_err)?;
    } else {
        // `write_small_catalog_with_files` writes `dir/small.lrcat` with
        // placeholder originals below `dir`; copy it as the named hostile
        // catalog, then mutate that copy.
        std::fs::create_dir_all(dir).map_err(io_err)?;
        let small = crate::write_small_catalog_with_files(dir)?;
        std::fs::copy(&small, &catalog).map_err(io_err)?;
    }
    apply_case(&catalog, case)?;
    Ok(catalog)
}

/// Applies the mutation for `case` to the catalog at `path`.
fn apply_case(path: &Path, case: HostileCase) -> Result<()> {
    use HostileCase::*;
    let c = Connection::open(path)?;
    match case {
        NullPhotoRating => exec(
            &c,
            "UPDATE Adobe_images SET rating = NULL WHERE id_local = 1",
        ),
        NullPhotoGlobalId => {
            make_nullable(&c, "Adobe_images", "id_global NOT NULL,")?;
            exec(
                &c,
                "UPDATE Adobe_images SET id_global = NULL WHERE id_local = 1",
            )
        }
        NullPhotoPick => {
            make_nullable(&c, "Adobe_images", "pick NOT NULL,")?;
            exec(&c, "UPDATE Adobe_images SET pick = NULL WHERE id_local = 1")
        }
        NullPhotoColorLabel => {
            make_nullable(&c, "Adobe_images", "colorLabels NOT NULL,")?;
            exec(
                &c,
                "UPDATE Adobe_images SET colorLabels = NULL WHERE id_local = 1",
            )
        }
        NullKeywordName => exec(
            &c,
            "UPDATE AgLibraryKeyword SET name = NULL WHERE id_local = 2",
        ),
        NullKeywordGlobalId => {
            make_nullable_keyword_global_id(&c)?;
            exec(
                &c,
                "UPDATE AgLibraryKeyword SET id_global = NULL WHERE id_local = 2",
            )
        }
        NullRootAbsolutePath => {
            make_nullable(&c, "AgLibraryRootFolder", "absolutePath NOT NULL,")?;
            exec(
                &c,
                "UPDATE AgLibraryRootFolder SET absolutePath = NULL WHERE id_local = 1",
            )
        }
        NullCollectionName => {
            make_nullable(&c, "AgLibraryCollection", "name NOT NULL,")?;
            exec(
                &c,
                "UPDATE AgLibraryCollection SET name = NULL WHERE id_local = 4",
            )
        }
        DanglingFolderRoot => exec(
            &c,
            "UPDATE AgLibraryFolder SET rootFolder = 987654 WHERE id_local = 2",
        ),
        DanglingKeywordParent => exec(
            &c,
            "UPDATE AgLibraryKeyword SET parent = 987654 WHERE id_local = 2",
        ),
        DanglingCollectionParent => exec(
            &c,
            "UPDATE AgLibraryCollection SET parent = 987654 WHERE id_local = 4",
        ),
        DanglingCollectionImage => exec(
            &c,
            "UPDATE AgLibraryCollectionImage SET image = 987654 WHERE id_local = 1",
        ),
        DanglingStackMember => exec(
            &c,
            "UPDATE AgLibraryFolderStackImage SET image = 987654 WHERE rowid = 1",
        ),
        KeywordCycle => {
            // Keyword 4 parent 5, keyword 5 parent 4 (a two-step cycle).
            exec(
                &c,
                "UPDATE AgLibraryKeyword SET parent = 5 WHERE id_local = 4",
            )?;
            exec(
                &c,
                "UPDATE AgLibraryKeyword SET parent = 4 WHERE id_local = 5",
            )
        }
        CollectionCycle => {
            // Collections 4 and 5 point at each other as parents.
            exec(
                &c,
                "UPDATE AgLibraryCollection SET parent = 5 WHERE id_local = 4",
            )?;
            exec(
                &c,
                "UPDATE AgLibraryCollection SET parent = 4 WHERE id_local = 5",
            )
        }
        DuplicatePhotoGlobalId => exec(
            &c,
            "UPDATE Adobe_images SET id_global = (SELECT id_global FROM Adobe_images WHERE id_local = 1) WHERE id_local = 4",
        ),
        DuplicateFolderPath => {
            // Folder 5 already has pathFromRoot 'Trips/'; make folder 2 use the
            // same root and relative path so the assembled paths collide.
            exec(
                &c,
                "UPDATE AgLibraryFolder SET rootFolder = 2, pathFromRoot = 'Trips/' WHERE id_local = 2",
            )
        }
        EmbeddedNulName => {
            // A keyword name containing a NUL byte (valid in SQLite text).
            c.execute(
                "UPDATE AgLibraryKeyword SET name = ?1 WHERE id_local = 2",
                rusqlite::params!["ok\0bad".to_string()],
            )?;
            Ok(())
        }
        BlobKeywordName => {
            // Store a blob (a lone surrogate-like byte) in the text column.
            c.execute(
                "UPDATE AgLibraryKeyword SET name = ?1 WHERE id_local = 2",
                rusqlite::params![rusqlite::types::Value::Blob(b"\xed\xa0\x80".to_vec())],
            )?;
            Ok(())
        }
        LongKeywordName => {
            let long = "x".repeat(4096);
            c.execute(
                "UPDATE AgLibraryKeyword SET name = ?1 WHERE id_local = 2",
                rusqlite::params![long],
            )?;
            Ok(())
        }
        RatingBelowRange => exec(&c, "UPDATE Adobe_images SET rating = -1 WHERE id_local = 1"),
        RatingAboveRange => exec(&c, "UPDATE Adobe_images SET rating = 9 WHERE id_local = 1"),
        BadPickValue => exec(&c, "UPDATE Adobe_images SET pick = 2 WHERE id_local = 1"),
        EmptyCatalog => {
            c.execute_batch(
                "DELETE FROM Adobe_images; \
                 DELETE FROM AgLibraryFile; \
                 DELETE FROM AgLibraryFolder; \
                 DELETE FROM AgLibraryRootFolder; \
                 DELETE FROM AgLibraryKeyword; \
                 DELETE FROM AgLibraryKeywordImage; \
                 DELETE FROM AgLibraryKeywordSynonym; \
                 DELETE FROM AgLibraryCollection; \
                 DELETE FROM AgLibraryCollectionImage; \
                 DELETE FROM AgLibraryIPTC;",
            )?;
            Ok(())
        }
        ExtraTable => exec(
            &c,
            "CREATE TABLE lr2lc_hostile_extra (id INTEGER, note TEXT)",
        ),
        MissingOptionalTable => exec(&c, "DROP TABLE Adobe_AdditionalMetadata"),
        MissingOptionalColumn => exec(&c, "ALTER TABLE AgLibraryKeyword DROP COLUMN lc_name"),
        MissingRequiredTable => exec(&c, "DROP TABLE Adobe_images"),
        MissingRequiredColumn => exec(&c, "ALTER TABLE Adobe_images DROP COLUMN rating"),
    }
}

fn exec(c: &Connection, sql: &str) -> Result<()> {
    c.execute_batch(sql)
}

/// Rebuilds `table` with one `NOT NULL` column made nullable, copying the
/// data. `marker` is the exact fragment to remove from the `CREATE TABLE`
/// text (for example `"pick NOT NULL,"`).
fn make_nullable(c: &Connection, table: &str, marker: &str) -> Result<()> {
    let sql: String = c.query_row(
        &format!("SELECT sql FROM sqlite_master WHERE name = '{table}'"),
        [],
        |r| r.get(0),
    )?;
    assert!(
        sql.contains(marker),
        "schema fragment {marker:?} not found in {table}"
    );
    c.execute_batch("BEGIN")?;
    c.execute_batch(&format!("ALTER TABLE {table} RENAME TO {table}_old"))?;
    c.execute_batch(&sql.replace(marker, &marker.replace(" NOT NULL", "")))?;
    c.execute_batch(&format!("INSERT INTO {table} SELECT * FROM {table}_old"))?;
    c.execute_batch(&format!("DROP TABLE {table}_old"))?;
    c.execute_batch("COMMIT")?;
    Ok(())
}

/// `AgLibraryKeyword.id_global` is `NOT NULL`; rebuild it nullable.
fn make_nullable_keyword_global_id(c: &Connection) -> Result<()> {
    // The keyword table has `id_global NOT NULL` right after `dateCreated`.
    let sql: String = c.query_row(
        "SELECT sql FROM sqlite_master WHERE name = 'AgLibraryKeyword'",
        [],
        |r| r.get(0),
    )?;
    // The first `id_global NOT NULL` belongs to the keyword table itself.
    let nullable = sql.replacen("id_global NOT NULL", "id_global", 1);
    c.execute_batch("BEGIN")?;
    c.execute_batch("ALTER TABLE AgLibraryKeyword RENAME TO AgLibraryKeyword_old")?;
    c.execute_batch(&nullable)?;
    c.execute_batch("INSERT INTO AgLibraryKeyword SELECT * FROM AgLibraryKeyword_old")?;
    c.execute_batch("DROP TABLE AgLibraryKeyword_old")?;
    c.execute_batch("COMMIT")?;
    Ok(())
}

fn io_err(e: std::io::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_unique_and_windows_safe() {
        let mut seen = std::collections::HashSet::new();
        for case in HostileCase::all() {
            let s = case.slug();
            assert!(seen.insert(s), "duplicate slug {s}");
            assert!(
                !s.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']),
                "slug {s} is not Windows-safe"
            );
        }
    }

    #[test]
    fn every_case_writes_a_catalog() {
        let dir = tempfile::tempdir().unwrap();
        for &case in HostileCase::all() {
            let path = write_hostile_catalog(dir.path(), case).unwrap();
            assert!(path.exists(), "{}: catalog not written", case.slug());
            // The file must be a usable SQLite database.
            let c = Connection::open(&path).unwrap();
            let n: i64 = c
                .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
                .unwrap();
            assert!(n > 0, "{}: no schema", case.slug());
        }
    }
}
