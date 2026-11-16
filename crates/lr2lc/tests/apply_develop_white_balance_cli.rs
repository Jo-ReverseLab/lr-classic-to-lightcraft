//! CLI acceptance tests for the white-balance safety switch (P6-4, phase 6a).
//!
//! Contract checked here (Lead, P6-4). The probe is not implemented yet, so:
//!
//! - `--probe-white-balance` exists and is off by default.
//! - Given while the probe is not implemented, the run aborts with exit code
//!   2 and a clear message that says `white balance probe` and `not
//!   implemented`, before the catalog is opened and before any original file
//!   is opened. Nothing is written: no report, no mapping file, the catalog
//!   stays byte-identical.
//! - Without the option a dry run behaves as before and never opens an
//!   original file, even when the placeholder originals are unreadable.
//! - `report.json`, area `develop`, counts custom white balances (#9):
//!   `white_balance_custom` (converted photos with a custom temperature or
//!   tint), `white_balance_to_check` (dry run: Kelvin values whose transfer
//!   depends on LightCraft's scale), `white_balance_written` and
//!   `white_balance_not_carried` (applied run). Always present, also as 0.
//!   Only counts, never a value or a photo id.
//! - A relative pair (`Incremental*`) is converted and written; a Kelvin pair
//!   is written only when `photo.inspect` shows a raw LightCraft edits in
//!   Kelvin. A named mode carries the white balance by itself.
//! - The readable summary ends the `develop:` line with `custom white balance
//!   N (to check N, written N, not carried N)`.
//!
//! Mandatory in every test: the catalog is byte-identical afterwards,
//! `photos.read == converted + skipped`, and a second run creates nothing
//! twice (see the existing develop CLI tests for the writer side).

use lrcat_fixtures::develop::{edge as e, write_white_balance_catalog_with_files};
use serde_json::Value;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Setup {
    #[cfg_attr(not(unix), allow(dead_code))]
    photos: tempfile::TempDir,
    work: tempfile::TempDir,
    catalog: PathBuf,
    library: PathBuf,
}

impl Setup {
    fn white_balance() -> Setup {
        let photos = tempfile::tempdir().unwrap();
        let catalog = write_white_balance_catalog_with_files(photos.path()).unwrap();
        Setup::with_catalog(catalog, photos)
    }

    fn small() -> Setup {
        let photos = tempfile::tempdir().unwrap();
        let catalog = lrcat_fixtures::write_small_catalog_with_files(photos.path()).unwrap();
        Setup::with_catalog(catalog, photos)
    }

    fn with_catalog(catalog: PathBuf, photos: tempfile::TempDir) -> Setup {
        let work = tempfile::tempdir().unwrap();
        let library = work.path().join("Test.lightcraft");
        Setup {
            photos,
            work,
            catalog,
            library,
        }
    }

    fn report_path(&self) -> PathBuf {
        self.work.path().join("report.json")
    }

    fn state_path(&self) -> PathBuf {
        let mut name = self.library.file_name().unwrap().to_os_string();
        name.push(".lr2lc.json");
        self.work.path().join(name)
    }

    fn args(&self, catalog: &Path, extra: &[&str]) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec![
            catalog.as_os_str().to_owned(),
            OsString::from("--library"),
            self.library.as_os_str().to_owned(),
            OsString::from("--report"),
            self.report_path().into_os_string(),
        ];
        for item in extra {
            args.push(OsString::from(item));
        }
        args
    }

    /// A dry run with `--report`, plus the given options.
    fn run(&self, catalog: &Path, extra: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_lr2lc"))
            .current_dir(self.work.path())
            .args(self.args(catalog, extra))
            .output()
            .unwrap()
    }

    /// An `--apply` run against the fake LightCraft on `port`.
    fn apply(&self, port: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_lr2lc"))
            .current_dir(self.work.path())
            .args(self.args(&self.catalog, &["--port", port, "--apply"]))
            .output()
            .unwrap()
    }

    fn report(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.report_path()).unwrap())
            .expect("a valid report")
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

fn assert_balanced(report: &Value) {
    let photos = &report["photos"];
    assert_eq!(
        n(&photos["read"]),
        n(&photos["converted"]) + photos["skipped"].as_array().unwrap().len(),
        "read = converted + skipped: {photos}"
    );
}

#[test]
fn the_probe_is_refused_before_the_catalog_is_read() {
    let s = Setup::white_balance();
    let before = fs::read(&s.catalog).unwrap();

    let out = s.run(&s.catalog, &["--probe-white-balance"]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let lower = stderr.to_lowercase();
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(
        lower.contains("white balance probe"),
        "the message must name the probe: {stderr}"
    );
    assert!(
        lower.contains("not implemented"),
        "the message must say the probe is not implemented: {stderr}"
    );
    assert!(
        !s.report_path().exists(),
        "the refused run must not write a report"
    );
    assert_eq!(
        before,
        fs::read(&s.catalog).unwrap(),
        "the catalog changed although the run was refused"
    );
}

#[test]
fn the_probe_is_refused_even_before_a_missing_catalog_is_reported() {
    // A path that cannot be opened: if the tool tried the catalog first, it
    // would report a different error and exit 1. The "not implemented" message
    // proves the switch is checked before any catalog access.
    let s = Setup::white_balance();
    let missing = s.work.path().join("does-not-exist.lrcat");

    let out = s.run(&missing, &["--probe-white-balance"]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.to_lowercase().contains("not implemented"),
        "the probe refusal must come before the catalog: {stderr}"
    );
    assert!(
        !s.report_path().exists(),
        "the refused run must not write a report"
    );
}

#[test]
fn help_mentions_the_probe_option() {
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        stdout.contains("--probe-white-balance"),
        "the option is missing from the help: {stdout}"
    );
}

#[test]
fn a_refused_probe_run_creates_no_mapping_file_next_to_the_library() {
    let s = Setup::white_balance();
    let extra = ["--probe-white-balance"];
    let out = Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(s.work.path())
        .args(s.args(&s.catalog, &extra))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.to_lowercase().contains("not implemented"),
        "{stderr}"
    );
    assert!(!s.report_path().exists(), "a refused run writes no report");
    assert!(
        !s.state_path().exists(),
        "a refused run writes no mapping file"
    );
}

/// Without the switch the run behaves as before and never opens an original.
/// The placeholder photos are made unreadable; a dry run that opened one
/// would fail or lose the photo. The white-balance settings are still read
/// from the catalog.
#[cfg(unix)]
#[test]
fn without_the_probe_a_dry_run_never_reads_an_original_photo() {
    use std::fs::File;
    use std::os::unix::fs::PermissionsExt;

    let s = Setup::white_balance();
    let mut blocked = Vec::new();
    make_originals_unreadable(s.photos.path(), &mut blocked);
    assert!(!blocked.is_empty(), "the fixture must create originals");
    for path in &blocked {
        assert!(
            File::open(path).is_err(),
            "the canary at {} is still readable",
            path.display()
        );
    }
    let before = fs::read(&s.catalog).unwrap();

    let out = s.run(&s.catalog, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    assert_eq!(
        before,
        fs::read(&s.catalog).unwrap(),
        "the catalog changed during the run"
    );

    let report: Value = serde_json::from_str(&fs::read_to_string(s.report_path()).unwrap())
        .expect("a valid report");
    assert_balanced(&report);
    assert_eq!(
        n(&develop(&report)["photos"]),
        9,
        "the settings are read without opening the originals (two photos of \
         the fixture have no original and are skipped)"
    );
    assert!(!s.state_path().exists(), "a dry run writes no mapping file");
    assert!(
        !s.library.exists(),
        "a dry run creates no LightCraft library folder"
    );

    for path in &blocked {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o600);
        let _ = fs::set_permissions(path, permissions);
    }
}

/// Takes read permission from every file below `root` except the catalog.
#[cfg(unix)]
fn make_originals_unreadable(root: &Path, blocked: &mut Vec<PathBuf>) {
    use std::os::unix::fs::PermissionsExt;

    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            make_originals_unreadable(&path, blocked);
            continue;
        }
        if path.extension().is_some_and(|ext| ext == "lrcat") {
            continue;
        }
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o000);
        fs::set_permissions(&path, permissions).unwrap();
        blocked.push(path);
    }
}

fn state(s: &Setup) -> Value {
    serde_json::from_str(&fs::read_to_string(s.state_path()).unwrap()).expect("a mapping file")
}

/// The LightCraft id the mapping file records for a Lightroom photo.
fn lightcraft_id(s: &Setup, lightroom_id: i64) -> i64 {
    state(s)["photos"]
        .as_object()
        .unwrap()
        .values()
        .find(|p| p["lightroom_id"] == lightroom_id)
        .and_then(|p| p["lightcraft_id"].as_i64())
        .unwrap_or_else(|| panic!("photo {lightroom_id} is not in the mapping file"))
}

/// The file name of a fixture photo, learned from a throwaway applied run
/// (the fixture's names are not exported). Used to tell the fake which
/// photo is a Kelvin raw.
fn file_name_of(lightroom_id: i64) -> String {
    use lc_test_support::fake_lightcraft::{Config, FakeLightCraft};

    let s = Setup::white_balance();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let out = s.apply(&fake.port().to_string());
    assert_eq!(out.status.code(), Some(0));
    let path = fake.path_of(lightcraft_id(&s, lightroom_id)).unwrap();
    let name = Path::new(&path).file_name().unwrap().to_string_lossy();
    format!("/{name}")
}

/// The `develop.set` parameters the run sent for one LightCraft photo that
/// carry a white balance.
fn white_balance_sets(
    fake: &lc_test_support::fake_lightcraft::FakeLightCraft,
    id: i64,
) -> Vec<Value> {
    fake.commands()
        .into_iter()
        .filter(|(command, _)| command == "develop.set")
        .map(|(_, params)| params)
        .filter(|p| p["ids"] == serde_json::json!([id]) && p["values"].get("wb.temp").is_some())
        .collect()
}

#[test]
fn the_dry_run_counts_custom_white_balances_and_leaks_no_value() {
    let s = Setup::white_balance();
    let out = s.run(&s.catalog, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    let report: Value = serde_json::from_str(&fs::read_to_string(s.report_path()).unwrap())
        .expect("a valid report");
    assert_balanced(&report);

    let d = develop(&report);
    assert_eq!(
        n(&d["white_balance_custom"]),
        2,
        "photo {} (Kelvin) and photo {} (incremental) are the only custom pairs; \
         the named modes with numbers are carried by their mode",
        e::WB_ABSOLUTE_PHOTO,
        e::WB_RELATIVE_PHOTO
    );
    assert_eq!(n(&d["white_balance_to_check"]), 1, "only the Kelvin pair");
    assert_eq!(n(&d["white_balance_written"]), 0);
    assert_eq!(n(&d["white_balance_not_carried"]), 0);
    // Count only: no stored value may leak.
    let raw = fs::read_to_string(s.report_path()).unwrap();
    assert!(
        !raw.contains(&e::WB_TEMPERATURE.to_string()),
        "the report leaks the stored temperature"
    );
    for id in [e::WB_ABSOLUTE_PHOTO, e::WB_RELATIVE_PHOTO] {
        assert!(
            !raw.contains(&format!("photo {id}")),
            "the counter must not name photo {id}"
        );
    }
}

#[test]
fn a_catalog_without_white_balance_fields_reports_zero() {
    let s = Setup::small();
    let out = s.run(&s.catalog, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    let report: Value = serde_json::from_str(&fs::read_to_string(s.report_path()).unwrap())
        .expect("a valid report");
    let d = develop(&report);
    assert_eq!(n(&d["photos"]), 0);
    for key in [
        "white_balance_custom",
        "white_balance_to_check",
        "white_balance_written",
        "white_balance_not_carried",
    ] {
        assert_eq!(n(&d[key]), 0, "{key}");
    }
}

#[test]
fn the_dry_run_summary_names_the_white_balance_counts() {
    let s = Setup::white_balance();
    let out = s.run(&s.catalog, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    assert!(
        stdout.contains("custom white balance 2 (to check 1, written 0, not carried 0)"),
        "the summary must name the counts: {stdout}"
    );

    let small = Setup::small();
    let out = small.run(&small.catalog, &[]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        stdout.contains("custom white balance 0 (to check 0, written 0, not carried 0)"),
        "the summary must carry the zeros too: {stdout}"
    );
}

#[test]
fn a_relative_pair_is_written_and_a_kelvin_pair_waits_for_a_kelvin_scale() {
    use lc_test_support::fake_lightcraft::{Config, FakeLightCraft};

    // The fake answers every photo as a rendered file: the Kelvin pair is
    // not carried, the relative pair is.
    let s = Setup::white_balance();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let before = fs::read(&s.catalog).unwrap();
    let out = s.apply(&fake.port().to_string());
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    assert_eq!(before, fs::read(&s.catalog).unwrap(), "the catalog changed");

    let report = s.report();
    assert_balanced(&report);
    let d = develop(&report);
    assert_eq!(n(&d["white_balance_custom"]), 2);
    assert_eq!(n(&d["white_balance_to_check"]), 0);
    assert_eq!(n(&d["white_balance_written"]), 1);
    assert_eq!(n(&d["white_balance_not_carried"]), 1);
    assert_eq!(n(&d["written"]), 9, "every settings row is still written");

    let absolute = lightcraft_id(&s, e::WB_ABSOLUTE_PHOTO);
    assert!(
        white_balance_sets(&fake, absolute).is_empty(),
        "a Kelvin value on a relative scale would be wrong"
    );
    assert!(
        fake.commands()
            .iter()
            .any(|(c, p)| c == "photo.inspect" && p["id"] == absolute),
        "the scale is asked for the Kelvin photo"
    );

    let relative = lightcraft_id(&s, e::WB_RELATIVE_PHOTO);
    let sets = white_balance_sets(&fake, relative);
    assert_eq!(sets.len(), 1, "{sets:?}");
    let temp = sets[0]["values"]["wb.temp"].as_f64().unwrap();
    // -20 steps of 0.8 mired around 6500 K.
    let expected = 1e6 / (1e6 / 6500.0 + 0.8 * 20.0);
    assert!((temp - expected).abs() < 1e-6, "{temp} vs {expected}");
    assert_eq!(sets[0]["values"]["wb.tint"], e::WB_INCREMENTAL_TINT);
    assert!(
        !fake
            .commands()
            .iter()
            .any(|(c, p)| c == "photo.inspect" && p["id"] == relative),
        "a relative pair needs no scale check"
    );
}

#[test]
fn a_kelvin_pair_is_written_as_is_on_a_kelvin_raw() {
    use lc_test_support::fake_lightcraft::{Config, FakeLightCraft};

    let name = file_name_of(e::WB_ABSOLUTE_PHOTO);
    let s = Setup::white_balance();
    let mut config = Config::new(&s.library);
    config.kelvin_raw_for.push(name);
    let fake = FakeLightCraft::start(config);
    let out = s.apply(&fake.port().to_string());
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(0), "{stderr}");

    let d = develop(&s.report()).clone();
    assert_eq!(n(&d["white_balance_written"]), 2);
    assert_eq!(n(&d["white_balance_not_carried"]), 0);
    let sets = white_balance_sets(&fake, lightcraft_id(&s, e::WB_ABSOLUTE_PHOTO));
    assert_eq!(sets.len(), 1, "{sets:?}");
    assert_eq!(
        sets[0]["values"],
        serde_json::json!({"wb.temp": e::WB_TEMPERATURE, "wb.tint": e::WB_TINT})
    );
}

#[test]
fn a_raw_rendered_from_its_preview_is_not_a_kelvin_raw() {
    use lc_test_support::fake_lightcraft::{Config, FakeLightCraft};

    let name = file_name_of(e::WB_ABSOLUTE_PHOTO);
    let s = Setup::white_balance();
    let mut config = Config::new(&s.library);
    config.preview_only_for.push(name);
    let fake = FakeLightCraft::start(config);
    let out = s.apply(&fake.port().to_string());
    assert_eq!(out.status.code(), Some(0));
    let d = develop(&s.report()).clone();
    assert_eq!(n(&d["white_balance_written"]), 1);
    assert_eq!(n(&d["white_balance_not_carried"]), 1);
    assert!(white_balance_sets(&fake, lightcraft_id(&s, e::WB_ABSOLUTE_PHOTO)).is_empty());
}

#[test]
fn a_rejected_white_balance_leaves_the_photo_incomplete_for_the_next_run() {
    use lc_test_support::fake_lightcraft::{Config, Failure, FakeLightCraft};

    let s = Setup::white_balance();
    let mut config = Config::new(&s.library);
    config.failures.push(Failure {
        command: "photo.inspect".into(),
        photo_id: None,
        message: "busy".into(),
    });
    let fake = FakeLightCraft::start(config);
    let out = s.apply(&fake.port().to_string());
    assert_eq!(out.status.code(), Some(0));
    let report = s.report();
    let d = develop(&report);
    assert_eq!(n(&d["white_balance_written"]), 1);
    assert_eq!(n(&d["white_balance_not_carried"]), 0);
    assert_eq!(n(&d["written"]), 8, "the photo is incomplete, not written");
    let incomplete = report["writer"]["photos_incomplete"].to_string();
    assert!(incomplete.contains("photo.inspect"), "{incomplete}");
}

#[test]
fn a_second_apply_run_sends_no_develop_command_again() {
    use lc_test_support::fake_lightcraft::{Config, FakeLightCraft};

    let s = Setup::white_balance();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let port = fake.port().to_string();

    let first = s.apply(&port);
    assert_eq!(
        first.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let sent = fake.commands().len();

    let second = s.apply(&port);
    let stderr = String::from_utf8_lossy(&second.stderr).into_owned();
    assert_eq!(second.status.code(), Some(0), "{stderr}");
    let second_report = s.report();
    assert_balanced(&second_report);

    let again: Vec<String> = fake.commands()[sent..]
        .iter()
        .map(|(command, _)| command.clone())
        .filter(|command| command.starts_with("develop.") || command == "photo.inspect")
        .collect();
    assert!(again.is_empty(), "a second run sent {again:?}");
    let d = develop(&second_report);
    assert_eq!(n(&d["planned"]), 0);
    assert_eq!(
        n(&d["white_balance_custom"]),
        2,
        "the settings are still read on the second run"
    );
    assert_eq!(n(&d["white_balance_written"]), 0);
    assert_eq!(n(&d["white_balance_not_carried"]), 0);
}
