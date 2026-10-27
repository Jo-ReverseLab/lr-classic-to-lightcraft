//! CLI acceptance tests for writing collections as albums and album folders
//! (task P5b-3b), with the generator fixture `collection_edge`, against the
//! stateful fake LightCraft.
//!
//! STATUS: waiting for P5b-3b. The whole file is disabled by the crate-level
//! `cfg(any())` on the first line. The developer may ONLY remove that line.
//! Any other change to this file needs the test author.
//!
//! `report.json`, section `writer`:
//! - applied run: integers `folders_created`, `albums_created`,
//!   `collections_known`, `members_added`, `members_removed`,
//!   `collections_renamed`, `collections_moved`, `collections_removed`; lists
//!   of `{reason}`: `collections_skipped`, `collections_incomplete`,
//!   `members_skipped`;
//! - dry run: `writer.planned` has `folders`, `albums`, `members_added`,
//!   `members_removed`, `collections_known`;
//! - balance: `collections.converted == folders_created + albums_created +
//!   collections_known + len(collections_skipped)` in every run.
//!
//! stdout of a dry run: the `writer:` line ends with
//! `, albums to create A, album members to add M` (A counts albums, not
//! folders).
//!
//! Always: the catalog is byte-identical after every run, a dry run writes
//! nothing and sends no command, a second run sends no `album.*` command and
//! no `library.select`, and no collection name (converted or skipped) appears
//! in stdout, stderr or `report.json`.

use lrcat_fixtures::collection_edge as e;
use lrcat_fixtures::write_collection_catalog_with_files;
use serde_json::Value;
use std::collections::BTreeMap;
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
    let catalog = write_collection_catalog_with_files(photos.path()).unwrap();
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

    /// A run that must succeed; checks the catalog and the names.
    fn ok(&self, port: u16, apply: bool) -> Run {
        let before = fs::read(&self.catalog).unwrap();
        let o = self.run(port, apply);
        let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
        assert_eq!(o.status.code(), Some(0), "{stderr}");
        assert_eq!(before, fs::read(&self.catalog).unwrap(), "catalog changed");
        let raw = fs::read_to_string(self.work.path().join("report.json")).unwrap();
        let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
        let all = format!("{stdout}\n{stderr}\n{raw}");
        for (_, name, _) in e::NAMES {
            // "Empty" is an ordinary word of the readable summary.
            if name != "Empty" {
                assert!(!all.contains(name), "name {name:?} leaked");
            }
        }
        for name in e::SKIPPED_NAMES {
            assert!(!all.contains(name), "name {name:?} leaked");
        }
        Run {
            report: serde_json::from_str(&raw).unwrap(),
            stdout,
        }
    }

    fn state(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.state_file()).unwrap()).unwrap()
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
    assert_eq!(
        n(&r.report["collections"]["converted"]),
        n(&w["folders_created"])
            + n(&w["albums_created"])
            + n(&w["collections_known"])
            + w["collections_skipped"].as_array().unwrap().len(),
        "{w}"
    );
}

fn album_commands(fake: &FakeLightCraft, from: usize) -> usize {
    fake.commands()[from..]
        .iter()
        .filter(|(c, _)| c.starts_with("album."))
        .count()
}

#[test]
fn apply_creates_the_tree_and_the_numbers_add_up() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let r = s.ok(fake.port(), true);
    assert_balance(&r);
    let w = &r.report["writer"];
    assert_eq!(n(&w["folders_created"]), e::FOLDERS as usize);
    assert_eq!(n(&w["albums_created"]), e::ALBUMS as usize);
    assert_eq!(n(&w["collections_known"]), 0);
    assert!(w["collections_skipped"].as_array().unwrap().is_empty());
    assert!(w["collections_incomplete"].as_array().unwrap().is_empty());
    for key in [
        "members_removed",
        "collections_renamed",
        "collections_moved",
        "collections_removed",
    ] {
        assert_eq!(n(&w[key]), 0, "{key}");
    }
    // Every member of a converted album is added or skipped with a reason.
    assert_eq!(
        n(&w["members_added"]) + w["members_skipped"].as_array().unwrap().len(),
        e::MEMBERS_CONVERTED as usize
    );
    assert!(
        w["members_skipped"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| !s["reason"].as_str().unwrap().is_empty())
    );
    assert!(w.get("aborted").is_none());

    // The fake library has the same tree as the catalog (names are trimmed
    // by the app).
    let albums = fake.albums();
    let by_id: BTreeMap<u64, &fake_lightcraft::AlbumView> =
        albums.iter().map(|a| (a.id, a)).collect();
    let mut have: Vec<(String, bool, Option<String>)> = albums
        .iter()
        .map(|a| {
            let parent = a.parent.map(|p| by_id[&p].name.clone());
            (a.name.clone(), a.folder, parent)
        })
        .collect();
    let parent_name = |p: Option<i64>| {
        p.map(|id| {
            let (_, name, _) = e::NAMES.iter().find(|(i, _, _)| *i == id).unwrap();
            name.trim().to_string()
        })
    };
    let mut want: Vec<(String, bool, Option<String>)> = e::NAMES
        .iter()
        .map(|(id, name, parent)| {
            (
                name.trim().to_string(),
                e::FOLDER_IDS.contains(id),
                parent_name(*parent),
            )
        })
        .collect();
    have.sort();
    want.sort();
    assert_eq!(have, want);

    // Members: nothing in a folder, never more than the catalog has, and the
    // total matches the report.
    let mut total = 0;
    for a in &albums {
        if a.folder {
            assert!(a.photos.is_empty());
        }
        total += a.photos.len();
    }
    assert_eq!(total, n(&w["members_added"]));
    assert!(fake.albums().iter().filter(|a| a.name == "Twin").count() == 2);
    assert_eq!(
        s.state()["collections"].as_object().unwrap().len(),
        e::CONVERTED as usize
    );
    assert!(
        !fake
            .received()
            .iter()
            .any(|r| r.line.contains("addSelected") || r.line.contains("album.delete"))
    );
}

#[test]
fn second_run_sends_no_album_command_and_no_select() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);
    let before = fake.commands().len();
    let albums = fake.albums();
    let r = s.ok(fake.port(), true);
    assert_balance(&r);
    let w = &r.report["writer"];
    assert_eq!(n(&w["folders_created"]) + n(&w["albums_created"]), 0);
    assert_eq!(n(&w["collections_known"]), e::CONVERTED as usize);
    assert_eq!(n(&w["members_added"]) + n(&w["members_removed"]), 0);
    assert_eq!(album_commands(&fake, before), 0);
    assert!(
        fake.commands()[before..]
            .iter()
            .all(|(c, _)| c != "library.select")
    );
    assert_eq!(fake.albums(), albums);
}

#[test]
fn dry_run_reports_the_plan_and_writes_nothing() {
    let s = setup();
    let r = s.ok(closed(), false);
    let planned = &r.report["writer"]["planned"];
    assert_eq!(n(&planned["folders"]), e::FOLDERS as usize);
    assert_eq!(n(&planned["albums"]), e::ALBUMS as usize);
    assert_eq!(n(&planned["members_added"]), e::MEMBERS_CONVERTED as usize);
    assert_eq!(n(&planned["members_removed"]), 0);
    assert_eq!(n(&planned["collections_known"]), 0);
    let line = r
        .stdout
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("writer: keywords to create "))
        .unwrap_or_else(|| panic!("no writer line in {}", r.stdout));
    assert!(
        line.ends_with(&format!(
            ", albums to create {}, album members to add {}",
            e::ALBUMS,
            e::MEMBERS_CONVERTED
        )),
        "{line}"
    );
    assert!(!s.state_file().exists(), "a dry run writes no mapping file");
}

#[test]
fn dry_run_sends_nothing_to_a_running_app() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), false);
    assert!(fake.write_commands().is_empty());
    assert!(fake.albums().is_empty());
    assert!(!s.state_file().exists());
}

#[test]
fn dry_run_after_apply_knows_every_collection() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);
    let state_before = fs::read(s.state_file()).unwrap();
    let r = s.ok(closed(), false);
    let planned = &r.report["writer"]["planned"];
    assert_eq!(n(&planned["folders"]) + n(&planned["albums"]), 0);
    assert_eq!(n(&planned["collections_known"]), e::CONVERTED as usize);
    assert_eq!(
        n(&planned["members_added"]) + n(&planned["members_removed"]),
        0
    );
    assert_eq!(state_before, fs::read(s.state_file()).unwrap());
    assert!(r.stdout.lines().any(|l| {
        l.trim()
            .ends_with(", albums to create 0, album members to add 0")
    }));
}

#[test]
fn a_removed_collection_in_lightroom_is_counted_and_nothing_is_deleted() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);
    let before = fake.commands().len();
    let albums = fake.albums();
    lrcat_fixtures::clear_collections(&s.catalog).unwrap();
    let r = s.ok(fake.port(), true);
    assert_balance(&r);
    let w = &r.report["writer"];
    assert_eq!(n(&w["collections_removed"]), e::CONVERTED as usize);
    assert_eq!(n(&w["collections_known"]), 0);
    assert_eq!(album_commands(&fake, before), 0);
    assert_eq!(fake.albums(), albums);
}
