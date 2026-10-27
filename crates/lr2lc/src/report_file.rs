//! Safe handling of the `--report FILE` target.
//!
//! The report must never destroy the catalog, its SQLite companion files or
//! any other user file (such as a photo). Before anything is read, the target
//! is checked:
//!
//! - it must not resolve to the catalog or to `<catalog>-wal`, `-journal` or
//!   `-shm`, also through a hard link (same device and inode on Unix) or a
//!   differently cased path (case-insensitive comparison elsewhere);
//! - if it exists, it must be an earlier lr2lc report (a JSON object with the
//!   key `tool_version`); anything else is left alone.
//!
//! Writing then creates a new file with `create_new`, or re-checks and
//! truncates the earlier report, so a file appearing in between is not
//! overwritten.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Existing files larger than this are never treated as an earlier report.
const MAX_REPORT_BYTES: u64 = 64 * 1024 * 1024;

/// SQLite companion files that belong to a catalog.
const COMPANION_SUFFIXES: [&str; 3] = ["-wal", "-journal", "-shm"];

/// A checked report target.
#[derive(Debug, PartialEq)]
pub enum ReportTarget {
    /// The file does not exist yet; `path` is the resolved location.
    New { path: PathBuf },
    /// The file is an earlier lr2lc report; `path` is the resolved location.
    EarlierReport { path: PathBuf },
}

/// Why a report target is refused.
#[derive(Debug)]
pub enum TargetError {
    /// The target is the catalog or one of its companion files.
    Catalog { given: PathBuf },
    /// The target exists and is not an earlier lr2lc report.
    Exists { given: PathBuf },
    /// The target cannot be resolved (missing directory, no file name, ...).
    Unresolvable { given: PathBuf, source: io::Error },
}

impl fmt::Display for TargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TargetError::Catalog { given } => write!(
                f,
                "refusing to write the report to {}: it is the catalog or one of its \
                 companion files (-wal, -journal, -shm)",
                given.display()
            ),
            TargetError::Exists { given } => write!(
                f,
                "refusing to write the report to {}: the file exists and is not an \
                 earlier lr2lc report; choose another file name",
                given.display()
            ),
            TargetError::Unresolvable { given, source } => {
                write!(f, "cannot use {} as report file: {source}", given.display())
            }
        }
    }
}

impl std::error::Error for TargetError {}

/// Resolves `path` without requiring it to exist: an existing file is
/// canonicalized (following symlinks), otherwise the existing parent
/// directory is canonicalized and the file name appended.
fn resolve(path: &Path) -> io::Result<PathBuf> {
    if let Ok(real) = fs::canonicalize(path) {
        return Ok(real);
    }
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    Ok(fs::canonicalize(parent)?.join(name))
}

/// Appends `suffix` to the file name of `path`.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// All resolved paths that belong to the catalog.
fn protected_paths(catalog: &Path) -> Vec<PathBuf> {
    let mut bases = vec![catalog.to_path_buf()];
    if let Ok(resolved) = resolve(catalog) {
        bases.push(resolved);
    }
    let mut out = Vec::new();
    for base in &bases {
        out.push(resolve(base).unwrap_or_else(|_| base.clone()));
        for suffix in COMPANION_SUFFIXES {
            let companion = with_suffix(base, suffix);
            out.push(resolve(&companion).unwrap_or(companion));
        }
    }
    out
}

/// Key that identifies an existing file independently of its path.
#[cfg(unix)]
fn identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

/// Key that identifies an existing file independently of its path. File
/// identity is not available through stable std here, so the canonical path
/// is compared without regard to case.
#[cfg(not(unix))]
fn identity(path: &Path) -> Option<String> {
    fs::canonicalize(path)
        .ok()
        .map(|p| p.to_string_lossy().to_lowercase())
}

/// True if the existing file `path` is the same file as one of the existing
/// `protected` paths, even under another name.
fn same_file_as_any(path: &Path, protected: &[PathBuf]) -> bool {
    let Some(target) = identity(path) else {
        return false;
    };
    protected
        .iter()
        .any(|p| identity(p).as_ref() == Some(&target))
}

/// True if `bytes` is an earlier lr2lc report.
fn is_earlier_report(bytes: &[u8]) -> bool {
    matches!(
        serde_json::from_slice::<serde_json::Value>(bytes),
        Ok(serde_json::Value::Object(map)) if map.contains_key("tool_version")
    )
}

/// Reads `file` from its current position if it is small enough and checks
/// whether it is an earlier report.
fn file_is_earlier_report(file: &mut File) -> io::Result<bool> {
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > MAX_REPORT_BYTES {
        return Ok(false);
    }
    let mut bytes = Vec::new();
    file.take(MAX_REPORT_BYTES + 1).read_to_end(&mut bytes)?;
    Ok(bytes.len() as u64 <= MAX_REPORT_BYTES && is_earlier_report(&bytes))
}

/// Checks the `--report` target before anything is read.
pub fn check(report: &Path, catalog: &Path) -> Result<ReportTarget, TargetError> {
    let given = report.to_path_buf();
    let path = resolve(report).map_err(|source| TargetError::Unresolvable {
        given: given.clone(),
        source,
    })?;
    let protected = protected_paths(catalog);
    if protected.contains(&path) || same_file_as_any(&path, &protected) {
        return Err(TargetError::Catalog { given });
    }
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(ReportTarget::New { path }),
        Err(source) => Err(TargetError::Unresolvable { given, source }),
        Ok(_) => {
            let earlier = File::open(&path)
                .and_then(|mut f| file_is_earlier_report(&mut f))
                .unwrap_or(false);
            if earlier {
                Ok(ReportTarget::EarlierReport { path })
            } else {
                Err(TargetError::Exists { given })
            }
        }
    }
}

/// Writes `contents` to a checked target without overwriting anything that
/// is not an earlier report, even if the file changed since [`check`].
pub fn write(target: &ReportTarget, contents: &str) -> io::Result<()> {
    match target {
        ReportTarget::New { path } => {
            let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
            file.write_all(contents.as_bytes())?;
            file.sync_all()
        }
        ReportTarget::EarlierReport { path } => {
            let mut file = OpenOptions::new().read(true).write(true).open(path)?;
            if !file_is_earlier_report(&mut file)? {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "the file changed and is no longer an earlier lr2lc report",
                ));
            }
            file.set_len(0)?;
            file.seek(SeekFrom::Start(0))?;
            file.write_all(contents.as_bytes())?;
            file.sync_all()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let cat = dir.path().join("cat.lrcat");
        fs::write(&cat, b"SQLite format 3\0").unwrap();
        (dir, cat)
    }

    #[test]
    fn earlier_report_detection() {
        assert!(is_earlier_report(br#"{"tool_version": "0.1"}"#));
        assert!(is_earlier_report(br#"{"tool_version": null, "x": 1}"#));
        assert!(!is_earlier_report(br#"{"hello": 1}"#));
        assert!(!is_earlier_report(br#"["tool_version"]"#));
        assert!(!is_earlier_report(b"tool_version"));
        assert!(!is_earlier_report(b""));
    }

    #[test]
    fn new_file_is_accepted_and_resolved() {
        let (dir, cat) = setup();
        let target = check(&dir.path().join("r.json"), &cat).unwrap();
        let expected = fs::canonicalize(dir.path()).unwrap().join("r.json");
        assert_eq!(target, ReportTarget::New { path: expected });
    }

    #[test]
    fn catalog_and_companions_are_refused_even_if_missing() {
        let (_dir, cat) = setup();
        for suffix in ["", "-wal", "-journal", "-shm"] {
            let err = check(&with_suffix(&cat, suffix), &cat).unwrap_err();
            assert!(
                matches!(err, TargetError::Catalog { .. }),
                "{suffix}: {err}"
            );
            assert!(err.to_string().contains("catalog"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn catalog_given_via_symlink_protects_real_companions() {
        let (dir, cat) = setup();
        let link = dir.path().join("link.lrcat");
        std::os::unix::fs::symlink(&cat, &link).unwrap();
        let err = check(&with_suffix(&cat, "-wal"), &link).unwrap_err();
        assert!(matches!(err, TargetError::Catalog { .. }));
        let err = check(&with_suffix(&link, "-wal"), &link).unwrap_err();
        assert!(matches!(err, TargetError::Catalog { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn hard_links_to_catalog_and_companions_are_same_file() {
        let (dir, cat) = setup();
        let wal = with_suffix(&cat, "-wal");
        fs::write(&wal, b"wal").unwrap();
        let protected = protected_paths(&cat);
        for (i, original) in [&cat, &wal].into_iter().enumerate() {
            let link = dir.path().join(format!("link{i}.json"));
            fs::hard_link(original, &link).unwrap();
            assert!(same_file_as_any(&link, &protected));
            let err = check(&link, &cat).unwrap_err();
            assert!(matches!(err, TargetError::Catalog { .. }), "{err}");
        }
    }

    #[test]
    fn other_or_missing_file_is_not_same_file() {
        let (dir, cat) = setup();
        let protected = protected_paths(&cat);
        let other = dir.path().join("other.json");
        assert!(!same_file_as_any(&other, &protected));
        fs::write(&other, b"SQLite format 3\0").unwrap();
        assert!(!same_file_as_any(&other, &protected));
    }

    #[test]
    fn foreign_file_is_refused() {
        let (dir, cat) = setup();
        let other = dir.path().join("photo.jpg");
        fs::write(&other, b"\xff\xd8\xff").unwrap();
        let err = check(&other, &cat).unwrap_err();
        assert!(matches!(err, TargetError::Exists { .. }));
        assert!(err.to_string().contains("exists"));
    }

    #[test]
    fn directory_is_refused() {
        let (dir, cat) = setup();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        assert!(matches!(check(&sub, &cat), Err(TargetError::Exists { .. })));
    }

    #[test]
    fn missing_directory_is_unresolvable() {
        let (dir, cat) = setup();
        let err = check(&dir.path().join("nope").join("r.json"), &cat).unwrap_err();
        assert!(matches!(err, TargetError::Unresolvable { .. }));
    }

    #[test]
    fn earlier_report_is_overwritten() {
        let (dir, cat) = setup();
        let report = dir.path().join("r.json");
        fs::write(
            &report,
            br#"{"tool_version": "old", "padding": "xxxxxxxxxxxx"}"#,
        )
        .unwrap();
        let target = check(&report, &cat).unwrap();
        assert!(matches!(target, ReportTarget::EarlierReport { .. }));
        write(&target, "{\"tool_version\": \"new\"}\n").unwrap();
        assert_eq!(
            fs::read_to_string(&report).unwrap(),
            "{\"tool_version\": \"new\"}\n"
        );
    }

    #[test]
    fn new_target_does_not_overwrite_file_created_meanwhile() {
        let (dir, cat) = setup();
        let report = dir.path().join("r.json");
        let target = check(&report, &cat).unwrap();
        fs::write(&report, b"precious").unwrap();
        assert!(write(&target, "{}").is_err());
        assert_eq!(fs::read(&report).unwrap(), b"precious");
    }

    #[test]
    fn earlier_report_replaced_meanwhile_is_not_overwritten() {
        let (dir, cat) = setup();
        let report = dir.path().join("r.json");
        fs::write(&report, br#"{"tool_version": "old"}"#).unwrap();
        let target = check(&report, &cat).unwrap();
        fs::write(&report, b"precious").unwrap();
        assert!(write(&target, "{}").is_err());
        assert_eq!(fs::read(&report).unwrap(), b"precious");
    }
}
