//! Reading folders (`AgLibraryFolder` joined with `AgLibraryRootFolder`).

use crate::catalog::{Catalog, ReadError};
use model::Folder;

impl Catalog {
    /// Reads all folders, ordered by their catalog id.
    ///
    /// Each path is the root folder's `absolutePath` followed by the folder's
    /// `pathFromRoot`. Paths are assembled from the stored strings only; the
    /// file system is never accessed. A folder whose root folder is missing
    /// yields [`ReadError::MissingRootFolder`] instead of being dropped.
    pub fn folders(&self) -> Result<Vec<Folder>, ReadError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT f.id_local, f.rootFolder, f.pathFromRoot, r.absolutePath \
                 FROM AgLibraryFolder f \
                 LEFT JOIN AgLibraryRootFolder r ON r.id_local = f.rootFolder \
                 ORDER BY f.id_local",
            )
            .map_err(ReadError::Query)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(ReadError::Query)?;
        let mut folders = Vec::new();
        for row in rows {
            let (folder_id, root_id, rel, root) = row.map_err(ReadError::Query)?;
            let root = root.ok_or(ReadError::MissingRootFolder { folder_id, root_id })?;
            let path = join_folder_path(&root, rel.as_deref().unwrap_or(""));
            folders.push(Folder::new(path));
        }
        Ok(folders)
    }
}

/// Joins a root folder path and a path relative to it.
///
/// `pathFromRoot` always uses `/`. If the root uses `\` only (a Windows
/// root), the relative part is converted to `\` so the result is consistent.
/// A trailing separator is removed unless the path is a bare root such as
/// `/` or `C:\`.
fn join_folder_path(root: &str, rel: &str) -> String {
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

    #[test]
    fn folder_with_missing_root_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.lrcat");
        lrcat_fixtures::write_small_catalog(&path).unwrap();
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute(
            "UPDATE AgLibraryFolder SET rootFolder = 99 WHERE id_local = 3",
            [],
        )
        .unwrap();
        drop(c);
        let catalog = Catalog::open(&path).unwrap();
        assert!(matches!(
            catalog.folders(),
            Err(ReadError::MissingRootFolder {
                folder_id: 3,
                root_id: 99
            })
        ));
    }
}
