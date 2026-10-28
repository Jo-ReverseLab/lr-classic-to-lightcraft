#![cfg(any())]
//! CLI acceptance tests for writing smart collections as smart albums (task
//! P5b-4b), with the generator fixture
//! `write_smart_translate_catalog_with_files`, against the stateful fake
//! LightCraft.
//!
//! STATUS: waiting for P5b-4b. The whole file is disabled by the crate-level
//! `cfg(any())` on the first line. The developer may ONLY remove that line.
//! Any other change to this file needs the test author.
//!
//! Translatable (see `lrcat_fixtures::smart_translate`): 4 collections, all
//! with a plain `rating` rule. Every other smart collection of the fixture is
//! skipped with a reason.
//!
//! `report.json`:
//! - `collections.smart`: smart collections that are converted (4, in the
//!   dry run and in a run with `--apply`); they count in
//!   `collections.converted`; the others are in `collections.skipped` with a
//!   reason; `read == converted + len(skipped)` in every run;
//! - `writer` of an applied run: integers `smart_created`, `smart_known`,
//!   `smart_rules_changed`, list of `{reason}` `smart_skipped` (writer-side
//!   skips only, such as a rejected `album.createSmart`);
//! - `writer.planned` of a dry run: `smart_albums` (to create),
//!   `smart_known`; stdout contains `smart albums to create <N>`;
//! - balance: `collections.converted - collections.quick ==
//!   folders_created + albums_created + collections_known + smart_created +
//!   smart_known + len(collections_skipped) + len(smart_skipped)`.
//!
//! Always: the catalog is byte-identical after every run; a dry run sends no
//! write command and writes no mapping file; a second run sends no
//! `album.createSmart`; neither stdout, stderr nor `report.json` contain a
//! collection name or a rule value text other than criterion names.

use lrcat_fixtures::smart_edge as e;
use lrcat_fixtures::smart_translate as t;
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

#[path = "support/fake_lightcraft.rs"]
mod fake_lightcraft;
use fake_lightcraft::{Config, FakeLightCraft};

struct Setup {
    _photos: tempfile::TempDir,
    work: tempfile::TempDir,
    catalog: PathBuf,
    library: PathBuf,
}

fn setup() -> Setup {
    let photos = tempfile::tempdir().unwrap();
    let catalog = lrcat_fixtures::write_smart_translate_catalog_with_files(photos.path()).unwrap();
    let work = tempfile::tempdir().unwrap();
    let library = work.path().join("Test.lightcraft");
    Setup {
        _photos: photos,
        work,
        catalog,
        library,
    }
}

struct Run {
    report: Value,
    stdout: String,
}

impl Setup {
    fn state_file(&self) -> PathBuf {
        let mut name = self.library.file_name().unwrap().to_os_string();
        name.push(".lr2lc.json");
        self.work.path().join(name)
    }

    fn run(&self, port: u16, apply: bool) -> Output {
        let port_text = port.to_string();
        let report = self.work.path().join("report.json");
        let mut args: Vec<&OsStr> = vec![
            self.catalog.as_os_str(),
            OsStr::new("--library"),
            self.library.as_os_str(),
            OsStr::new("--port"),
            OsStr::new(&port_text),
            OsStr::new("--report"),
            report.as_os_str(),
        ];
        if apply {
            args.push(OsStr::new("--apply"));
        }
        Command::new(env!("CARGO_BIN_EXE_lr2lc"))
            .current_dir(self.work.path())
            .args(args)
            .output()
            .unwrap()
    }

    fn ok(&self, port: u16, apply: bool) -> Run {
        let before = fs::read(&self.catalog).unwrap();
        let o = self.run(port, apply);
        let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
        assert_eq!(o.status.code(), Some(0), "{stderr}");
        assert_eq!(before, fs::read(&self.catalog).unwrap(), "catalog changed");
        let raw = fs::read_to_string(self.work.path().join("report.json")).unwrap();
        let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
        let all = format!("{stdout}\n{stderr}\n{raw}");
        assert!(!all.contains(e::SECRET), "name or value leaked");
        for word in ["zyxOp", "zyxCombine", "intersect", "union", "inLast"] {
            assert!(!all.contains(word), "{word} leaked");
        }
        let report: Value = serde_json::from_str(&raw).unwrap();
        let c = &report["collections"];
        assert_eq!(
            n(&c["read"]),
            n(&c["converted"]) + c["skipped"].as_array().unwrap().len()
        );
        Run { report, stdout }
    }
}

fn closed() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

fn n(v: &Value) -> usize {
    v.as_u64().unwrap_or_else(|| panic!("not a number: {v}")) as usize
}

fn assert_balance(r: &Run) {
    let w = &r.report["writer"];
    let c = &r.report["collections"];
    assert_eq!(
        n(&c["converted"]) - c["quick"].as_u64().unwrap_or(0) as usize,
        n(&w["folders_created"])
            + n(&w["albums_created"])
            + n(&w["collections_known"])
            + n(&w["smart_created"])
            + n(&w["smart_known"])
            + w["collections_skipped"].as_array().unwrap().len()
            + w["smart_skipped"].as_array().unwrap().len(),
        "{w}"
    );
}

fn create_smart_calls(fake: &FakeLightCraft, from: usize) -> Vec<Value> {
    fake.commands()[from..]
        .iter()
        .filter(|(c, _)| c == "album.createSmart")
        .map(|(_, p)| p.clone())
        .collect()
}

#[test]
fn dry_run_counts_translatable_collections_as_converted_and_writes_nothing() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let r = s.ok(fake.port(), false);
    let c = &r.report["collections"];
    assert_eq!(n(&c["smart"]), t::TRANSLATABLE.len());
    let reasons: Vec<&str> = c["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["reason"].as_str().unwrap())
        .collect();
    // 25 (P5b-4a) + 3 new smart collections - 3 that now translate.
    // Collections 101, 201, 202 and 203 are not skipped any more.
    for id in t::TRANSLATABLE {
        assert!(
            !reasons
                .iter()
                .any(|r| r.starts_with(&format!("collection {id}:"))),
            "{id} is skipped: {reasons:?}"
        );
    }
    for id in t::NEW_SKIPPED {
        assert!(
            reasons
                .iter()
                .any(|r| r.starts_with(&format!("collection {id}:"))),
            "{id} is not skipped"
        );
    }
    let skipped_smart = reasons
        .iter()
        .filter(|r| r.to_lowercase().contains("smart collection"))
        .count();
    assert_eq!(
        skipped_smart as i64 + t::TRANSLATABLE.len() as i64,
        e::SMART_TOTAL + t::ADDED
    );
    let planned = &r.report["writer"]["planned"];
    assert_eq!(n(&planned["smart_albums"]), t::TRANSLATABLE.len());
    assert_eq!(n(&planned["smart_known"]), 0);
    assert!(
        r.stdout
            .contains(&format!("smart albums to create {}", t::TRANSLATABLE.len())),
        "{}",
        r.stdout
    );
    assert!(fake.write_commands().is_empty());
    assert!(fake.albums().is_empty());
    assert!(!s.state_file().exists());
}

#[test]
fn dry_run_without_a_running_app_needs_no_connection() {
    let s = setup();
    let r = s.ok(closed(), false);
    assert_eq!(
        n(&r.report["writer"]["planned"]["smart_albums"]),
        t::TRANSLATABLE.len()
    );
    assert!(!s.state_file().exists());
}

#[test]
fn apply_creates_the_smart_albums_with_the_translated_rules() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let r = s.ok(fake.port(), true);
    assert_balance(&r);
    let w = &r.report["writer"];
    assert_eq!(n(&w["smart_created"]), t::TRANSLATABLE.len());
    assert_eq!((n(&w["smart_known"]), n(&w["smart_rules_changed"])), (0, 0));
    assert!(w["smart_skipped"].as_array().unwrap().is_empty());

    let calls = create_smart_calls(&fake, 0);
    assert_eq!(calls.len(), t::TRANSLATABLE.len());
    let mut sent: Vec<Value> = calls.iter().map(|p| p["rules"].clone()).collect();
    let mut want: Vec<Value> = t::RULES
        .iter()
        .map(|(_, text)| serde_json::from_str(text).unwrap())
        .collect();
    let key = |v: &Value| v.to_string();
    sent.sort_by_key(key);
    want.sort_by_key(key);
    assert_eq!(sent, want);

    // The one inside the collection set has that set's LightCraft id as
    // parent; the others have none.
    let with_parent: Vec<&Value> = calls.iter().filter(|p| p.get("parent").is_some()).collect();
    assert_eq!(with_parent.len(), 1);
    let parent_id = with_parent[0]["parent"].as_u64().unwrap();
    let parent = fake
        .albums()
        .into_iter()
        .find(|a| a.id == parent_id)
        .unwrap();
    assert!(parent.folder);
    assert_eq!(
        with_parent[0]["rules"],
        serde_json::from_str::<Value>(t::RULES[2].1).unwrap()
    );

    // They come after every regular album command.
    let all = fake.commands();
    let first = all
        .iter()
        .position(|(c, _)| c == "album.createSmart")
        .unwrap();
    assert!(all[first..].iter().all(|(c, _)| c == "album.createSmart"));
    assert_eq!(
        fake.albums().iter().filter(|a| a.rules.is_some()).count(),
        t::TRANSLATABLE.len()
    );

    // The mapping file records them by Lightroom id.
    let state: Value = serde_json::from_str(&fs::read_to_string(s.state_file()).unwrap()).unwrap();
    for id in t::TRANSLATABLE {
        assert_eq!(
            state["collections"][id.to_string()]["kind"],
            json!("smart"),
            "{id}"
        );
    }
}

#[test]
fn second_run_creates_no_smart_album_again() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);
    let before = fake.commands().len();
    let albums = fake.albums();
    let r = s.ok(fake.port(), true);
    assert_balance(&r);
    let w = &r.report["writer"];
    assert_eq!(n(&w["smart_created"]), 0);
    assert_eq!(n(&w["smart_known"]), t::TRANSLATABLE.len());
    assert_eq!(n(&w["collections_removed"]), 0);
    assert!(create_smart_calls(&fake, before).is_empty());
    assert_eq!(fake.albums(), albums);

    let dry = s.ok(closed(), false);
    let planned = &dry.report["writer"]["planned"];
    assert_eq!(n(&planned["smart_albums"]), 0);
    assert_eq!(n(&planned["smart_known"]), t::TRANSLATABLE.len());
}

#[test]
fn skipped_smart_collections_name_only_criteria() {
    let s = setup();
    let r = s.ok(closed(), false);
    let reasons: Vec<String> = r.report["collections"]["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["reason"].as_str().unwrap().to_string())
        .collect();
    let of = |id: i64| {
        reasons
            .iter()
            .find(|r| r.starts_with(&format!("collection {id}:")))
            .unwrap_or_else(|| panic!("{id}"))
            .clone()
    };
    assert!(of(204).contains("pick"), "{}", of(204));
    assert!(of(205).contains("rating"), "{}", of(205));
    assert!(of(206).contains("parent"), "{}", of(206));
    // Rules that never translate stay skipped with their first criterion.
    assert!(of(103).contains("labelColor"));
    assert!(of(104).contains("keywords"));
    assert!(of(106).contains("captureTime"));
    assert!(of(110).contains("zyxCriterion"));
}
