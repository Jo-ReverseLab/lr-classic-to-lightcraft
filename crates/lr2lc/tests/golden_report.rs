//! Golden-file acceptance test (task T19, deliverable 2): a stored expected
//! output for the full feature catalog, compared in a test.
//!
//! The golden files live in `crates/lr2lc/tests/golden/`:
//! - `report.json` — the exact `report.json` of a dry run on the full catalog.
//! - `summary.txt` — the readable stdout summary of the same dry run.
//!
//! Golden files contain only generated data (no hand-written counts). They are
//! regenerated with one command:
//! ```text
//! UPDATE_GOLDEN=1 cargo test -p lr2lc golden
//! ```
//! The school workspace has no Rust toolchain, so the golden files are not
//! pre-committed: the first run bootstraps them (writes them and passes). A
//! maintainer with a toolchain runs the command above once and commits
//! `crates/lr2lc/tests/golden/`; from then on the test compares and fails on
//! any drift — including a renamed key. The output is deterministic (it holds
//! only counts, Lightroom ids and the tool/catalog version, never temp paths),
//! so a committed golden stays stable until the code, the fixture or the
//! version changes.
//!
//! A dry run (no `--apply`) is used so the golden is independent of the fake
//! LightCraft and of any library state: `writer` is the `planned` section.
//!
//! Windows: the golden files are committed with `\n` line endings. Before
//! comparison both sides are normalised with `\r\n` -> `\n`, so the test
//! passes on Windows as on Unix (see [`normalize`]). Path separators are not
//! a concern: the report and summary contain no file paths at all (Principle
//! 9 — only field names and Lightroom ids, never paths), so there is nothing
//! platform-specific to normalise beyond line endings. The golden files are
//! written with `std::fs::write`, which writes the bytes as given (`\n`).

use lrcat_fixtures::write_full_catalog_with_files;
use std::ffi::OsStr;
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};

/// The golden files are committed next to this test, under `tests/golden/`.
fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
}

/// Names that must never appear in stdout or report.json.
const SECRET_NAMES: [&str; 4] = [
    "Bad|Name",
    "Sonnenuntergang",
    "Example Creator",
    "ZyxSecret",
];

struct Setup {
    photos: tempfile::TempDir,
    work: tempfile::TempDir,
    catalog: PathBuf,
    library: PathBuf,
}

impl Setup {
    fn report(&self) -> PathBuf {
        self.work.path().join("report.json")
    }

    /// `lr2lc <catalog> --library <lib> --port <port> --report <file>`.
    fn run_dry(&self) -> Output {
        let port = closed_port();
        let port_text = port.to_string();
        let report = self.report();
        let args: [&OsStr; 7] = [
            self.catalog.as_os_str(),
            OsStr::new("--library"),
            self.library.as_os_str(),
            OsStr::new("--port"),
            OsStr::new(&port_text),
            OsStr::new("--report"),
            report.as_os_str(),
        ];
        Command::new(env!("CARGO_BIN_EXE_lr2lc"))
            .current_dir(self.work.path())
            .args(args)
            .output()
            .unwrap()
    }

    fn report_text(&self) -> String {
        fs::read_to_string(self.report()).unwrap()
    }
}

fn setup() -> Setup {
    let photos = tempfile::tempdir().unwrap();
    let catalog = write_full_catalog_with_files(photos.path()).unwrap();
    let work = tempfile::tempdir().unwrap();
    let library = work.path().join("Full.lightcraft");
    Setup {
        photos,
        work,
        catalog,
        library,
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn assert_exit(o: &Output, code: i32) {
    assert_eq!(
        o.status.code(),
        Some(code),
        "stdout: {}\nstderr: {}",
        stdout(o),
        stderr(o)
    );
}

fn closed_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

/// Normalise Windows `\r\n` to `\n` so the comparison is platform-neutral.
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn no_secrets(text: &str) {
    for needle in SECRET_NAMES {
        assert!(!text.contains(needle), "secret leaked: {needle}");
    }
}

/// A readable, line-based diff around the first divergence of `expected` and
/// `actual`. Not a full unified diff (no LCS), but enough to point a human at
/// the changed key.
fn readable_diff(expected: &str, actual: &str) -> String {
    let exp: Vec<&str> = expected.lines().collect();
    let act: Vec<&str> = actual.lines().collect();
    let first = exp
        .iter()
        .zip(act.iter())
        .position(|(e, a)| e != a)
        .unwrap_or_else(|| exp.len().min(act.len()));
    let ctx = 3;
    let start = first.saturating_sub(ctx);
    let end = (first + ctx + 1).min(exp.len().max(act.len()));
    let mut out = String::new();
    for i in start..end {
        let e = exp.get(i);
        let a = act.get(i);
        match (e, a) {
            (Some(e), Some(a)) if e == a => {
                out.push_str(&format!("  {i:>4}: {e}\n"));
            }
            (Some(e), Some(a)) => {
                out.push_str(&format!("- {i:>4}: {e}\n"));
                out.push_str(&format!("+ {i:>4}: {a}\n"));
            }
            (Some(e), None) => out.push_str(&format!("- {i:>4}: {e}\n")),
            (None, Some(a)) => out.push_str(&format!("+ {i:>4}: {a}\n")),
            (None, None) => {}
        }
    }
    if exp.len() != act.len() {
        out.push_str(&format!(
            "({} expected lines, {} actual lines)\n",
            exp.len(),
            act.len()
        ));
    }
    out
}

/// Run the dry run once and return `(report_text, summary_text)`.
fn dry_output(s: &Setup) -> (String, String) {
    // Sanity: the catalog lives inside the photos dir (which must stay alive
    // for the reader to detect the missing original).
    assert!(s.catalog.starts_with(s.photos.path()));
    let o = s.run_dry();
    assert_exit(&o, 0);
    let summary = stdout(&o);
    assert!(summary.contains("dry run"), "not a dry run: {summary}");
    let report = s.report_text();
    no_secrets(&summary);
    no_secrets(&report);
    (report, summary)
}

fn write_golden(report: &str, summary: &str) {
    let dir = golden_dir();
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("report.json"), normalize(report)).unwrap();
    fs::write(dir.join("summary.txt"), normalize(summary)).unwrap();
}

/// Assert that `obj` has exactly the expected keys (no more, no less).
fn assert_keys(obj: &serde_json::Value, area: &str, keys: &[&str]) {
    let got: Vec<&str> = obj
        .as_object()
        .unwrap_or_else(|| panic!("{area} is not an object: {obj}"))
        .keys()
        .map(String::as_str)
        .collect();
    let mut got = got;
    got.sort_unstable();
    let mut want: Vec<&str> = keys.to_vec();
    want.sort_unstable();
    assert_eq!(got, want, "{area}: key set drifted");
}

/// The four keys of a plain `AreaJson`.
const AREA_KEYS: &[&str] = &["read", "converted", "skipped", "by_code"];

/// `AreaJson` flattened into `CollectionsJson`, plus the collection sub-counts.
const COLLECTION_KEYS: &[&str] = &[
    "read",
    "converted",
    "skipped",
    "by_code",
    "albums",
    "folders",
    "duplicate_names",
    "quick",
    "smart",
];

/// `AreaJson` flattened into `StacksJson`, plus the stack sub-counts.
const STACK_KEYS: &[&str] = &[
    "read",
    "converted",
    "skipped",
    "by_code",
    "collapsed",
    "no_top",
];

/// `AreaJson` flattened into `KeywordsJson`, plus the keyword sub-counts.
const KEYWORD_KEYS: &[&str] = &[
    "read",
    "converted",
    "skipped",
    "by_code",
    "persons",
    "not_exported",
    "no_parents_on_export",
    "no_synonyms_on_export",
    "unknown_type",
    "unused",
    "trimmed",
];

const PHOTO_AREA_KEYS: &[&str] = &[
    "read",
    "converted",
    "skipped",
    "by_code",
    "videos",
    "flags",
    "labels",
    "captions",
    "copyrights",
    "creators",
    "titles",
    "titles_unreadable",
];

const FLAG_KEYS: &[&str] = &["pick", "reject", "none", "unknown"];

const LABEL_KEYS: &[&str] = &[
    "red",
    "yellow",
    "green",
    "blue",
    "purple",
    "none",
    "custom",
    "label_set",
    "matched_sets",
    "detected_set",
    "custom_reasons",
];

const PLANNED_KEYS: &[&str] = &[
    "keywords",
    "photos",
    "updates",
    "photos_updated",
    "photos_unchanged",
    "folders",
    "albums",
    "members_added",
    "members_removed",
    "collections_known",
    "collections_removed",
    "stacks",
    "stacks_known",
    "stack_members_removed",
    "quick_add",
    "quick_already",
    "quick_removed",
    "smart_albums",
    "smart_known",
    "smart_rules_changed",
    "smart_no_longer_translatable",
];

const PLANNED_SECTION_KEYS: &[&str] = &[
    "planned",
    "photos_skipped",
    "photos_skipped_by_code",
    "keywords_not_created",
    "keywords_not_created_by_code",
    "members_skipped",
    "members_skipped_by_code",
    "stacks_skipped",
    "stacks_skipped_by_code",
    "quick_skipped",
    "quick_skipped_by_code",
    "smart_skipped",
    "smart_skipped_by_code",
];

const TOP_LEVEL_KEYS: &[&str] = &[
    "tool_version",
    "catalog_version",
    "dry_run",
    "catalog_unchanged",
    "folders",
    "photos",
    "collections",
    "collection_members",
    "stacks",
    "stack_members",
    "keywords",
    "keyword_assignments",
    "keyword_synonyms",
    "skipped_by_code",
    "ratings",
    "develop",
    "writer",
];

/// The top-level `develop` section: counts and unmapped field names only.
const DEVELOP_KEYS: &[&str] = &[
    "photos",
    "planned",
    "written",
    "unmapped",
    "unmapped_count",
    "unmapped_photos",
    "not_applied",
    "overwritten",
    "camera_profiles",
    "camera_profiles_count",
    "photos_with_profile",
    "masks",
    "spots",
    "white_balance_custom",
    "white_balance_to_check",
    "white_balance_written",
    "white_balance_not_carried",
];

#[test]
fn golden_report_matches() {
    let s = setup();
    let (report, summary) = dry_output(&s);

    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        write_golden(&report, &summary);
        eprintln!("golden files written to {}", golden_dir().display());
        return;
    }

    let dir = golden_dir();
    let report_path = dir.join("report.json");
    let summary_path = dir.join("summary.txt");
    if !report_path.exists() || !summary_path.exists() {
        // No Rust toolchain in the school workspace, so the golden files are
        // not pre-committed. Bootstrap them on first run and pass; a
        // maintainer commits them to switch on drift detection. Until then,
        // `report_keys_match_the_documented_schema` below guards renames.
        write_golden(&report, &summary);
        eprintln!(
            "golden files were absent; wrote them to {}. \
             Commit them (tests/golden/) to enable drift detection.",
            dir.display()
        );
        return;
    }

    let expected_report = normalize(&fs::read_to_string(&report_path).unwrap());
    let expected_summary = normalize(&fs::read_to_string(&summary_path).unwrap());
    let actual_report = normalize(&report);
    let actual_summary = normalize(&summary);

    if expected_report != actual_report {
        panic!(
            "report.json differs from the golden file ({})\n\
             first divergence:\n{}\n\
             To update the golden files, run:\n    \
             UPDATE_GOLDEN=1 cargo test -p lr2lc golden",
            report_path.display(),
            readable_diff(&expected_report, &actual_report)
        );
    }
    if expected_summary != actual_summary {
        panic!(
            "summary.txt differs from the golden file ({})\n\
             first divergence:\n{}\n\
             To update the golden files, run:\n    \
             UPDATE_GOLDEN=1 cargo test -p lr2lc golden",
            summary_path.display(),
            readable_diff(&expected_summary, &actual_summary)
        );
    }
}

/// Structural guard that needs no pre-committed golden: the report of a dry
/// run on the full catalog must carry exactly the documented keys. This fails
/// the moment a key is renamed, added or removed — on every platform, even
/// before the golden files are committed.
#[test]
fn report_keys_match_the_documented_schema() {
    let s = setup();
    let (report, _) = dry_output(&s);
    let v: serde_json::Value = serde_json::from_str(&report).unwrap();

    assert_keys(&v, "top level", TOP_LEVEL_KEYS);
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["catalog_unchanged"], true);

    for area in [
        "folders",
        "collection_members",
        "stack_members",
        "keyword_assignments",
        "keyword_synonyms",
    ] {
        assert_keys(&v[area], area, AREA_KEYS);
    }
    assert_keys(&v["collections"], "collections", COLLECTION_KEYS);
    assert_keys(&v["stacks"], "stacks", STACK_KEYS);
    assert_keys(&v["keywords"], "keywords", KEYWORD_KEYS);
    assert_keys(&v["photos"], "photos", PHOTO_AREA_KEYS);
    assert_keys(&v["photos"]["flags"], "photos.flags", FLAG_KEYS);
    assert_keys(&v["photos"]["labels"], "photos.labels", LABEL_KEYS);
    assert_keys(&v["develop"], "develop", DEVELOP_KEYS);

    // The writer section of a dry run is the planned section.
    assert_keys(&v["writer"], "writer", PLANNED_SECTION_KEYS);
    assert_keys(&v["writer"]["planned"], "writer.planned", PLANNED_KEYS);

    // Every area balances even in the golden catalog.
    for area in [
        "folders",
        "photos",
        "collections",
        "collection_members",
        "stacks",
        "stack_members",
        "keywords",
        "keyword_assignments",
        "keyword_synonyms",
    ] {
        let a = &v[area];
        assert_eq!(
            a["read"].as_u64().unwrap(),
            a["converted"].as_u64().unwrap() + a["skipped"].as_array().unwrap().len() as u64,
            "{area}: read != converted + skipped"
        );
    }
}
