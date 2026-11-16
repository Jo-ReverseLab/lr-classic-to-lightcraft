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
    value.as_object_mut().unwrap().remove("catalog");
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
