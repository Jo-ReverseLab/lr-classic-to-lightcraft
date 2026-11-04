//! Opening a Lightroom Classic catalog strictly read-only.

use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

/// The only `Adobe_DBVersion` this reader understands (Lightroom Classic 15).
pub const SUPPORTED_VERSION: &str = "1500000";

/// Errors while opening or reading a catalog.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// The file is missing, unreadable or not a SQLite database.
    #[error("cannot open catalog: {0}")]
    Open(#[source] rusqlite::Error),
    /// The catalog has no `Adobe_DBVersion` entry.
    #[error("catalog has no Adobe_DBVersion entry")]
    MissingVersion,
    /// The `Adobe_DBVersion` value holds bytes that are not valid UTF-8.
    #[error("catalog Adobe_DBVersion is not valid UTF-8")]
    InvalidVersion,
    /// The catalog was written by a Lightroom version this reader does not support.
    #[error("unsupported catalog version {0} (supported: {SUPPORTED_VERSION})")]
    UnsupportedVersion(String),
    /// A query on an opened catalog failed.
    #[error("cannot read catalog: {0}")]
    Query(#[source] rusqlite::Error),
    /// A non-empty `-wal` or `-journal` file sits next to the catalog, so
    /// Lightroom may still hold changes that are not in the catalog file.
    #[error(
        "Lightroom may still hold unsaved changes in {}; close Lightroom and try again",
        path.display()
    )]
    PendingChanges {
        /// The side file that was found.
        path: PathBuf,
    },
    /// A table or column that the readers query does not exist.
    #[error("{}", missing_schema_message(table, column.as_deref()))]
    MissingSchema {
        /// The table that is missing or lacks a column.
        table: String,
        /// The missing column, or `None` if the whole table is missing.
        column: Option<String>,
    },
}

/// Converts a row value like `From<ValueRef> for Value`, except that text
/// which is not valid UTF-8 becomes a blob holding the same bytes.
///
/// `rusqlite`'s own conversion calls `expect("invalid UTF-8")` on such a
/// text value and aborts the run. The readers already decide how to treat a
/// cell that is not text (a name becomes `NameInvalid`, an id counts as
/// missing), so handing them a blob lets them report the problem instead of
/// panicking.
pub(crate) fn value_or_blob(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::Integer(i),
        ValueRef::Real(r) => Value::Real(r),
        ValueRef::Text(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) => Value::Text(text.to_owned()),
            Err(_) => Value::Blob(bytes.to_vec()),
        },
        ValueRef::Blob(bytes) => Value::Blob(bytes.to_vec()),
    }
}

/// A catalog cell that the reader needs as text (a path, a file name, a
/// schema name), classified without letting `rusqlite` abort on bad bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TextCell {
    /// SQL NULL.
    Null,
    /// Text, or a blob that holds valid UTF-8.
    Text(String),
    /// Bytes that are not valid UTF-8.
    InvalidUtf8,
    /// A value of another type (integer or real).
    NotText,
}

impl TextCell {
    /// Why the cell cannot be used as text, or `None` when it can.
    pub(crate) fn problem(&self) -> Option<&'static str> {
        match self {
            TextCell::Null | TextCell::Text(_) => None,
            TextCell::InvalidUtf8 => Some("is not valid UTF-8"),
            TextCell::NotText => Some("is not text"),
        }
    }
}

/// Reads a cell as text without `rusqlite`'s `String` conversion, which
/// calls `expect("invalid UTF-8")` and aborts the run on such a value.
///
/// The callers decide what an unusable cell means for their row; nothing is
/// replaced and no bytes are dropped silently.
pub(crate) fn text_cell(value: ValueRef<'_>) -> TextCell {
    let bytes = match value {
        ValueRef::Null => return TextCell::Null,
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => bytes,
        ValueRef::Integer(_) | ValueRef::Real(_) => return TextCell::NotText,
    };
    match std::str::from_utf8(bytes) {
        Ok(text) => TextCell::Text(text.to_owned()),
        Err(_) => TextCell::InvalidUtf8,
    }
}

fn missing_schema_message(table: &str, column: Option<&str>) -> String {
    match column {
        Some(c) => format!("catalog table {table} has no column {c}; the catalog may be damaged"),
        None => format!("catalog has no table {table}; the catalog may be damaged"),
    }
}

/// Every table and column the readers query. This is the single source of
/// truth for the schema check in [`Catalog::open`]: a new column in any
/// SELECT of the readers must be added here. The unit test
/// `readers_need_only_required_schema` fails otherwise.
const REQUIRED_SCHEMA: &[(&str, &[&str])] = &[
    (
        "Adobe_images",
        &[
            "id_local",
            "masterImage",
            "rating",
            "fileFormat",
            "rootFile",
            "id_global",
            "pick",
            "colorLabels",
        ],
    ),
    (
        "AgLibraryFile",
        &["id_local", "baseName", "extension", "folder"],
    ),
    (
        "AgLibraryFolder",
        &["id_local", "rootFolder", "pathFromRoot"],
    ),
    ("AgLibraryRootFolder", &["id_local", "absolutePath"]),
    (
        "AgLibraryCollection",
        &["id_local", "creationId", "systemOnly", "name", "parent"],
    ),
    (
        "AgLibraryCollectionImage",
        &["id_local", "collection", "image", "positionInCollection"],
    ),
    (
        "AgLibraryKeyword",
        &[
            "id_local",
            "name",
            "parent",
            "includeOnExport",
            "includeParents",
            "includeSynonyms",
            "keywordType",
            "id_global",
        ],
    ),
    ("AgLibraryKeywordImage", &["id_local", "image", "tag"]),
    ("AgLibraryKeywordSynonym", &["id_local", "keyword", "name"]),
    (
        "AgLibraryIPTC",
        &["id_local", "image", "caption", "copyright"],
    ),
];

/// Suffixes SQLite appends to the full catalog file name for side files
/// that can hold changes not yet merged into the catalog.
const SIDE_FILE_SUFFIXES: [&str; 2] = ["-wal", "-journal"];

/// An open Lightroom Classic catalog. The connection is read-only and
/// immutable, so SQLite never writes to the file or creates sidecar files.
#[derive(Debug)]
pub struct Catalog {
    pub(crate) conn: Connection,
    version: String,
}

impl Catalog {
    /// Opens the catalog at `path` read-only and checks its version.
    ///
    /// Fails with [`ReadError::PendingChanges`] if a non-empty `-wal` or
    /// `-journal` file exists, because the immutable connection would
    /// silently ignore its contents.
    pub fn open(path: &Path) -> Result<Catalog, ReadError> {
        if let Some(side) = pending_side_file(path) {
            return Err(ReadError::PendingChanges { path: side });
        }
        let uri = read_only_uri(path).map_err(ReadError::Open)?;
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(uri, flags).map_err(ReadError::Open)?;

        let value: Option<Option<Result<String, ()>>> = conn
            .query_row(
                "SELECT value FROM Adobe_variablesTable WHERE name = 'Adobe_DBVersion' LIMIT 1",
                [],
                |row| {
                    Ok(match row.get_ref(0)? {
                        ValueRef::Null => None,
                        ValueRef::Integer(i) => Some(Ok(i.to_string())),
                        ValueRef::Real(r) => Some(Ok(r.to_string())),
                        ValueRef::Text(bytes) => Some(
                            std::str::from_utf8(bytes)
                                .map(str::to_owned)
                                .map_err(|_| ()),
                        ),
                        ValueRef::Blob(bytes) => {
                            Some(String::from_utf8(bytes.to_vec()).map_err(|_| ()))
                        }
                    })
                },
            )
            .optional()
            .map_err(ReadError::Open)?;
        let version = match value {
            None | Some(None) => return Err(ReadError::MissingVersion),
            Some(Some(Err(()))) => return Err(ReadError::InvalidVersion),
            Some(Some(Ok(version))) => version,
        };
        if version != SUPPORTED_VERSION {
            return Err(ReadError::UnsupportedVersion(version));
        }
        check_schema(&conn)?;
        Ok(Catalog { conn, version })
    }

    /// The catalog's `Adobe_DBVersion`.
    pub fn version(&self) -> &str {
        &self.version
    }
}

/// Checks that every table and column in [`REQUIRED_SCHEMA`] exists.
/// Names are compared exactly.
fn check_schema(conn: &Connection) -> Result<(), ReadError> {
    let mut table_exists = conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1")
        .map_err(ReadError::Open)?;
    let mut columns = conn
        .prepare("SELECT name FROM pragma_table_info(?1)")
        .map_err(ReadError::Open)?;
    for &(table, required) in REQUIRED_SCHEMA {
        if !table_exists.exists([table]).map_err(ReadError::Open)? {
            return Err(ReadError::MissingSchema {
                table: table.to_owned(),
                column: None,
            });
        }
        // A hostile catalog can store a column name that is not valid
        // UTF-8. Such a name cannot equal any required ASCII name, so it
        // counts as absent; reading it must not abort the run.
        let present = columns
            .query_map([table], |row| Ok(text_cell(row.get_ref(0)?)))
            .map_err(ReadError::Open)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(ReadError::Open)?;
        if let Some(missing) = required.iter().find(|c| {
            !present
                .iter()
                .any(|p| matches!(p, TextCell::Text(name) if name == *c))
        }) {
            return Err(ReadError::MissingSchema {
                table: table.to_owned(),
                column: Some((*missing).to_owned()),
            });
        }
    }
    Ok(())
}

/// Returns the first `-wal` or `-journal` file next to `catalog` that is not
/// empty. Only metadata is read; nothing is created or opened for writing.
/// A side file whose metadata cannot be read counts as pending, so changes
/// are never ignored silently.
fn pending_side_file(catalog: &Path) -> Option<PathBuf> {
    let name = catalog.file_name()?;
    SIDE_FILE_SUFFIXES.iter().find_map(|suffix| {
        let mut side_name = name.to_os_string();
        side_name.push(suffix);
        let side = catalog.with_file_name(side_name);
        match std::fs::metadata(&side) {
            Ok(meta) if meta.len() == 0 => None,
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            _ => Some(side),
        }
    })
}

/// SHA-256 of the file at `path`, used to prove a catalog stays unchanged.
pub fn checksum(path: &Path) -> io::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().into())
}

/// Builds a SQLite URI `file:<path>?mode=ro&immutable=1` with the path
/// percent-encoded, so `?`, `#`, `%` and non-ASCII characters are safe.
fn read_only_uri(path: &Path) -> Result<String, rusqlite::Error> {
    let raw = path_bytes(path)?;
    Ok(format!("file:{}?mode=ro&immutable=1", encode_path(&raw)))
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Result<Vec<u8>, rusqlite::Error> {
    use std::os::unix::ffi::OsStrExt;
    Ok(path.as_os_str().as_bytes().to_vec())
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Result<Vec<u8>, rusqlite::Error> {
    // SQLite expects UTF-8 file names on Windows.
    path.to_str()
        .map(|s| s.replace('\\', "/").into_bytes())
        .ok_or_else(|| rusqlite::Error::InvalidPath(path.to_path_buf()))
}

/// Percent-encodes a path for the current platform; see [`encode_path_for`].
fn encode_path(raw: &[u8]) -> String {
    encode_path_for(raw, cfg!(windows))
}

/// Percent-encodes everything except RFC 3986 unreserved characters and `/`.
///
/// With `windows` set, a path that starts with a drive letter (`C:/...`)
/// gets a leading `/` so SQLite reads it as `file:///C:/...`; the drive
/// colon stays unencoded. Elsewhere `a:x` is an ordinary relative name, so
/// the colon is encoded and the path stays relative.
fn encode_path_for(raw: &[u8], windows: bool) -> String {
    let mut out = String::with_capacity(raw.len() + 8);
    let has_drive = windows && raw.len() >= 2 && raw[0].is_ascii_alphabetic() && raw[1] == b':';
    if has_drive {
        out.push('/');
        out.push(raw[0] as char);
        out.push(':');
    }
    let rest = if has_drive { &raw[2..] } else { raw };
    let absolute = rest.first() == Some(&b'/') || has_drive;
    if absolute {
        // Make the authority empty: `file:///path` instead of `file:/path`.
        out.insert_str(0, "//");
    }
    for &b in rest {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_plain_unix_path() {
        assert_eq!(encode_path(b"/a/b.lrcat"), "///a/b.lrcat");
    }

    #[test]
    fn encodes_special_characters() {
        assert_eq!(
            encode_path("/x y/a?b#c%d&e.lrcat".as_bytes()),
            "///x%20y/a%3Fb%23c%25d%26e.lrcat"
        );
        assert_eq!(encode_path("/ü".as_bytes()), "///%C3%BC");
    }

    #[test]
    fn encodes_windows_drive_path() {
        assert_eq!(
            encode_path_for(b"C:/data/x y.lrcat", true),
            "///C:/data/x%20y.lrcat"
        );
    }

    #[test]
    fn drive_pattern_is_plain_relative_name_off_windows() {
        assert_eq!(encode_path_for(b"a:x.lrcat", false), "a%3Ax.lrcat");
        assert_eq!(
            encode_path_for(b"C:/data/x y.lrcat", false),
            "C%3A/data/x%20y.lrcat"
        );
    }

    #[test]
    fn both_variants_agree_on_paths_without_drive() {
        for raw in [&b"/a/b.lrcat"[..], b"dir/cat.lrcat", b"/x y/a?b"] {
            assert_eq!(encode_path_for(raw, true), encode_path_for(raw, false));
        }
    }

    #[cfg(unix)]
    #[test]
    fn uri_keeps_relative_path_with_colon_relative_on_unix() {
        let uri = read_only_uri(Path::new("a:x.lrcat")).unwrap();
        assert_eq!(uri, "file:a%3Ax.lrcat?mode=ro&immutable=1");
    }

    #[test]
    fn keeps_relative_path_relative() {
        assert_eq!(encode_path(b"dir/cat.lrcat"), "dir/cat.lrcat");
    }

    #[test]
    fn uri_has_read_only_parameters() {
        let uri = read_only_uri(Path::new("/a/b.lrcat")).unwrap();
        assert_eq!(uri, "file:///a/b.lrcat?mode=ro&immutable=1");
    }

    #[test]
    fn checksum_of_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty");
        std::fs::write(&path, b"").unwrap();
        let sum = checksum(&path).unwrap();
        // SHA-256 of the empty input starts with e3b0c442.
        assert_eq!(&sum[..4], &[0xe3, 0xb0, 0xc4, 0x42]);
    }

    #[test]
    fn opens_path_with_special_characters() {
        let dir = tempfile::tempdir().unwrap();
        // A real directory is needed here, so the name holds only characters
        // every platform allows: `?` is illegal in a Windows file name. `?`
        // and the other reserved marks are covered by
        // `encodes_special_characters`, which needs no file system.
        let sub = dir.path().join("a #b+c%20 ü");
        std::fs::create_dir(&sub).unwrap();
        let path = sub.join("cat & co.lrcat");
        lrcat_fixtures::write_small_catalog(&path).unwrap();
        let catalog = Catalog::open(&path).expect("open");
        assert_eq!(catalog.version(), SUPPORTED_VERSION);
    }

    #[test]
    fn invalid_utf8_dbversion_is_a_read_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cat.lrcat");
        lrcat_fixtures::write_small_catalog(&path).unwrap();
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "UPDATE Adobe_variablesTable SET value = CAST(x'FFFE' AS TEXT) \
             WHERE name = 'Adobe_DBVersion'",
            [],
        )
        .unwrap();
        drop(conn);
        assert!(matches!(
            Catalog::open(&path),
            Err(ReadError::InvalidVersion)
        ));
    }

    #[test]
    fn side_file_appends_suffix_to_full_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cat.lrcat");
        assert_eq!(pending_side_file(&path), None);
        std::fs::write(dir.path().join("cat-wal"), b"x").unwrap();
        assert_eq!(pending_side_file(&path), None);
        let wal = dir.path().join("cat.lrcat-wal");
        std::fs::write(&wal, b"x").unwrap();
        assert_eq!(pending_side_file(&path), Some(wal));
    }

    #[test]
    fn wal_is_reported_before_journal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cat.lrcat");
        let journal = dir.path().join("cat.lrcat-journal");
        std::fs::write(&journal, b"x").unwrap();
        assert_eq!(pending_side_file(&path), Some(journal));
        let wal = dir.path().join("cat.lrcat-wal");
        std::fs::write(&wal, b"x").unwrap();
        assert_eq!(pending_side_file(&path), Some(wal));
    }

    #[test]
    fn empty_side_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cat.lrcat");
        std::fs::write(dir.path().join("cat.lrcat-wal"), b"").unwrap();
        std::fs::write(dir.path().join("cat.lrcat-journal"), b"").unwrap();
        assert_eq!(pending_side_file(&path), None);
    }

    #[test]
    fn pending_changes_message_names_lightroom_and_file() {
        let err = ReadError::PendingChanges {
            path: PathBuf::from("cat.lrcat-wal"),
        };
        let msg = err.to_string();
        assert!(msg.contains("Lightroom"), "{msg}");
        assert!(msg.contains("cat.lrcat-wal"), "{msg}");
    }

    /// Writes a catalog that has exactly the tables and columns of
    /// [`REQUIRED_SCHEMA`] plus the version entry, with one row per table.
    fn write_minimal_catalog(path: &Path) {
        let c = Connection::open(path).unwrap();
        c.execute_batch("CREATE TABLE Adobe_variablesTable (name, value);")
            .unwrap();
        c.execute(
            "INSERT INTO Adobe_variablesTable VALUES ('Adobe_DBVersion', ?1)",
            [SUPPORTED_VERSION],
        )
        .unwrap();
        for &(table, columns) in REQUIRED_SCHEMA {
            c.execute_batch(&format!(
                "CREATE TABLE {table} ({}); INSERT INTO {table} (id_local) VALUES (1);",
                columns.join(", ")
            ))
            .unwrap();
        }
    }

    #[test]
    fn readers_need_only_required_schema() {
        // If a reader SELECTs a column missing from REQUIRED_SCHEMA, the
        // statement fails to prepare here: add the column to the list.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("minimal.lrcat");
        write_minimal_catalog(&path);
        let catalog = Catalog::open(&path).expect("open");
        let mut report = model::Report::default();
        catalog.folders(&mut report).expect("folders");
        catalog.photos(&mut report).expect("photos");
        catalog.collection_data(&[]).expect("collections");
        let mut assignments = model::Report::default();
        catalog
            .keywords(&[], &mut report, &mut assignments)
            .expect("keywords");
    }

    #[test]
    fn every_required_column_is_checked() {
        for &(table, columns) in REQUIRED_SCHEMA {
            for &column in columns {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("damaged.lrcat");
                write_minimal_catalog(&path);
                let c = Connection::open(&path).unwrap();
                c.execute_batch(&format!("ALTER TABLE {table} DROP COLUMN {column};"))
                    .unwrap();
                drop(c);
                match Catalog::open(&path) {
                    Err(ReadError::MissingSchema {
                        table: t,
                        column: col,
                    }) => {
                        assert_eq!(t, table);
                        assert_eq!(col.as_deref(), Some(column));
                    }
                    other => panic!("{table}.{column}: expected MissingSchema, got {other:?}"),
                }
            }
        }
    }

    #[test]
    fn column_names_are_compared_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("case.lrcat");
        write_minimal_catalog(&path);
        let c = Connection::open(&path).unwrap();
        c.execute_batch("ALTER TABLE Adobe_images RENAME COLUMN rating TO Rating;")
            .unwrap();
        drop(c);
        assert!(matches!(
            Catalog::open(&path),
            Err(ReadError::MissingSchema { column: Some(c), .. }) if c == "rating"
        ));
    }

    #[test]
    fn missing_schema_message_names_table_and_column() {
        let err = ReadError::MissingSchema {
            table: "T".into(),
            column: Some("c".into()),
        };
        let msg = err.to_string();
        assert!(msg.contains("table T") && msg.contains("column c"), "{msg}");
        let err = ReadError::MissingSchema {
            table: "T".into(),
            column: None,
        };
        assert!(err.to_string().contains("table T"));
    }

    #[test]
    fn database_without_version_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.lrcat");
        let c = Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE Adobe_variablesTable (name, value);")
            .unwrap();
        drop(c);
        assert!(matches!(
            Catalog::open(&path),
            Err(ReadError::MissingVersion)
        ));
    }

    #[test]
    fn text_cell_classifies_without_converting_lossily() {
        assert_eq!(text_cell(ValueRef::Null), TextCell::Null);
        assert_eq!(
            text_cell(ValueRef::Text(b"ok")),
            TextCell::Text("ok".into())
        );
        assert_eq!(text_cell(ValueRef::Text(&[0xFF])), TextCell::InvalidUtf8);
        assert_eq!(
            text_cell(ValueRef::Blob(b"ok")),
            TextCell::Text("ok".into())
        );
        assert_eq!(text_cell(ValueRef::Blob(&[0xFF])), TextCell::InvalidUtf8);
        assert_eq!(text_cell(ValueRef::Integer(1)), TextCell::NotText);
        assert_eq!(text_cell(ValueRef::Real(1.0)), TextCell::NotText);
    }

    /// A schema name is catalog data too: a hostile catalog can store bytes
    /// that are not valid UTF-8 in `sqlite_master`. Such a name must count as
    /// absent instead of aborting the run through `rusqlite`'s `String`.
    #[test]
    fn invalid_utf8_column_name_counts_as_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad-schema.lrcat");
        write_minimal_catalog(&path);
        let c = Connection::open(&path).unwrap();
        c.execute_batch("PRAGMA writable_schema = ON;").unwrap();
        c.execute(
            "UPDATE sqlite_master SET sql = CAST(?1 AS TEXT) \
             WHERE type = 'table' AND name = 'AgLibraryFolder'",
            [b"CREATE TABLE AgLibraryFolder (id_local, rootFolder, pathFromRoot\xFF)".to_vec()],
        )
        .unwrap();
        c.execute_batch("PRAGMA writable_schema = OFF;").unwrap();
        drop(c);
        match Catalog::open(&path) {
            Err(ReadError::MissingSchema { table, column }) => {
                assert_eq!(table, "AgLibraryFolder");
                assert_eq!(column.as_deref(), Some("pathFromRoot"));
            }
            other => panic!("expected MissingSchema, got {other:?}"),
        }
    }
}
