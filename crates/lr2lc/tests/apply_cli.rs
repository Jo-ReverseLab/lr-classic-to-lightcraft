//! Acceptance tests for `--apply` and the writer options of the CLI, against a stateful fake LightCraft (`tests/support`).
//!
//! New options: `--library <folder>`, `--port <n>` (default 7980),
//! `--allow-xmp`, `--script <file>`.
//!
//! Behaviour checked:
//! - `--apply` without `--library` -> exit 2 before anything is read; a bad
//!   `--port` value -> exit 2;
//! - a dry run (no `--apply`) never opens a connection (Principle 3). With
//!   `--library` the state is loaded from `lc_writer::state::state_path`,
//!   otherwise it is empty. stdout gets one line
//!   `writer: keywords to create K, photos to import P, updates U`;
//!   report.json gets `writer.planned = {"keywords": K, "photos": P,
//!   "updates": U}` and `writer.photos_skipped` and `writer.keywords_not_created`
//!   (each `[{"reason"}]`). Nothing is written
//!   next to the library (no state file, no library folder);
//! - `--script FILE` writes the plan as JSON lines, also in a dry run, with
//!   the same protection as `--report` (never the catalog or its companions);
//! - `--apply` with `--library`: catalog read as in a dry run, then
//!   pre-flight, then the writer. Exit 0; report.json has `dry_run: false`
//!   and `writer = {keywords_created, photos_imported, photos_existing,
//!   updates, not_saved, photos_skipped, photos_incomplete, keywords_not_created
//!   (each `[{"reason"}]`)}`; the strict balance holds (a photo counts once):
//!   `photos.converted == photos_imported + photos_existing + (photos
//!   skipped by the writer)`; no names in stdout or report.json;
//! - pre-flight or connection failures (XMP auto-write without
//!   `--allow-xmp`, wrong library, no server, lost connection) -> exit 1 with
//!   a clear message and no writing command; a lost connection says
//!   "outcome unknown, run again";
//! - always: the catalog is byte-identical afterwards, originals are not
//!   touched, a second `--apply` sends only read commands.
//!

use lrcat_fixtures::write_keyword_edge_catalog_with_files;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use fake_lightcraft::{Config, FakeLightCraft, READ_COMMANDS};
use lc_test_support::fake_lightcraft;

/// Names that must never appear in stdout or report.json.
const SECRET_NAMES: [&str; 6] = [
    "Colosseum",
    "Süßwasser",
    "Köln",
    "🌅",
    "Sunrise",
    "Bad|Name",
];

struct Setup {
    /// Holds the catalog and the original files.
    photos: tempfile::TempDir,
    /// Holds the library folder, the state file, the report and the script.
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
        photos,
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

    fn fake(&self) -> FakeLightCraft {
        FakeLightCraft::start(Config::new(&self.library))
    }

    /// `lr2lc <catalog> --library <lib> --port <port> --report <file> [extra]`
    fn run(&self, port: u16, extra: &[&str]) -> Output {
        let mut args: Vec<&OsStr> = vec![self.catalog.as_os_str()];
        args.extend([
            OsStr::new("--library"),
            self.library.as_os_str(),
            OsStr::new("--report"),
        ]);
        let report = self.report();
        let port_text = port.to_string();
        let mut owned: Vec<&OsStr> = args;
        owned.push(report.as_os_str());
        owned.extend([OsStr::new("--port"), OsStr::new(&port_text)]);
        owned.extend(extra.iter().map(OsStr::new));
        run(self.work.path(), &owned)
    }

    fn report_json(&self) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(self.report()).unwrap()).unwrap()
    }
}

fn run(cwd: &Path, args: &[&OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn assert_exit(o: &Output, code: i32) {
    assert_eq!(
        o.status.code(),
        Some(code),
        "stdout: {}\nstderr: {}",
        stdout(o),
        stderr(o)
    );
}

/// All files below `dir` with their content.
fn tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                todo.push(p);
            } else {
                out.insert(p.clone(), fs::read(&p).unwrap());
            }
        }
    }
    out
}

/// A port nobody listens on.
fn closed_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

/// `(keywords, photos, updates)` from the stdout line
/// `writer: keywords to create K, photos to import P, updates U`.
fn planned_line(text: &str) -> (u64, u64, u64) {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("writer:"))
        .unwrap_or_else(|| panic!("no `writer:` line in:\n{text}"));
    let numbers: Vec<u64> = ["keywords to create ", "photos to import ", "updates "]
        .iter()
        .map(|key| {
            let rest = line
                .split(key)
                .nth(1)
                .unwrap_or_else(|| panic!("{key}: {line}"));
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().unwrap_or_else(|_| panic!("{key}: {line}"))
        })
        .collect();
    (numbers[0], numbers[1], numbers[2])
}

fn no_secrets(text: &str) {
    for name in SECRET_NAMES {
        assert!(!text.contains(name), "{name} leaked");
    }
}

fn only_reads(fake: &FakeLightCraft, from: usize) {
    let sent: Vec<String> = fake.commands()[from..]
        .iter()
        .map(|(c, _)| c.clone())
        .collect();
    assert!(
        sent.iter().all(|c| READ_COMMANDS.contains(&c.as_str())),
        "{sent:?}"
    );
}

// ------------------------------------------------------------ usage errors

#[test]
fn apply_without_library_is_a_usage_error_and_reads_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let fake = FakeLightCraft::start(Config::new(&dir.path().join("x.lightcraft")));
    let report = dir.path().join("report.json");
    let missing = dir.path().join("does-not-exist.lrcat");
    let port = fake.port().to_string();
    let o = run(
        dir.path(),
        &[
            missing.as_os_str(),
            OsStr::new("--apply"),
            OsStr::new("--port"),
            OsStr::new(&port),
            OsStr::new("--report"),
            report.as_os_str(),
        ],
    );
    assert_exit(&o, 2);
    assert!(stderr(&o).contains("--library"), "{}", stderr(&o));
    assert_eq!(fake.connections(), 0);
    assert!(!report.exists(), "nothing is written");
}

#[test]
fn apply_without_library_on_a_real_catalog_changes_nothing() {
    let s = setup();
    let before = fs::read(&s.catalog).unwrap();
    let fake = s.fake();
    let port = fake.port().to_string();
    let o = run(
        s.work.path(),
        &[
            s.catalog.as_os_str(),
            OsStr::new("--apply"),
            OsStr::new("--port"),
            OsStr::new(&port),
        ],
    );
    assert_exit(&o, 2);
    assert_eq!(fake.connections(), 0);
    assert_eq!(fs::read(&s.catalog).unwrap(), before);
}

#[test]
fn invalid_port_is_a_usage_error() {
    let s = setup();
    for bad in ["abc", "-1", "70000", ""] {
        let o = run(
            s.work.path(),
            &[s.catalog.as_os_str(), OsStr::new("--port"), OsStr::new(bad)],
        );
        assert_exit(&o, 2);
    }
}

#[test]
fn help_documents_the_writer_options() {
    let o = run(Path::new("."), &[OsStr::new("--help")]);
    assert_exit(&o, 0);
    let text = stdout(&o);
    for opt in ["--library", "--port", "--allow-xmp", "--script", "7980"] {
        assert!(text.contains(opt), "{opt}");
    }
}

// ------------------------------------------------------------------ dry run

#[test]
fn dry_run_never_connects_and_prints_the_plan_numbers() {
    let s = setup();
    let fake = s.fake();
    let before = fs::read(&s.catalog).unwrap();
    let o = s.run(fake.port(), &[]);
    assert_exit(&o, 0);
    assert_eq!(fake.connections(), 0, "a dry run must not connect");
    assert!(fake.received().is_empty());

    let (keywords, photos, updates) = planned_line(&stdout(&o));
    assert!(keywords > 0 && photos > 0 && updates > 0);
    let report = s.report_json();
    assert_eq!(report["writer"]["planned"]["keywords"], keywords);
    assert_eq!(report["writer"]["planned"]["photos"], photos);
    assert_eq!(report["writer"]["planned"]["updates"], updates);
    assert_eq!(report["dry_run"], true);
    // Every photo that is read and converted is planned or skipped.
    let skipped_photos = report["writer"]["photos_skipped"].as_array().unwrap().len() as u64;
    assert!(report["writer"]["keywords_not_created"].is_array());
    assert_eq!(
        report["photos"]["converted"].as_u64().unwrap(),
        photos + skipped_photos
    );
    assert_eq!(fs::read(&s.catalog).unwrap(), before);
}

#[test]
fn dry_run_writes_nothing_next_to_the_library() {
    let s = setup();
    let originals = tree(s.photos.path());
    let o = s.run(closed_port(), &[]);
    assert_exit(&o, 0);
    assert!(!s.library.exists());
    assert!(!s.state_file().exists());
    assert_eq!(
        tree(s.photos.path()),
        originals,
        "originals and catalog are untouched"
    );
}

#[test]
fn dry_run_without_library_uses_an_empty_state() {
    let s = setup();
    let o = run(s.work.path(), &[s.catalog.as_os_str()]);
    assert_exit(&o, 0);
    let with_library = s.run(closed_port(), &[]);
    assert_eq!(
        planned_line(&stdout(&o)),
        planned_line(&stdout(&with_library))
    );
}

#[test]
fn dry_run_output_has_no_names() {
    let s = setup();
    let o = s.run(closed_port(), &[]);
    assert_exit(&o, 0);
    no_secrets(&stdout(&o));
    no_secrets(&fs::read_to_string(s.report()).unwrap());
}

#[test]
fn dry_run_after_a_complete_apply_plans_nothing() {
    let s = setup();
    let fake = s.fake();
    assert_exit(&s.run(fake.port(), &["--apply"]), 0);
    let o = s.run(fake.port(), &[]);
    assert_exit(&o, 0);
    assert_eq!(
        planned_line(&stdout(&o)),
        (0, 0, 0),
        "the state file is used"
    );
}

// ------------------------------------------------------------------- script

#[test]
fn script_in_a_dry_run_contains_valid_json_lines() {
    let s = setup();
    let fake = s.fake();
    let script = s.work.path().join("plan.jsonl");
    let o = s.run(fake.port(), &["--script", script.to_str().unwrap()]);
    assert_exit(&o, 0);
    assert_eq!(fake.connections(), 0);
    let (keywords, photos, updates) = planned_line(&stdout(&o));

    let text = fs::read_to_string(&script).unwrap();
    assert!(text.ends_with('\n'));
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l}: {e}")))
        .collect();
    // Album commands and the selects before them are checked in
    // `apply_albums_cli.rs`; the numbers of this line do not count them.
    let command = |l: &serde_json::Value| l["params"]["command"].as_str().unwrap_or("").to_string();
    let is_album = |l: &serde_json::Value| command(l).starts_with("album.");
    let is_member_change = |l: &serde_json::Value| {
        let c = command(l);
        c == "album.addPhotos" || c == "album.removePhotos"
    };
    let lines: Vec<serde_json::Value> = lines
        .iter()
        .enumerate()
        .filter(|(i, l)| {
            !is_album(l)
                && !(command(l) == "library.select"
                    && lines.get(i + 1).is_some_and(is_member_change))
        })
        .map(|(_, l)| l.clone())
        .collect();
    // `library.select` lines are not updates: one per photo with
    // updates, before them.
    let selects = lines
        .iter()
        .filter(|l| l["params"]["command"] == "library.select")
        .count() as u64;
    assert_eq!(lines.len() as u64, keywords + photos + updates + selects);
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for l in &lines {
        assert_eq!(l["method"], "engine.execute");
        let command = l["params"]["command"].as_str().unwrap();
        *counts.entry(command.to_string()).or_default() += 1;
        match command {
            "keyword.create" => assert!(l["params"]["params"].get("ids").is_none()),
            "library.import" => {
                let p = &l["params"]["params"];
                assert_eq!(p["mode"], "add");
                assert_eq!(p["paths"].as_array().unwrap().len(), 1);
                assert_eq!(p.as_object().unwrap().len(), 2);
            }
            "library.select" => {
                let p = &l["params"]["params"];
                let id = p["ids"][0].as_str().unwrap();
                assert!(id.starts_with("lr:"), "{id}: photo not imported yet");
                assert_eq!(p["ids"], serde_json::json!([p["active"].clone()]));
            }
            c if c.starts_with("photo.") => {
                let id = l["params"]["params"]["ids"][0].as_str().unwrap();
                assert!(id.starts_with("lr:"), "{id}: photo not imported yet");
            }
            other => panic!("unexpected command {other}"),
        }
    }
    assert_eq!(counts["keyword.create"], keywords);
    assert_eq!(counts["library.import"], photos);
}

#[test]
fn script_without_a_library_works_too() {
    let s = setup();
    let script = s.work.path().join("plan.jsonl");
    let o = run(
        s.work.path(),
        &[
            s.catalog.as_os_str(),
            OsStr::new("--script"),
            script.as_os_str(),
        ],
    );
    assert_exit(&o, 0);
    assert!(fs::metadata(&script).unwrap().len() > 0);
}

#[test]
fn script_refuses_an_existing_foreign_file() {
    let s = setup();
    let photo = s.work.path().join("photo.jpg");
    fs::write(&photo, b"\xff\xd8 not a script").unwrap();
    // A file whose lines are JSON but not engine.execute requests is foreign too.
    let other = s.work.path().join("other.json");
    fs::write(&other, b"{\"method\":\"something.else\"}\n").unwrap();
    for target in [&photo, &other] {
        let before = fs::read(target).unwrap();
        let o = run(
            s.work.path(),
            &[
                s.catalog.as_os_str(),
                OsStr::new("--script"),
                target.as_os_str(),
            ],
        );
        assert_exit(&o, 2);
        assert_eq!(fs::read(target).unwrap(), before, "file was modified");
    }
}

#[test]
fn script_overwrites_an_earlier_script_or_an_empty_file() {
    let s = setup();
    let earlier = s.work.path().join("earlier.jsonl");
    fs::write(
        &earlier,
        b"{\"method\":\"engine.execute\",\"params\":{\"command\":\"x\",\"params\":{}}}\n\n",
    )
    .unwrap();
    let empty = s.work.path().join("empty.jsonl");
    fs::write(&empty, b"").unwrap();
    for target in [&earlier, &empty] {
        let o = run(
            s.work.path(),
            &[
                s.catalog.as_os_str(),
                OsStr::new("--script"),
                target.as_os_str(),
            ],
        );
        assert_exit(&o, 0);
        let text = fs::read_to_string(target).unwrap();
        assert!(!text.contains("\"command\":\"x\""), "old content is gone");
        assert!(text.contains("keyword.create"));
    }
}

#[test]
fn script_never_overwrites_the_catalog_or_its_companions() {
    let s = setup();
    let wal = {
        let mut n = s.catalog.file_name().unwrap().to_os_string();
        n.push("-wal");
        s.catalog.with_file_name(n)
    };
    fs::write(&wal, b"keep").unwrap();
    for target in [s.catalog.clone(), wal.clone()] {
        let before = fs::read(&target).unwrap();
        let o = run(
            s.work.path(),
            &[
                s.catalog.as_os_str(),
                OsStr::new("--script"),
                target.as_os_str(),
            ],
        );
        assert_exit(&o, 2);
        assert!(stderr(&o).contains("catalog"), "{}", stderr(&o));
        assert_eq!(fs::read(&target).unwrap(), before);
    }
}

// -------------------------------------------------------------------- apply

#[test]
fn apply_writes_through_the_port_and_reports_the_numbers() {
    let s = setup();
    let fake = s.fake();
    let catalog_before = fs::read(&s.catalog).unwrap();
    let originals = tree(s.photos.path());
    let dry = s.run(closed_port(), &[]);
    let (keywords, photos, updates) = planned_line(&stdout(&dry));

    let o = s.run(fake.port(), &["--apply"]);
    assert_exit(&o, 0);

    let report = s.report_json();
    let w = &report["writer"];
    assert_eq!(report["dry_run"], false);
    assert_eq!(w["keywords_created"], keywords);
    assert_eq!(w["photos_imported"], photos);
    assert_eq!(w["photos_existing"], 0);
    assert_eq!(w["updates"], updates);
    assert_eq!(w["not_saved"], 0);
    let skipped = w["photos_skipped"].as_array().unwrap();
    for item in skipped
        .iter()
        .chain(w["photos_incomplete"].as_array().unwrap())
        .chain(w["keywords_not_created"].as_array().unwrap())
    {
        assert!(!item["reason"].as_str().unwrap().is_empty());
    }
    // The books balance: read = converted + skipped, converted = written + skipped by the writer.
    let p = &report["photos"];
    assert_eq!(
        p["read"].as_u64().unwrap(),
        p["converted"].as_u64().unwrap() + p["skipped"].as_array().unwrap().len() as u64
    );
    assert_eq!(
        p["converted"].as_u64().unwrap(),
        photos + skipped.len() as u64,
        "converted photos = written + skipped by the writer"
    );

    // The fake received what the report says.
    assert_eq!(fake.photos().len() as u64, photos);
    assert_eq!(fake.keywords().len() as u64, keywords);
    for path in fake.photos().keys() {
        assert!(Path::new(path).starts_with(s.photos.path()), "{path}");
        assert!(
            Path::new(path).exists(),
            "{path}: only existing files are imported"
        );
    }
    assert!(
        s.state_file().exists(),
        "the mapping file is next to the library"
    );

    // Principles 1 and 2: the catalog and the originals are unchanged.
    assert_eq!(fs::read(&s.catalog).unwrap(), catalog_before);
    assert_eq!(tree(s.photos.path()), originals);
    no_secrets(&stdout(&o));
    no_secrets(&fs::read_to_string(s.report()).unwrap());
}

#[test]
fn apply_runs_the_preflight_before_the_first_write() {
    let s = setup();
    let fake = s.fake();
    assert_exit(&s.run(fake.port(), &["--apply"]), 0);
    let names: Vec<String> = fake.commands().into_iter().map(|(c, _)| c).collect();
    let first_write = names
        .iter()
        .position(|c| !READ_COMMANDS.contains(&c.as_str()))
        .unwrap();
    for read in [
        "library.info",
        "ui.inspect",
        "library.xmpPreferences",
        "keyword.list",
    ] {
        let at = names
            .iter()
            .position(|c| c == read)
            .unwrap_or_else(|| panic!("{read} missing"));
        assert!(at < first_write, "{read} must come before the first write");
    }
}

#[test]
fn every_import_is_a_single_reference_import() {
    let s = setup();
    let fake = s.fake();
    assert_exit(&s.run(fake.port(), &["--apply"]), 0);
    let imports: Vec<_> = fake
        .commands()
        .into_iter()
        .filter(|(c, _)| c == "library.import")
        .collect();
    assert!(!imports.is_empty());
    for (_, p) in imports {
        assert_eq!(p.as_object().unwrap().len(), 2, "{p}");
        assert_eq!(p["mode"], "add");
        assert_eq!(p["paths"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn second_apply_sends_only_read_commands_and_creates_no_duplicates() {
    let s = setup();
    let fake = s.fake();
    assert_exit(&s.run(fake.port(), &["--apply"]), 0);
    let (photos, keywords) = (fake.photos(), fake.keywords());
    let before = fake.commands().len();
    let state_before = fs::read(s.state_file()).unwrap();

    let o = s.run(fake.port(), &["--apply"]);
    assert_exit(&o, 0);
    only_reads(&fake, before);
    assert_eq!(fake.photos(), photos);
    assert_eq!(fake.keywords(), keywords);
    assert_eq!(fs::read(s.state_file()).unwrap(), state_before);
    let w = &s.report_json()["writer"];
    for key in [
        "keywords_created",
        "photos_imported",
        "photos_existing",
        "updates",
    ] {
        assert_eq!(w[key], 0, "{key}");
    }
}

#[test]
fn apply_with_a_lost_state_file_finds_the_photos_as_existing() {
    let s = setup();
    let fake = s.fake();
    assert_exit(&s.run(fake.port(), &["--apply"]), 0);
    let imported = s.report_json()["writer"]["photos_imported"]
        .as_u64()
        .unwrap();
    fs::remove_file(s.state_file()).unwrap();
    let photos = fake.photos();

    assert_exit(&s.run(fake.port(), &["--apply"]), 0);
    let w = &s.report_json()["writer"];
    assert_eq!(w["photos_imported"], 0);
    assert_eq!(w["photos_existing"].as_u64().unwrap(), imported);
    assert_eq!(w["keywords_created"], 0);
    assert_eq!(fake.photos(), photos, "no duplicates in the library");
}

#[test]
fn apply_leaves_the_catalog_checksum_unchanged_even_when_it_fails() {
    let s = setup();
    let before = fs::read(&s.catalog).unwrap();
    let o = s.run(closed_port(), &["--apply"]);
    assert_exit(&o, 1);
    assert_eq!(fs::read(&s.catalog).unwrap(), before);
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut n = s.catalog.file_name().unwrap().to_os_string();
        n.push(suffix);
        assert!(!s.catalog.with_file_name(n).exists(), "{suffix}");
    }
}

// ----------------------------------------------------------------- failures

#[test]
fn xmp_auto_write_stops_the_run_before_any_write() {
    let s = setup();
    let mut config = Config::new(&s.library);
    config.auto_write = true;
    let fake = FakeLightCraft::start(config);
    let o = s.run(fake.port(), &["--apply"]);
    assert_exit(&o, 1);
    assert!(stderr(&o).contains("XMP"), "{}", stderr(&o));
    assert!(
        fake.write_commands().is_empty(),
        "{:?}",
        fake.write_commands()
    );
    assert!(fake.photos().is_empty());
}

#[test]
fn allow_xmp_lets_the_run_continue() {
    let s = setup();
    let mut config = Config::new(&s.library);
    config.auto_write = true;
    let fake = FakeLightCraft::start(config);
    let o = s.run(fake.port(), &["--apply", "--allow-xmp"]);
    assert_exit(&o, 0);
    assert!(!fake.photos().is_empty());
}

#[test]
fn wrong_library_stops_the_run_before_any_write() {
    let s = setup();
    let other = s.work.path().join("Other.lightcraft");
    let fake = FakeLightCraft::start(Config::new(&other));
    let o = s.run(fake.port(), &["--apply"]);
    assert_exit(&o, 1);
    assert!(stderr(&o).contains("library"), "{}", stderr(&o));
    assert!(fake.write_commands().is_empty());
    assert!(!s.state_file().exists());
}

#[test]
fn in_memory_session_stops_the_run_before_any_write() {
    let s = setup();
    let mut config = Config::new(&s.library);
    config.persistent = false;
    let fake = FakeLightCraft::start(config);
    assert_exit(&s.run(fake.port(), &["--apply"]), 1);
    assert!(fake.write_commands().is_empty());
}

#[test]
fn library_problem_stops_the_run_before_any_write() {
    let s = setup();
    let mut config = Config::new(&s.library);
    config.library_problem = Some("damaged".into());
    let fake = FakeLightCraft::start(config);
    assert_exit(&s.run(fake.port(), &["--apply"]), 1);
    assert!(fake.write_commands().is_empty());
}

#[test]
fn no_server_gives_exit_1_with_a_clear_message() {
    let s = setup();
    let o = s.run(closed_port(), &["--apply"]);
    assert_exit(&o, 1);
    assert!(
        stderr(&o).to_lowercase().contains("connect"),
        "{}",
        stderr(&o)
    );
    assert!(!s.state_file().exists());
}

#[test]
fn lost_connection_says_outcome_unknown_and_a_rerun_finishes_without_duplicates() {
    let s = setup();
    let mut config = Config::new(&s.library);
    // Lines 1-3 are the pre-flight, 4 the keyword list; line 20 lies among
    // the keyword creations and photo commands.
    config.close_at = Some(20);
    let fake = FakeLightCraft::start(config);
    let o = s.run(fake.port(), &["--apply"]);
    assert_exit(&o, 1);
    assert!(
        stderr(&o).contains("outcome unknown, run again"),
        "{}",
        stderr(&o)
    );
    assert_eq!(fake.commands().len(), 20, "nothing is sent after the loss");

    let o = s.run(fake.port(), &["--apply"]);
    assert_exit(&o, 0);
    let dry = s.run(closed_port(), &[]);
    assert_eq!(
        planned_line(&stdout(&dry)),
        (0, 0, 0),
        "everything is done now"
    );
    let photos = fake.photos();
    let total = s.report_json()["photos"]["converted"].as_u64().unwrap();
    let skipped = s.report_json()["writer"]["photos_skipped"]
        .as_array()
        .unwrap()
        .len() as u64;
    assert_eq!(
        photos.len() as u64,
        total - skipped,
        "each photo exactly once"
    );
    let mut ids: Vec<i64> = photos.values().copied().collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), photos.len());
}

#[test]
fn error_reply_for_a_command_is_reported_as_skipped_not_fatal() {
    let s = setup();
    let mut config = Config::new(&s.library);
    config.failures.push(fake_lightcraft::Failure {
        command: "photo.rate".into(),
        photo_id: None,
        message: "refused".into(),
    });
    let fake = FakeLightCraft::start(config);
    let o = s.run(fake.port(), &["--apply"]);
    assert_exit(&o, 0);
    let w = &s.report_json()["writer"];
    let incomplete = w["photos_incomplete"].as_array().unwrap();
    assert!(!incomplete.is_empty());
    assert!(
        incomplete
            .iter()
            .all(|i| i["reason"].as_str().unwrap().contains("photo.rate"))
    );
    // Strict balance: a photo with a refused update was imported and counts once.
    let p = &s.report_json()["photos"];
    assert_eq!(
        p["converted"].as_u64().unwrap(),
        w["photos_imported"].as_u64().unwrap()
            + w["photos_existing"].as_u64().unwrap()
            + w["photos_skipped"].as_array().unwrap().len() as u64
    );
    assert!(incomplete.len() as u64 <= w["photos_imported"].as_u64().unwrap());
    no_secrets(&fs::read_to_string(s.report()).unwrap());
}
