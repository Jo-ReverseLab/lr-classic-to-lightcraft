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
//! - `report.json`, top-level area `develop`, gains the counter
//!   `white_balance_not_carried`: the number of converted photos whose
//!   `Develop` holds a `Temperature`/`Tint` pair (absolute or incremental)
//!   that was not carried over. The counter is always present, also as 0.
//!   Only the count is reported, never a value or a photo id.
//! - The readable summary names the same number on the existing `develop:`
//!   line as `white balance not carried N`.
//!
//! `white_balance_not_carried` is the field name fixed by the test author for
//! P6-4; the developer follows it. It is the last key of the `develop` area
//! (after `overwritten`), and the summary appends `, white balance not
//! carried N` to the end of the existing `develop:` line. The golden reports
//! in this repository already carry the key and the phrase, so a
//! field placed exactly there needs no regeneration.
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

#[test]
fn white_balance_not_carried_counts_only_photos_with_a_temperature_pair() {
    let s = Setup::white_balance();
    let out = s.run(&s.catalog, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    let report: Value = serde_json::from_str(&fs::read_to_string(s.report_path()).unwrap())
        .expect("a valid report");
    assert_balanced(&report);

    let d = develop(&report);
    assert_eq!(
        n(&d["white_balance_not_carried"]),
        2,
        "photo {} (absolute) and photo {} (incremental) are the only pairs",
        e::WB_ABSOLUTE_PHOTO,
        e::WB_RELATIVE_PHOTO
    );
    // Count only: no stored value may leak. (The unmapped field *names* may
    // appear, that is their documented report; the numbers may not.)
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
    assert_eq!(n(&develop(&report)["photos"]), 0);
    assert_eq!(n(&develop(&report)["white_balance_not_carried"]), 0);
}

#[test]
fn the_dry_run_summary_names_the_white_balance_count() {
    let s = Setup::white_balance();
    let out = s.run(&s.catalog, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    assert!(
        stdout.contains("white balance not carried 2"),
        "the summary must name the count: {stdout}"
    );

    let small = Setup::small();
    let out = small.run(&small.catalog, &[]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        stdout.contains("white balance not carried 0"),
        "the summary must carry the zero too: {stdout}"
    );
}

#[test]
fn the_applied_run_reports_the_same_white_balance_count() {
    use lc_test_support::fake_lightcraft::{Config, FakeLightCraft};

    let s = Setup::white_balance();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    let port = fake.port().to_string();
    let before = fs::read(&s.catalog).unwrap();

    let out = s.apply(&port);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    // A run that stops still writes its report; this fixture must not stop.
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    assert_eq!(
        before,
        fs::read(&s.catalog).unwrap(),
        "the catalog changed during the applied run"
    );

    let report = s.report();
    assert_balanced(&report);
    let d = develop(&report);
    assert_eq!(n(&d["white_balance_not_carried"]), 2);
    assert_eq!(
        n(&d["written"]),
        9,
        "every generated settings row of a converted photo is written"
    );
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

    let again = fake.commands()[sent..]
        .iter()
        .filter(|(command, _)| command.starts_with("develop."))
        .count();
    assert_eq!(again, 0, "a second run must create no duplicate settings");
    let d = develop(&second_report);
    assert_eq!(n(&d["planned"]), 0);
    assert_eq!(
        n(&d["white_balance_not_carried"]),
        2,
        "the settings are still read on the second run"
    );
}
