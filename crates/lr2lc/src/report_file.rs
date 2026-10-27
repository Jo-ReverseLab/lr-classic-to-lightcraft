//! Safe handling of the `--report FILE` and `--script FILE` targets.
//!
//! The report must never destroy the catalog, its SQLite companion files or
//! any other user file (such as a photo). Before anything is read, the target
//! is checked:
//!
//! - it must not resolve to the catalog or to `<catalog>-wal`, `-journal` or
//!   `-shm`, also through a hard link (same device and inode on Unix) or a
//!   differently cased path (case-insensitive comparison elsewhere);
//! - if it exists, it must be an earlier lr2lc report (a JSON object with the
//!   key `tool_version`), or for `--script` an earlier lr2lc script (empty,
//!   or every non-empty line a JSON object with `"method":"engine.execute"`);
//!   anything else is left alone.
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

/// What is written to a checked target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The JSON report (`--report`).
    Report,
    /// The plan as JSON lines (`--script`).
    Script,
}

impl Kind {
    fn noun(self) -> &'static str {
        match self {
            Kind::Report => "report",
            Kind::Script => "script",
        }
    }

    /// True if `bytes` is an earlier file of this kind written by lr2lc.
    fn is_earlier(self, bytes: &[u8]) -> bool {
        match self {
            Kind::Report => is_earlier_report(bytes),
            Kind::Script => is_earlier_script(bytes),
        }
    }
}

/// A checked report target.
#[derive(Debug, PartialEq)]
pub enum ReportTarget {
    /// The file does not exist yet; `path` is the resolved location.
    New { path: PathBuf },
    /// The file is an earlier lr2lc report; `path` is the resolved location.
    EarlierReport { path: PathBuf },
}

impl ReportTarget {
    /// The resolved location.
    pub fn path(&self) -> &Path {
        match self {
            ReportTarget::New { path } | ReportTarget::EarlierReport { path } => path,
        }
    }
}

/// Why a report target is refused.
#[derive(Debug)]
pub enum TargetError {
    /// The target is the catalog or one of its companion files.
    Catalog { kind: Kind, given: PathBuf },
    /// The target exists and is not an earlier lr2lc file of its kind.
    Exists { kind: Kind, given: PathBuf },
    /// The target cannot be resolved (missing directory, no file name, ...).
    Unresolvable {
        kind: Kind,
        given: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for TargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TargetError::Catalog { kind, given } => write!(
                f,
                "refusing to write the {} to {}: it is the catalog or one of its \
                 companion files (-wal, -journal, -shm)",
                kind.noun(),
                given.display()
            ),
            TargetError::Exists { kind, given } => write!(
                f,
                "refusing to write the {} to {}: the file exists and is not an \
                 earlier lr2lc {}; choose another file name",
                kind.noun(),
                given.display(),
                kind.noun()
            ),
            TargetError::Unresolvable {
                kind,
                given,
                source,
            } => write!(
                f,
                "cannot use {} as {} file: {source}",
                given.display(),
                kind.noun()
            ),
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
        .map(|p| case_insensitive_key(&p))
}

/// Comparison key for a path on a case-insensitive file system: the same
/// file spelled with different letter case yields the same key. Compiled on
/// every platform for tests, used by `identity` outside Unix.
#[cfg(any(not(unix), test))]
fn case_insensitive_key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
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

/// True if `bytes` is an earlier lr2lc script: empty, or every non-empty
/// line a JSON object with `"method": "engine.execute"`.
fn is_earlier_script(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    text.lines().filter(|l| !l.trim().is_empty()).all(|l| {
        match serde_json::from_str::<serde_json::Value>(l) {
            Ok(serde_json::Value::Object(map)) => {
                map.get("method").and_then(|m| m.as_str()) == Some("engine.execute")
            }
            _ => false,
        }
    })
}

/// Reads `file` from its current position if it is small enough and checks
/// whether it is an earlier file of `kind`.
fn file_is_earlier(file: &mut File, kind: Kind) -> io::Result<bool> {
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > MAX_REPORT_BYTES {
        return Ok(false);
    }
    let mut bytes = Vec::new();
    file.take(MAX_REPORT_BYTES + 1).read_to_end(&mut bytes)?;
    Ok(bytes.len() as u64 <= MAX_REPORT_BYTES && kind.is_earlier(&bytes))
}

/// Checks the `--report` target before anything is read.
#[cfg(test)]
pub fn check(report: &Path, catalog: &Path) -> Result<ReportTarget, TargetError> {
    check_as(Kind::Report, report, catalog)
}

/// Checks a target of `kind` before anything is read.
pub fn check_as(kind: Kind, target: &Path, catalog: &Path) -> Result<ReportTarget, TargetError> {
    let given = target.to_path_buf();
    let path = resolve(target).map_err(|source| TargetError::Unresolvable {
        kind,
        given: given.clone(),
        source,
    })?;
    let protected = protected_paths(catalog);
    if protected.contains(&path) || same_file_as_any(&path, &protected) {
        return Err(TargetError::Catalog { kind, given });
    }
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(ReportTarget::New { path }),
        Err(source) => Err(TargetError::Unresolvable {
            kind,
            given,
            source,
        }),
        Ok(_) => {
            let earlier = File::open(&path)
                .and_then(|mut f| file_is_earlier(&mut f, kind))
                .unwrap_or(false);
            if earlier {
                Ok(ReportTarget::EarlierReport { path })
            } else {
                Err(TargetError::Exists { kind, given })
            }
        }
    }
}

/// Writes `contents` to a checked target without overwriting anything that
/// is not an earlier report, even if the file changed since [`check`].
pub fn write(target: &ReportTarget, contents: &str) -> io::Result<()> {
    write_as(Kind::Report, target, contents)
}

/// Writes `contents` to a target checked with [`check_as`] for `kind`.
pub fn write_as(kind: Kind, target: &ReportTarget, contents: &str) -> io::Result<()> {
    match target {
        ReportTarget::New { path } => {
            let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
            file.write_all(contents.as_bytes())?;
            file.sync_all()
        }
        ReportTarget::EarlierReport { path } => {
            let mut file = OpenOptions::new().read(true).write(true).open(path)?;
            if !file_is_earlier(&mut file, kind)? {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "the file changed and is no longer an earlier lr2lc {}",
                        kind.noun()
                    ),
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
    fn case_insensitive_key_ignores_letter_case() {
        assert_eq!(
            case_insensitive_key(Path::new(r"C:\Data\Cat.lrcat")),
            case_insensitive_key(Path::new(r"c:\data\cat.LRCAT"))
        );
        assert_eq!(
            case_insensitive_key(Path::new("/Data/Cat.lrcat-WAL")),
            case_insensitive_key(Path::new("/data/cat.lrcat-wal"))
        );
    }

    #[test]
    fn case_insensitive_key_keeps_different_files_apart() {
        let cat = case_insensitive_key(Path::new(r"C:\Data\Cat.lrcat"));
        for other in [
            r"C:\Data\Cat.lrcat-wal",
            r"C:\Data\Cat2.lrcat",
            r"D:\Data\Cat.lrcat",
            r"C:\Other\Cat.lrcat",
        ] {
            assert_ne!(cat, case_insensitive_key(Path::new(other)), "{other}");
        }
    }

    #[test]
    fn earlier_script_detection() {
        assert!(is_earlier_script(b""));
        assert!(is_earlier_script(b"\n\n"));
        assert!(is_earlier_script(
            b"{\"method\":\"engine.execute\"}\n\n{\"method\":\"engine.execute\",\"id\":1}\n"
        ));
        assert!(!is_earlier_script(b"{\"method\":\"other\"}\n"));
        assert!(!is_earlier_script(
            b"{\"method\":\"engine.execute\"}\nnot json\n"
        ));
        assert!(!is_earlier_script(b"[1]"));
        assert!(!is_earlier_script(b"\xff\xd8"));
    }

    #[test]
    fn script_target_refuses_the_catalog_and_mentions_it() {
        let (dir, cat) = setup();
        let err = check_as(Kind::Script, &cat, &cat).unwrap_err();
        assert!(err.to_string().contains("catalog"), "{err}");
        assert!(err.to_string().contains("script"), "{err}");
        let empty = dir.path().join("s.jsonl");
        fs::write(&empty, b"").unwrap();
        let target = check_as(Kind::Script, &empty, &cat).unwrap();
        write_as(Kind::Script, &target, "{}\n").unwrap();
        assert_eq!(fs::read(&empty).unwrap(), b"{}\n");
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
