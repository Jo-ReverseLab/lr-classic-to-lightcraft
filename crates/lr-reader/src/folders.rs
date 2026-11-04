//! Reading folders (`AgLibraryFolder` joined with `AgLibraryRootFolder`).

use crate::catalog::{Catalog, ReadError, TextCell, text_cell};
use model::{Folder, Report, SkipCode, Skipped};

impl Catalog {
    /// Reads all folders, ordered by their catalog id.
    ///
    /// Each path is the root folder's `absolutePath` followed by the folder's
    /// `pathFromRoot`. Paths are assembled from the stored strings only; the
    /// file system is never accessed.
    ///
    /// Every folder row is counted with [`Report::read`]. A folder whose root
    /// folder row is missing or has no path is recorded with
    /// [`Report::skipped`] (the reason names the folder id, never a path) and
    /// left out. Nothing is marked converted here.
    pub fn folders(&self, report: &mut Report) -> Result<Vec<Folder>, ReadError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT f.id_local, f.rootFolder, f.pathFromRoot, r.id_local, r.absolutePath \
                 FROM AgLibraryFolder f \
                 LEFT JOIN AgLibraryRootFolder r ON r.id_local = f.rootFolder \
                 ORDER BY f.id_local",
            )
            .map_err(ReadError::Query)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(FolderRow {
                    folder_id: row.get(0)?,
                    root_id: row.get(1)?,
                    rel: text_cell(row.get_ref(2)?),
                    root_row: row.get::<_, Option<i64>>(3)?.is_some(),
                    root_path: text_cell(row.get_ref(4)?),
                })
            })
            .map_err(ReadError::Query)?;
        let mut folders = Vec::new();
        for row in rows {
            let row = row.map_err(ReadError::Query)?;
            report.read();
            match folder_from_row(row) {
                Ok(folder) => folders.push(folder),
                Err(skip) => report.push_skipped(skip),
            }
        }
        Ok(folders)
    }
}

/// One joined folder row.
struct FolderRow {
    folder_id: i64,
    root_id: Option<i64>,
    rel: TextCell,
    root_row: bool,
    root_path: TextCell,
}

/// Turns a joined row into a folder, or returns the reason it is skipped.
/// Reasons name catalog ids only, never paths.
fn folder_from_row(row: FolderRow) -> Result<Folder, Skipped> {
    let id = row.folder_id;
    if !row.root_row {
        return Err(match row.root_id {
            Some(root) => Skipped::new(
                SkipCode::RootFolderNotFound,
                format!("folder {id}: root folder {root} not found in catalog"),
            ),
            None => Skipped::new(
                SkipCode::NoRootFolder,
                format!("folder {id}: has no root folder"),
            ),
        });
    }
    let root = match row.root_path {
        TextCell::Text(path) => path,
        TextCell::Null => {
            return Err(Skipped::new(
                SkipCode::RootFolderNoPath,
                format!("folder {id}: root folder has no path"),
            ));
        }
        cell => {
            return Err(Skipped::new(
                SkipCode::PathNotUtf8,
                format!(
                    "folder {id}: root folder path {}",
                    cell.problem().unwrap_or("is not usable text")
                ),
            ));
        }
    };
    let rel = match row.rel {
        TextCell::Text(path) => path,
        TextCell::Null => String::new(),
        cell => {
            return Err(Skipped::new(
                SkipCode::PathNotUtf8,
                format!(
                    "folder {id}: path from root {}",
                    cell.problem().unwrap_or("is not usable text")
                ),
            ));
        }
    };
    Ok(Folder::new(join_folder_path(&root, &rel)))
}

/// Joins a root folder path and a path relative to it.
///
/// `pathFromRoot` always uses `/`. If the root uses `\` only (a Windows
/// root), the relative part is converted to `\` so the result is consistent.
/// A trailing separator is removed unless the path is a bare root such as
/// `/` or `C:\`.
pub(crate) fn join_folder_path(root: &str, rel: &str) -> String {
    let windows = root.contains('\\') && !root.contains('/');
    let sep = if windows { '\\' } else { '/' };
    let mut path = root.to_string();
    let rel = rel.trim_start_matches(['/', '\\']);
    if !rel.is_empty() {
        if !path.ends_with(['/', '\\']) {
            path.push(sep);
        }
        if windows {
            path.push_str(&rel.replace('/', "\\"));
        } else {
            path.push_str(rel);
        }
    }
    while path.ends_with(['/', '\\']) && !is_bare_root(&path) {
        path.pop();
    }
    path
}

/// True for `/`, `\` or a drive root like `C:\` / `C:/`.
fn is_bare_root(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() == 1 || (b.len() == 3 && b[0].is_ascii_alphabetic() && b[1] == b':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_unix_root_and_relative_path() {
        assert_eq!(join_folder_path("/a/b/", "c/d/"), "/a/b/c/d");
    }

    #[test]
    fn root_without_trailing_separator_gets_one() {
        assert_eq!(join_folder_path("/a/b", "c/"), "/a/b/c");
    }

    #[test]
    fn empty_relative_path_yields_root() {
        assert_eq!(join_folder_path("/a/b/", ""), "/a/b");
    }

    #[test]
    fn windows_root_uses_backslashes() {
        assert_eq!(join_folder_path("C:\\x\\", "y/z/"), "C:\\x\\y\\z");
    }

    #[test]
    fn windows_root_with_forward_slashes_is_kept() {
        assert_eq!(join_folder_path("C:/x/", "y/"), "C:/x/y");
    }

    #[test]
    fn bare_roots_keep_their_separator() {
        assert_eq!(join_folder_path("/", ""), "/");
        assert_eq!(join_folder_path("C:\\", ""), "C:\\");
        assert_eq!(join_folder_path("C:\\", "a/"), "C:\\a");
    }

    fn row() -> FolderRow {
        FolderRow {
            folder_id: 3,
            root_id: Some(1),
            rel: TextCell::Text("a/".into()),
            root_row: true,
            root_path: TextCell::Text("/r/".into()),
        }
    }

    #[test]
    fn valid_row_becomes_folder() {
        assert_eq!(
            folder_from_row(row()).unwrap().path,
            std::path::PathBuf::from("/r/a")
        );
    }

    #[test]
    fn missing_root_row_is_skipped_with_ids() {
        let mut r = row();
        r.root_row = false;
        r.root_id = Some(99);
        let skip = folder_from_row(r).unwrap_err();
        assert_eq!(skip.code, SkipCode::RootFolderNotFound);
        let reason = skip.reason;
        assert!(reason.contains("root") && reason.contains('3') && reason.contains("99"));
        assert!(!reason.contains('/'));
    }

    #[test]
    fn null_root_reference_is_skipped() {
        let mut r = row();
        r.root_row = false;
        r.root_id = None;
        let skip = folder_from_row(r).unwrap_err();
        assert_eq!(skip.code, SkipCode::NoRootFolder);
        assert!(skip.reason.contains("root"));
    }

    #[test]
    fn root_without_path_is_skipped() {
        let mut r = row();
        r.root_path = TextCell::Null;
        let skip = folder_from_row(r).unwrap_err();
        assert_eq!(skip.code, SkipCode::RootFolderNoPath);
        let reason = skip.reason;
        assert!(reason.contains("root") && reason.contains('3'));
    }

    #[test]
    fn invalid_root_path_is_skipped() {
        let mut r = row();
        r.root_path = TextCell::InvalidUtf8;
        let skip = folder_from_row(r).unwrap_err();
        assert_eq!(skip.code, SkipCode::PathNotUtf8);
        assert_eq!(skip.reason, "folder 3: root folder path is not valid UTF-8");
    }

    #[test]
    fn invalid_relative_path_is_skipped() {
        let mut r = row();
        r.rel = TextCell::InvalidUtf8;
        let skip = folder_from_row(r).unwrap_err();
        assert_eq!(skip.code, SkipCode::PathNotUtf8);
        assert_eq!(skip.reason, "folder 3: path from root is not valid UTF-8");
    }

    #[test]
    fn non_text_path_is_skipped_without_naming_a_path() {
        let mut r = row();
        r.rel = TextCell::NotText;
        let skip = folder_from_row(r).unwrap_err();
        assert_eq!(skip.code, SkipCode::PathNotUtf8);
        assert_eq!(skip.reason, "folder 3: path from root is not text");
    }

    /// Writes the small fixture and stores a raw value in one column.
    fn catalog_with_sql(sql: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("folders.lrcat");
        lrcat_fixtures::write_small_catalog(&path).expect("generate fixture");
        let conn = rusqlite::Connection::open(&path).expect("open for setup");
        conn.execute_batch(sql).expect("setup");
        drop(conn);
        (dir, path)
    }

    #[test]
    fn invalid_utf8_text_folder_path_is_skipped() {
        let (_dir, path) = catalog_with_sql(
            "UPDATE AgLibraryFolder SET pathFromRoot = CAST(x'FFFE' AS TEXT) WHERE id_local = 2;",
        );
        let catalog = Catalog::open(&path).expect("open catalog");
        let mut report = Report::new();
        let folders = catalog.folders(&mut report).expect("read folders");
        let skipped = report
            .skipped_items()
            .iter()
            .find(|s| s.reason.starts_with("folder 2: "))
            .expect("folder 2 is skipped");
        assert_eq!(skipped.code, SkipCode::PathNotUtf8);
        assert_eq!(
            skipped.reason,
            "folder 2: path from root is not valid UTF-8"
        );
        assert_eq!(folders.len() + report.skipped_count(), report.read_count());
    }

    #[test]
    fn invalid_utf8_text_root_folder_path_is_skipped() {
        let (_dir, path) = catalog_with_sql(
            "UPDATE AgLibraryRootFolder SET absolutePath = CAST(x'FFFE' AS TEXT) \
             WHERE id_local = 2;",
        );
        let catalog = Catalog::open(&path).expect("open catalog");
        let mut report = Report::new();
        let folders = catalog.folders(&mut report).expect("read folders");
        let skipped = report
            .skipped_items()
            .iter()
            .find(|s| s.reason.starts_with("folder 4: "))
            .expect("folder 4 is skipped");
        assert_eq!(skipped.code, SkipCode::PathNotUtf8);
        assert_eq!(
            skipped.reason,
            "folder 4: root folder path is not valid UTF-8"
        );
        assert_eq!(report.skipped_count(), 3);
        assert_eq!(folders.len() + report.skipped_count(), report.read_count());
    }
}
