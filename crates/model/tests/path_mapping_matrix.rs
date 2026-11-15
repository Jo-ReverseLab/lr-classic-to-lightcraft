//! Table-driven matrix for path mapping (principle 6: "runs everywhere").
//!
//! Every case is pure: no file system is touched. The cases pin the documented
//! rule in `crates/model/src/mapping.rs` and the two defects fixed alongside it
//! (`..` must not climb out of the new prefix; two photos must not silently
//! merge onto the same path). See the README section "Path mapping".

use model::{MappedPath, PathMapping, PathMappings};
use std::path::Path;

/// One single-rule case for the matrix.
struct Case {
    name: &'static str,
    from: &'static str,
    to: &'static str,
    input: &'static str,
    expected: Option<&'static str>,
}

fn one(from: &str, to: &str) -> PathMappings {
    PathMappings::new(vec![PathMapping::new(from, to).unwrap()])
}

fn apply(m: &PathMappings, input: &str) -> Option<String> {
    m.apply(Path::new(input))
        .map(|p| p.to_str().unwrap().to_string())
}

const CASES: &[Case] = &[
    // --- prefix boundary: /a/b must not rewrite /a/bc/x ---
    Case {
        name: "prefix-boundary-no-match",
        from: "/a/b",
        to: "/x",
        input: "/a/bc/d.jpg",
        expected: None,
    },
    Case {
        name: "prefix-boundary-match",
        from: "/a/b",
        to: "/x",
        input: "/a/b/d.jpg",
        expected: Some("/x/d.jpg"),
    },
    // --- trailing separators on either side make no difference ---
    Case {
        name: "trailing-sep-on-old",
        from: "/data/",
        to: "/mnt",
        input: "/data/a.jpg",
        expected: Some("/mnt/a.jpg"),
    },
    Case {
        name: "trailing-sep-on-new",
        from: "/data",
        to: "/mnt/",
        input: "/data/a.jpg",
        expected: Some("/mnt/a.jpg"),
    },
    Case {
        name: "trailing-sep-on-path",
        from: "/data",
        to: "/mnt",
        input: "/data/",
        expected: Some("/mnt"),
    },
    // --- Windows drive letters and case ---
    Case {
        name: "drive-letter-match",
        from: "C:\\data",
        to: "D:\\photos",
        input: "C:\\data\\x.jpg",
        expected: Some("D:\\photos\\x.jpg"),
    },
    Case {
        name: "drive-letter-case-insensitive",
        from: "C:\\data",
        to: "D:\\photos",
        input: "c:\\data\\x.jpg",
        expected: Some("D:\\photos\\x.jpg"),
    },
    Case {
        name: "folder-name-case-sensitive",
        from: "C:\\Data",
        to: "D:\\photos",
        input: "C:\\data\\x.jpg",
        expected: None,
    },
    // --- UNC paths while running on Linux ---
    Case {
        name: "unc-path",
        from: "\\\\server\\share",
        to: "/mnt",
        input: "\\\\server\\share\\x.jpg",
        expected: Some("/mnt/x.jpg"),
    },
    // --- mixed separators ---
    Case {
        name: "mixed-separators",
        from: "/data",
        to: "/mnt",
        input: "/data\\sub/x.jpg",
        expected: Some("/mnt/sub/x.jpg"),
    },
    // --- non-ASCII: composed and decomposed stay different (no normalization) ---
    Case {
        name: "composed-acute-preserved",
        from: "/data",
        to: "/mnt",
        input: "/data/café.jpg",
        expected: Some("/mnt/café.jpg"),
    },
    Case {
        name: "decomposed-acute-preserved",
        from: "/data",
        to: "/mnt",
        input: "/data/cafe\u{0301}.jpg",
        expected: Some("/mnt/cafe\u{0301}.jpg"),
    },
    // --- empty NEW yields a relative rest ---
    Case {
        name: "empty-new-yields-relative",
        from: "/data",
        to: "",
        input: "/data/a/b.jpg",
        expected: Some("a/b.jpg"),
    },
    // --- relative paths ---
    Case {
        name: "relative-prefix-matches-relative",
        from: "data",
        to: "/mnt",
        input: "data/a.jpg",
        expected: Some("/mnt/a.jpg"),
    },
    Case {
        name: "relative-prefix-no-match-absolute",
        from: "data",
        to: "/mnt",
        input: "/data/a.jpg",
        expected: None,
    },
    // --- `..` that climbs out of the new prefix is refused ---
    Case {
        name: "dotdot-escape-refused",
        from: "/data",
        to: "/mnt",
        input: "/data/../secret/x.jpg",
        expected: None,
    },
    // --- `..` that stays within the new prefix is allowed (no normalization) ---
    Case {
        name: "dotdot-within-prefix-allowed",
        from: "/data",
        to: "/mnt",
        input: "/data/sub/../x.jpg",
        expected: Some("/mnt/sub/../x.jpg"),
    },
];

#[test]
fn matrix_single_rule() {
    for c in CASES {
        let m = one(c.from, c.to);
        let got = apply(&m, c.input);
        assert_eq!(got.as_deref(), c.expected, "case `{}`", c.name);
    }
}

#[test]
fn composed_and_decomposed_stay_different() {
    // No Unicode normalization: the two forms map to two different paths.
    let composed = apply(&one("/data", "/mnt"), "/data/café.jpg").unwrap();
    let decomposed = apply(&one("/data", "/mnt"), "/data/cafe\u{0301}.jpg").unwrap();
    assert_ne!(composed, decomposed);
}

/// Several `--map` options: the longest matching old prefix wins.
#[test]
fn longest_old_prefix_wins() {
    let m = PathMappings::new(vec![
        PathMapping::new("/a", "/root").unwrap(),
        PathMapping::new("/a/b", "/deep").unwrap(),
    ]);
    assert_eq!(apply(&m, "/a/b/x.jpg").as_deref(), Some("/deep/x.jpg"));
    // The shorter rule still applies where the longer does not.
    assert_eq!(apply(&m, "/a/c.jpg").as_deref(), Some("/root/c.jpg"));
}

/// Among rules with the same old-prefix length (for example `/a` and `/a/`),
/// the first one given wins.
#[test]
fn tie_breaks_to_first_rule() {
    let m = PathMappings::new(vec![
        PathMapping::new("/a", "/first").unwrap(),
        PathMapping::new("/a/", "/second").unwrap(),
    ]);
    assert_eq!(apply(&m, "/a/x.jpg").as_deref(), Some("/first/x.jpg"));
}

/// `classify` tells the three outcomes apart, which `apply` collapses.
#[test]
fn classify_distinguishes_unchanged_mapped_and_escaped() {
    let m = one("/data", "/mnt");
    // No match → unchanged, original path back.
    assert_eq!(
        m.classify(Path::new("/other/x.jpg")),
        MappedPath::Unchanged(Path::new("/other/x.jpg").to_path_buf())
    );
    // Match → mapped.
    assert_eq!(
        m.classify(Path::new("/data/a.jpg")),
        MappedPath::Mapped(Path::new("/mnt/a.jpg").to_path_buf())
    );
    // `..` climbs out of the new prefix → escaped, original back.
    assert_eq!(
        m.classify(Path::new("/data/../secret/x.jpg")),
        MappedPath::Escaped(Path::new("/data/../secret/x.jpg").to_path_buf())
    );
}

/// A `..` that cancels a component added within the rest is allowed; a `..`
/// that would pop to the new prefix's parent or above is refused.
#[test]
fn dotdot_boundary_at_prefix_root() {
    let m = one("/data", "/mnt/photos");
    // rest `["sub", "..", "x.jpg"]`: `..` cancels `sub`, stays at the prefix — allowed.
    assert_eq!(
        apply(&m, "/data/sub/../x.jpg").as_deref(),
        Some("/mnt/photos/sub/../x.jpg")
    );
    // rest `["..", "x.jpg"]` pops to the prefix's parent `/mnt` — refused.
    assert!(apply(&m, "/data/../x.jpg").is_none());
    assert!(matches!(
        m.classify(Path::new("/data/../x.jpg")),
        MappedPath::Escaped(_)
    ));
    // rest `["..", "..", "x.jpg"]` climbs further above — refused.
    assert!(apply(&m, "/data/../../x.jpg").is_none());
}

/// An empty new prefix has depth 0, so any leading `..` in the rest escapes.
#[test]
fn empty_new_prefix_escapes_on_leading_dotdot() {
    let m = one("/data", "");
    assert!(apply(&m, "/data/../x.jpg").is_none());
    assert!(matches!(
        m.classify(Path::new("/data/../x.jpg")),
        MappedPath::Escaped(_)
    ));
    // A non-escaping relative rest is still fine.
    assert_eq!(apply(&m, "/data/a.jpg").as_deref(), Some("a.jpg"));
}

/// Two different paths that map to the same target collide; the later index is
/// reported. Nothing is merged silently.
#[test]
fn collisions_reports_later_duplicate() {
    let m = PathMappings::new(vec![
        PathMapping::new("/a/b", "/x").unwrap(),
        PathMapping::new("/a/c", "/x").unwrap(),
    ]);
    let paths: Vec<&Path> = [
        Path::new("/a/b/p.jpg"),
        Path::new("/a/c/p.jpg"), // maps to /x/p.jpg — collision with index 0
        Path::new("/a/b/q.jpg"), // maps to /x/q.jpg — no collision
    ]
    .into_iter()
    .collect();
    assert_eq!(m.collisions(&paths), vec![1]);
}

/// Unmapped paths can collide too (two identical original paths).
#[test]
fn collisions_sees_unmapped_duplicates() {
    let m = PathMappings::new(vec![]);
    let paths: Vec<&Path> = [Path::new("/same.jpg"), Path::new("/same.jpg")]
        .into_iter()
        .collect();
    assert_eq!(m.collisions(&paths), vec![1]);
}
