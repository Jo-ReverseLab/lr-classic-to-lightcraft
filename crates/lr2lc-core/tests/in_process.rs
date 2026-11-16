//! A whole run driven through the library API, once through the control
//! port and once through a `HostSink` in the same process: both write the
//! same commands, the same mapping file and the same report numbers.

use lc_test_support::fake_lightcraft::{Config, FakeLightCraft};
use lr2lc_core::labels::LabelMode;
use lr2lc_core::{HostSink, Job, NoProgress, Request, finish, finish_with, prepare};
use lrcat_fixtures::write_keyword_edge_catalog_with_files;
use model::PathMappings;
use std::path::PathBuf;

struct Setup {
    _photos: tempfile::TempDir,
    _work: tempfile::TempDir,
    job: Job,
}

fn setup() -> Setup {
    let photos = tempfile::tempdir().unwrap();
    let catalog = write_keyword_edge_catalog_with_files(photos.path()).unwrap();
    let work = tempfile::tempdir().unwrap();
    let library: PathBuf = work.path().join("Test.lightcraft");
    let job = Job {
        catalog,
        mappings: PathMappings::default(),
        labels: LabelMode::Auto,
        library: Some(library),
        apply: true,
        port: 0,
        allow_xmp: false,
        overwrite_develop: false,
        areas: lr2lc_core::Areas::ALL,
        anonymize: false,
    };
    Setup {
        _photos: photos,
        _work: work,
        job,
    }
}

fn state_text(job: &Job) -> String {
    let library = job.library.as_deref().unwrap();
    std::fs::read_to_string(lc_writer::state::state_path(library)).unwrap()
}

/// The report without the parts that differ between two runs by design.
fn numbers(report: &str) -> serde_json::Value {
    let mut value: serde_json::Value = serde_json::from_str(report).unwrap();
    // The entries name each run's own temporary files.
    value.as_object_mut().unwrap().remove("entries");
    value.as_object_mut().unwrap().remove("timing");
    value
}

#[test]
fn a_run_through_a_host_sink_matches_a_run_through_the_port() {
    let mut by_port = setup();
    let port_fake = FakeLightCraft::start(Config::new(by_port.job.library.as_deref().unwrap()));
    by_port.job.port = port_fake.port();
    let prepared = prepare(&by_port.job, &mut NoProgress).unwrap();
    prepared.check_writable().unwrap();
    let port_done = finish(&by_port.job, prepared, &mut NoProgress).unwrap();
    port_done.result().unwrap();

    let local = setup();
    let fake = FakeLightCraft::start(Config::new(local.job.library.as_deref().unwrap()));
    let mut sink = HostSink::new(|request, params| match request {
        Request::Command(command) => fake.dispatch(command, params, true),
        Request::Method(method) => fake.dispatch(method, params, false),
    });
    let prepared = prepare(&local.job, &mut NoProgress).unwrap();
    let local_done = finish_with(&local.job, prepared, &mut sink, &mut NoProgress).unwrap();
    local_done.result().unwrap();

    assert!(!fake.commands().is_empty());
    assert_eq!(fake.commands().len(), port_fake.commands().len());
    assert_eq!(fake.keywords(), port_fake.keywords());
    assert_eq!(fake.ratings(), port_fake.ratings());
    assert_eq!(local_done.writer_summary, port_done.writer_summary);
    assert_eq!(
        numbers(&local_done.report_json()),
        numbers(&port_done.report_json())
    );
    assert_eq!(
        state_text(&local.job).lines().count(),
        state_text(&by_port.job).lines().count()
    );
}

/// Records phases; cancels once `stop_at` has started.
struct Phases {
    seen: Vec<lr2lc_core::Phase>,
    stop_at: Option<lr2lc_core::Phase>,
}

impl lr2lc_core::Progress for Phases {
    fn phase(&mut self, phase: lr2lc_core::Phase, _total: usize) {
        self.seen.push(phase);
    }
    fn cancelled(&self) -> bool {
        self.stop_at.is_some_and(|p| self.seen.contains(&p))
    }
}

#[test]
fn reading_reports_its_phases() {
    use lr2lc_core::Phase;
    let setup = setup();
    let mut progress = Phases {
        seen: Vec::new(),
        stop_at: None,
    };
    prepare(&setup.job, &mut progress).unwrap();
    assert_eq!(
        progress.seen,
        [Phase::Checksum, Phase::Reading, Phase::CheckingOriginals]
    );
}

#[test]
fn a_cancel_while_reading_is_its_own_error_kind() {
    let setup = setup();
    let mut progress = Phases {
        seen: Vec::new(),
        stop_at: Some(lr2lc_core::Phase::Checksum),
    };
    let Err(error) = prepare(&setup.job, &mut progress) else {
        panic!("expected a cancel");
    };
    assert_eq!(error.kind, lr2lc_core::ErrorKind::Cancelled);
    assert_eq!(
        error.to_string(),
        "stopped by the user; nothing was written"
    );
}

#[test]
fn areas_left_out_are_named_in_report_and_summary() {
    let mut setup = setup();
    setup.job.apply = false;
    setup.job.areas = lr2lc_core::Areas::skipping("develop,keywords").unwrap();
    let prepared = prepare(&setup.job, &mut NoProgress).unwrap();
    let done = finish(&setup.job, prepared, &mut NoProgress).unwrap();
    assert!(
        done.writer_summary
            .starts_with("not selected: keywords, develop\n")
    );
    let report: serde_json::Value = serde_json::from_str(&done.report_json()).unwrap();
    assert_eq!(
        report["not_selected"],
        serde_json::json!(["keywords", "develop"])
    );
    assert_eq!(report["writer"]["planned"]["keywords"], 0);

    setup.job.areas = lr2lc_core::Areas::ALL;
    let prepared = prepare(&setup.job, &mut NoProgress).unwrap();
    let done = finish(&setup.job, prepared, &mut NoProgress).unwrap();
    let report: serde_json::Value = serde_json::from_str(&done.report_json()).unwrap();
    assert!(report.get("not_selected").is_none());
}

#[test]
fn the_log_names_files_unless_anonymized() {
    let mut setup = setup();
    setup.job.apply = false;
    let prepared = prepare(&setup.job, &mut NoProgress).unwrap();
    let done = finish(&setup.job, prepared, &mut NoProgress).unwrap();

    let named = done.log.render(false);
    let imports: Vec<_> = named
        .iter()
        .filter(|e| e.message == "Will be imported as a reference")
        .collect();
    assert!(!imports.is_empty());
    for e in &imports {
        assert!(e.item.contains('.'), "a file name: {:?}", e.item);
        assert!(e.source.as_deref().is_some_and(|s| s.ends_with(&e.item)));
        assert!(e.photo.is_some());
    }

    let report: serde_json::Value = serde_json::from_str(&done.report_json_as(true)).unwrap();
    assert_eq!(report["anonymized"], true);
    let catalog_dir = setup
        .job
        .catalog
        .parent()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    for e in report["entries"].as_array().unwrap() {
        for key in ["item", "message", "reason"] {
            let text = e[key].as_str().unwrap_or("");
            assert!(!text.contains(&catalog_dir), "no path in an anonymized log");
        }
    }
    for e in report["entries"].as_array().unwrap() {
        assert!(e.get("source").is_none() && e.get("result").is_none());
        if let Some(id) = e["photo"].as_i64() {
            assert_eq!(e["item"], format!("photo {id}"));
        }
    }
    let named: serde_json::Value = serde_json::from_str(&done.report_json()).unwrap();
    assert_eq!(named["anonymized"], false);
    // Compare the parsed values: JSON escapes Windows backslashes.
    assert!(named["entries"].as_array().unwrap().iter().any(|e| {
        e["source"]
            .as_str()
            .is_some_and(|s| s.contains(&catalog_dir))
    }));
}

#[test]
fn the_probe_reads_the_open_library_and_xmp_setting() {
    let setup = setup();
    let library = setup.job.library.clone().unwrap();
    let mut config = Config::new(&library);
    config.auto_write = true;
    let fake = FakeLightCraft::start(config);
    let probe = lr2lc_core::probe::probe(fake.port()).unwrap();
    assert!(probe.persistent);
    assert_eq!(probe.xmp_auto_write, Some(true));
    assert!(probe.has_open(&library));
    assert!(fake.write_commands().is_empty());
    assert!(lr2lc_core::probe::probe(1).is_err());
}

#[test]
fn a_library_is_created_and_opened_through_the_app() {
    use lr2lc_core::probe::{LibraryAction, library_action, probe};
    let setup = setup();
    let library = setup.job.library.clone().unwrap();
    let fake = FakeLightCraft::start(Config::new(&library));
    let folder = library.parent().unwrap().to_path_buf();
    let created = library_action(
        fake.port(),
        &LibraryAction::Create {
            folder: folder.clone(),
            name: "New Library".into(),
        },
    )
    .unwrap();
    let new = folder.join("New Library");
    assert!(new.is_dir());
    assert!(created.has_open(&new));
    let back = library_action(fake.port(), &LibraryAction::Open(library.clone())).unwrap();
    assert!(back.has_open(&library));
    assert!(probe(fake.port()).unwrap().has_open(&library));
}
