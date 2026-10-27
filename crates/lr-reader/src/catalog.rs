//! Opening a Lightroom Classic catalog strictly read-only.

use rusqlite::types::Value;
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
        &["id_local", "masterImage", "rating", "rootFile"],
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
        &["id_local", "creationId", "systemOnly"],
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

        let value: Option<Value> = conn
            .query_row(
                "SELECT value FROM Adobe_variablesTable WHERE name = 'Adobe_DBVersion' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(ReadError::Open)?;
        let version = match value {
            None => return Err(ReadError::MissingVersion),
            Some(Value::Text(s)) => s,
            Some(Value::Integer(i)) => i.to_string(),
            Some(Value::Null) => return Err(ReadError::MissingVersion),
            Some(Value::Real(r)) => r.to_string(),
            Some(Value::Blob(b)) => String::from_utf8_lossy(&b).into_owned(),
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
        let present = columns
            .query_map([table], |row| row.get::<_, String>(0))
            .map_err(ReadError::Open)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(ReadError::Open)?;
        if let Some(missing) = required.iter().find(|c| !present.iter().any(|p| p == *c)) {
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
        let sub = dir.path().join("a #b?c%20 ü");
        std::fs::create_dir(&sub).unwrap();
        let path = sub.join("cat & co.lrcat");
        lrcat_fixtures::write_small_catalog(&path).unwrap();
        let catalog = Catalog::open(&path).expect("open");
        assert_eq!(catalog.version(), SUPPORTED_VERSION);
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
        catalog.collections(&mut report).expect("collections");
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
}
