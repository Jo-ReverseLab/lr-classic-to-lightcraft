//! Path mapping for moved catalogs and photo folders (principle 6).
//!
//! A [`PathMapping`] replaces an old path prefix with a new one, for example
//! when photos moved from a Mac volume to a Linux mount or a Windows drive.
//!
//! Matching is purely string based and never touches the file system, so the
//! result is the same on every platform:
//!
//! - Prefixes match only at component boundaries: `/a/b` matches `/a/b` and
//!   `/a/b/c`, but not `/a/bc`.
//! - `/` and `\` are equivalent separators when matching.
//! - Case is significant, except for a drive letter (`C:`) at the start.
//! - A trailing separator in the old or new prefix makes no difference.
//! - If several rules match, the one with the longest old prefix wins.
//! - The rest of the path is appended to the new prefix with the new
//!   prefix's separator style: `\` if the new prefix contains `\` or starts
//!   with a drive letter, `/` otherwise. Nothing else is normalized.
//! - The rest must not climb out of the new prefix via `..`. If it would,
//!   the mapping is refused ([`PathMappings::classify`] returns
//!   [`MappedPath::Escaped`]) so no path is ever silently rewritten to a
//!   location outside the new prefix.
//!
//! Paths that are not valid UTF-8 never match.

use crate::ModelError;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The outcome of mapping one path, with enough detail to report problems.
///
/// [`PathMappings::classify`] returns this; [`PathMappings::apply`] and
/// [`PathMappings::map`] collapse it to the safe default (a mapped path, or
/// the original unchanged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MappedPath {
    /// No rule matched; the path is returned unchanged.
    Unchanged(PathBuf),
    /// A rule matched and the path was rewritten.
    Mapped(PathBuf),
    /// A rule matched but the remaining components would climb out of the new
    /// prefix via `..`. The original path is returned unchanged and the caller
    /// must report it; nothing is silently rewritten outside the new prefix.
    Escaped(PathBuf),
}

/// One rule that maps an old path prefix to a new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMapping {
    from: String,
    to: String,
}

impl PathMapping {
    /// Creates a rule mapping the prefix `from` to `to`.
    ///
    /// Returns [`ModelError::EmptyMappingPrefix`] if `from` is empty.
    /// An empty `to` is allowed; the rest of the path is then returned
    /// as a relative path.
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Result<PathMapping, ModelError> {
        let from = from.into();
        if from.is_empty() {
            return Err(ModelError::EmptyMappingPrefix);
        }
        Ok(PathMapping {
            from,
            to: to.into(),
        })
    }

    /// Parses a rule written as `OLD=NEW`, split at the first `=`.
    ///
    /// Returns [`ModelError::InvalidMappingSpec`] if there is no `=`, and
    /// [`ModelError::EmptyMappingPrefix`] if the old part is empty.
    pub fn parse(spec: &str) -> Result<PathMapping, ModelError> {
        let (from, to) = spec
            .split_once('=')
            .ok_or_else(|| ModelError::InvalidMappingSpec(spec.to_string()))?;
        PathMapping::new(from, to)
    }

    /// The old prefix as given.
    pub fn from(&self) -> &str {
        &self.from
    }

    /// The new prefix as given.
    pub fn to(&self) -> &str {
        &self.to
    }

    /// Components of the old prefix, without trailing empty components.
    fn old_components(&self) -> Vec<&str> {
        let mut parts: Vec<&str> = self.from.split(is_separator).collect();
        // Keep a leading empty component: it marks an absolute path.
        while parts.len() > 1 && parts.last() == Some(&"") {
            parts.pop();
        }
        parts
    }

    /// If `path` starts with the old prefix, returns the number of matched
    /// components and the remaining components.
    fn matches<'p>(&self, path: &'p str) -> Option<(usize, Vec<&'p str>)> {
        let prefix = self.old_components();
        let parts: Vec<&str> = path.split(is_separator).collect();
        if parts.len() < prefix.len() {
            return None;
        }
        for (i, (want, have)) in prefix.iter().zip(&parts).enumerate() {
            let equal = if i == 0 && is_drive(want) && is_drive(have) {
                want.eq_ignore_ascii_case(have)
            } else {
                want == have
            };
            if !equal {
                return None;
            }
        }
        let mut rest = parts[prefix.len()..].to_vec();
        // A trailing separator on the path itself adds nothing to the rest.
        if rest.iter().all(|p| p.is_empty()) {
            rest.clear();
        }
        Some((prefix.len(), rest))
    }

    /// Builds the new path from the remaining components.
    fn join(&self, rest: &[&str]) -> String {
        let sep = if self.to.contains('\\') || starts_with_drive(&self.to) {
            '\\'
        } else {
            '/'
        };
        let mut out = trim_trailing_separators(&self.to).to_string();
        if rest.is_empty() {
            return out;
        }
        if !out.is_empty() && !out.ends_with(is_separator) {
            out.push(sep);
        }
        out.push_str(&rest.join(&sep.to_string()));
        out
    }
}

/// An ordered set of [`PathMapping`] rules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathMappings {
    rules: Vec<PathMapping>,
}

impl PathMappings {
    /// Creates a set of rules. The order does not matter: the rule with the
    /// longest matching old prefix wins. Among rules with the same old prefix
    /// (for example `/a` and `/a/`), the first one wins.
    pub fn new(rules: Vec<PathMapping>) -> PathMappings {
        PathMappings { rules }
    }

    /// The rules in the order given.
    pub fn rules(&self) -> &[PathMapping] {
        &self.rules
    }

    /// Maps `path` with the best matching rule.
    ///
    /// Returns `None` if no rule matches, `path` is not valid UTF-8, or the
    /// mapping would climb out of the new prefix via `..` (use
    /// [`classify`](Self::classify) to tell those apart). In all `None` cases
    /// [`map`](Self::map) leaves the path unchanged.
    pub fn apply(&self, path: &Path) -> Option<PathBuf> {
        match self.classify(path) {
            MappedPath::Mapped(p) => Some(p),
            _ => None,
        }
    }

    /// Classifies `path` into [`MappedPath`]: unchanged, mapped, or escaped
    /// (the rest would climb out of the new prefix via `..`).
    pub fn classify(&self, path: &Path) -> MappedPath {
        let path_str = match path.to_str() {
            Some(s) => s,
            None => return MappedPath::Unchanged(path.to_path_buf()),
        };
        let mut best: Option<(usize, &PathMapping, Vec<&str>)> = None;
        for rule in &self.rules {
            if let Some((len, rest)) = rule.matches(path_str)
                && best.as_ref().is_none_or(|(b, _, _)| len > *b)
            {
                best = Some((len, rule, rest));
            }
        }
        match best {
            None => MappedPath::Unchanged(path.to_path_buf()),
            Some((_, rule, rest)) => {
                if escapes_prefix(rule.to(), &rest) {
                    MappedPath::Escaped(path.to_path_buf())
                } else {
                    MappedPath::Mapped(PathBuf::from(rule.join(&rest)))
                }
            }
        }
    }

    /// Like [`apply`](Self::apply), but returns `path` unchanged if no rule
    /// matches or the mapping would escape.
    pub fn map(&self, path: &Path) -> PathBuf {
        self.apply(path).unwrap_or_else(|| path.to_path_buf())
    }

    /// Returns the indices of paths in `paths` that, after mapping, collide
    /// with an earlier path. Escaped and unchanged paths count by their
    /// effective (unchanged) path. The result is ascending, so a binary
    /// search tells whether index `i` is a collision. Used to report silent
    /// merges instead of letting two photos land on the same path.
    pub fn collisions(&self, paths: &[&Path]) -> Vec<usize> {
        let mut seen: HashSet<PathBuf> = HashSet::new();
        let mut dup = Vec::new();
        for (i, p) in paths.iter().enumerate() {
            let effective = self.map(p);
            if !seen.insert(effective) {
                dup.push(i);
            }
        }
        dup
    }
}

fn is_separator(c: char) -> bool {
    c == '/' || c == '\\'
}

/// Number of real components in `s`: non-empty parts that are not `.`.
fn component_depth(s: &str) -> usize {
    s.split(is_separator)
        .filter(|p| !p.is_empty() && *p != ".")
        .count()
}

/// True if appending `rest` to the new prefix `to` would climb above `to`
/// via `..`. `..` may cancel a component added within the rest (or descend
/// back to `to`), but the first `..` that would pop to `to`'s parent or above
/// is an escape and is refused.
fn escapes_prefix(to: &str, rest: &[&str]) -> bool {
    let root_depth = component_depth(to);
    let mut depth = root_depth;
    for part in rest {
        if *part == ".." {
            if depth <= root_depth {
                return true;
            }
            depth -= 1;
        } else if !part.is_empty() && *part != "." {
            depth += 1;
        }
    }
    false
}

/// True for a bare drive like `C:`.
fn is_drive(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// True if `s` starts with a drive letter and a colon.
fn starts_with_drive(s: &str) -> bool {
    s.get(..2).is_some_and(is_drive)
}

/// Removes trailing separators, but keeps a root (`/`) or a drive root
/// (`C:\`) intact.
fn trim_trailing_separators(s: &str) -> &str {
    let trimmed = s.trim_end_matches(is_separator);
    if trimmed.len() < s.len() && (trimmed.is_empty() || is_drive(trimmed)) {
        // Keep exactly one separator after the root.
        &s[..trimmed.len() + 1]
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(from: &str, to: &str, path: &str) -> Option<String> {
        let m = PathMappings::new(vec![PathMapping::new(from, to).unwrap()]);
        m.apply(Path::new(path))
            .map(|p| p.to_str().unwrap().to_string())
    }

    #[test]
    fn root_prefix_matches_absolute_paths() {
        assert_eq!(
            apply("/", "/mnt", "/a/b.jpg").as_deref(),
            Some("/mnt/a/b.jpg")
        );
    }

    #[test]
    fn root_target_keeps_single_separator() {
        assert_eq!(apply("/old", "/", "/old/a.jpg").as_deref(), Some("/a.jpg"));
        assert_eq!(apply("/old", "/", "/old").as_deref(), Some("/"));
    }

    #[test]
    fn drive_root_target_keeps_separator() {
        assert_eq!(apply("/old", "D:\\", "/old").as_deref(), Some("D:\\"));
        assert_eq!(
            apply("/old", "D:\\", "/old/a.jpg").as_deref(),
            Some("D:\\a.jpg")
        );
    }

    #[test]
    fn relative_path_does_not_match_absolute_prefix() {
        assert_eq!(apply("/data", "/mnt", "data/a.jpg"), None);
    }

    #[test]
    fn trailing_separator_on_path_is_dropped() {
        assert_eq!(apply("/data", "/mnt", "/data/").as_deref(), Some("/mnt"));
    }

    #[test]
    fn empty_target_yields_relative_rest() {
        assert_eq!(
            apply("/data", "", "/data/a/b.jpg").as_deref(),
            Some("a/b.jpg")
        );
    }

    #[test]
    fn drive_letter_only_ignored_at_start() {
        assert_eq!(apply("/x/c:", "/y", "/x/C:/a"), None);
    }

    #[test]
    fn parse_error_without_equals_names_spec() {
        let err = PathMapping::parse("/only").unwrap_err();
        assert_eq!(err, ModelError::InvalidMappingSpec("/only".into()));
        assert!(err.to_string().contains("/only"));
    }

    #[test]
    fn parse_empty_old_part_is_empty_prefix() {
        assert_eq!(
            PathMapping::parse("=/new").unwrap_err(),
            ModelError::EmptyMappingPrefix
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_path_does_not_match() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let m = PathMappings::new(vec![PathMapping::new("/data", "/mnt").unwrap()]);
        let p = Path::new(OsStr::from_bytes(b"/data/\xff.jpg"));
        assert_eq!(m.apply(p), None);
        assert_eq!(m.map(p), p.to_path_buf());
    }
}
