//! CLI acceptance tests for the selection before photo commands,
//! against the stateful fake LightCraft, which behaves like the real app
//! (photo commands fail with `no photos selected` while nothing is selected).
//!
//! Rules:
//! - with `--apply` every `photo.*` command follows a `library.select
//!   {ids:[id], active:id}` naming the same photo, directly before the first
//!   update of that photo and once per photo;
//! - a photo without updates gets no select; a second run without changes
//!   sends no `library.select` at all, even when the app has no selection;
//! - `library.select` is not an update: `writer.updates` counts the
//!   `photo.*` commands only, and the stdout line
//!   `writer: keywords to create K, photos to import P, updates U` keeps
//!   its meaning (U without selects);
//! - a dry run never connects; `--script` holds a `library.select` line
//!   before the updates of each photo, with the same `ids` as those updates
//!   (a `lr:<global id>` placeholder for a photo imported by the plan, the
//!   LightCraft id for a known photo).
//!

use lrcat_fixtures::write_keyword_edge_catalog_with_files;
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use fake_lightcraft::{Config, FakeLightCraft, READ_COMMANDS};
use lc_test_support::fake_lightcraft;

struct Setup {
    _photos: tempfile::TempDir,
    work: tempfile::TempDir,
    catalog: PathBuf,
    library: PathBuf,
}

fn setup() -> Setup {
    let photos = tempfile::tempdir().unwrap();
    let catalog = write_keyword_edge_catalog_with_files(photos.path()).unwrap();
    let work = tempfile::tempdir().unwrap();
    let library = work.path().join("Test.lightcraft");
    Setup {
        _photos: photos,
        work,
        catalog,
        library,
    }
}

impl Setup {
    fn report(&self) -> PathBuf {
        self.work.path().join("report.json")
    }

    fn state_file(&self) -> PathBuf {
        let mut name = self.library.file_name().unwrap().to_os_string();
        name.push(".lr2lc.json");
        self.work.path().join(name)
    }

    fn run(&self, port: u16, apply: bool) -> Output {
        let port_text = port.to_string();
        let report = self.report();
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

    fn fake(&self) -> FakeLightCraft {
        FakeLightCraft::start(Config::new(&self.library))
    }

    /// A dry run that also writes the plan as a script.
    fn dry_script(&self, port: u16, script: &std::path::Path) -> Output {
        let port_text = port.to_string();
        let report = self.report();
        Command::new(env!("CARGO_BIN_EXE_lr2lc"))
            .current_dir(self.work.path())
            .arg(&self.catalog)
            .args(["--library".as_ref(), self.library.as_os_str()])
            .args(["--port", port_text.as_str()])
            .args(["--report".as_ref(), report.as_os_str()])
            .args(["--script".as_ref(), script.as_os_str()])
            .output()
            .unwrap()
    }

    /// A run that must succeed; returns the parsed report.
    fn ok(&self, port: u16, apply: bool) -> (Value, String) {
        let before = fs::read(&self.catalog).unwrap();
        let o = self.run(port, apply);
        assert_eq!(
            o.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert_eq!(before, fs::read(&self.catalog).unwrap(), "catalog changed");
        let text = fs::read_to_string(self.report()).unwrap();
        (
            serde_json::from_str(&text).unwrap(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
        )
    }

    fn state(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.state_file()).unwrap()).unwrap()
    }

    /// Changes the rating of one recorded photo (a difference to the catalog).
    fn change_one_rating(&self) {
        let mut state = self.state();
        let photos = state["photos"].as_object_mut().unwrap();
        let entry = photos.values_mut().next().expect("a recorded photo");
        let rating = entry["fields"]["rating"].as_u64().unwrap();
        entry["fields"]["rating"] = Value::from((rating + 1) % 6);
        fs::write(self.state_file(), serde_json::to_string(&state).unwrap()).unwrap();
    }
}

fn n(v: &Value) -> u64 {
    v.as_u64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn commands_of(fake: &FakeLightCraft, from: usize) -> Vec<(String, Value)> {
    fake.commands()[from..].to_vec()
}

/// The commands without album steps: `album.*` commands and the
/// `library.select` directly before `album.addPhotos`,
/// `album.removePhotos` or `album.toggleTarget` are removed.
fn without_album_steps(commands: &[(String, Value)]) -> Vec<(String, Value)> {
    let is_member_change = |c: &(String, Value)| {
        c.0 == "album.addPhotos" || c.0 == "album.removePhotos" || c.0 == "album.toggleTarget"
    };
    commands
        .iter()
        .enumerate()
        .filter(|(i, (c, _))| {
            !c.starts_with("album.")
                && !(c == "library.select" && commands.get(i + 1).is_some_and(is_member_change))
        })
        .map(|(_, c)| c.clone())
        .collect()
}

fn photo_commands(commands: &[(String, Value)]) -> usize {
    commands
        .iter()
        .filter(|(c, _)| c.starts_with("photo."))
        .count()
}

fn select_count(commands: &[(String, Value)]) -> usize {
    commands
        .iter()
        .filter(|(c, _)| c == "library.select")
        .count()
}

/// Every `photo.*` command follows a select naming its photo; every select
/// is directly followed by an update of the photo it names.
fn assert_always_selected(commands: &[(String, Value)]) {
    let mut selected: Option<i64> = None;
    for (i, (command, params)) in commands.iter().enumerate() {
        if command == "library.select" {
            assert_eq!(params["ids"].as_array().map(Vec::len), Some(1), "{params}");
            assert_eq!(params["ids"][0], params["active"], "{params}");
            selected = params["ids"][0].as_i64();
            let next = commands.get(i + 1).expect("a select without an update");
            assert!(next.0.starts_with("photo."), "select not used: {next:?}");
            assert_eq!(next.1["ids"][0].as_i64(), selected);
        } else if command.starts_with("photo.") {
            assert!(selected.is_some(), "{command} sent without any select");
            assert_eq!(params["ids"][0].as_i64(), selected, "{command} {params}");
        }
    }
}

#[test]
fn apply_selects_each_photo_before_its_updates_and_counts_no_select() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let (report, _) = s.ok(fake.port(), true);
    let commands = without_album_steps(&commands_of(&fake, 0));
    let updates = photo_commands(&commands);
    assert!(updates > 0, "the fixture must have something to set");
    assert!(select_count(&commands) > 0);
    assert!(select_count(&commands) <= updates);
    assert_always_selected(&commands);
    assert_eq!(n(&report["writer"]["updates"]), updates as u64);
    assert!(
        report["writer"]["photos_incomplete"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{}",
        report["writer"]
    );
}

#[test]
fn a_second_run_sends_no_select_even_without_a_selection() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);
    fake.clear_selection();
    let before = fake.commands().len();
    let (report, _) = s.ok(fake.port(), true);
    let commands = commands_of(&fake, before);
    assert_eq!(select_count(&commands), 0, "{commands:?}");
    assert_eq!(photo_commands(&commands), 0, "{commands:?}");
    assert_eq!(n(&report["writer"]["updates"]), 0);
}

#[test]
fn a_changed_known_photo_is_selected_in_a_fresh_session() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);
    fake.clear_selection(); // the app was restarted
    s.change_one_rating();
    let before = fake.commands().len();
    let (report, _) = s.ok(fake.port(), true);
    let commands = commands_of(&fake, before);
    let names: Vec<&str> = commands
        .iter()
        .map(|(c, _)| c.as_str())
        .filter(|c| !READ_COMMANDS.contains(c) || *c == "library.select")
        .collect();
    assert_eq!(names, ["library.select", "photo.rate"], "{commands:?}");
    assert_always_selected(&commands);
    assert_eq!(n(&report["writer"]["updates"]), 1);
    assert_eq!(n(&report["writer"]["photos_updated"]), 1);
    assert!(
        report["writer"]["photos_incomplete"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{}",
        report["writer"]
    );
}

/// The script lines as `(command, params)`.
fn script_lines(path: &std::path::Path) -> Vec<(String, Value)> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| {
            let v: Value = serde_json::from_str(l).unwrap();
            (
                v["params"]["command"].as_str().unwrap().to_string(),
                v["params"]["params"].clone(),
            )
        })
        .collect()
}

/// Every group of `photo.*` lines of one photo is directly preceded by one
/// select line with the same `ids`; returns the number of selects.
fn assert_script_selects(lines: &[(String, Value)]) -> usize {
    let mut selects = 0;
    let mut selected: Option<Value> = None;
    for (command, params) in lines {
        if command == "library.select" {
            assert_eq!(params["ids"], json!([params["active"].clone()]), "{params}");
            assert!(params["ids"][0].is_string() || params["ids"][0].is_number());
            selected = Some(params["ids"].clone());
            selects += 1;
        } else if command.starts_with("photo.") {
            assert_eq!(
                Some(&params["ids"]),
                selected.as_ref(),
                "{command} {params}"
            );
        } else {
            // An import or keyword line ends the photo's group.
            selected = None;
        }
    }
    selects
}

#[test]
fn script_has_a_select_line_before_the_updates_of_each_new_photo() {
    let s = setup();
    let fake = s.fake();
    let script = s.work.path().join("plan.jsonl");
    let o = s.dry_script(fake.port(), &script);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(fake.connections(), 0, "a dry run must not connect");
    let lines = without_album_steps(&script_lines(&script));
    let selects = assert_script_selects(&lines);
    let updates = photo_commands(&lines);
    assert!(selects > 0 && selects <= updates);
    // One select per photo with updates: placeholder ids of imported photos.
    for (c, p) in &lines {
        if c == "library.select" {
            assert!(p["ids"][0].as_str().unwrap().starts_with("lr:"), "{p}");
        }
    }
    // The stdout number of updates does not count selects.
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    let line = text
        .lines()
        .find(|l| l.starts_with("writer: keywords to create "))
        .unwrap();
    let numbers: Vec<usize> = line
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| p.parse().unwrap())
        .collect();
    assert_eq!(numbers[2], updates, "{line}");
    assert_eq!(lines.len(), numbers[0] + numbers[1] + numbers[2] + selects);
}

#[test]
fn script_of_a_known_photo_selects_its_lightcraft_id() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);
    s.change_one_rating();
    let script = s.work.path().join("plan.jsonl");
    let connections = fake.connections();
    let o = s.dry_script(fake.port(), &script);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        fake.connections(),
        connections,
        "a dry run must not connect"
    );
    let lines = script_lines(&script);
    let names: Vec<&str> = lines.iter().map(|(c, _)| c.as_str()).collect();
    assert_eq!(names, ["library.select", "photo.rate"], "{lines:?}");
    assert_script_selects(&lines);
    assert!(!lines[0].1["ids"][0].to_string().contains("lr:"));
}
