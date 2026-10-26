//! Reading folders (`AgLibraryFolder` joined with `AgLibraryRootFolder`).

use crate::catalog::{Catalog, ReadError};
use model::{Folder, Report};

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
                    rel: row.get(2)?,
                    root_row: row.get::<_, Option<i64>>(3)?.is_some(),
                    root_path: row.get(4)?,
                })
            })
            .map_err(ReadError::Query)?;
        let mut folders = Vec::new();
        for row in rows {
            let row = row.map_err(ReadError::Query)?;
            report.read();
            match folder_from_row(row) {
                Ok(folder) => folders.push(folder),
                Err(reason) => report.skipped(reason),
            }
        }
        Ok(folders)
    }
}

/// One joined folder row.
struct FolderRow {
    folder_id: i64,
    root_id: Option<i64>,
    rel: Option<String>,
    root_row: bool,
    root_path: Option<String>,
}

/// Turns a joined row into a folder, or returns the reason it is skipped.
/// Reasons name catalog ids only, never paths.
fn folder_from_row(row: FolderRow) -> Result<Folder, String> {
    let id = row.folder_id;
    if !row.root_row {
        return Err(match row.root_id {
            Some(root) => format!("folder {id}: root folder {root} not found in catalog"),
            None => format!("folder {id}: has no root folder"),
        });
    }
    let root = row
        .root_path
        .ok_or_else(|| format!("folder {id}: root folder has no path"))?;
    Ok(Folder::new(join_folder_path(
        &root,
        row.rel.as_deref().unwrap_or(""),
    )))
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
            rel: Some("a/".into()),
            root_row: true,
            root_path: Some("/r/".into()),
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
        let reason = folder_from_row(r).unwrap_err();
        assert!(reason.contains("root") && reason.contains('3') && reason.contains("99"));
        assert!(!reason.contains('/'));
    }

    #[test]
    fn null_root_reference_is_skipped() {
        let mut r = row();
        r.root_row = false;
        r.root_id = None;
        assert!(folder_from_row(r).unwrap_err().contains("root"));
    }

    #[test]
    fn root_without_path_is_skipped() {
        let mut r = row();
        r.root_path = None;
        let reason = folder_from_row(r).unwrap_err();
        assert!(reason.contains("root") && reason.contains('3'));
    }
}
