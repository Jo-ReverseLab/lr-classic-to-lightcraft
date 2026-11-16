//! Acceptance tests for machine-readable skip codes.
//!
//! Background: the evaluation of a run must not depend on free
//! texts. Every skipped (or incomplete) entry in `report.json` gets a stable
//! snake_case `code` next to the unchanged free text `reason`.
//!
//! Public API
//! ----------
//! - Every entry of a `skipped` array has `{"reason": <unchanged text>,
//!   "code": <code>}`. This holds for the areas `folders`, `photos`,
//!   `collections`, `collection_members`, `stacks`, `stack_members`,
//!   `keywords`, `keyword_assignments`, `keyword_synonyms` (key `skipped`
//!   inside the area object) and for every `writer` list ending in
//!   `_skipped` or `_incomplete` (`photos_skipped`, `photos_incomplete`,
//!   `keywords_not_created`, `members_skipped`, `collections_skipped`,
//!   `collections_incomplete`, `stacks_skipped`, `stacks_incomplete`,
//!   `quick_skipped`, `quick_incomplete`, `smart_skipped`).
//! - Each area object gets `by_code`: an object `{code: count}` with one key
//!   per code that occurs; the counts add up to `len(skipped)`. An area
//!   without skips has `"by_code": {}`. Each writer list `<name>` gets a
//!   sibling `<name>_by_code` (e.g. `writer.photos_skipped_by_code`) with the
//!   same rule. Counts are integers.
//! - The top level gets `skipped_by_code`: `{code: count}` summed over the
//!   nine catalog areas above (not over `writer`).
//! - `photos.videos` equals `photos.by_code.video` (0 when absent). The CLI
//!   counts videos by code, not by reason text.
//! - Text summary (stdout): after the existing `skipped <area>:` blocks, for
//!   each area with skips a block `skip codes <area>:` (area names as in the
//!   existing text, e.g. `photos`, `collection members`) with one line per
//!   code `  {count:>6} x {code}`, sorted by count (descending), then code.
//!   The existing `skipped <area>:` lines (grouped free text) are unchanged.
//! - A code is a fixed snake_case key (`[a-z][a-z0-9_]*`). It never contains
//!   a value from the catalog (no ids, names, paths, creation ids). The free
//!   text `reason` is unchanged, so existing tests stay green.
//! - The balance `read == converted + len(skipped)` is unchanged.
//!
//! Code table: code -> meaning -> today's free text (ids shown as N)
//! -----------------------------------------------------------------
//! Reader: folders
//!   no_root_folder        folder has no root folder row id   "folder N: has no root folder"
//!   root_folder_not_found root folder row missing            "folder N: root folder R not found in catalog"
//!   root_folder_no_path   root folder without path           "folder N: root folder has no path"
//! Reader: photos
//!   virtual_copy          virtual copy                       "photo N: virtual copy of photo M is not converted"
//!   video                 video file                         "photo N: video files are not supported by LightCraft"
//!   missing               original file not on disk          "photo N: original file is missing" (also "... (cannot check it: KIND)")
//!   missing_global_id     no Lightroom global id             "photo N: missing global id"
//!   duplicate_global_id   global id used by an earlier photo "photo N: duplicate global id"
//!   file_row_not_found    file row missing                   "photo N: file row not found in catalog"
//!   folder_row_not_found  folder row missing                 "photo N: folder row not found in catalog"
//!   root_folder_not_found root folder row missing            "photo N: root folder row not found in catalog"
//!   root_folder_no_path   root folder without path           "photo N: root folder has no path"
//!   invalid_rating        rating not an integer 0..=5        "photo N: rating is not a whole number in 0..=5"
//! Reader: collections
//!   collection_kind_unsupported creation id is neither plain, set, smart nor quick
//!                                                            "collection N: collection type \"X\" is not converted"
//!   collection_kind_missing no creation id                   "collection N: collection type is missing"
//!   quick_duplicate       second and later quick collection  "collection N: more than one quick collection; not converted"
//!   smart_unreadable      smart rules cannot be read         "collection N: smart collection is not converted (unreadable rules)"
//!   smart_no_criteria     smart collection without criteria  "... (no criteria)"
//!   smart_untranslatable  rules without exact LightCraft equivalent
//!                                                            "... (first criterion: X)" / "... (criterion \"X\" has no exact LightCraft equivalent)" / "... (the combine mode has no exact LightCraft equivalent)"
//!   name_invalid          name is not valid text             "collection N: name is not valid text"
//!   parent_invalid        parent id not valid                "collection N: parent is not a valid id"
//!   parent_missing        parent row missing                 "collection N: parent P is missing"
//!   parent_skipped        parent is skipped                  "collection N: parent P is skipped"
//!   parent_not_set        parent is an album, not a set      "collection N: parent P is not a collection set"
//!                         (writer, smart collection)         "smart collection N: its parent collection P is not a converted collection set; not converted"
//!   parent_cycle          parent chain is a cycle            "collection N: parent chain forms a cycle"
//! Reader: collection_members
//!   collection_id_invalid collection id not valid            "photo N: collection id is not valid"
//!   collection_photo_invalid photo id not valid              "collection C: photo id is not valid"
//!   collection_skipped    collection itself is skipped       "photo N: collection skipped (collection C)"
//!   collection_unknown    collection row missing             "photo N: collection C is unknown"
//!   photo_not_converted   photo skipped or unknown           "photo N: skipped or unknown photo, not added to collection C"
//!   duplicate_member      same photo twice                   "photo N: duplicate member of collection C"
//!   collection_is_set     member of a collection set         "photo N: collection C is a collection set and holds no photos"
//! Reader: stacks
//!   too_few_photos        fewer than two converted photos    "stack N: fewer than two converted photos"
//! Reader: stack_members
//!   stack_unknown         stack row missing                  "photo N: stack S is unknown"
//!   stack_id_invalid      stack id not valid                 "photo N: stack id is not valid"
//!   ids_invalid           row has no valid ids               "stack member row R: ids are not valid"
//!   photo_invalid         photo id not valid                 "stack member row R: photo id is not valid (stack S)"
//!   photo_not_converted   photo skipped or unknown           "photo N: skipped or unknown photo, not added to stack S"
//!   already_in_stack      photo already in another stack     "photo N: already in stack S"
//!   duplicate_member      same photo twice                   "photo N: duplicate member of stack S"
//!   stack_skipped         the stack is skipped               "photo N: stack S is skipped"
//! Reader: keywords
//!   name_empty            empty name                         "keyword N: name is empty"
//!   name_has_bar          name contains `|`                  "keyword N: name contains a vertical bar"
//!   missing_global_id     no global id                       "keyword N: missing global id"
//!   duplicate_global_id   global id used twice               "keyword N: duplicate global id"
//!   parent_skipped        parent keyword is skipped          "keyword N: parent keyword P is skipped"
//! Reader: keyword_assignments
//!   assignment_photo_skipped   photo skipped or unknown      "assignment N: photo P is skipped or unknown"
//!   assignment_keyword_skipped keyword skipped               "assignment N: keyword K is skipped"
//! Reader: keyword_synonyms
//!   name_empty            empty synonym                      "synonym N: name is empty"
//!   keyword_skipped       its keyword is skipped             "synonym N: keyword K is skipped"
//!   keyword_unknown       its keyword does not exist         "synonym N: keyword K is unknown"
//! Writer (plan and apply)
//!   keyword_changed       keyword attributes changed after creation, not updated
//!                                                            "keyword synonyms or attributes changed after the keyword was created in LightCraft; not updated"
//!   duplicate_global_id   two photos share a global id       "photo N has the same global id as another photo"
//!   missing_global_id     photo without global id            "photo N has no global id"
//!   path_not_utf8         path is not valid UTF-8            "photo N has a file path that is not valid UTF-8"
//!   too_few_photos        stack with < 2 photos              "stack S: fewer than two photos with a global id; not grouped" / "... with a LightCraft id; not grouped"
//!   no_global_id          member photo has no global id      "photo N: not added to an album; the photo is not converted or has no global id" / "photo N: no global id; not added to the quick collection"
//!   no_lightcraft_id      member photo has no LightCraft id  "photo N: not added to an album; the photo has no LightCraft id" / "photo N: no LightCraft id; not added to the quick collection" / "a photo without a LightCraft id was not added to the quick collection"
//!   ambiguous             several matching albums            "collection N: ambiguous: K albums ..." / "quick collection: ambiguous, the library has K top-level albums ..."
//!   parent_not_created    parent album was not created       "collection N: its parent collection P was not created" (also smart)
//!   parent_failed         parent keyword was not created     "keyword not created: `keyword.create` failed for its parent keyword"
//!   no_id_returned        command returned no id             "collection N: `album.create` returned no id", "`library.import` returned no id for the file", smart likewise
//!   applied_not_saved     applied but not saved, no id       "... was applied but not saved and returned no id; ..."
//!   import_failed         import reported the file failed    "photo N: `library.import` reported the file as failed"
//!   rejected              LightCraft rejected a command      "... LightCraft rejected `CMD` ..." (skipped and incomplete lists)
//!
//! Codes that need fault injection or a damaged catalog (no generator
//! fixture can reach them) are listed in `NOT_FIXTURE_REACHABLE`. Every
//! other code must occur at least once over the fixtures used below.

use lrcat_fixtures as fx;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fake_lightcraft::{AlbumFailure, Config, Failure, FakeLightCraft};
use lc_test_support::fake_lightcraft;

/// The catalog areas: (json key, name in the text summary).
const AREAS: [(&str, &str); 9] = [
    ("folders", "folders"),
    ("photos", "photos"),
    ("collections", "collections"),
    ("collection_members", "collection members"),
    ("stacks", "stacks"),
    ("stack_members", "stack members"),
    ("keywords", "keywords"),
    ("keyword_assignments", "keyword assignments"),
    ("keyword_synonyms", "keyword synonyms"),
];

const WRITER_LISTS: [&str; 11] = [
    "photos_skipped",
    "photos_incomplete",
    "keywords_not_created",
    "members_skipped",
    "collections_skipped",
    "collections_incomplete",
    "stacks_skipped",
    "stacks_incomplete",
    "quick_skipped",
    "quick_incomplete",
    "smart_skipped",
];

/// Every code (see the table in the header).
const CODES: [&str; 54] = [
    "no_root_folder",
    "root_folder_not_found",
    "root_folder_no_path",
    "virtual_copy",
    "video",
    "missing",
    "missing_global_id",
    "duplicate_global_id",
    "file_row_not_found",
    "folder_row_not_found",
    "invalid_rating",
    "collection_kind_unsupported",
    "collection_kind_missing",
    "quick_duplicate",
    "smart_unreadable",
    "smart_no_criteria",
    "smart_untranslatable",
    "name_invalid",
    "parent_invalid",
    "parent_missing",
    "parent_skipped",
    "parent_not_set",
    "parent_cycle",
    "collection_id_invalid",
    "collection_photo_invalid",
    "collection_skipped",
    "collection_unknown",
    "photo_not_converted",
    "duplicate_member",
    "collection_is_set",
    "too_few_photos",
    "stack_unknown",
    "stack_id_invalid",
    "ids_invalid",
    "photo_invalid",
    "already_in_stack",
    "stack_skipped",
    "name_empty",
    "name_has_bar",
    "assignment_photo_skipped",
    "assignment_keyword_skipped",
    "keyword_skipped",
    "keyword_unknown",
    "keyword_changed",
    "path_not_utf8",
    "no_global_id",
    "no_lightcraft_id",
    "ambiguous",
    "parent_not_created",
    "parent_failed",
    "no_id_returned",
    "applied_not_saved",
    "import_failed",
    "rejected",
];

/// Codes no generator fixture reaches: they need a damaged catalog (rows
/// that Lightroom itself never writes) or an inconsistent LightCraft state.
/// `no_id_returned` and `applied_not_saved` are reached through the fake's
/// `albums_no_id`, `import_no_id` and `not_saved_creates` switches (runs
/// "apply, no ids" and "apply, not saved"). `ambiguous` stays listed here:
/// it needs seeded duplicate albums named like a fixture collection and a
/// lost mapping file; `lc-writer/tests/apply_skip_codes.rs` covers it with
/// the code asserted.
const NOT_FIXTURE_REACHABLE: [&str; 22] = [
    "duplicate_global_id",
    "no_global_id",
    "no_root_folder",
    "root_folder_not_found",
    "root_folder_no_path",
    "missing_global_id",
    "file_row_not_found",
    "folder_row_not_found",
    "invalid_rating",
    "collection_kind_missing",
    "name_invalid",
    "parent_invalid",
    "collection_id_invalid",
    "collection_photo_invalid",
    "stack_id_invalid",
    "ids_invalid",
    "photo_invalid",
    "keyword_changed",
    "path_not_utf8",
    "ambiguous",
    "stack_unknown",
    "collection_unknown",
];

/// The code that today's free text of a skip entry must get. `None` means
/// the text is unknown: add it to the table above first.
fn expected_code(reason: &str) -> Option<&'static str> {
    let has = |s: &str| reason.contains(s);
    // Writer texts first; they may embed reader-like words.
    let table: [(&str, &str); 15] = [
        ("is not a converted collection set", "parent_not_set"),
        ("LightCraft rejected", "rejected"),
        ("applied but not saved", "applied_not_saved"),
        ("returned no id", "no_id_returned"),
        ("reported the file as failed", "import_failed"),
        ("was not created", "parent_not_created"),
        ("failed for its parent keyword", "parent_failed"),
        ("ambiguous", "ambiguous"),
        ("synonyms or attributes changed", "keyword_changed"),
        ("same global id as another photo", "duplicate_global_id"),
        ("not valid UTF-8", "path_not_utf8"),
        (
            "not added to an album; the photo has no LightCraft id",
            "no_lightcraft_id",
        ),
        (
            "not added to an album; the photo is not converted",
            "no_global_id",
        ),
        (
            "no global id; not added to the quick collection",
            "no_global_id",
        ),
        (
            "LightCraft id; not added to the quick collection",
            "no_lightcraft_id",
        ),
    ];
    for (needle, code) in table {
        if has(needle) {
            return Some(code);
        }
    }
    if has("a photo without a LightCraft id") {
        return Some("no_lightcraft_id");
    }
    if has("fewer than two") {
        return Some("too_few_photos");
    }
    if has(" has no global id") {
        return Some("missing_global_id");
    }
    let table: [(&str, &str); 38] = [
        ("has no root folder", "no_root_folder"),
        ("root folder has no path", "root_folder_no_path"),
        ("not found in catalog", "ROOT_OR_ROW"),
        ("virtual copy of", "virtual_copy"),
        ("video files are not supported", "video"),
        ("original file is missing", "missing"),
        ("missing global id", "missing_global_id"),
        ("duplicate global id", "duplicate_global_id"),
        ("rating is not a whole number", "invalid_rating"),
        ("collection type is missing", "collection_kind_missing"),
        ("is not converted", "KIND_OR_SMART"),
        ("more than one quick collection", "quick_duplicate"),
        ("name is not valid text", "name_invalid"),
        ("parent is not a valid id", "parent_invalid"),
        ("is not a collection set", "parent_not_set"),
        ("parent chain forms a cycle", "parent_cycle"),
        ("parent keyword", "parent_skipped"),
        ("is missing", "parent_missing"),
        ("collection id is not valid", "collection_id_invalid"),
        ("photo id is not valid (stack", "photo_invalid"),
        ("photo id is not valid", "collection_photo_invalid"),
        ("collection skipped", "collection_skipped"),
        ("holds no photos", "collection_is_set"),
        ("not added to collection", "photo_not_converted"),
        ("not added to stack", "photo_not_converted"),
        ("already in stack", "already_in_stack"),
        ("duplicate member", "duplicate_member"),
        ("stack id is not valid", "stack_id_invalid"),
        ("ids are not valid", "ids_invalid"),
        ("name is empty", "name_empty"),
        ("name contains a vertical bar", "name_has_bar"),
        ("is skipped or unknown", "assignment_photo_skipped"),
        ("synonym", "SYNONYM"),
        ("assignment", "assignment_keyword_skipped"),
        ("stack", "STACK"),
        ("parent", "parent_skipped"),
        ("is unknown", "UNKNOWN"),
        ("is skipped", "SKIPPED"),
    ];
    for (needle, code) in table {
        if has(needle) {
            return Some(resolve(reason, code));
        }
    }
    None
}

/// Resolves the entries of the table above that depend on more context.
fn resolve(reason: &str, code: &'static str) -> &'static str {
    let tail = |s: &str| reason.contains(s);
    match code {
        "ROOT_OR_ROW" => {
            if tail("file row") {
                "file_row_not_found"
            } else if tail("folder row not found") && !tail("root folder row") {
                "folder_row_not_found"
            } else {
                "root_folder_not_found"
            }
        }
        "KIND_OR_SMART" => {
            if tail("unreadable rules") {
                "smart_unreadable"
            } else if tail("no criteria") {
                "smart_no_criteria"
            } else if tail("smart collection") {
                "smart_untranslatable"
            } else {
                "collection_kind_unsupported"
            }
        }
        "SYNONYM" => {
            if tail("is unknown") {
                "keyword_unknown"
            } else {
                "keyword_skipped"
            }
        }
        "STACK" => {
            if tail("is unknown") {
                "stack_unknown"
            } else {
                "stack_skipped"
            }
        }
        "UNKNOWN" => {
            if tail("collection") {
                "collection_unknown"
            } else {
                "stack_unknown"
            }
        }
        "SKIPPED" => "stack_skipped",
        other => other,
    }
}

// ---------------------------------------------------------------------
// Running the CLI
// ---------------------------------------------------------------------

struct Run {
    report: Value,
    stdout: String,
}

fn dry_run(catalog: &Path) -> Run {
    let cwd = tempfile::tempdir().unwrap();
    let before = fs::read(catalog).unwrap();
    let report = cwd.path().join("report.json");
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd.path())
        .arg(catalog)
        .arg("--anonymize")
        .arg("--report")
        .arg(&report)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(before, fs::read(catalog).unwrap(), "catalog changed");
    // Without --apply nothing but the report file is written.
    let names: Vec<_> = fs::read_dir(cwd.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![OsStr::new("report.json")]);
    Run {
        report: serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
    }
}

/// A run against the fake LightCraft; with `apply` it writes.
fn fake_run(catalog: &Path, apply: bool, tweak: impl FnOnce(&mut Config)) -> Run {
    let work = tempfile::tempdir().unwrap();
    let library = work.path().join("Test.lightcraft");
    let mut config = Config::new(&library);
    tweak(&mut config);
    let fake = FakeLightCraft::start(config);
    let before = fs::read(catalog).unwrap();
    let report = work.path().join("report.json");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lr2lc"));
    cmd.current_dir(work.path())
        .arg(catalog)
        .arg("--library")
        .arg(&library)
        .arg("--port")
        .arg(fake.port().to_string())
        .arg("--anonymize")
        .arg("--report")
        .arg(&report);
    if apply {
        cmd.arg("--apply");
    }
    let out = cmd.output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(before, fs::read(catalog).unwrap(), "catalog changed");
    Run {
        report: serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
    }
}

type Make = fn(&Path) -> PathBuf;

/// Fixtures with skips of many kinds.
fn fixtures() -> Vec<(&'static str, Make)> {
    vec![
        (
            "small",
            (|d| fx::write_small_catalog_with_files(d).unwrap()) as Make,
        ),
        (
            "keyword_edge",
            (|d| fx::write_keyword_edge_catalog_with_files(d).unwrap()) as Make,
        ),
        (
            "keyword_attr",
            (|d| fx::write_keyword_attr_catalog_with_files(d).unwrap()) as Make,
        ),
        (
            "video",
            (|d| fx::write_video_catalog_with_files(d).unwrap()) as Make,
        ),
        (
            "collection",
            (|d| fx::write_collection_catalog_with_files(d).unwrap()) as Make,
        ),
        (
            "quick",
            (|d| fx::write_quick_catalog_with_files(d).unwrap()) as Make,
        ),
        (
            "smart",
            (|d| fx::write_smart_catalog_with_files(d).unwrap()) as Make,
        ),
        (
            "smart_translate",
            (|d| fx::write_smart_translate_catalog_with_files(d).unwrap()) as Make,
        ),
        (
            "stack",
            (|d| fx::write_stack_catalog_with_files(d).unwrap()) as Make,
        ),
    ]
}

/// All runs: a dry run per fixture, a dry run and an apply run against the
/// fake LightCraft per fixture, and apply runs with injected failures.
fn all_runs() -> &'static [(String, Run)] {
    static RUNS: std::sync::OnceLock<Vec<(String, Run)>> = std::sync::OnceLock::new();
    RUNS.get_or_init(build_runs)
}

fn build_runs() -> Vec<(String, Run)> {
    let mut runs = Vec::new();
    for (name, make) in fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let cat = make(dir.path());
        runs.push((format!("{name} dry"), dry_run(&cat)));
        runs.push((format!("{name} plan"), fake_run(&cat, false, |_| {})));
        runs.push((format!("{name} apply"), fake_run(&cat, true, |_| {})));
        runs.push((
            format!("{name} apply, rejecting"),
            fake_run(&cat, true, |c| {
                for command in ["keyword.create", "photo.setMeta", "stack.group"] {
                    c.failures.push(Failure {
                        command: command.to_string(),
                        photo_id: None,
                        message: "no".to_string(),
                    });
                }
                for command in ["album.create", "album.createSmart", "album.addPhotos"] {
                    c.album_failures.push(AlbumFailure {
                        command: command.to_string(),
                        album_name: None,
                        message: "no".to_string(),
                    });
                }
                c.import_failed_for = Some("a".to_string());
            }),
        ));
        // Commands that apply but answer without an id, or answer `saved in
        // memory but not written to disk` (no_id_returned, applied_not_saved).
        runs.push((
            format!("{name} apply, no ids"),
            fake_run(&cat, true, |c| {
                c.albums_no_id = true;
                c.import_no_id = true;
            }),
        ));
        runs.push((
            format!("{name} apply, not saved"),
            fake_run(&cat, true, |c| c.not_saved_creates = true),
        ));
    }
    runs
}

/// Every skip list of a report: (label, entries, by_code object).
fn lists(report: &Value) -> Vec<(String, Vec<Value>, Value)> {
    let mut out = Vec::new();
    for (key, _) in AREAS {
        let a = &report[key];
        out.push((
            key.to_string(),
            a["skipped"].as_array().unwrap().clone(),
            a["by_code"].clone(),
        ));
    }
    if let Some(w) = report.get("writer") {
        for name in WRITER_LISTS {
            if let Some(list) = w.get(name) {
                out.push((
                    format!("writer.{name}"),
                    list.as_array().unwrap().clone(),
                    w[format!("{name}_by_code")].clone(),
                ));
            }
        }
    }
    out
}

fn counts(entries: &[Value]) -> BTreeMap<String, u64> {
    let mut m = BTreeMap::new();
    for e in entries {
        *m.entry(e["code"].as_str().unwrap_or("<none>").to_string())
            .or_default() += 1;
    }
    m
}

fn is_snake_case(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[test]
fn registry_is_consistent() {
    let set: BTreeSet<_> = CODES.iter().collect();
    assert_eq!(set.len(), CODES.len(), "duplicate code in CODES");
    for c in CODES {
        assert!(is_snake_case(c), "{c}");
    }
    for c in NOT_FIXTURE_REACHABLE {
        assert!(set.contains(&c), "{c} is not a code");
    }
}

#[test]
fn every_skip_entry_has_the_code_of_its_reason() {
    let mut checked = 0;
    for (label, run) in all_runs().iter() {
        for (list, entries, _) in lists(&run.report) {
            for e in entries {
                let reason = e["reason"].as_str().unwrap();
                let want = expected_code(reason)
                    .unwrap_or_else(|| panic!("{label} {list}: unknown reason text {reason:?}"));
                assert_eq!(e["code"], want, "{label} {list}: {reason:?}");
                assert!(CODES.contains(&want));
                checked += 1;
            }
        }
    }
    assert!(checked > 100, "only {checked} entries checked");
}

#[test]
fn by_code_counts_equal_the_skip_entries_in_every_list() {
    for (label, run) in all_runs().iter() {
        for (list, entries, by_code) in lists(&run.report) {
            let obj = by_code
                .as_object()
                .unwrap_or_else(|| panic!("{label} {list}: by_code is missing"));
            let got: BTreeMap<String, u64> = obj
                .iter()
                .map(|(k, v)| (k.clone(), v.as_u64().unwrap()))
                .collect();
            assert_eq!(got, counts(&entries), "{label} {list}");
            assert!(got.values().all(|&n| n > 0), "{label} {list}: zero count");
            assert_eq!(
                got.values().sum::<u64>() as usize,
                entries.len(),
                "{label} {list}"
            );
        }
    }
}

#[test]
fn the_balance_read_converted_skipped_still_holds() {
    for (label, run) in all_runs().iter() {
        for (key, _) in AREAS {
            let a = &run.report[key];
            assert_eq!(
                a["read"].as_u64().unwrap(),
                a["converted"].as_u64().unwrap() + a["skipped"].as_array().unwrap().len() as u64,
                "{label} {key}"
            );
        }
    }
}

#[test]
fn top_level_skipped_by_code_sums_the_catalog_areas() {
    for (label, run) in all_runs().iter() {
        let mut want: BTreeMap<String, u64> = BTreeMap::new();
        for (key, _) in AREAS {
            for e in run.report[key]["skipped"].as_array().unwrap() {
                *want
                    .entry(e["code"].as_str().unwrap().to_string())
                    .or_default() += 1;
            }
        }
        let got: BTreeMap<String, u64> = run.report["skipped_by_code"]
            .as_object()
            .unwrap_or_else(|| panic!("{label}: skipped_by_code is missing"))
            .iter()
            .map(|(k, v)| (k.clone(), v.as_u64().unwrap()))
            .collect();
        assert_eq!(got, want, "{label}");
    }
}

#[test]
fn videos_are_counted_by_code() {
    let dir = tempfile::tempdir().unwrap();
    let cat = fx::write_video_catalog_with_files(dir.path()).unwrap();
    let r = dry_run(&cat).report;
    assert_eq!(r["photos"]["by_code"]["video"], 3);
    assert_eq!(r["photos"]["videos"], 3);
    // Only the virtual copy 21 of video 4 keeps the virtual-copy code; the
    // copy 20 of photo 3 converts with its master (T03) and is dropped as a
    // missing original.
    assert_eq!(r["photos"]["by_code"]["virtual_copy"], 1);
    assert_eq!(r["photos"]["by_code"]["missing"], 2);
    assert_eq!(r["skipped_by_code"]["video"], 3);
    // Every other fixture: videos == by_code.video.
    for (label, run) in all_runs().iter() {
        let by = run.report["photos"]["by_code"]["video"]
            .as_u64()
            .unwrap_or(0);
        assert_eq!(
            run.report["photos"]["videos"].as_u64().unwrap(),
            by,
            "{label}"
        );
    }
}

#[test]
fn empty_areas_have_an_empty_by_code_object() {
    let dir = tempfile::tempdir().unwrap();
    let cat = fx::write_small_catalog_with_files(dir.path()).unwrap();
    let r = dry_run(&cat).report;
    for (key, _) in AREAS {
        let a = &r[key];
        if a["skipped"].as_array().unwrap().is_empty() {
            assert_eq!(a["by_code"], serde_json::json!({}), "{key}");
        }
    }
}

#[test]
fn the_text_summary_counts_each_code() {
    for (label, run) in all_runs().iter() {
        for (key, text_name) in AREAS {
            let entries = run.report[key]["skipped"].as_array().unwrap();
            let heading = format!("skip codes {text_name}:");
            if entries.is_empty() {
                assert!(
                    !run.stdout.lines().any(|l| l == heading),
                    "{label}: {heading} for an area without skips"
                );
                continue;
            }
            let mut want: Vec<(u64, String)> =
                counts(entries).into_iter().map(|(c, n)| (n, c)).collect();
            want.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            let lines: Vec<&str> = run.stdout.lines().collect();
            let at = lines
                .iter()
                .position(|l| *l == heading)
                .unwrap_or_else(|| panic!("{label}: no {heading:?} in\n{}", run.stdout));
            for (i, (n, c)) in want.iter().enumerate() {
                assert_eq!(
                    lines[at + 1 + i],
                    format!("  {n:>6} x {c}"),
                    "{label} {key}"
                );
            }
        }
    }
}

#[test]
fn the_grouped_reason_text_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let cat = fx::write_video_catalog_with_files(dir.path()).unwrap();
    let r = dry_run(&cat);
    assert!(
        r.stdout
            .contains("     3 x video files are not supported by LightCraft"),
        "{}",
        r.stdout
    );
    assert!(r.stdout.contains("videos: skipped 3"));
}

#[test]
fn codes_are_snake_case_and_contain_no_catalog_values() {
    for (label, run) in all_runs().iter() {
        for (list, entries, _) in lists(&run.report) {
            for e in entries {
                let code = e["code"].as_str().unwrap();
                assert!(is_snake_case(code), "{label} {list}: {code}");
                assert!(
                    CODES.contains(&code),
                    "{label} {list}: {code} not in registry"
                );
            }
        }
    }
}

#[test]
fn fixtures_reach_every_code_that_can_be_reached() {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (_, run) in all_runs().iter() {
        for (_, entries, _) in lists(&run.report) {
            for e in entries {
                // By the free text, so the fixtures are checked independently
                // of the implementation.
                let reason = e["reason"].as_str().unwrap();
                let code = expected_code(reason)
                    .unwrap_or_else(|| panic!("unknown reason text {reason:?}"));
                seen.insert(code.to_string());
            }
        }
    }
    let missing: Vec<&str> = CODES
        .iter()
        .copied()
        .filter(|c| !NOT_FIXTURE_REACHABLE.contains(c) && !seen.contains(*c))
        .collect();
    assert!(missing.is_empty(), "no fixture reaches: {missing:?}");
    // Keep the list honest: a code a fixture reaches is not listed.
    let stale: Vec<&str> = NOT_FIXTURE_REACHABLE
        .iter()
        .copied()
        .filter(|c| seen.contains(*c))
        .collect();
    assert!(
        stale.is_empty(),
        "reachable, remove from the list: {stale:?}"
    );
}

#[test]
fn a_second_run_gives_the_same_codes() {
    for (name, make) in fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let cat = make(dir.path());
        let a = dry_run(&cat).report["skipped_by_code"].clone();
        let b = dry_run(&cat).report["skipped_by_code"].clone();
        assert!(a.is_object(), "{name}");
        assert_eq!(a, b, "{name}");
    }
}
