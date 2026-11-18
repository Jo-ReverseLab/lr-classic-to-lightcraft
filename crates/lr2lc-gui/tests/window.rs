//! The window driven by a scripted host: what the start buttons allow, how
//! a finished run reaches the Log, and filters. `render_screens` writes
//! PNG renderings for a visual check (ignored: it needs a GPU).

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use lr2lc_core::Job;
use lr2lc_core::probe::{LibraryAction, Probe};
use lr2lc_gui::app::{App, Tab};
use lr2lc_gui::host::Host;
use lr2lc_gui::run::{Answer, ChannelProgress, Event, Question, RunResult, execute};
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
    rolled_back: bool,
    events: Vec<Event>,
    scan: Option<RunResult>,
    scans: usize,
    probe: Option<Result<Probe, String>>,
    library: Option<PathBuf>,
    new_folder: Option<PathBuf>,
    actions: Vec<LibraryAction>,
    action_answer: Option<Result<Probe, String>>,
    answers: Vec<Answer>,
    shown: Vec<serde_json::Value>,
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
    fn cancel(&mut self, roll_back: bool) {
        self.cancelled = true;
        self.rolled_back |= roll_back;
    }
    fn poll(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }
    fn answer(&mut self, answer: Answer) {
        self.answers.push(answer);
    }
    fn show_photo(&mut self, _port: u16, id: serde_json::Value) {
        self.shown.push(id);
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
    fn poll_probe(&mut self) -> Option<(u16, Result<Probe, String>)> {
        self.probe.clone().map(|answer| (7980, answer))
    }
    fn pick_new_library_folder(&mut self) -> Option<PathBuf> {
        self.new_folder.clone()
    }
    fn library_action(&mut self, _port: u16, action: LibraryAction) {
        // Answer like the app: the new or opened library is open now.
        let path = match &action {
            LibraryAction::Open(path) => path.clone(),
            LibraryAction::Create { folder, name } => folder.join(name),
        };
        let answer = Probe {
            persistent: true,
            library: Some(path),
            xmp_auto_write: Some(false),
        };
        self.probe = Some(Ok(answer.clone()));
        self.action_answer = Some(Ok(answer));
        self.actions.push(action);
    }
    fn poll_library_action(&mut self) -> Option<Result<Probe, String>> {
        self.action_answer.take()
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
    Harness::builder()
        .with_size([width, 1400.0])
        .build_ui_state(
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
    assert!(h.state().app.scan.summary.is_some(), "the scan arrived");
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
            // The question before writing: for all, then per photo.
            let mut h = asked(&w);
            h.state_mut().app.settings.theme = mode;
            h.set_size(egui::vec2(width, 1100.0));
            theme::install_fonts(&h.ctx);
            theme::apply(&h.ctx, mode);
            h.run_steps(6);
            h.render()
                .unwrap()
                .save(dir.join(format!("existing-{theme_name}-{width}.png")))
                .unwrap();
            h.get_by_label("Decide per photo").click();
            h.run_steps(4);
            h.render()
                .unwrap()
                .save(dir.join(format!("existing-each-{theme_name}-{width}.png")))
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
        if a.scan.summary.is_some() && matches!(a.app.answer, Some(Ok(_))) {
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
    assert!(run.error.is_none(), "{:?}", run.error);
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
    let mut h = Harness::builder().with_size([400.0, 200.0]).build_ui_state(
        |ui, ready: &mut bool| {
            if !*ready {
                return;
            }
            for font in [
                egui::FontId::proportional(13.0),
                egui::FontId::monospace(12.0),
                theme::font(13.0, Some(theme::MEDIUM)),
                theme::font(13.0, Some(theme::SEMIBOLD)),
            ] {
                ui.label(egui::RichText::new("Köln 🌅 Sonnenuntergang").font(font));
            }
        },
        false,
    );
    theme::install_fonts(&h.ctx);
    h.run_steps(1);
    *h.state_mut() = true;
    h.run_steps(3);
}

#[test]
fn an_empty_target_follows_the_library_open_in_lightcraft() {
    let w = world();
    let mut state = ready(&w);
    state.app.settings.library.clear();
    let mut h = harness(state, 1280.0);
    h.run_steps(4);
    assert_eq!(
        h.state().app.settings.library,
        w.library.display().to_string()
    );
    h.get_by_label("Follows the library that is open in LightCraft.");

    // Choosing another library stops following; one click follows again.
    let other = w.library.parent().unwrap().join("Other.lightcraft");
    std::fs::create_dir_all(&other).unwrap();
    h.state_mut().host.library = Some(other.clone());
    h.get_by_label("Open existing…").click();
    h.run_steps(3);
    assert_eq!(h.state().app.settings.library, other.display().to_string());
    assert!(!h.state().app.settings.follow_app);
    h.get_by_label("LightCraft has a different library open than the one chosen here");
    h.get_by_label("Use library open in LightCraft").click();
    h.run_steps(3);
    assert_eq!(
        h.state().app.settings.library,
        w.library.display().to_string()
    );
}

#[test]
fn the_chosen_library_can_be_opened_in_lightcraft() {
    let w = world();
    let mut state = ready(&w);
    let other = w.library.parent().unwrap().join("Other.lightcraft");
    std::fs::create_dir_all(&other).unwrap();
    state.app.settings.library = other.display().to_string();
    state.app.settings.follow_app = false;
    let mut h = harness(state, 1280.0);
    h.run_steps(4);
    h.get_by_label("Open the chosen library in LightCraft")
        .click();
    h.run_steps(4);
    assert_eq!(h.state().host.actions, [LibraryAction::Open(other)]);
    h.get_by_label("LightCraft is running, control port connected");
}

#[test]
fn a_new_library_is_only_created_after_confirmation() {
    let w = world();
    let mut state = ready(&w);
    let folder = w.library.parent().unwrap().to_path_buf();
    state.host.new_folder = Some(folder.clone());
    let mut h = harness(state, 1280.0);
    h.run_steps(4);
    h.get_by_label("Create new…").click();
    h.run_steps(3);
    assert!(
        h.state().host.actions.is_empty(),
        "nothing yet: it asks first"
    );
    h.get_by_label("Create a new library");
    h.get_by_label("Cancel").click();
    h.run_steps(6);
    assert!(h.state().host.actions.is_empty());
    assert!(h.state().app.new_library.is_none());

    h.get_by_label("Create new…").click();
    h.run_steps(3);
    h.get_by_label("Create library").click();
    h.run_steps(4);
    let expected = folder.join("LightCraft Library");
    assert_eq!(
        h.state().host.actions,
        [LibraryAction::Create {
            folder,
            name: "LightCraft Library".into()
        }]
    );
    assert_eq!(
        h.state().app.settings.library,
        expected.display().to_string()
    );
    assert!(h.state().app.settings.follow_app);
}

#[test]
fn checks_explain_themselves_and_options_show_their_counts() {
    let w = world();
    let mut h = harness(ready(&w), 1280.0);
    h.run_steps(4);
    for what in [
        "Catalog file (.lrcat)",
        "Companion file (.lrcat-data)",
        "Catalog version",
        "Catalog open in Lightroom",
        "Write XMP automatically",
    ] {
        assert_eq!(
            h.query_all_by_label_contains(&format!("About {what}: "))
                .count(),
            1,
            "{what} has an explanation"
        );
    }
    // The fixture has 12 keywords; the option row shows the number.
    let keywords = h.state().app.scan.summary.clone().unwrap().keywords;
    assert!(keywords > 0);
    assert!(h.query_all_by_label_contains("Keywords").count() >= 1);
}

#[test]
fn the_log_counts_photos_once_and_groups_entries_by_photo() {
    let w = world();
    let mut h = harness(ready(&w), 1280.0);
    h.run_steps(4);
    h.get_by_label("Dry run (writes nothing)").click();
    h.run_steps(2);
    h.state_mut()
        .host
        .events
        .push(Event::Done(Box::new(dry_run(&w))));
    h.run_steps(4);
    let data = h.state().app.log.data.clone();
    let counts = data.photo_counts.expect("the report counts photos");
    assert!(counts.total > 0 && (data.rows.len() as u64) > counts.total);
    h.get_by_label(&counts.line(true));

    h.get_by_label("Group by photo (one row per photo)").click();
    h.run_steps(3);
    assert!(h.state().app.log.group_by_photo);
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(3);
    let photo = h
        .state()
        .app
        .log
        .selected_photo
        .expect("a photo is selected");
    let groups = data.by_photo(&data.filtered(&Default::default()));
    assert_eq!(groups[0].photo, photo);
    assert!(groups.len() as u64 <= counts.total);
}

#[test]
fn a_stopped_transfer_is_not_shown_as_finished() {
    let w = world();
    let mut h = harness(ready(&w), 1280.0);
    h.run_steps(4);
    h.get_by_label("Transfer now").click();
    h.run_steps(2);
    let stopped = RunResult::failed(
        true,
        lr2lc_core::ErrorKind::Stopped,
        "writing to LightCraft stopped: LightCraft has another library open".into(),
    );
    h.state_mut()
        .host
        .events
        .push(Event::Done(Box::new(stopped)));
    h.run_steps(4);
    assert!(h.query_all_by_label("Transfer stopped").count() >= 1);
    assert_eq!(h.query_all_by_label("Transfer finished").count(), 0);
    h.get_by_label_contains("LightCraft has another library open");
    assert_eq!(h.query_all_by_label("100 %").count(), 0);
    // The reason is also an entry of the run in the Log.
    let rows = &h.state().app.log.data.rows;
    assert!(
        rows.iter()
            .any(|r| r.message == "The run stopped before it was finished")
    );
}

#[test]
fn a_port_change_drops_the_old_answer_and_typing_does_not_scan_per_key() {
    let w = world();
    let mut h = harness(ready(&w), 1280.0);
    h.run_steps(4);
    assert_eq!(h.state().host.scans, 1);
    // An answer for the old port is not applied to the new one.
    h.state_mut().app.settings.port = 7999;
    h.run_steps(3);
    assert!(h.state().app.app.answer.is_none());
    h.state_mut().app.settings.port = 7980;
    h.run_steps(3);
    assert!(matches!(h.state().app.app.answer, Some(Ok(_))));
    // Editing a mapping: the old numbers go at once, one scan follows later.
    for text in ["/a", "/ab", "/abc"] {
        let rows = h.state_mut().app.settings.rows_mut();
        rows.clear();
        rows.push(lr2lc_gui::settings::MappingRow {
            old: text.into(),
            new: "/x".into(),
        });
        h.run_steps(2);
    }
    assert!(h.state().app.scan.summary.is_none());
    assert_eq!(h.state().host.scans, 1, "no scan per key");
    h.run_steps(40);
    assert_eq!(h.state().host.scans, 2);
}

#[test]
fn stop_keeps_and_cancel_asks_before_taking_back() {
    let w = world();
    let mut h = harness(ready(&w), 1280.0);
    h.run_steps(4);
    h.get_by_label("Transfer now").click();
    h.run_steps(3);
    h.get_by_label("Cancel…").click();
    h.run_steps(3);
    assert!(!h.state().host.cancelled, "Cancel asks first");
    h.get_by_label("Keep running").click();
    h.run_steps(3);
    assert!(!h.state().host.cancelled);
    h.get_by_label("Cancel…").click();
    h.run_steps(3);
    h.get_by_label("Cancel and take back").click();
    h.run_steps(3);
    assert!(h.state().host.cancelled && h.state().host.rolled_back);

    let mut h = harness(ready(&w), 1280.0);
    h.run_steps(4);
    h.get_by_label("Transfer now").click();
    h.run_steps(3);
    h.get_by_label("Stop").click();
    h.run_steps(3);
    assert!(h.state().host.cancelled && !h.state().host.rolled_back);
    h.get_by_label("Stopping at the next photo…");
}

#[test]
fn a_cancelled_transfer_leaves_the_library_as_it_was() {
    use lc_test_support::fake_lightcraft::{Config, FakeLightCraft};
    let w = world();
    let fake = FakeLightCraft::start(Config::new(&w.library));
    let mut settings = settings(&w);
    settings.port = fake.port();
    let job = settings.job(true).unwrap();
    let (tx, _rx) = channel();
    let mut progress = ChannelProgress::new(tx, Arc::new(AtomicBool::new(false)));
    progress.set_roll_back(true);
    let result = execute(&job, &mut progress, None);
    let (kind, message) = result.error.clone().expect("a cancelled run reports it");
    assert_eq!(kind, lr2lc_core::ErrorKind::Cancelled, "{message}");
    assert!(
        fake.photos().is_empty(),
        "every imported photo removed again"
    );
    // LightCraft's own (empty) Quick Collection may stay; no album of ours.
    assert!(
        fake.albums()
            .iter()
            .all(|a| a.name == "Quick Collection" && a.photos.is_empty()),
        "{:?}",
        fake.albums()
    );
    assert!(fake.keywords().is_empty());
    let report: serde_json::Value = serde_json::from_str(&result.report_text).unwrap();
    assert!(report["rollback"]["photos_removed"].as_u64().unwrap() > 0);
    assert_eq!(report["rollback"]["mapping_restored"], true);
}

/// A transfer against a real LightCraft, for `ops/tools/cancel-live.sh`:
/// catalog, library and path mapping from the environment, with Cancel
/// (`LR2LC_LIVE_ROLLBACK=1`) or without.
#[test]
#[ignore = "needs a running LightCraft with a throwaway library"]
fn live_transfer() {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    let mut settings = Settings {
        catalog: var("LR2LC_LIVE_CATALOG"),
        library: var("LR2LC_LIVE_LIBRARY"),
        ..Settings::default()
    };
    if let Some((old, new)) = var("LR2LC_LIVE_MAP").split_once('=') {
        settings.rows_mut().push(lr2lc_gui::settings::MappingRow {
            old: old.into(),
            new: new.into(),
        });
    }
    let job = settings.job(true).unwrap();
    let (tx, _rx) = channel();
    let mut progress = ChannelProgress::new(tx, Arc::new(AtomicBool::new(false)));
    progress.set_roll_back(var("LR2LC_LIVE_ROLLBACK") == "1");
    let result = execute(&job, &mut progress, None);
    let report: serde_json::Value = serde_json::from_str(&result.report_text).unwrap_or_default();
    println!(
        "LIVE error={:?} imported={} rollback={}",
        result.error.as_ref().map(|(k, _)| k),
        report["writer"]["photos_imported"],
        report["rollback"]
    );
    let failed: Vec<&str> = report["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["stage"] == "rollback" && e["status"] == "error")
        .filter_map(|e| e["message"].as_str())
        .collect();
    println!("LIVE rollback errors: {failed:?}");
}

#[test]
fn a_quiet_lightcraft_is_pointed_out() {
    let w = world();
    let mut h = harness(ready(&w), 1280.0);
    h.run_steps(4);
    h.get_by_label("Transfer now").click();
    h.run_steps(2);
    h.state_mut()
        .host
        .events
        .push(Event::Phase(lr2lc_core::Phase::Writing, 10));
    h.run_steps(2);
    assert_eq!(h.query_all_by_label("Show LightCraft").count(), 0);
    if let Some(run) = &mut h.state_mut().app.run {
        run.last_event = chrono::Local::now() - chrono::Duration::seconds(20);
    }
    h.run_steps(2);
    h.get_by_label("Show LightCraft");
}

/// An answer with `choice` for every existing photo, deleted ones imported
/// again.
fn go(choice: lr2lc_core::ExistingChoice) -> Answer {
    Answer::Go {
        existing: lr2lc_core::ExistingDecisions::all(choice),
        removed: lr2lc_core::RemovedDecisions::default(),
    }
}

/// Two photos LightCraft already has; the second one edited there.
fn two_existing() -> Question {
    let photo = |n: i64, edited: bool| lr2lc_core::ExistingPhoto {
        global_id: format!("GID-{n}"),
        lightroom_id: n,
        path: PathBuf::from(format!("/Users/example/Pictures/IMG_{n}.CR3")),
        file_name: format!("IMG_{n}.CR3"),
        lightcraft_id: serde_json::json!(700 + n),
        reason: lr2lc_core::ExistingReason::Path,
        live: lr2lc_core::LivePhoto {
            rating: 3,
            flag: "none".to_owned(),
            label: None,
            title: None,
            keywords: Vec::new(),
            caption: None,
            copyright: None,
            creator: None,
            edited,
        },
    };
    Question {
        existing: vec![photo(1, false), photo(2, true)],
        ..Question::default()
    }
}

/// Starts a transfer in the scripted window and lets the check find
/// `question`.
fn asked_with(w: &World, question: Question) -> Harness<'static, State> {
    let mut h = harness(ready(w), 1280.0);
    h.run_steps(4);
    h.get_by_label("Transfer now").click();
    h.run_steps(3);
    h.state_mut()
        .host
        .events
        .push(Event::Question(Box::new(question)));
    // The window sizes itself to its content over a few frames.
    h.run_steps(8);
    h
}

/// Starts a transfer in the scripted window and lets the check find
/// [`two_existing`].
fn asked(w: &World) -> Harness<'static, State> {
    asked_with(w, two_existing())
}

#[test]
fn photos_already_in_lightcraft_are_asked_about_before_writing() {
    let w = world();
    let mut h = asked(&w);
    h.get_by_label("2 photos are already in LightCraft");
    h.get_by_label("waiting for your choice");
    assert!(h.state().host.answers.is_empty(), "nothing answered yet");
    // The safe choice is preselected; one click applies it to all.
    h.get_by_label("Continue transfer").click();
    h.run_steps(3);
    assert_eq!(
        h.state().host.answers,
        vec![go(lr2lc_core::ExistingChoice::DevelopIfEmpty)]
    );
    assert!(h.state().app.run.as_ref().unwrap().question.is_none());
    assert!(h.query_by_label("Continue transfer").is_none());

    // Another choice for all.
    let mut h = asked(&w);
    h.get_by_label("Skip").click();
    h.run_steps(2);
    h.get_by_label("Continue transfer").click();
    h.run_steps(2);
    assert_eq!(
        h.state().host.answers,
        vec![go(lr2lc_core::ExistingChoice::Skip)]
    );
}

#[test]
fn each_photo_can_be_decided_and_shown_in_lightcraft() {
    let w = world();
    let mut h = asked(&w);
    h.get_by_label("Decide per photo").click();
    h.run_steps(3);
    h.get_by_label("IMG_2.CR3");
    h.get_all_by_label("Show in LightCraft")
        .nth(1)
        .unwrap()
        .click();
    h.run_steps(2);
    assert_eq!(h.state().host.shown, vec![serde_json::json!(702)]);
    // Only the edited one is listed with the filter on.
    h.get_by_label("Only photos edited in LightCraft").click();
    h.run_steps(3);
    assert!(h.query_by_label("IMG_1.CR3").is_none());
    h.get_by_label("Only photos edited in LightCraft").click();
    h.run_steps(3);
    // Choose per photo (the combo box is set through the state here).
    h.state_mut()
        .app
        .run
        .as_mut()
        .unwrap()
        .question
        .as_mut()
        .unwrap()
        .per_photo[1] = lr2lc_core::ExistingChoice::Skip;
    h.run_steps(2);
    h.get_by_label("Continue transfer").click();
    h.run_steps(2);
    let Answer::Go {
        existing: decisions,
        ..
    } = &h.state().host.answers[0]
    else {
        panic!("{:?}", h.state().host.answers);
    };
    assert_eq!(
        decisions.default,
        lr2lc_core::ExistingChoice::DevelopIfEmpty
    );
    assert_eq!(
        decisions.choice_for("GID-1"),
        lr2lc_core::ExistingChoice::DevelopIfEmpty
    );
    assert_eq!(
        decisions.choice_for("GID-2"),
        lr2lc_core::ExistingChoice::Skip
    );
}

#[test]
fn cancelling_the_question_writes_nothing() {
    let w = world();
    let mut h = asked(&w);
    h.get_by_label("Cancel transfer").click();
    h.run_steps(3);
    assert_eq!(h.state().host.answers, vec![Answer::Abort]);
    assert!(h.state().app.run.as_ref().unwrap().cancelling);
}

/// The worker side against the fake: the check asks, the answer decides.
fn transfer_with_answer(answer: Answer) -> (RunResult, Vec<Event>, Vec<serde_json::Value>) {
    use lc_test_support::fake_lightcraft::{Config, FakeLightCraft, SeedPhoto};
    let w = world();
    let mut settings = settings(&w);
    let probe_job = settings.job(true).unwrap();
    let prepared = lr2lc_core::prepare(&probe_job, &mut lr2lc_core::NoProgress).unwrap();
    let first = prepared
        .input
        .photos
        .iter()
        .filter(|p| p.global_id.is_some() && p.master.is_none())
        .min_by_key(|p| p.global_id.clone())
        .unwrap();
    let mut config = Config::new(&w.library);
    config.seed_photos = vec![SeedPhoto {
        rating: 3,
        edited: true,
        ..SeedPhoto::plain(7)
    }];
    config.file_contents.insert(
        "/Users/example/Pictures/seed-7.jpg".to_owned(),
        "same bytes".into(),
    );
    config.file_contents.insert(
        first.path.to_string_lossy().into_owned(),
        "same bytes".into(),
    );
    let fake = FakeLightCraft::start(config);
    settings.port = fake.port();
    let job = settings.job(true).unwrap();
    let (tx, rx) = channel();
    let (answers, answer_rx) = channel();
    answers.send(answer).unwrap();
    let mut progress = ChannelProgress::new(tx, Arc::new(AtomicBool::new(false)));
    progress.set_answers(answer_rx);
    let result = execute(&job, &mut progress, None);
    let events: Vec<Event> = rx.try_iter().collect();
    let photos = fake
        .photos()
        .values()
        .map(|id| serde_json::json!(id))
        .collect();
    (result, events, photos)
}

#[test]
fn the_run_waits_for_the_answer_and_applies_it() {
    let (result, events, _) = transfer_with_answer(go(lr2lc_core::ExistingChoice::Skip));
    assert!(result.error.is_none(), "{:?}", result.error);
    let asked: Vec<&Question> = events
        .iter()
        .filter_map(|e| match e {
            Event::Question(q) => Some(&**q),
            _ => None,
        })
        .collect();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].existing.len(), 1);
    assert_eq!(asked[0].existing[0].lightcraft_id, serde_json::json!(7));
    assert!(asked[0].existing[0].live.edited);
    let report: serde_json::Value = serde_json::from_str(&result.report_text).unwrap();
    assert_eq!(
        report["writer"]["existing"]["skip"], 1,
        "{}",
        report["writer"]
    );
}

#[test]
fn cancelling_before_writing_leaves_the_library_alone() {
    let (result, _, photos) = transfer_with_answer(Answer::Abort);
    let (kind, _) = result.error.expect("the run reports the cancel");
    assert_eq!(kind, lr2lc_core::ErrorKind::Cancelled);
    assert_eq!(photos, vec![serde_json::json!(7)], "nothing imported");
}

#[test]
fn photos_deleted_in_lightcraft_are_imported_again_or_left() {
    let removed = |n: i64, trashed: bool| lr2lc_core::RemovedInLightCraft {
        global_id: format!("GID-{n}"),
        lightroom_id: n,
        path: PathBuf::from(format!("/Users/example/Pictures/IMG_{n}.CR3")),
        file_name: format!("IMG_{n}.CR3"),
        lightcraft_id: serde_json::json!(800 + n),
        trashed,
        virtual_copy: false,
    };
    let question = || Question {
        removed: vec![removed(5, false), removed(6, true)],
        ..Question::default()
    };
    let w = world();
    let mut h = asked_with(&w, question());
    h.get_by_label("2 photos were deleted in LightCraft");
    assert!(h.query_by_label("Same for all photos").is_none());
    h.get_by_label("Continue transfer").click();
    h.run_steps(2);
    assert_eq!(
        h.state().host.answers,
        vec![go(lr2lc_core::ExistingChoice::DevelopIfEmpty)],
        "imported again by default"
    );

    let mut h = asked_with(&w, question());
    h.get_by_label("Leave them deleted").click();
    h.run_steps(2);
    h.get_by_label("Continue transfer").click();
    h.run_steps(2);
    let Answer::Go { removed, .. } = &h.state().host.answers[0] else {
        panic!("{:?}", h.state().host.answers);
    };
    assert_eq!(removed.default, lr2lc_core::RemovedChoice::Leave);

    // Both at once: one window, one answer.
    let mut both = two_existing();
    both.removed = question().removed;
    let mut h = asked_with(&w, both);
    h.get_by_label("2 photos are already in LightCraft");
    h.get_by_label("2 photos were deleted in LightCraft");
    h.get_by_label("Continue transfer").click();
    h.run_steps(2);
    assert_eq!(h.state().host.answers.len(), 1);
}

#[test]
fn originals_only_in_icloud_are_pointed_out_before_the_check() {
    let w = world();
    let mut h = asked_with(
        &w,
        Question {
            cloud_only: (3, 75_000_000),
            ..Question::default()
        },
    );
    h.get_by_label("3 photos are only in iCloud");
    h.get_by_label("Cancel transfer").click();
    h.run_steps(2);
    assert_eq!(h.state().host.answers, vec![Answer::Abort]);

    let mut h = asked_with(
        &w,
        Question {
            cloud_only: (1, 25_000_000),
            ..Question::default()
        },
    );
    h.get_by_label("1 photo is only in iCloud");
    h.get_by_label("Transfer anyway").click();
    h.run_steps(2);
    assert!(matches!(h.state().host.answers[..], [Answer::Go { .. }]));
}
