//! Acceptance tests for colour labels in the CLI dry run (task P5b-2).
//!
//! Fixtures: `lrcat_fixtures::write_label_catalog_with_files` with the
//! variants of `lrcat_fixtures::label_edge::Fixture` (17 converted photos in
//! each; 6 of them carry a label in all variants but `Empty`).
//!
//! Option `--label-set en|de|auto|<FILE.json>` (default `auto`), given as a
//! separate argument. A file is a JSON object `{"<label name>": "<colour>"}`
//! whose values are exactly `red`, `yellow`, `green`, `blue` or `purple`.
//! Names are looked up byte for byte (case, spaces, Unicode form).
//!
//! Resolution:
//! - `auto`: if every non-empty label of the converted photos belongs
//!   exactly to the English set, or exactly to the German set, that set
//!   applies (`label_set` "en" / "de"). Otherwise (mixed, custom names, no
//!   labels) nothing is guessed: `label_set` is "none" and every non-empty
//!   label is a custom label. Labels of skipped photos are not considered.
//! - `en` / `de` / file: names not in that set are custom labels. The
//!   `label_set` is "en", "de" or "file".
//! - A custom label never skips a photo: it is converted without a label.
//!
//! report.json, area `photos`: `labels` is an object with exactly these
//! fields: integers `red`, `yellow`, `green`, `blue`, `purple`, `none`,
//! `custom` (they count converted photos only; their sum is `converted`),
//! string `label_set`, and `custom_reasons`, an array of objects
//! `{"reason": "photo <id>: custom label not in label set"}`, ascending by
//! id, one per custom photo, no names. `none` counts empty and NULL labels.
//! `photos.skipped` is not changed by custom labels.
//!
//! stdout: always exactly one line
//! `labels: red R, yellow Y, green G, blue B, purple P, none N, custom C, set S`
//! (S = en, de, file or none). If `--label-set` is auto, the set is "none"
//! and there is at least one custom label, one more line mentioning
//! `--label-set` recommends choosing a set. In all other cases (also with an
//! explicit set) the recommendation line is absent; a catalog without labels
//! gets no recommendation.
//!
//! Label text never appears in stdout, stderr or report.json.
//!
//! A bad `--label-set` (unreadable file, not JSON, not an object, unknown
//! colour value, non-string value) is a usage error: exit code 2, nothing
//! on stdout, no report file, the catalog is unchanged.
//!
//! Always: dry run (nothing written, catalog byte-identical), photo balance
//! unchanged (read 20, would convert 17, skipped 3), second run identical,
//! `catalog_unchanged` true.

use lrcat_fixtures::label_edge::{self, Fixture};
use lrcat_fixtures::write_label_catalog_with_files;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    report: Option<(Value, String)>,
    cwd_entries: usize,
}

fn run(cat: &Path, extra: &[&str]) -> Run {
    let cwd = tempfile::tempdir().unwrap();
    let report = cwd.path().join("report.json");
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd.path())
        .arg(cat)
        .args(extra)
        .arg("--report")
        .arg(&report)
        .output()
        .unwrap();
    let raw = fs::read_to_string(&report).ok();
    let entries = fs::read_dir(cwd.path()).unwrap().count();
    Run {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        report: raw.map(|r| (serde_json::from_str(&r).unwrap(), r)),
        cwd_entries: entries,
    }
}

fn ok(cat: &Path, extra: &[&str]) -> (String, Value, String) {
    let r = run(cat, extra);
    assert_eq!(r.code, Some(0), "stderr: {}", r.stderr);
    let (v, raw) = r.report.expect("report written");
    (r.stdout, v, raw)
}

fn fixture(f: Fixture) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_label_catalog_with_files(dir.path(), f).unwrap();
    (dir, cat)
}

fn write_set(dir: &Path, value: &Value) -> PathBuf {
    let p = dir.join("labels.json");
    fs::write(&p, serde_json::to_string(value).unwrap()).unwrap();
    p
}

/// Expected counts: red, yellow, green, blue, purple, none, custom.
type Counts = [i64; 7];

fn check(v: &Value, text: &str, counts: Counts, set: &str) {
    let l = &v["photos"]["labels"];
    let keys = ["red", "yellow", "green", "blue", "purple", "none", "custom"];
    for (k, want) in keys.iter().zip(counts) {
        assert_eq!(l[*k], want, "{k}: {l}");
    }
    assert_eq!(l["label_set"], set);
    assert_eq!(
        l.as_object().unwrap().len(),
        9,
        "fields: 7 counters, label_set, custom_reasons: {l}"
    );
    assert_eq!(counts.iter().sum::<i64>(), label_edge::CONVERTED);
    assert_eq!(v["photos"]["converted"], label_edge::CONVERTED);
    let want = format!(
        "labels: red {}, yellow {}, green {}, blue {}, purple {}, none {}, custom {}, set {set}",
        counts[0], counts[1], counts[2], counts[3], counts[4], counts[5], counts[6]
    );
    assert_eq!(
        text.lines().filter(|l| l.trim() == want).count(),
        1,
        "want `{want}` in stdout: {text}"
    );
    assert_eq!(
        text.lines().filter(|l| l.starts_with("labels:")).count(),
        1,
        "exactly one labels line: {text}"
    );
}

fn custom_ids(v: &Value) -> Vec<i64> {
    v["photos"]["labels"]["custom_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let s = r["reason"].as_str().unwrap();
            let id = s
                .strip_prefix("photo ")
                .and_then(|t| t.strip_suffix(": custom label not in label set"))
                .unwrap_or_else(|| panic!("bad reason: {s}"));
            assert_eq!(r.as_object().unwrap().len(), 1);
            id.parse().unwrap()
        })
        .collect()
}

fn has_recommendation(text: &str) -> bool {
    text.lines()
        .any(|l| !l.starts_with("labels:") && l.contains("--label-set"))
}

const EN: Counts = [2, 1, 1, 1, 1, 11, 0];
const ALL_STD_NAMES: [&str; 10] = [
    "Red",
    "Yellow",
    "Green",
    "Blue",
    "Purple",
    "Rot",
    "Gelb",
    "Blau",
    "Lila",
    "Gr\u{fc}n",
];

// --- auto -------------------------------------------------------------

#[test]
#[ignore = "waiting for P5b-2"]
fn auto_detects_the_english_set() {
    let (_d, cat) = fixture(Fixture::En);
    let (text, v, _) = ok(&cat, &[]);
    check(&v, &text, EN, "en");
    assert!(custom_ids(&v).is_empty());
    assert!(!has_recommendation(&text), "{text}");
}

#[test]
#[ignore = "waiting for P5b-2"]
fn auto_detects_the_german_set_with_nfc_gruen() {
    let (_d, cat) = fixture(Fixture::De);
    let (text, v, _) = ok(&cat, &[]);
    check(&v, &text, EN, "de");
    assert!(custom_ids(&v).is_empty());
    assert!(!has_recommendation(&text), "{text}");
}

#[test]
#[ignore = "waiting for P5b-2"]
fn auto_on_mixed_sets_guesses_nothing_and_recommends() {
    let (_d, cat) = fixture(Fixture::Mixed);
    let (text, v, _) = ok(&cat, &[]);
    check(&v, &text, [0, 0, 0, 0, 0, 11, 6], "none");
    assert_eq!(custom_ids(&v), label_edge::LABELLED_IDS);
    assert!(has_recommendation(&text), "{text}");
}

#[test]
#[ignore = "waiting for P5b-2"]
fn auto_on_odd_names_guesses_nothing_and_recommends() {
    // NFD "Grün", "rot", "Red " and a custom name: no set fits exactly, even
    // though "Green" and "Red" alone would match the English set.
    let (_d, cat) = fixture(Fixture::Odd);
    let (text, v, _) = ok(&cat, &[]);
    check(&v, &text, [0, 0, 0, 0, 0, 11, 6], "none");
    assert_eq!(custom_ids(&v), label_edge::LABELLED_IDS);
    assert!(has_recommendation(&text), "{text}");
}

#[test]
#[ignore = "waiting for P5b-2"]
fn auto_without_labels_has_set_none_and_no_recommendation() {
    let (_d, cat) = fixture(Fixture::Empty);
    let (text, v, _) = ok(&cat, &[]);
    check(&v, &text, [0, 0, 0, 0, 0, 17, 0], "none");
    assert!(custom_ids(&v).is_empty());
    assert!(!has_recommendation(&text), "{text}");
}

#[test]
#[ignore = "waiting for P5b-2"]
fn explicit_auto_equals_the_default() {
    let (_d, cat) = fixture(Fixture::De);
    let a = ok(&cat, &[]);
    let b = ok(&cat, &["--label-set", "auto"]);
    assert_eq!(a, b);
}

#[test]
#[ignore = "waiting for P5b-2"]
fn small_catalog_without_label_columns_values_has_zero_labels() {
    let dir = tempfile::tempdir().unwrap();
    let cat = lrcat_fixtures::write_small_catalog_with_files(dir.path()).unwrap();
    let (text, v, _) = ok(&cat, &[]);
    let converted = v["photos"]["converted"].as_i64().unwrap();
    assert_eq!(v["photos"]["labels"]["none"], converted);
    assert_eq!(v["photos"]["labels"]["custom"], 0);
    assert_eq!(v["photos"]["labels"]["label_set"], "none");
    assert!(!has_recommendation(&text));
    assert!(text.lines().any(|l| l.starts_with("labels:")));
}

// --- explicit set -----------------------------------------------------

#[test]
#[ignore = "waiting for P5b-2"]
fn explicit_en_on_en_catalog() {
    let (_d, cat) = fixture(Fixture::En);
    let (text, v, _) = ok(&cat, &["--label-set", "en"]);
    check(&v, &text, EN, "en");
}

#[test]
#[ignore = "waiting for P5b-2"]
fn explicit_de_on_de_catalog() {
    let (_d, cat) = fixture(Fixture::De);
    let (text, v, _) = ok(&cat, &["--label-set", "de"]);
    check(&v, &text, EN, "de");
}

#[test]
#[ignore = "waiting for P5b-2"]
fn wrong_explicit_set_makes_every_label_custom_but_keeps_photos() {
    let (_d, cat) = fixture(Fixture::En);
    let (text, v, _) = ok(&cat, &["--label-set", "de"]);
    check(&v, &text, [0, 0, 0, 0, 0, 11, 6], "de");
    assert_eq!(custom_ids(&v), label_edge::LABELLED_IDS);
    assert_eq!(v["photos"]["converted"], label_edge::CONVERTED);
    assert!(!has_recommendation(&text), "{text}");
}

#[test]
#[ignore = "waiting for P5b-2"]
fn explicit_en_on_mixed_catalog_maps_only_english_names() {
    // Photos 4 Red, 6 Green, 8 Purple match; 5 Rot, 7 Blau, 9 Gelb are custom.
    let (_d, cat) = fixture(Fixture::Mixed);
    let (text, v, _) = ok(&cat, &["--label-set", "en"]);
    check(&v, &text, [1, 0, 1, 0, 1, 11, 3], "en");
    assert_eq!(custom_ids(&v), [5, 7, 9]);
}

#[test]
#[ignore = "waiting for P5b-2"]
fn explicit_de_on_mixed_catalog_maps_only_german_names() {
    // Photos 5 Rot, 7 Blau, 9 Gelb match; 4, 6, 8 are custom.
    let (_d, cat) = fixture(Fixture::Mixed);
    let (text, v, _) = ok(&cat, &["--label-set", "de"]);
    check(&v, &text, [1, 1, 0, 1, 0, 11, 3], "de");
    assert_eq!(custom_ids(&v), [4, 6, 8]);
}

#[test]
#[ignore = "waiting for P5b-2"]
fn exact_matching_in_the_english_set() {
    // 4 NFD Grün, 5 "rot", 6 "Red " and 7 custom do not match. 8 "Green"
    // and 9 "Red" do.
    let (_d, cat) = fixture(Fixture::Odd);
    let (text, v, _) = ok(&cat, &["--label-set", "en"]);
    check(&v, &text, [1, 0, 1, 0, 0, 11, 4], "en");
    assert_eq!(custom_ids(&v), [4, 5, 6, 7]);
}

#[test]
#[ignore = "waiting for P5b-2"]
fn nfd_gruen_does_not_match_the_german_set() {
    let (_d, cat) = fixture(Fixture::Odd);
    let (text, v, _) = ok(&cat, &["--label-set", "de"]);
    check(&v, &text, [0, 0, 0, 0, 0, 11, 6], "de");
    assert_eq!(custom_ids(&v), label_edge::LABELLED_IDS);
}

// --- file -------------------------------------------------------------

#[test]
#[ignore = "waiting for P5b-2"]
fn file_set_maps_odd_names_exactly() {
    let (d, cat) = fixture(Fixture::Odd);
    let file = write_set(
        d.path(),
        &json!({
            "Gru\u{308}n": "blue",
            "rot": "red",
            "Red ": "green",
            label_edge::CUSTOM_NAME: "purple",
            "Green": "yellow"
        }),
    );
    let (text, v, _) = ok(&cat, &["--label-set", file.to_str().unwrap()]);
    // 4 blue, 5 red, 6 green, 7 purple, 8 yellow; 9 "Red" is not in the file.
    check(&v, &text, [1, 1, 1, 1, 1, 11, 1], "file");
    assert_eq!(custom_ids(&v), [9]);
}

#[test]
#[ignore = "waiting for P5b-2"]
fn file_set_replaces_the_built_in_meaning_of_names() {
    let (d, cat) = fixture(Fixture::En);
    let file = write_set(d.path(), &json!({"Red": "blue"}));
    let (text, v, _) = ok(&cat, &["--label-set", file.to_str().unwrap()]);
    // Photos 4 and 9 are "Red" and become blue; the other four are custom.
    check(&v, &text, [0, 0, 0, 2, 0, 11, 4], "file");
    assert_eq!(custom_ids(&v), [5, 6, 7, 8]);
}

#[test]
#[ignore = "waiting for P5b-2"]
fn empty_object_file_is_valid_and_makes_everything_custom() {
    let (d, cat) = fixture(Fixture::De);
    let file = write_set(d.path(), &json!({}));
    let (text, v, _) = ok(&cat, &["--label-set", file.to_str().unwrap()]);
    check(&v, &text, [0, 0, 0, 0, 0, 11, 6], "file");
}

#[test]
#[ignore = "waiting for P5b-2"]
fn file_set_works_for_a_german_catalog() {
    let (d, cat) = fixture(Fixture::De);
    let file = write_set(
        d.path(),
        &json!({"Rot": "red", "Gelb": "yellow", "Gr\u{fc}n": "green",
                "Blau": "blue", "Lila": "purple"}),
    );
    let (text, v, _) = ok(&cat, &["--label-set", file.to_str().unwrap()]);
    check(&v, &text, EN, "file");
}

// --- bad file ---------------------------------------------------------

fn expect_usage_error(cat: &Path, set: &str) {
    let before = fs::read(cat).unwrap();
    let r = run(cat, &["--label-set", set]);
    assert_eq!(r.code, Some(2), "stderr: {}", r.stderr);
    assert!(r.report.is_none(), "no report on a usage error");
    assert_eq!(r.cwd_entries, 0, "nothing written");
    assert!(r.stdout.trim().is_empty(), "stdout: {}", r.stdout);
    assert!(!r.stderr.trim().is_empty());
    assert!(!r.stderr.contains(label_edge::CUSTOM_NAME));
    assert_eq!(before, fs::read(cat).unwrap(), "catalog changed");
}

#[test]
#[ignore = "waiting for P5b-2"]
fn missing_file_is_a_usage_error() {
    let (d, cat) = fixture(Fixture::En);
    let p = d.path().join("nope.json");
    expect_usage_error(&cat, p.to_str().unwrap());
}

#[test]
#[ignore = "waiting for P5b-2"]
fn unknown_set_name_is_a_usage_error() {
    let (_d, cat) = fixture(Fixture::En);
    // Neither en, de, auto nor an existing file.
    expect_usage_error(&cat, "fr");
}

#[test]
#[ignore = "waiting for P5b-2"]
fn directory_instead_of_file_is_a_usage_error() {
    let (d, cat) = fixture(Fixture::En);
    expect_usage_error(&cat, d.path().to_str().unwrap());
}

#[test]
#[ignore = "waiting for P5b-2"]
fn broken_files_are_usage_errors() {
    let (d, cat) = fixture(Fixture::Odd);
    let cases: [(&str, String); 7] = [
        ("not json", "this is not json".to_string()),
        ("empty file", String::new()),
        ("truncated", "{\"Red\": \"red\"".to_string()),
        ("array", "[\"red\"]".to_string()),
        ("unknown colour", "{\"Red\": \"pink\"}".to_string()),
        ("capitalised colour", "{\"Red\": \"Red\"}".to_string()),
        ("non-string value", "{\"Red\": 1}".to_string()),
    ];
    for (what, body) in cases {
        let p = d.path().join("bad.json");
        fs::write(&p, body).unwrap();
        let before = fs::read(&cat).unwrap();
        let r = run(&cat, &["--label-set", p.to_str().unwrap()]);
        assert_eq!(r.code, Some(2), "{what}: stderr {}", r.stderr);
        assert!(r.report.is_none(), "{what}");
        assert_eq!(r.cwd_entries, 0, "{what}");
        assert!(r.stdout.trim().is_empty(), "{what}: {}", r.stdout);
        assert_eq!(before, fs::read(&cat).unwrap(), "{what}");
    }
}

#[test]
#[ignore = "waiting for P5b-2"]
fn label_set_without_a_value_is_a_usage_error() {
    let (_d, cat) = fixture(Fixture::En);
    let cwd = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd.path())
        .arg(&cat)
        .arg("--label-set")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

// --- privacy, balance, repeatability -----------------------------------

#[test]
#[ignore = "waiting for P5b-2"]
fn no_label_name_in_any_output() {
    let (d, cat) = fixture(Fixture::Odd);
    let file = write_set(d.path(), &json!({"Red": "red"}));
    let file_arg = file.to_str().unwrap();
    for extra in [
        vec![],
        vec!["--label-set", "en"],
        vec!["--label-set", "de"],
        vec!["--label-set", file_arg],
    ] {
        let r = run(&cat, &extra);
        assert_eq!(r.code, Some(0), "{extra:?}: {}", r.stderr);
        let (_, raw) = r.report.unwrap();
        for name in ["Gru\u{308}n", "Red ", label_edge::CUSTOM_NAME, "Zyx"] {
            for (what, text) in [
                ("stdout", &r.stdout),
                ("stderr", &r.stderr),
                ("report", &raw),
            ] {
                assert!(!text.contains(name), "{what} {extra:?} has {name:?}");
            }
        }
    }
    for f in [Fixture::En, Fixture::De, Fixture::Mixed] {
        let (_d, cat) = fixture(f);
        for extra in [vec![], vec!["--label-set", "en"], vec!["--label-set", "de"]] {
            let r = run(&cat, &extra);
            let (_, raw) = r.report.unwrap();
            for name in ALL_STD_NAMES {
                for (what, text) in [
                    ("stdout", &r.stdout),
                    ("stderr", &r.stderr),
                    ("report", &raw),
                ] {
                    assert!(!text.contains(name), "{what} {f:?} {extra:?} has {name}");
                }
            }
        }
    }
}

#[test]
#[ignore = "waiting for P5b-2"]
fn custom_labels_do_not_change_the_photo_balance() {
    let (_d, cat) = fixture(Fixture::Odd);
    let (text, v, _) = ok(&cat, &[]);
    let want = format!(
        "photos: read {}, would convert {}, skipped {}",
        label_edge::PHOTOS,
        label_edge::CONVERTED,
        label_edge::SKIPPED
    );
    assert!(text.lines().any(|l| l.trim() == want), "stdout: {text}");
    let p = &v["photos"];
    assert_eq!(p["read"], label_edge::PHOTOS);
    let skipped = p["skipped"].as_array().unwrap();
    assert_eq!(skipped.len() as i64, label_edge::SKIPPED);
    let mut ids: Vec<String> = skipped
        .iter()
        .map(|s| {
            s["reason"]
                .as_str()
                .unwrap()
                .split(':')
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    ids.sort();
    // Only the virtual copy and the two missing originals; the labels of
    // photos 2 and 20 are not counted and never listed as custom.
    assert_eq!(ids, ["photo 2", "photo 20", "photo 3"]);
    assert_eq!(custom_ids(&v), label_edge::LABELLED_IDS);
    let c = p["labels"]["custom"].as_i64().unwrap();
    assert_eq!(c, 6);
    assert_eq!(
        p["read"].as_i64().unwrap(),
        p["converted"].as_i64().unwrap() + skipped.len() as i64
    );
}

#[test]
#[ignore = "waiting for P5b-2"]
fn labels_of_skipped_photos_do_not_influence_auto() {
    // Fixture En has "Red" on photo 2 and "Green" on virtual copy 20; they
    // are skipped and must be ignored either way. The counts above already
    // show 2 red / 1 green. Check the totals stay the same on the German set.
    for (f, set) in [(Fixture::En, "en"), (Fixture::De, "de")] {
        let (_d, cat) = fixture(f);
        let (_, v, _) = ok(&cat, &[]);
        assert_eq!(v["photos"]["labels"]["label_set"], set);
        assert_eq!(v["photos"]["labels"]["red"], 2);
        assert_eq!(v["photos"]["labels"]["green"], 1);
    }
}

#[test]
#[ignore = "waiting for P5b-2"]
fn catalog_unchanged_second_run_identical_and_nothing_written() {
    for (f, extra) in [
        (Fixture::En, vec![]),
        (Fixture::Odd, vec![]),
        (Fixture::Odd, vec!["--label-set", "en"]),
        (Fixture::Empty, vec![]),
    ] {
        let (_d, cat) = fixture(f);
        let before = fs::read(&cat).unwrap();
        let dir = cat.parent().unwrap().to_path_buf();
        let listing = |p: &Path| {
            let mut v: Vec<_> = fs::read_dir(p)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            v.sort();
            v
        };
        let files_before = listing(&dir);
        let a = ok(&cat, &extra);
        let b = ok(&cat, &extra);
        assert_eq!(a, b, "{f:?} {extra:?}: second run differs");
        assert_eq!(a.1["catalog_unchanged"], true);
        assert_eq!(a.1["dry_run"], true);
        assert_eq!(before, fs::read(&cat).unwrap(), "{f:?}");
        assert_eq!(files_before, listing(&dir), "{f:?}");
    }
}

#[test]
#[ignore = "waiting for P5b-2"]
fn apply_is_still_refused_with_a_label_set_and_writes_nothing() {
    let (_d, cat) = fixture(Fixture::De);
    let before = fs::read(&cat).unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd.path())
        .arg(&cat)
        .args(["--label-set", "de", "--apply"])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    assert_eq!(before, fs::read(&cat).unwrap());
    assert_eq!(fs::read_dir(cwd.path()).unwrap().count(), 0);
}

#[test]
#[ignore = "waiting for P5b-2"]
fn help_mentions_label_set() {
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .arg("--help")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("--label-set"));
}
