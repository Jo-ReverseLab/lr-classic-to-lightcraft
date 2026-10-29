//! CLI acceptance tests for writing stacks, with the generator
//! fixture `stack_edge`, against the stateful fake LightCraft.
//!
//! `report.json`, section `writer`:
//! - applied run: integers `stacks_grouped`, `stacks_known`,
//!   `stack_members_removed`; lists of `{reason}`: `stacks_skipped`,
//!   `stacks_incomplete`;
//! - dry run: `writer.planned.stacks` is the number of stacks to group;
//! - balance: `stacks.converted == stacks_grouped + stacks_known +
//!   len(stacks_skipped) + len(stacks_incomplete)` in every run.
//!
//! Always: the catalog is byte-identical after every run, a dry run writes
//! nothing and sends no command, a second run sends no `stack.*` command,
//! every stack is grouped with `collapsed: false` (also stack 2, which is
//! collapsed in the catalog), and no command removes, ungroups or splits.

use lrcat_fixtures::stack_edge as e;
use lrcat_fixtures::write_stack_catalog_with_files;
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use fake_lightcraft::{Config, FakeLightCraft};
use lc_test_support::fake_lightcraft;

struct Setup {
    _photos: tempfile::TempDir,
    work: tempfile::TempDir,
    catalog: PathBuf,
    library: PathBuf,
}

fn setup() -> Setup {
    let photos = tempfile::tempdir().unwrap();
    let catalog = write_stack_catalog_with_files(photos.path()).unwrap();
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

    /// A run that must succeed; checks that the catalog is unchanged.
    fn ok(&self, port: u16, apply: bool) -> Run {
        let before = fs::read(&self.catalog).unwrap();
        let o = self.run(port, apply);
        let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
        assert_eq!(o.status.code(), Some(0), "{stderr}");
        assert_eq!(before, fs::read(&self.catalog).unwrap(), "catalog changed");
        let raw = fs::read_to_string(self.work.path().join("report.json")).unwrap();
        Run {
            report: serde_json::from_str(&raw).unwrap(),
        }
    }

    fn state(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.state_file()).unwrap()).unwrap()
    }

    /// Lightroom photo id -> LightCraft id, from the mapping file.
    fn photo_ids(&self) -> BTreeMap<i64, i64> {
        self.state()["photos"]
            .as_object()
            .unwrap()
            .values()
            .map(|p| {
                (
                    p["lightroom_id"].as_i64().unwrap(),
                    p["lightcraft_id"].as_i64().unwrap(),
                )
            })
            .collect()
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
        n(&r.report["stacks"]["converted"]),
        n(&w["stacks_grouped"])
            + n(&w["stacks_known"])
            + w["stacks_skipped"].as_array().unwrap().len()
            + w["stacks_incomplete"].as_array().unwrap().len(),
        "{w}"
    );
}

fn stack_commands(fake: &FakeLightCraft, from: usize) -> usize {
    fake.commands()[from..]
        .iter()
        .filter(|(c, _)| c.starts_with("stack."))
        .count()
}

#[test]
fn apply_groups_the_converted_stacks_and_the_numbers_add_up() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let r = s.ok(fake.port(), true);
    assert_balance(&r);
    let w = &r.report["writer"];
    assert_eq!(n(&w["stacks_grouped"]), e::CONVERTED as usize);
    assert_eq!(n(&w["stacks_known"]), 0);
    assert_eq!(n(&w["stack_members_removed"]), 0);
    assert!(w["stacks_skipped"].as_array().unwrap().is_empty());
    assert!(w["stacks_incomplete"].as_array().unwrap().is_empty());
    assert!(w.get("aborted").is_none());

    // The fake library has the stacks of the catalog, members in order.
    let ids = s.photo_ids();
    let mut want: Vec<Vec<i64>> = e::STACK_MEMBERS
        .iter()
        .map(|(_, members)| members.iter().map(|m| ids[m]).collect())
        .collect();
    let mut have: Vec<Vec<i64>> = fake.stacks().into_iter().map(|st| st.photos).collect();
    want.sort();
    have.sort();
    assert_eq!(have, want);
    assert_eq!(
        s.state()["stacks"].as_object().unwrap().len(),
        e::CONVERTED as usize
    );
}

#[test]
fn every_stack_is_expanded_even_the_one_collapsed_in_lightroom() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);
    assert_eq!(fake.stacks().len(), e::CONVERTED as usize);
    assert!(fake.stacks().iter().all(|st| !st.collapsed));
    let groups: Vec<Value> = fake
        .commands()
        .into_iter()
        .filter(|(c, _)| c == "stack.group")
        .map(|(_, p)| p)
        .collect();
    assert_eq!(groups.len(), e::CONVERTED as usize);
    assert!(groups.iter().all(|p| p["collapsed"] == false));
}

#[test]
fn no_remove_ungroup_or_split_is_ever_sent() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);
    s.ok(fake.port(), true);
    for (c, _) in fake.commands() {
        assert!(
            !["stack.remove", "stack.ungroup", "stack.split"].contains(&c.as_str()),
            "{c}"
        );
    }
}

#[test]
fn second_run_sends_no_stack_command() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);
    let before = fake.commands().len();
    let stacks = fake.stacks();
    let r = s.ok(fake.port(), true);
    assert_balance(&r);
    let w = &r.report["writer"];
    assert_eq!(n(&w["stacks_grouped"]), 0);
    assert_eq!(n(&w["stacks_known"]), e::CONVERTED as usize);
    assert_eq!(stack_commands(&fake, before), 0);
    assert_eq!(fake.stacks(), stacks);
}

#[test]
fn dry_run_reports_the_plan_and_writes_nothing() {
    let s = setup();
    let r = s.ok(closed(), false);
    assert_eq!(
        n(&r.report["writer"]["planned"]["stacks"]),
        e::CONVERTED as usize
    );
    assert!(!s.state_file().exists(), "a dry run writes no mapping file");
}

#[test]
fn dry_run_sends_nothing_to_a_running_app() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), false);
    assert!(fake.write_commands().is_empty());
    assert!(fake.stacks().is_empty());
    assert!(!s.state_file().exists());
}

#[test]
fn dry_run_after_apply_plans_no_stack() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);
    let state_before = fs::read(s.state_file()).unwrap();
    let r = s.ok(closed(), false);
    assert_eq!(n(&r.report["writer"]["planned"]["stacks"]), 0);
    assert_eq!(state_before, fs::read(s.state_file()).unwrap());
}

#[test]
fn a_reordered_stack_in_lightroom_is_sent_once() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);
    let before = fake.commands().len();
    // Stack 1 is [6, 7]: put photo 7 on top.
    lrcat_fixtures::set_stack_member_position(&s.catalog, 1, 7, 0.5).unwrap();
    let r = s.ok(fake.port(), true);
    assert_balance(&r);
    let w = &r.report["writer"];
    assert_eq!(n(&w["stacks_grouped"]), 1);
    assert_eq!(n(&w["stacks_known"]), e::CONVERTED as usize - 1);
    let groups: Vec<Value> = fake.commands()[before..]
        .iter()
        .filter(|(c, _)| c == "stack.group")
        .map(|(_, p)| p.clone())
        .collect();
    assert_eq!(groups.len(), 1);
    let ids = s.photo_ids();
    assert_eq!(groups[0]["top"], ids[&7]);
    assert_eq!(fake.stacks().len(), e::CONVERTED as usize);
}
