//! Read-only checks of the chosen catalog's files: does it exist, is its
//! companion folder there, is Lightroom holding it open.

use std::path::{Path, PathBuf};

/// What the files next to the catalog say.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CatalogFiles {
    pub exists: bool,
    /// The `.lrcat-data` folder Lightroom Classic keeps next to newer
    /// catalogs. lr2lc does not read it.
    pub companion: bool,
    /// Lightroom holds the catalog open: its `.lrcat.lock` file exists, or
    /// unsaved changes wait in a `-wal` or `-journal` file.
    pub open_in_lightroom: bool,
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Looks at the files of `catalog`; never opens or changes one.
pub fn catalog_files(catalog: &Path) -> CatalogFiles {
    let exists = catalog.is_file();
    let companion = catalog.with_extension("lrcat-data").is_dir();
    let non_empty = |p: PathBuf| std::fs::metadata(p).is_ok_and(|m| m.len() > 0);
    let open_in_lightroom = sibling(catalog, ".lock").exists()
        || non_empty(sibling(catalog, "-wal"))
        || non_empty(sibling(catalog, "-journal"));
    CatalogFiles {
        exists,
        companion,
        open_in_lightroom,
    }
}

/// `Classic 15` for a catalog version such as `1500000`, else the raw text.
pub fn version_name(raw: &str) -> String {
    match raw.trim().parse::<u64>() {
        Ok(v) if v >= 100_000 => format!("Classic {}", v / 100_000),
        _ => raw.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_and_wal_files_mean_lightroom_has_it_open() {
        let dir = tempfile::tempdir().unwrap();
        let cat = dir.path().join("Test.lrcat");
        assert!(!catalog_files(&cat).exists);
        std::fs::write(&cat, b"x").unwrap();
        let files = catalog_files(&cat);
        assert!(files.exists && !files.companion && !files.open_in_lightroom);
        std::fs::create_dir(dir.path().join("Test.lrcat-data")).unwrap();
        assert!(catalog_files(&cat).companion);
        std::fs::write(dir.path().join("Test.lrcat-wal"), b"").unwrap();
        assert!(
            !catalog_files(&cat).open_in_lightroom,
            "an empty -wal is fine"
        );
        std::fs::write(dir.path().join("Test.lrcat.lock"), b"").unwrap();
        assert!(catalog_files(&cat).open_in_lightroom);
    }

    #[test]
    fn versions_read_as_classic_releases() {
        assert_eq!(version_name("1500000"), "Classic 15");
        assert_eq!(version_name("odd"), "odd");
    }
}
