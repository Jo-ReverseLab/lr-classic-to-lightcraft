//! The window driven by a scripted host: what the start buttons allow, how
//! a finished run reaches the Log, and filters. `render_screens` writes
//! PNG renderings for a visual check (ignored: it needs a GPU).

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use lr2lc_core::Job;
use lr2lc_core::probe::Probe;
use lr2lc_gui::app::{App, Tab};
use lr2lc_gui::host::Host;
use lr2lc_gui::run::{ChannelProgress, Event, RunResult, execute};
use lr2lc_gui::settings::Settings;
use lr2lc_gui::theme;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;

/// A host that records what the window asks and answers from a script.
#[derive(Default)]
struct Scripted {
    started: Vec<Job>,
    cancelled: bool,
    events: Vec<Event>,
    scan: Option<RunResult>,
    scans: usize,
    probe: Option<Result<Probe, String>>,
    library: Option<PathBuf>,
}

impl Host for Scripted {
    fn pick_catalog(&mut self) -> Option<PathBuf> {
        None
    }
    fn pick_library(&mut self) -> Option<PathBuf> {
        self.library.clone()
    }
    fn pick_label_file(&mut self) -> Option<PathBuf> {
        None
    }
    fn pick_report(&mut self) -> Option<PathBuf> {
        None
    }
    fn start(&mut self, job: Job) {
        self.started.push(job);
    }
    fn cancel(&mut self) {
        self.cancelled = true;
    }
    fn poll(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }
    fn scan(&mut self, _job: Job) {
        self.scans += 1;
    }
    fn poll_scan(&mut self) -> Option<RunResult> {
        if self.scans == 0 {
            return None;
        }
        self.scan.take()
    }
    fn probe(&mut self, _port: u16) {}
    fn poll_probe(&mut self) -> Option<Result<Probe, String>> {
        self.probe.clone()
    }
}

struct World {
    _photos: tempfile::TempDir,
    _work: tempfile::TempDir,
    catalog: PathBuf,
    library: PathBuf,
    history: PathBuf,
}

fn world() -> World {
    let photos = tempfile::tempdir().unwrap();
    let catalog = lrcat_fixtures::write_keyword_edge_catalog_with_files(photos.path()).unwrap();
    let work = tempfile::tempdir().unwrap();
    let library = work.path().join("Test.lightcraft");
    std::fs::create_dir_all(&library).unwrap();
    let history = work.path().join("runs");
    World {
        _photos: photos,
        _work: work,
        catalog,
        library,
        history,
    }
}

/// A real dry run of the fixture, as the scan or a finished run.
fn dry_run(w: &World) -> RunResult {
    let settings = settings(w);
    let job = settings.job(false).unwrap();
    let (tx, _rx) = channel();
    let mut progress = ChannelProgress::new(tx, Arc::new(AtomicBool::new(false)));
    execute(&job, &mut progress, None)
}

fn settings(w: &World) -> Settings {
    Settings {
        catalog: w.catalog.display().to_string(),
        library: w.library.display().to_string(),
        ..Settings::default()
    }
}

fn connected(w: &World) -> Option<Result<Probe, String>> {
    Some(Ok(Probe {
        persistent: true,
        library: Some(w.library.clone()),
        xmp_auto_write: Some(false),
    }))
}

struct State {
    app: App,
    host: Scripted,
}

fn harness(state: State, width: f32) -> Harness<'static, State> {
    Harness::builder().with_size([width, 860.0]).build_ui_state(
        |ui, state: &mut State| {
            state.app.ui(ui, &mut state.host);
        },
        state,
    )
}

fn ready(w: &World) -> State {
    let app = App::new(
        settings(w),
        Some(lr2lc_gui::history::History::new(w.history.clone())),
    );
    State {
        app,
        host: Scripted {
            scan: Some(dry_run(w)),
            probe: connected(w),
            ..Scripted::default()
        },
    }
}

#[test]
fn a_ready_catalog_starts_a_dry_run_and_a_transfer() {
    let w = world();
    let mut h = harness(ready(&w), 1280.0);
    h.run_steps(4);
    assert!(h.state().app.scan.report.is_some(), "the scan arrived");
    h.get_by_label("Dry run (writes nothing)").click();
    h.run_steps(2);
    let started = &h.state().host.started;
    assert_eq!(started.len(), 1);
    assert!(!started[0].apply);
    assert!(h.state().app.is_running());
}

#[test]
fn without_lightcraft_only_the_dry_run_is_allowed() {
    let w = world();
    let mut state = ready(&w);
    state.host.probe = Some(Err("cannot connect to the LightCraft control port".into()));
    let mut h = harness(state, 1280.0);
    h.run_steps(4);
    h.get_by_label("Transfer now").click();
    h.run_steps(2);
    assert!(h.state().host.started.is_empty(), "the transfer is blocked");
    h.get_by_label("LightCraft not reachable on port 7980");
    h.get_by_label("Dry run (writes nothing)").click();
    h.run_steps(2);
    assert_eq!(h.state().host.started.len(), 1);
}

#[test]
fn a_catalog_open_in_lightroom_blocks_both() {
    let w = world();
    let mut lock = w.catalog.as_os_str().to_owned();
    lock.push(".lock");
    std::fs::write(PathBuf::from(lock), b"").unwrap();
    let mut h = harness(ready(&w), 1280.0);
    h.run_steps(4);
    h.get_by_label("Catalog open in Lightroom: yes, close Lightroom first");
    h.get_by_label("Dry run (writes nothing)").click();
    h.get_by_label("Transfer now").click();
    h.run_steps(2);
    assert!(h.state().host.started.is_empty());
}

#[test]
fn everything_never_turns_on_the_deliberate_options() {
    let w = world();
    let mut h = harness(ready(&w), 1280.0);
    h.run_steps(2);
    h.get_by_label("Folders and photos only").click();
    h.run_steps(2);
    assert!(!h.state().app.settings.selection.keywords);
    h.get_by_label("Everything").click();
    h.run_steps(2);
    let s = &h.state().app.settings;
    assert!(s.selection.keywords && s.selection.develop);
    assert!(!s.overwrite_develop && !s.allow_xmp);
}

#[test]
fn a_finished_dry_run_opens_its_log_with_filters_and_details() {
    let w = world();
    let mut h = harness(ready(&w), 1280.0);
    h.run_steps(4);
    h.get_by_label("Dry run (writes nothing)").click();
    h.run_steps(2);
    let result = dry_run(&w);
    h.state_mut()
        .host
        .events
        .push(Event::Done(Box::new(result)));
    h.run_steps(4);
    let app = &h.state().app;
    assert_eq!(app.tab, Tab::Log);
    assert!(!app.log.data.rows.is_empty());
    assert_eq!(app.log.records.len(), 1, "the run is in the history");
    let skipped = app.log.data.count(model::Status::Skipped);
    assert!(skipped > 0);

    h.get_by_label("Skipped").click();
    h.run_steps(2);
    assert_eq!(
        h.state().app.log.filter.status,
        Some(model::Status::Skipped)
    );
    h.get_by_label("Select an entry to see details.");
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(2);
    assert!(h.state().app.log.selected.is_some());
    h.get_by_label("Copy details");
    h.get_by_label("Reset filters").click();
    h.run_steps(2);
    assert_eq!(h.state().app.log.filter.status, None);
}

#[test]
#[ignore = "needs a GPU; writes PNG renderings to LR2LC_RENDER_DIR"]
fn render_screens() {
    let Some(dir) = std::env::var_os("LR2LC_RENDER_DIR").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let w = world();
    for (theme_name, mode) in [
        ("dark", theme::ThemeMode::Dark),
        ("light", theme::ThemeMode::Light),
    ] {
        for width in [1280.0, 900.0] {
            let mut state = ready(&w);
            state.app.settings.theme = mode;
            let mut h = Harness::builder()
                .with_size([width, 1100.0])
                .build_ui_state(
                    |ui, state: &mut State| {
                        state.app.ui(ui, &mut state.host);
                    },
                    state,
                );
            theme::install_fonts(&h.ctx);
            theme::apply(&h.ctx, mode);
            h.run_steps(6);
            h.render()
                .unwrap()
                .save(dir.join(format!("transfer-{theme_name}-{width}.png")))
                .unwrap();
            // Running, then the Log of a finished dry run.
            h.get_by_label("Dry run (writes nothing)").click();
            h.run_steps(3);
            h.render()
                .unwrap()
                .save(dir.join(format!("running-{theme_name}-{width}.png")))
                .unwrap();
            let result = dry_run(&w);
            h.state_mut()
                .host
                .events
                .push(Event::Done(Box::new(result)));
            h.run_steps(4);
            h.key_press(egui::Key::ArrowDown);
            h.run_steps(3);
            h.render()
                .unwrap()
                .save(dir.join(format!("log-{theme_name}-{width}.png")))
                .unwrap();
        }
    }
}

#[test]
fn a_transfer_runs_on_its_thread_and_ends_in_the_log() {
    use lc_test_support::fake_lightcraft::{Config, FakeLightCraft};
    use lr2lc_gui::host::StandaloneHost;

    struct Real {
        app: App,
        host: StandaloneHost,
    }
    let w = world();
    let fake = FakeLightCraft::start(Config::new(&w.library));
    let mut settings = settings(&w);
    settings.port = fake.port();
    let app = App::new(
        settings,
        Some(lr2lc_gui::history::History::new(w.history.clone())),
    );
    let mut h = Harness::builder()
        .with_size([1280.0, 900.0])
        .build_ui_state(
            |ui, state: &mut Real| state.app.ui(ui, &mut state.host),
            Real {
                app,
                host: StandaloneHost::default(),
            },
        );
    // Wait for the scan and the probe.
    for _ in 0..200 {
        h.run_steps(1);
        let a = &h.state().app;
        if a.scan.report.is_some() && matches!(a.app.answer, Some(Ok(_))) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.get_by_label("Transfer now").click();
    for _ in 0..500 {
        h.run_steps(1);
        if h.state().app.run.as_ref().is_some_and(|r| r.finished()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let app = &h.state().app;
    let run = app.run.as_ref().expect("the run screen stays");
    assert!(run.finished());
    assert!(
        run.result.as_ref().unwrap().error.is_none(),
        "{:?}",
        run.result
    );
    assert!(!fake.photos().is_empty(), "photos were imported");
    assert!(
        run.counts
            .get(&model::Status::Transferred)
            .copied()
            .unwrap_or(0)
            > 0
    );
    assert_eq!(app.log.records.len(), 1);
    assert_eq!(app.log.records[0].kind, lr2lc_gui::history::Kind::Transfer);
    assert!(h.query_all_by_label("Transfer finished").count() >= 1);
    h.get_by_label("View log").click();
    h.run_steps(2);
    assert_eq!(h.state().app.tab, Tab::Log);
}

#[test]
fn emoji_in_catalog_names_have_glyphs_in_every_font() {
    // Keyword and file names may hold emoji; every font the window uses
    // must draw them (the harness panics on a missing glyph).
    let mut h = Harness::builder().with_size([400.0, 200.0]).build_ui(|ui| {
        for font in [
            egui::FontId::proportional(13.0),
            egui::FontId::monospace(12.0),
            theme::font(13.0, Some(theme::MEDIUM)),
            theme::font(13.0, Some(theme::SEMIBOLD)),
        ] {
            ui.label(egui::RichText::new("Köln 🌅 Sonnenuntergang").font(font));
        }
    });
    theme::install_fonts(&h.ctx);
    h.run_steps(3);
}
