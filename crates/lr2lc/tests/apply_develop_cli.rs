//! CLI acceptance tests for writing development settings (P6-2, phase 6a)
//! and for the `develop` area of `report.json`, with generated catalogs from
//! `lrcat_fixtures::develop` against the stateful fake LightCraft.
//!
//! `report.json`, top-level area `develop` (new in P6-2), present in a dry run
//! and in an applied run:
//! - `photos`: converted photos that carry development settings (`Develop` is
//!   `Some`), whether or not a command was needed;
//! - `planned`: photos the plan sends develop commands for;
//! - `written`: photos whose develop commands all succeeded (0 in a dry run);
//! - `unmapped`: the distinct field names that could not be carried over,
//!   sorted, at most 20 entries;
//! - `unmapped_count`: the number of distinct field names (may exceed 20);
//! - `not_applied`: known photos whose settings changed in Lightroom and were
//!   left alone because `--overwrite-develop` was not given;
//! - `overwritten`: photos written because `--overwrite-develop` was given;
//! - `white_balance_custom`, `white_balance_to_check`,
//!   `white_balance_written`, `white_balance_not_carried` (#9): custom white
//!   balances (see `apply_develop_white_balance_cli.rs`; only counts, never a
//!   value).
//!
//! The readable summary contains one line:
//! `develop: photos with settings N, commands planned N, written N, unmapped
//! fields N (N listed), changed and not applied N, overwritten N, photos with
//! camera profile N, photos with masks N, photos with spots N, custom white
//! balance N (to check N, written N, not carried N)`.
//!
//! Always: the catalog is byte-identical after every run, a dry run connects
//! to nothing and writes no mapping file, a second run sends no `develop.*`
//! command, `photos.read == converted + skipped`, and no command ever carries
//! a temperature, a tint or an unmapped value.

use lrcat_fixtures::develop::{self as dev, edge as e, settings, write_develop_catalog_with_files};
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use fake_lightcraft::{Config, FakeLightCraft};
use lc_test_support::fake_lightcraft;

struct Setup {
    _photos: tempfile::TempDir,
    work: tempfile::TempDir,
    catalog: PathBuf,
    library: PathBuf,
}

impl Setup {
    fn with_catalog(catalog: PathBuf, photos: tempfile::TempDir) -> Setup {
        let work = tempfile::tempdir().unwrap();
        let library = work.path().join("Test.lightcraft");
        Setup {
            _photos: photos,
            work,
            catalog,
            library,
        }
    }

    fn sliders() -> Setup {
        let photos = tempfile::tempdir().unwrap();
        let catalog = dev::write_all_sliders_catalog_with_files(photos.path()).unwrap();
        Setup::with_catalog(catalog, photos)
    }

    fn state_file(&self) -> PathBuf {
        let mut name = self.library.file_name().unwrap().to_os_string();
        name.push(".lr2lc.json");
        self.work.path().join(name)
    }

    fn report_path(&self) -> PathBuf {
        self.work.path().join("report.json")
    }

    fn run(&self, catalog: &Path, port: u16, apply: bool, extra: &[&str]) -> Output {
        let port_text = port.to_string();
        let report = self.report_path();
        let mut args: Vec<OsString> = vec![
            catalog.as_os_str().to_owned(),
            OsString::from("--library"),
            self.library.as_os_str().to_owned(),
            OsString::from("--port"),
            OsString::from(port_text),
            OsString::from("--anonymize"),
            OsString::from("--report"),
            report.as_os_str().to_owned(),
        ];
        if apply {
            args.push(OsString::from("--apply"));
        }
        for item in extra {
            args.push(OsString::from(item));
        }
        Command::new(env!("CARGO_BIN_EXE_lr2lc"))
            .current_dir(self.work.path())
            .args(args)
            .output()
            .unwrap()
    }

    /// A run that must succeed; checks that the catalog is unchanged.
    fn ok(&self, catalog: &Path, port: u16, apply: bool, extra: &[&str]) -> Value {
        let before = fs::read(catalog).unwrap();
        let output = self.run(catalog, port, apply, extra);
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert_eq!(output.status.code(), Some(0), "{stderr}");
        assert_eq!(
            before,
            fs::read(catalog).unwrap(),
            "the catalog changed during the run"
        );
        serde_json::from_str(&fs::read_to_string(self.report_path()).unwrap()).unwrap()
    }

    fn state(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.state_file()).unwrap()).unwrap()
    }

    /// Lightroom photo id -> LightCraft id, from the mapping file.
    fn lightcraft_ids(&self) -> BTreeMap<i64, i64> {
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

fn n(value: &Value) -> usize {
    value
        .as_u64()
        .unwrap_or_else(|| panic!("not a number: {value}")) as usize
}

fn develop(report: &Value) -> &Value {
    report
        .get("develop")
        .unwrap_or_else(|| panic!("no develop area in the report: {report}"))
}

fn develop_commands(commands: &[(String, Value)], from: usize) -> Vec<(String, Value)> {
    commands[from..]
        .iter()
        .filter(|(c, _)| c.starts_with("develop."))
        .cloned()
        .collect()
}

fn assert_balanced(report: &Value) {
    let photos = &report["photos"];
    assert_eq!(
        n(&photos["read"]),
        n(&photos["converted"]) + photos["skipped"].as_array().unwrap().len(),
        "read = converted + skipped: {photos}"
    );
}

#[test]
fn apply_writes_the_sliders_and_reports_the_develop_counts() {
    let s = Setup::sliders();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let report = s.ok(&s.catalog, fake.port(), true, &[]);
    assert_balanced(&report);

    let d = develop(&report);
    assert_eq!(n(&d["photos"]), 1, "only the generated photo has a row");
    assert_eq!(n(&d["planned"]), 1);
    assert_eq!(n(&d["written"]), 1);
    assert_eq!(n(&d["unmapped_count"]), 0);
    assert!(d["unmapped"].as_array().unwrap().is_empty());
    assert_eq!(n(&d["not_applied"]), 0);
    assert_eq!(n(&d["overwritten"]), 0);

    let commands = fake.commands();
    let develop = develop_commands(&commands, 0);
    assert_eq!(develop.len(), 1, "{develop:?}");
    assert_eq!(develop[0].0, "develop.set");

    let ids = s.lightcraft_ids();
    let photo = ids[&e::ALL_SLIDERS_PHOTO];
    assert_eq!(develop[0].1["ids"], serde_json::json!([photo]));
    let values = develop[0].1["values"].as_object().expect("values");
    let expected: BTreeMap<String, f64> = dev::all_controls()
        .into_iter()
        .map(|(_, control, value)| (control, value))
        .collect();
    assert_eq!(values.len(), expected.len());
    for (control, value) in expected {
        assert_eq!(
            values.get(&control).and_then(Value::as_f64),
            Some(value),
            "{control}"
        );
    }

    // The mapping file carries the source hash for the next run.
    let entry = s.state()["photos"]
        .as_object()
        .unwrap()
        .values()
        .find(|p| p["lightroom_id"] == serde_json::json!(e::ALL_SLIDERS_PHOTO))
        .cloned()
        .expect("recorded photo");
    let hash = entry["develop_hash"].as_str().expect("develop_hash");
    assert_eq!(hash.len(), 64);
    assert!(
        hash.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
}

#[test]
fn a_second_run_sends_no_develop_command() {
    let s = Setup::sliders();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(&s.catalog, fake.port(), true, &[]);
    let before = fake.commands().len();
    let report = s.ok(&s.catalog, fake.port(), true, &[]);
    assert_balanced(&report);
    let d = develop(&report);
    assert!(develop_commands(&fake.commands(), before).is_empty());
    assert_eq!(n(&d["planned"]), 0);
    assert_eq!(n(&d["written"]), 0);
    assert_eq!(n(&d["photos"]), 1, "the settings are still read");
}

#[test]
fn changed_settings_need_the_overwrite_flag() {
    let photos = tempfile::tempdir().unwrap();
    let catalog_a = write_develop_catalog_with_files(
        photos.path(),
        "develop-step-a.lrcat",
        &[(
            e::ALL_SLIDERS_PHOTO,
            settings().number("Exposure2012", 1.0).build(),
        )],
    )
    .unwrap();
    let catalog_b = write_develop_catalog_with_files(
        photos.path(),
        "develop-step-b.lrcat",
        &[(
            e::ALL_SLIDERS_PHOTO,
            settings().number("Exposure2012", 2.0).build(),
        )],
    )
    .unwrap();
    let s = Setup::with_catalog(catalog_a.clone(), photos);

    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(&catalog_a, fake.port(), true, &[]);

    // The same photo with different settings text: left alone by default.
    let before = fake.commands().len();
    let report = s.ok(&catalog_b, fake.port(), true, &[]);
    let d = develop(&report);
    assert!(
        develop_commands(&fake.commands(), before).is_empty(),
        "never overwrite silently"
    );
    assert_eq!(n(&d["planned"]), 0);
    assert_eq!(n(&d["written"]), 0);
    assert_eq!(n(&d["not_applied"]), 1);
    assert_eq!(n(&d["overwritten"]), 0);

    // With the explicit opt-in exactly one command is sent.
    let before = fake.commands().len();
    let report = s.ok(&catalog_b, fake.port(), true, &["--overwrite-develop"]);
    let d = develop(&report);
    let written = develop_commands(&fake.commands(), before);
    assert_eq!(written.len(), 1, "{written:?}");
    assert_eq!(written[0].0, "develop.set");
    assert_eq!(
        written[0].1["values"],
        serde_json::json!({"light.exposure": 2.0})
    );
    assert_eq!(n(&d["planned"]), 1);
    assert_eq!(n(&d["written"]), 1);
    assert_eq!(n(&d["not_applied"]), 0);
    assert_eq!(n(&d["overwritten"]), 1);
}

#[test]
fn a_dry_run_plans_the_commands_but_connects_to_nothing() {
    let s = Setup::sliders();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let report = s.ok(&s.catalog, fake.port(), false, &[]);
    assert_balanced(&report);
    assert_eq!(report["dry_run"], serde_json::json!(true));
    let d = develop(&report);
    assert_eq!(n(&d["photos"]), 1);
    assert_eq!(n(&d["planned"]), 1);
    assert_eq!(n(&d["written"]), 0);
    assert!(
        fake.commands().is_empty(),
        "a dry run never connects to the app"
    );
    assert!(!s.state_file().exists(), "a dry run writes no mapping file");
}

#[test]
fn merge_photos_are_selected_before_the_merge_and_carry_nested_settings() {
    let photos = tempfile::tempdir().unwrap();
    let catalog = dev::write_merge_catalog_with_files(photos.path()).unwrap();
    let s = Setup::with_catalog(catalog, photos);
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let report = s.ok(&s.catalog, fake.port(), true, &[]);
    assert_balanced(&report);
    let d = develop(&report);
    assert_eq!(n(&d["photos"]), 3);
    assert_eq!(n(&d["planned"]), 3);
    assert_eq!(n(&d["written"]), 3);

    let commands = fake.commands();
    let merges: Vec<usize> = commands
        .iter()
        .enumerate()
        .filter(|(_, (c, _))| c == "develop.merge")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(merges.len(), 3);
    for at in merges {
        assert_eq!(commands[at - 1].0, "library.select");
        let settings = commands[at]
            .1
            .get("settings")
            .and_then(Value::as_object)
            .unwrap_or_else(|| panic!("no settings: {}", commands[at].1));
        assert!(
            commands[at].1.get("ids").is_none(),
            "develop.merge takes no ids"
        );
        assert!(
            settings.contains_key("treatment") || settings.contains_key("optics"),
            "{settings:?}"
        );
    }
}

#[test]
fn unmapped_fields_are_listed_by_name_only_and_capped_at_twenty() {
    let photos = tempfile::tempdir().unwrap();
    let mut text = settings().number("Exposure2012", 2.0);
    for i in 0..25 {
        text = text.number(&format!("ZzMystery{i:02}"), 424242.0 + i as f64);
    }
    let catalog = write_develop_catalog_with_files(
        photos.path(),
        "develop-many-unmapped.lrcat",
        &[(e::UNMAPPED_PHOTO, text.build())],
    )
    .unwrap();
    let s = Setup::with_catalog(catalog, photos);
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let before = fs::read(&s.catalog).unwrap();
    let output = s.run(&s.catalog, fake.port(), true, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    assert_eq!(before, fs::read(&s.catalog).unwrap(), "catalog changed");
    let report: Value =
        serde_json::from_str(&fs::read_to_string(s.report_path()).unwrap()).unwrap();

    let d = develop(&report);
    assert_eq!(n(&d["unmapped_count"]), 25);
    let listed = d["unmapped"].as_array().unwrap();
    assert_eq!(listed.len(), 20, "the detail is capped: {listed:?}");
    // The photo counts are not capped: every name, each on its one photo.
    let per_name = d["unmapped_photos"].as_object().unwrap();
    assert_eq!(per_name.len(), 25, "{per_name:?}");
    assert!(per_name.values().all(|v| v == 1), "{per_name:?}");
    assert!(per_name.contains_key("ZzMystery24"));
    let names: Vec<&str> = listed.iter().filter_map(Value::as_str).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted, "the listed field names are sorted");
    assert!(
        names.iter().all(|s| s.starts_with("ZzMystery")),
        "field names only: {listed:?}"
    );

    let raw = fs::read_to_string(s.report_path()).unwrap();
    for value in ["424242", "424243", "424266"] {
        assert!(
            !raw.contains(value),
            "a value leaked into the report: {value}"
        );
    }
    assert!(
        !stdout.contains("424242"),
        "a value leaked into the summary"
    );
    let develop = develop_commands(&fake.commands(), 0);
    assert_eq!(develop.len(), 1, "the sliders are still written");
    let params = develop[0].1.to_string();
    assert!(!params.contains("ZzMystery"), "{params}");
    assert!(!params.contains("424242"), "{params}");
    assert!(
        stdout.contains("unmapped fields 25 (20 listed)"),
        "the summary aggregates and caps: {stdout}"
    );
}

#[test]
fn a_named_mode_writes_only_the_mode_and_never_its_stored_numbers() {
    let photos = tempfile::tempdir().unwrap();
    let catalog = dev::write_white_balance_catalog_with_files(photos.path()).unwrap();
    let s = Setup::with_catalog(catalog, photos);
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let report = s.ok(&s.catalog, fake.port(), true, &[]);
    assert_balanced(&report);

    let d = develop(&report);
    let unmapped: Vec<&str> = d["unmapped"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        !unmapped
            .iter()
            .any(|n| n.contains("Temperature") || n.contains("Tint")),
        "white balance has its own counters: {unmapped:?}"
    );

    let ids = s.lightcraft_ids();
    let named = ids[&e::WB_NAMED_ABSOLUTE_PHOTO];
    for (command, params) in develop_commands(&fake.commands(), 0) {
        let text = params.to_string();
        assert!(
            !text.contains("Temperature") && !text.contains("Tint"),
            "{command} must not carry Lightroom field names: {text}"
        );
        assert!(
            !text.contains("\"scale\""),
            "no Kelvin hint in this fixture: {text}"
        );
        if command == "develop.set" && params["ids"] == serde_json::json!([named]) {
            panic!("the named mode must not write a temperature: {text}");
        }
    }

    // The named mode with a stored Kelvin pair writes its mode, nothing else.
    let merge = develop_commands(&fake.commands(), 0)
        .into_iter()
        .find(|(c, p)| {
            c == "develop.merge" && p["settings"]["wb"]["mode"] == serde_json::json!("cloudy")
        })
        .map(|(_, p)| p)
        .unwrap_or_else(|| panic!("no merge for photo {named}"));
    assert_eq!(
        merge,
        serde_json::json!({"settings": {"wb": {"mode": "cloudy"}}})
    );
}

#[test]
fn a_catalog_without_develop_settings_reports_zero_and_writes_nothing() {
    // The small fixture has no Adobe_imageDevelopSettings table at all.
    let photos = tempfile::tempdir().unwrap();
    let catalog = lrcat_fixtures::write_small_catalog_with_files(photos.path()).unwrap();
    let s = Setup::with_catalog(catalog, photos);
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let report = s.ok(&s.catalog, fake.port(), true, &[]);
    let d = develop(&report);
    assert_eq!(n(&d["photos"]), 0);
    assert_eq!(n(&d["planned"]), 0);
    assert_eq!(n(&d["written"]), 0);
    assert!(develop_commands(&fake.commands(), 0).is_empty());
}

#[test]
fn an_old_mapping_file_without_develop_hash_is_written_once() {
    let s = Setup::sliders();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(&s.catalog, fake.port(), true, &[]);

    // An older lr2lc did not know the field: drop it from every photo entry.
    let mut state: Value =
        serde_json::from_str(&fs::read_to_string(s.state_file()).unwrap()).unwrap();
    for entry in state["photos"].as_object_mut().unwrap().values_mut() {
        if let Some(map) = entry.as_object_mut() {
            map.remove("develop_hash");
        }
    }
    fs::write(s.state_file(), serde_json::to_string(&state).unwrap()).unwrap();

    let before = fake.commands().len();
    let report = s.ok(&s.catalog, fake.port(), true, &[]);
    let d = develop(&report);
    assert_eq!(
        develop_commands(&fake.commands(), before).len(),
        1,
        "one write, not an error"
    );
    assert_eq!(n(&d["written"]), 1);
}

#[test]
fn the_report_and_the_summary_name_no_invented_value() {
    // The unmapped fixture contains a number and a text that must never be
    // echoed, and the white-balance values must not either.
    let photos = tempfile::tempdir().unwrap();
    let catalog = dev::write_unmapped_catalog_with_files(photos.path()).unwrap();
    let s = Setup::with_catalog(catalog, photos);
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let output = s.run(&s.catalog, fake.port(), true, &[]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let raw = fs::read_to_string(s.report_path()).unwrap();
    for forbidden in [
        format!("{}", e::UNMAPPED_NUMBER),
        e::UNMAPPED_TEXT.to_string(),
    ] {
        assert!(!raw.contains(&forbidden), "report leaks {forbidden}");
        assert!(!stdout.contains(&forbidden), "summary leaks {forbidden}");
    }
    let report: Value = serde_json::from_str(&raw).unwrap();
    let unmapped: Vec<&str> = develop(&report)["unmapped"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for name in e::UNMAPPED_NAMES {
        assert!(unmapped.contains(&name), "{name} missing: {unmapped:?}");
    }
}
