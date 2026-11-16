//! End-to-end acceptance test (task T19): one catalog that exercises every
//! supported feature, run through the whole tool pipeline — a dry run, then
//! `--apply` against a stateful fake LightCraft, then `--apply` again.
//!
//! The catalog is built by [`lrcat_fixtures::write_full_catalog_with_files`]:
//! the small fixture plus every edge (keywords with hierarchy/synonyms and
//! attributes, ratings, flags, color labels, collections and collection sets,
//! smart collections translatable and not, stacks, the quick collection,
//! titles, captions, copyright, creator, videos, a missing file and a virtual
//! copy).
//!
//! The assertions are the invariants that hold for *any* catalog, not
//! hand-counted numbers (the tool is the source of truth, CI is the proof):
//! - every area balances: `read == converted + skipped`;
//! - the photo sub-counts add up to `photos.converted` (flags, labels,
//!   ratings);
//! - the dry-run numbers equal the numbers of the first apply run;
//! - a photo counts once: `photos.converted == imported + existing +
//!   photos_skipped by the writer`;
//! - the catalog is byte-identical after every run (Principle 1) and the
//!   originals are untouched (Principle 2);
//! - a second `--apply` sends only read commands and changes nothing
//!   (Principle 5, idempotent).

use lrcat_fixtures::{smart_edge, write_full_catalog_with_files};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use fake_lightcraft::{Config, FakeLightCraft, READ_COMMANDS};
use lc_test_support::fake_lightcraft;

/// Every top-level area that carries `read` / `converted` / `skipped`.
const AREAS: [&str; 9] = [
    "folders",
    "photos",
    "collections",
    "collection_members",
    "stacks",
    "stack_members",
    "keywords",
    "keyword_assignments",
    "keyword_synonyms",
];

struct Setup {
    photos: tempfile::TempDir,
    work: tempfile::TempDir,
    catalog: PathBuf,
    library: PathBuf,
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

    /// `lr2lc <catalog> --library <lib> --port <port> --report <file> [extra]`.
    fn run(&self, port: u16, extra: &[&str]) -> Output {
        let port_text = port.to_string();
        let report = self.report();
        let args: [&OsStr; 8] = [
            self.catalog.as_os_str(),
            OsStr::new("--library"),
            self.library.as_os_str(),
            OsStr::new("--port"),
            OsStr::new(&port_text),
            OsStr::new("--anonymize"),
            OsStr::new("--report"),
            report.as_os_str(),
        ];
        run(self.work.path(), &args, extra)
    }

    fn report_json(&self) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(self.report()).unwrap()).unwrap()
    }
}

fn setup() -> Setup {
    let photos = tempfile::tempdir().unwrap();
    let catalog = write_full_catalog_with_files(photos.path()).unwrap();
    let work = tempfile::tempdir().unwrap();
    let library = work.path().join("Full.lightcraft");
    Setup {
        photos,
        work,
        catalog,
        library,
    }
}

fn run(cwd: &Path, prefix: &[&OsStr], extra: &[&str]) -> Output {
    let mut args: Vec<&OsStr> = prefix.to_vec();
    args.extend(extra.iter().map(OsStr::new));
    Command::new(env!("CARGO_BIN_EXE_lr2lc"))
        .current_dir(cwd)
        .args(&args)
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

/// A port nobody listens on, for a dry run that never connects.
fn closed_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

/// `read == converted + skipped.len()`, and every skip carries a reason.
fn assert_area_balance(v: &serde_json::Value, area: &str) {
    let a = &v[area];
    let read = a["read"].as_u64().unwrap();
    let converted = a["converted"].as_u64().unwrap();
    let skipped = a["skipped"]
        .as_array()
        .unwrap_or_else(|| panic!("{area}.skipped is not an array: {a}"));
    for s in skipped {
        assert!(
            !s["reason"].as_str().unwrap().is_empty(),
            "{area}: empty skip reason: {s}"
        );
    }
    assert_eq!(
        read,
        converted + skipped.len() as u64,
        "{area}: read != converted + skipped"
    );
}

/// Sum of the integer values of `obj[key]` for every `key` in `keys`.
fn sum_fields(obj: &serde_json::Value, keys: &[&str]) -> u64 {
    keys.iter().map(|k| obj[k].as_u64().unwrap_or(0)).sum()
}

/// All files below `dir` with their content (Principle 2 check).
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

fn only_reads(fake: &FakeLightCraft, from: usize) {
    let sent: Vec<String> = fake.commands()[from..]
        .iter()
        .map(|(c, _)| c.clone())
        .collect();
    assert!(
        sent.iter().all(|c| READ_COMMANDS.contains(&c.as_str())),
        "second apply sent write commands: {sent:?}"
    );
}

fn no_secrets(text: &str) {
    // The smart-collection marker and a few fixture names must never leak.
    for needle in [
        smart_edge::SECRET,
        "Bad|Name",
        "Sonnenuntergang",
        "Example Creator",
    ] {
        assert!(!text.contains(needle), "secret leaked: {needle}");
    }
}

#[test]
fn full_catalog_pipeline_round_trip() {
    let s = setup();
    let catalog_before = fs::read(&s.catalog).unwrap();
    let originals = tree(s.photos.path());

    // --- Dry run: never opens a connection (Principle 3). ---
    let dry = s.run(closed_port(), &[]);
    assert_exit(&dry, 0);
    let dry_stdout = stdout(&dry);
    assert!(dry_stdout.contains("dry run"), "{dry_stdout}");
    no_secrets(&dry_stdout);
    let dry_report = s.report_json();
    assert_eq!(dry_report["dry_run"], true);
    assert_eq!(dry_report["catalog_unchanged"], true);

    // Every area balances, and the catalog is unchanged.
    for area in AREAS {
        assert_area_balance(&dry_report, area);
    }

    // The photo sub-counts add up to the converted photos.
    let photos_converted = dry_report["photos"]["converted"].as_u64().unwrap();
    assert_eq!(
        sum_fields(
            &dry_report["photos"]["flags"],
            &["pick", "reject", "none", "unknown"]
        ),
        photos_converted,
        "flags do not add up to photos.converted"
    );
    assert_eq!(
        sum_fields(
            &dry_report["photos"]["labels"],
            &["red", "yellow", "green", "blue", "purple", "none", "custom"]
        ),
        photos_converted,
        "labels do not add up to photos.converted"
    );
    assert_eq!(
        dry_report["ratings"]
            .as_object()
            .unwrap()
            .values()
            .map(|v| v.as_u64().unwrap_or(0))
            .sum::<u64>(),
        photos_converted,
        "ratings do not add up to photos.converted"
    );

    // The writer plan from the dry run — the source of truth for the apply.
    let planned = &dry_report["writer"]["planned"];
    let plan_keywords = planned["keywords"].as_u64().unwrap();
    let plan_photos = planned["photos"].as_u64().unwrap();
    let plan_updates = planned["updates"].as_u64().unwrap();

    // --- First apply against the fake LightCraft. ---
    let fake = s.fake();
    let apply1 = s.run(fake.port(), &["--apply"]);
    assert_exit(&apply1, 0);
    no_secrets(&stdout(&apply1));
    no_secrets(&fs::read_to_string(s.report()).unwrap());
    let r1 = s.report_json();
    assert_eq!(r1["dry_run"], false);
    assert_eq!(r1["catalog_unchanged"], true);

    // The dry-run numbers equal the numbers of the first apply run: the
    // catalog counts are identical (same catalog), and the writer created
    // exactly what it planned in a fresh library.
    for area in AREAS {
        assert_eq!(r1[area]["read"], dry_report[area]["read"], "{area}.read");
        assert_eq!(
            r1[area]["converted"], dry_report[area]["converted"],
            "{area}.converted"
        );
        assert_eq!(
            r1[area]["skipped"].as_array().unwrap().len(),
            dry_report[area]["skipped"].as_array().unwrap().len(),
            "{area}.skipped length"
        );
    }
    let w1 = &r1["writer"];
    assert_eq!(w1["keywords_created"], plan_keywords);
    assert_eq!(w1["photos_imported"], plan_photos);
    assert_eq!(w1["photos_existing"], 0);
    assert_eq!(w1["updates"], plan_updates);
    assert_eq!(w1["not_saved"], 0);

    // A photo counts once: converted = imported + existing + writer-skipped.
    let writer_skipped = w1["photos_skipped"].as_array().unwrap().len()
        + w1["photos_incomplete"].as_array().unwrap().len();
    assert_eq!(
        photos_converted,
        w1["photos_imported"].as_u64().unwrap()
            + w1["photos_existing"].as_u64().unwrap()
            + writer_skipped as u64,
        "converted photos = written + skipped by the writer"
    );

    // The fake received what the report says; every imported path exists.
    assert_eq!(fake.photos().len() as u64, plan_photos);
    assert_eq!(fake.keywords().len() as u64, plan_keywords);
    for path in fake.photos().keys() {
        assert!(
            Path::new(path).starts_with(s.photos.path()),
            "{path}: imported path must stay below the photos dir"
        );
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

    // --- Second apply: idempotent (Principle 5). ---
    let (photos_after_1, keywords_after_1) = (fake.photos(), fake.keywords());
    let commands_after_1 = fake.commands().len();
    let state_after_1 = fs::read(s.state_file()).unwrap();

    let apply2 = s.run(fake.port(), &["--apply"]);
    assert_exit(&apply2, 0);
    only_reads(&fake, commands_after_1);
    assert_eq!(fake.photos(), photos_after_1);
    assert_eq!(fake.keywords(), keywords_after_1);
    assert_eq!(fs::read(s.state_file()).unwrap(), state_after_1);

    let w2 = &s.report_json()["writer"];
    for key in [
        "keywords_created",
        "photos_imported",
        "photos_existing",
        "updates",
    ] {
        assert_eq!(w2[key], 0, "second apply is not idempotent: {key}");
    }

    // The catalog is still byte-identical after the second run.
    assert_eq!(fs::read(&s.catalog).unwrap(), catalog_before);
    assert_eq!(tree(s.photos.path()), originals);
}
