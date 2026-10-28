//! CLI acceptance tests for recovering albums after the mapping file was lost
//! (principle 5), with the generator fixture `collection_edge`,
//! against the stateful fake LightCraft.
//!
//! After a first complete run and deleting the mapping file, a second run
//! with `--apply`:
//! - sends no `album.create`; the albums and their members in LightCraft are
//!   unchanged;
//! - adopts every collection with exactly one matching node: `report.json`
//!   `writer.collections_existing` counts them (10 of the 12 converted ones:
//!   the names are compared after trimming, as the app trims on create);
//! - the two collections "Twin" (same name, same parent) match two nodes
//!   each: both are in `writer.collections_skipped` with a reason that
//!   contains `ambiguous: 2 albums with the same name and parent in
//!   LightCraft` and no name, and nothing is created for them;
//! - balance: `collections.converted == folders_created + albums_created +
//!   collections_known + collections_existing + len(collections_skipped)`;
//! - the catalog is byte-identical, no collection name is printed or
//!   reported, a dry run afterwards sends no write command and does not
//!   recreate the mapping file.

use lrcat_fixtures::collection_edge as e;
use lrcat_fixtures::write_collection_catalog_with_files;
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

#[path = "support/fake_lightcraft.rs"]
mod fake_lightcraft;
use fake_lightcraft::{Config, FakeLightCraft};

const AMBIGUOUS: &str = "ambiguous: 2 albums with the same name and parent in LightCraft";
/// Converted collections minus the two "Twin" albums.
const ADOPTED: usize = e::CONVERTED as usize - 2;

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

    /// A run that must succeed; checks the catalog and that no name leaks.
    fn ok(&self, port: u16, apply: bool) -> Run {
        let before = fs::read(&self.catalog).unwrap();
        let o = self.run(port, apply);
        let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
        assert_eq!(o.status.code(), Some(0), "{stderr}");
        assert_eq!(before, fs::read(&self.catalog).unwrap(), "catalog changed");
        let raw = fs::read_to_string(self.work.path().join("report.json")).unwrap();
        let all = format!("{}\n{stderr}\n{raw}", String::from_utf8_lossy(&o.stdout));
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
        // The quick collection (0 or 1) is converted but is no album.
        n(&r.report["collections"]["converted"])
            - r.report["collections"]["quick"].as_u64().unwrap_or(0) as usize,
        n(&w["folders_created"])
            + n(&w["albums_created"])
            + n(&w["collections_known"])
            + n(&w["collections_existing"])
            + w["collections_skipped"].as_array().unwrap().len(),
        "{w}"
    );
}

fn creates(fake: &FakeLightCraft, from: usize) -> usize {
    fake.commands()[from..]
        .iter()
        .filter(|(c, _)| c == "album.create")
        .count()
}

/// First run, then the mapping file is gone.
fn lost_state() -> (Setup, FakeLightCraft) {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let r = s.ok(fake.port(), true);
    assert_balance(&r);
    assert_eq!(n(&r.report["writer"]["collections_existing"]), 0);
    fs::remove_file(s.state_file()).unwrap();
    (s, fake)
}

#[test]
fn lost_state_file_creates_no_album_twice() {
    let (s, fake) = lost_state();
    let albums = fake.albums();
    let members: Vec<Vec<i64>> = albums.iter().map(|a| fake.album_members(a.id)).collect();
    let before = fake.commands().len();

    let r = s.ok(fake.port(), true);
    assert_balance(&r);
    let w = &r.report["writer"];
    assert_eq!(n(&w["collections_existing"]), ADOPTED);
    assert_eq!(n(&w["folders_created"]) + n(&w["albums_created"]), 0);
    assert_eq!(n(&w["collections_known"]), 0);
    assert_eq!(creates(&fake, before), 0);
    assert_eq!(fake.albums(), albums, "same albums, same ids, same parents");
    let after: Vec<Vec<i64>> = fake
        .albums()
        .iter()
        .map(|a| fake.album_members(a.id))
        .collect();
    assert_eq!(members, after, "members unchanged");
    assert_eq!(n(&w["members_removed"]), 0);
    assert!(
        !fake.commands()[before..]
            .iter()
            .any(|(c, _)| c == "album.delete" || c == "album.removePhotos")
    );
    assert!(w.get("aborted").is_none());
}

#[test]
fn equal_twins_are_reported_instead_of_guessed() {
    let (s, fake) = lost_state();
    let r = s.ok(fake.port(), true);
    let skipped = r.report["writer"]["collections_skipped"]
        .as_array()
        .unwrap();
    assert_eq!(skipped.len(), 2, "{skipped:?}");
    for item in skipped {
        let reason = item["reason"].as_str().unwrap();
        assert!(reason.contains(AMBIGUOUS), "{reason}");
    }
    // The two "Twin" albums are still two, with their members.
    assert_eq!(fake.albums().iter().filter(|a| a.name == "Twin").count(), 2);
    // The mapping file knows everything that was adopted, not the twins.
    let st = s.state();
    let recorded = st["collections"].as_object().unwrap();
    assert_eq!(recorded.len(), ADOPTED);
    assert!(!recorded.contains_key("11") && !recorded.contains_key("12"));
}

#[test]
fn recovered_mapping_is_complete_and_stable() {
    let (s, fake) = lost_state();
    s.ok(fake.port(), true);
    let albums = fake.albums();
    let state_after_recovery = fs::read(s.state_file()).unwrap();

    // Every adopted album carries the LightCraft id of the real node.
    let st = s.state();
    for (id, name, _) in e::NAMES {
        if id == 11 || id == 12 {
            continue;
        }
        let entry = &st["collections"][id.to_string()];
        let lc_id = entry["lightcraft_id"].as_u64().unwrap();
        let node = albums.iter().find(|a| a.id == lc_id).unwrap();
        assert_eq!(node.name, name.trim(), "collection {id}");
        assert_eq!(node.folder, e::FOLDER_IDS.contains(&id));
    }

    // A further run changes nothing in LightCraft or in the mapping file.
    let before = fake.commands().len();
    let r = s.ok(fake.port(), true);
    assert_balance(&r);
    let w = &r.report["writer"];
    assert_eq!(n(&w["collections_known"]), ADOPTED);
    assert_eq!(n(&w["collections_existing"]), 0);
    assert_eq!(w["collections_skipped"].as_array().unwrap().len(), 2);
    assert_eq!(creates(&fake, before), 0);
    assert_eq!(fake.albums(), albums);
    assert_eq!(state_after_recovery, fs::read(s.state_file()).unwrap());
}

#[test]
fn dry_run_after_lost_state_writes_nothing() {
    let (s, fake) = lost_state();
    let albums = fake.albums();
    let before = fake.commands().len();
    s.ok(fake.port(), false);
    assert!(
        fake.commands()[before..]
            .iter()
            .all(|(c, _)| fake_lightcraft::READ_COMMANDS.contains(&c.as_str())),
        "a dry run sends no write command"
    );
    assert_eq!(fake.albums(), albums);
    assert!(!s.state_file().exists(), "a dry run writes no mapping file");
    // Also with no app at all.
    s.ok(closed(), false);
    assert!(!s.state_file().exists());
}
