//! `--existing overwrite|develop|develop-if-empty|skip`: what happens to
//! photos LightCraft already has but the mapping file does not know.
//!
//! - an invalid or repeated value ends with exit code 2 before anything is
//!   read;
//! - a dry run never connects and says `existing: checked when transferring`;
//! - with `--apply` the library is checked before writing; the summary says
//!   how many photos were found and what was done with them, and the report
//!   carries the same counts in `writer.existing`;
//! - the default is `develop-if-empty`; `skip` writes nothing to such a
//!   photo.

use lrcat_fixtures::write_keyword_edge_catalog_with_files;
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
        lc_writer::state::state_path(&self.library)
    }

    /// `lr2lc <catalog> --library <lib> --report <file> --port <port> [extra]`
    fn run(&self, port: u16, extra: &[&str]) -> Output {
        let port = port.to_string();
        let report = self.report();
        let mut args: Vec<&OsStr> = vec![
            self.catalog.as_os_str(),
            OsStr::new("--library"),
            self.library.as_os_str(),
            OsStr::new("--report"),
            report.as_os_str(),
            OsStr::new("--port"),
            OsStr::new(&port),
        ];
        args.extend(extra.iter().map(OsStr::new));
        Command::new(env!("CARGO_BIN_EXE_lr2lc"))
            .current_dir(self.work.path())
            .args(args)
            .output()
            .unwrap()
    }

    fn writer(&self) -> serde_json::Value {
        let report: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(self.report()).unwrap()).unwrap();
        report["writer"].clone()
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn assert_exit(o: &Output, code: i32) {
    assert_eq!(o.status.code(), Some(code), "{}", text(o));
}

/// The `existing:` line of the summary.
fn existing_line(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .find(|l| l.starts_with("existing:"))
        .unwrap_or_else(|| panic!("no `existing:` line in:\n{}", text(o)))
        .to_owned()
}

#[test]
fn an_invalid_or_repeated_choice_ends_with_exit_code_2() {
    let s = setup();
    let o = s.run(1, &["--existing", "keep"]);
    assert_exit(&o, 2);
    assert!(text(&o).contains("--existing: unknown choice `keep`"));
    let o = s.run(1, &["--existing", "skip", "--existing", "develop"]);
    assert_exit(&o, 2);
    assert!(text(&o).contains("--existing given more than once"));
    assert!(!s.report().exists());
}

#[test]
fn a_dry_run_says_the_check_happens_when_transferring() {
    let s = setup();
    // Nobody listens on port 1: a dry run never connects.
    let o = s.run(1, &["--existing", "overwrite"]);
    assert_exit(&o, 0);
    assert_eq!(existing_line(&o), "existing: checked when transferring");
}

/// A first run writes everything; then the mapping file is lost, so every
/// photo of the second run is one LightCraft already has.
fn library_without_mapping_file(s: &Setup) -> FakeLightCraft {
    let fake = FakeLightCraft::start(Config::new(&s.library));
    assert_exit(&s.run(fake.port(), &["--apply"]), 0);
    fs::remove_file(s.state_file()).unwrap();
    fake
}

#[test]
fn apply_reports_the_photos_found_with_the_default_choice() {
    let s = setup();
    let fake = library_without_mapping_file(&s);
    let photos = fake.photos();
    let o = s.run(fake.port(), &["--apply"]);
    assert_exit(&o, 0);
    let w = s.writer();
    let found = w["photos_existing"].as_u64().unwrap();
    assert!(found > 0);
    assert_eq!(w["photos_imported"], 0);
    assert_eq!(w["existing"]["found"], found);
    assert_eq!(w["existing"]["develop_if_empty"], found);
    assert_eq!(w["existing"]["found_while_importing"], 0);
    assert_eq!(
        existing_line(&o),
        format!(
            "existing: already in LightCraft {found} (overwrite 0, develop 0, \
             develop-if-empty {found}, develop kept 0, skip 0; found while importing 0)"
        )
    );
    assert_eq!(fake.photos(), photos, "nothing imported twice");
}

#[test]
fn skip_writes_nothing_to_photos_lightcraft_already_has() {
    let s = setup();
    let fake = library_without_mapping_file(&s);
    let before = fake.commands().len();
    let o = s.run(fake.port(), &["--apply", "--existing", "skip"]);
    assert_exit(&o, 0);
    let w = s.writer();
    assert_eq!(w["existing"]["skip"], w["photos_existing"]);
    let photo_writes: Vec<String> = fake.commands()[before..]
        .iter()
        .map(|(c, _)| c.clone())
        .filter(|c| c.starts_with("photo.") && c != "photo.inspect" || c.starts_with("develop."))
        .collect();
    assert!(photo_writes.is_empty(), "{photo_writes:?}");
}

#[test]
fn photos_deleted_in_lightcraft_are_imported_again_by_default() {
    let s = setup();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    assert_exit(&s.run(fake.port(), &["--apply"]), 0);
    let o = s.run(1, &["--removed", "leave"]);
    assert_exit(&o, 0);
    assert!(
        String::from_utf8_lossy(&o.stdout)
            .contains("removed in LightCraft: checked when transferring")
    );
    let deleted: Vec<i64> = fake.photos().values().copied().take(2).collect();
    for id in &deleted {
        fake.delete_permanently(*id);
    }
    let count = fake.photos().len();

    let o = s.run(fake.port(), &["--apply", "--removed", "leave"]);
    assert_exit(&o, 0);
    assert!(text(&o).contains("removed in LightCraft: 2 (imported again 0"));
    assert_eq!(fake.photos().len(), count, "left as they are");

    let o = s.run(fake.port(), &["--apply"]);
    assert_exit(&o, 0);
    let w = s.writer();
    assert_eq!(w["removed_in_lightcraft"]["found"], 2);
    assert_eq!(w["removed_in_lightcraft"]["reimported"], 2);
    assert_eq!(fake.photos().len(), count + 2);

    let o = s.run(fake.port(), &["--apply"]);
    assert_exit(&o, 0);
    assert_eq!(s.writer()["removed_in_lightcraft"]["found"], 0);
    assert_eq!(s.writer()["photos_imported"], 0);
}
