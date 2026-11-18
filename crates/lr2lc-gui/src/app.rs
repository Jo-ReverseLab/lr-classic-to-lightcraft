//! The window: top bar with tabs and the theme button, the three screens
//! (Transfer, Running, Log) and the status bar. Draws into any `egui::Ui`
//! and reaches its surroundings only through [`Host`], so the same window
//! can sit in the standalone app or inside a host.

use crate::checks::{CatalogFiles, catalog_files};
use crate::history::{History, Kind, Record, new_id};
use crate::host::Host;
use crate::icons::{self, Icon};
use crate::logdata::{Filter, LogData, Row, ScanSummary};
use crate::run::{Event, RunResult};
use crate::settings::Settings;
use crate::theme::{self, MEDIUM, SEMIBOLD, Tokens, font, tokens};
use crate::{log_view, running, transfer};
use chrono::{DateTime, Local};
use egui::{Align, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};
use lr2lc_core::probe::Probe;
use lr2lc_core::{ErrorKind, Phase};
use model::{Stage, Status};
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;

/// The tab the top bar shows as active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Transfer,
    Log,
}

/// A run in progress or just finished, as the Running screen shows it.
pub struct RunState {
    pub kind: Kind,
    pub started: DateTime<Local>,
    pub catalog: String,
    pub library: String,
    /// The reading phase, while reading.
    pub reading: Option<Phase>,
    /// Steps per stage, once the catalog is planned.
    pub stage_totals: Option<BTreeMap<Stage, usize>>,
    /// Phase totals and progress of the current phase.
    pub phase_total: usize,
    pub phase_done: usize,
    /// The library is being checked for photos it already has.
    pub checking_library: bool,
    pub library_checked: bool,
    /// What the check found, while the person decides.
    pub question: Option<crate::existing::QuestionState>,
    /// The library is being read (a second transfer).
    pub reading_library: bool,
    pub library_read: bool,
    /// Steps done while writing.
    pub written: usize,
    pub writing: bool,
    /// Entries seen so far, per status.
    pub counts: BTreeMap<Status, usize>,
    /// Entries seen so far, per stage.
    pub stage_entries: BTreeMap<Stage, BTreeMap<Status, usize>>,
    /// The photos that had an entry so far.
    pub photos_seen: std::collections::BTreeSet<i64>,
    /// The last entries with their time.
    pub tail: VecDeque<(DateTime<Local>, Row)>,
    pub cancelling: bool,
    /// Cancel was pressed; waiting for the confirmation.
    pub confirm_cancel: bool,
    /// The run is being taken back.
    pub rolling_back: bool,
    /// When the run last reported anything.
    pub last_event: DateTime<Local>,
    finished: bool,
    /// Why the run failed or stopped.
    pub error: Option<(ErrorKind, String)>,
    /// The photo count line of the finished run.
    pub photo_line: Option<String>,
}

impl RunState {
    pub fn finished(&self) -> bool {
        self.finished
    }

    pub fn cancelled(&self) -> bool {
        self.error
            .as_ref()
            .is_some_and(|(k, _)| *k == ErrorKind::Cancelled)
    }
}

/// The catalog scan behind the summary and the check list.
#[derive(Default)]
pub struct Scan {
    /// What the scan was made for (catalog, library, mappings, labels).
    pub key: String,
    pub pending: bool,
    pub summary: Option<ScanSummary>,
    pub error: Option<(ErrorKind, String)>,
    /// When the scan for `key` starts: a moment after the last change, so
    /// typing a path does not start a scan per character.
    due: Option<f64>,
}

/// The last answer of LightCraft's control port.
#[derive(Default)]
pub struct AppStatus {
    pub port: u16,
    pub asked_at: Option<f64>,
    pub answer: Option<Result<Probe, String>>,
}

/// The Log screen's state.
#[derive(Default)]
pub struct LogState {
    pub records: Vec<Record>,
    pub shown: Option<String>,
    pub data: LogData,
    pub report_text: String,
    pub anonymized_text: String,
    pub filter: Filter,
    pub selected: Option<usize>,
    /// One row per photo instead of one per entry.
    pub group_by_photo: bool,
    /// The photo selected in the grouped view.
    pub selected_photo: Option<i64>,
    pub copied: bool,
    pub saved: Option<Result<PathBuf, String>>,
    pub anonymize_save: bool,
    /// The search field's id, to leave arrow keys to it while it has focus.
    pub search_id: Option<egui::Id>,
    /// The entries that pass the filters and, when grouped, their photos;
    /// recomputed only when the report, the filters or the grouping change.
    pub visible: Vec<usize>,
    pub groups: Option<Vec<crate::logdata::PhotoGroup>>,
    cached_for: Option<(Filter, bool, usize)>,
    /// Scroll the list so that this row is in view.
    pub scroll_to: Option<usize>,
}

impl LogState {
    /// Brings `visible` and `groups` up to date.
    pub fn refresh(&mut self) {
        let key = (
            self.filter.clone(),
            self.group_by_photo,
            self.data.rows.len(),
        );
        if self.cached_for.as_ref() == Some(&key) {
            return;
        }
        self.visible = self.data.filtered(&self.filter);
        self.groups = self
            .group_by_photo
            .then(|| self.data.by_photo(&self.visible));
        // Keep a selection only while it is still shown.
        if let Some(sel) = self.selected
            && self.visible.binary_search(&sel).is_err()
        {
            self.selected = None;
        }
        if let (Some(sel), Some(groups)) = (self.selected_photo, &self.groups)
            && !groups.iter().any(|g| g.photo == sel)
        {
            self.selected_photo = None;
        }
        self.cached_for = Some(key);
    }
}

/// The whole window.
pub struct App {
    pub settings: Settings,
    pub changed: bool,
    pub tab: Tab,
    pub run: Option<RunState>,
    pub scan: Scan,
    pub app: AppStatus,
    pub files: CatalogFiles,
    pub files_checked_at: Option<f64>,
    pub history: Option<History>,
    /// Set by the writer thread when a run could not be stored.
    pub history_failed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub log: LogState,
    pub advanced_open: bool,
    pub problem: Option<String>,
    /// The new library being confirmed: folder and name.
    pub new_library: Option<(PathBuf, String)>,
    /// A library is being opened or created in LightCraft.
    pub library_busy: bool,
    pub library_error: Option<String>,
    /// Follow LightCraft's library once the running library action succeeds.
    pub follow_after_action: bool,
    /// Whether LightCraft has the chosen library open (`None`: unknown);
    /// checked with the catalog's files, not every frame.
    pub library_matches: Option<bool>,
    checked_library: String,
    recheck_library: bool,
    /// Fonts and themes are installed (on the first frame).
    styled: bool,
}

impl App {
    pub fn new(settings: Settings, history: Option<History>) -> App {
        let records = history.as_ref().map(History::list).unwrap_or_default();
        App {
            settings,
            changed: false,
            tab: Tab::Transfer,
            run: None,
            scan: Scan::default(),
            app: AppStatus::default(),
            files: CatalogFiles::default(),
            files_checked_at: None,
            history,
            history_failed: Default::default(),
            log: LogState {
                records,
                ..LogState::default()
            },
            advanced_open: false,
            problem: None,
            new_library: None,
            library_busy: false,
            library_error: None,
            follow_after_action: false,
            library_matches: None,
            checked_library: String::new(),
            recheck_library: false,
            styled: false,
        }
    }

    /// LightCraft is known to have another library open than the chosen one.
    pub fn library_mismatch(&self) -> bool {
        self.library_matches == Some(false)
    }

    /// Whether the settings changed since the last call (to save them).
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Whether a run is in progress.
    pub fn is_running(&self) -> bool {
        self.run.as_ref().is_some_and(|r| !r.finished())
    }

    /// Draws the window.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.styled {
            // Fonts take effect from the next frame: draw nothing until then.
            theme::install_fonts(ui.ctx());
            theme::apply(ui.ctx(), self.settings.theme);
            self.styled = true;
            ui.ctx().request_repaint();
            return;
        }
        let t = tokens(ui.ctx());
        self.poll(ui, host);
        egui::Panel::top("top-bar")
            .exact_size(52.0)
            .frame(
                Frame::new()
                    .fill(t.bg_bar)
                    .stroke(Stroke::new(1.0, t.border)),
            )
            .show(ui, |ui| self.top_bar(ui, t));
        egui::Panel::bottom("status-bar")
            .exact_size(36.0)
            .frame(
                Frame::new()
                    .fill(t.bg_bar)
                    .stroke(Stroke::new(1.0, t.border))
                    .inner_margin(Margin::symmetric(16, 0)),
            )
            .show(ui, |ui| self.status_bar(ui, t));
        egui::CentralPanel::default()
            .frame(Frame::new().fill(t.bg_app))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        Frame::new()
                            .inner_margin(Margin::same(16))
                            .show(ui, |ui| match self.tab {
                                Tab::Transfer if self.run.is_some() => {
                                    running::ui(self, ui, host, t)
                                }
                                Tab::Transfer => transfer::ui(self, ui, host, t),
                                Tab::Log => log_view::ui(self, ui, host, t),
                            });
                    });
            });
    }

    fn top_bar(&mut self, ui: &mut Ui, t: &Tokens) {
        ui.horizontal(|ui| {
            ui.set_height(52.0);
            ui.add_space(16.0);
            ui.label(
                RichText::new("lr2lc")
                    .font(font(15.0, Some(SEMIBOLD)))
                    .color(t.text_strong),
            );
            ui.label(
                RichText::new("Lightroom Classic → LightCraft")
                    .font(font(12.0, None))
                    .color(t.text_muted),
            );
            ui.add_space(16.0);
            for (tab, name) in [(Tab::Transfer, "Transfer"), (Tab::Log, "Log")] {
                if self.tab_button(ui, t, tab, name).clicked() {
                    self.select_tab(tab);
                }
                if tab == Tab::Log {
                    self.problem_badge(ui, t);
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                let mode = self.settings.theme;
                let (rect, response) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
                let icon = match mode {
                    theme::ThemeMode::System => Icon::Monitor,
                    theme::ThemeMode::Light => Icon::Sun,
                    theme::ThemeMode::Dark => Icon::Moon,
                };
                if response.hovered() {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(4), t.bg_raised);
                }
                icons::paint(
                    ui.painter(),
                    egui::Rect::from_center_size(rect.center(), vec2(16.0, 16.0)),
                    icon,
                    t.text,
                    0.0,
                );
                focus_ring(ui, &response, t);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, mode.label())
                });
                if response.on_hover_text(mode.label()).clicked() {
                    self.settings.theme = mode.next();
                    theme::apply(ui.ctx(), self.settings.theme);
                    self.changed = true;
                }
            });
        });
    }

    fn tab_button(&self, ui: &mut Ui, t: &Tokens, tab: Tab, name: &str) -> egui::Response {
        let active = self.tab == tab;
        let galley = ui.painter().layout_no_wrap(
            name.to_owned(),
            font(13.0, Some(MEDIUM)),
            if active { t.text_strong } else { t.text_muted },
        );
        let (rect, response) =
            ui.allocate_exact_size(vec2(galley.size().x + 32.0, 52.0), Sense::click());
        // The selected tab is lightly filled, a hovered one a little.
        if active {
            ui.painter().rect_filled(rect, 0.0, t.bg_selected);
        } else if response.hovered() {
            ui.painter().rect_filled(rect, 0.0, t.bg_raised);
        }
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, t.text);
        let bar = egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - 2.0), rect.max);
        if active {
            ui.painter().rect_filled(bar, 0.0, t.accent);
        }
        focus_ring(ui, &response, t);
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, active, name));
        response
    }

    fn problem_badge(&self, ui: &mut Ui, t: &Tokens) {
        let Some(latest) = self.latest_counts() else {
            return;
        };
        let errors = latest.get(&Status::Error).copied().unwrap_or(0);
        let conflicts = latest.get(&Status::Conflict).copied().unwrap_or(0);
        let mut parts = Vec::new();
        if errors > 0 {
            parts.push(plural(errors, "error"));
        }
        if conflicts > 0 {
            parts.push(plural(conflicts, "conflict"));
        }
        if parts.is_empty() {
            return;
        }
        Frame::new()
            .fill(t.error_badge_bg)
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(8, 2))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(parts.join(", "))
                        .font(font(11.0, Some(MEDIUM)))
                        .color(t.error),
                );
            });
    }

    /// Counts per status of the latest finished run.
    fn latest_counts(&self) -> Option<BTreeMap<Status, usize>> {
        if let Some(run) = &self.run {
            if run.finished() {
                return Some(run.counts.clone());
            }
            return None;
        }
        if self.log.shown.is_some()
            && self.log.shown == self.log.records.first().map(|r| r.id.clone())
        {
            return Some(
                Status::ALL
                    .iter()
                    .map(|s| (*s, self.log.data.count(*s)))
                    .collect(),
            );
        }
        None
    }

    fn status_bar(&self, ui: &mut Ui, t: &Tokens) {
        let state = match &self.run {
            Some(run) if !run.finished() => "Running…".to_owned(),
            Some(run) if run.cancelled() => "Run cancelled".to_owned(),
            Some(run) if run.error.is_some() => "Run stopped".to_owned(),
            Some(run) => match run.kind {
                Kind::DryRun => "Dry run finished".to_owned(),
                Kind::Transfer => "Transfer finished".to_owned(),
            },
            None => "Ready".to_owned(),
        };
        ui.horizontal_centered(|ui| {
            ui.label(
                RichText::new(state)
                    .font(font(12.0, None))
                    .color(t.text_muted),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("lr2lc {}", env!("CARGO_PKG_VERSION")))
                        .font(font(12.0, None))
                        .color(t.text_muted),
                );
            });
        });
    }

    pub fn select_tab(&mut self, tab: Tab) {
        self.tab = tab;
        if tab == Tab::Log
            && self.log.shown.is_none()
            && let Some(first) = self.log.records.first().map(|r| r.id.clone())
        {
            self.show_run(&first);
        }
    }

    /// Shows run `id` in the Log.
    pub fn show_run(&mut self, id: &str) {
        let read = |anonymized: bool| {
            self.history
                .as_ref()
                .and_then(|h| {
                    if anonymized {
                        h.anonymized(id)
                    } else {
                        h.report(id)
                    }
                })
                .unwrap_or_default()
        };
        let (text, anonymized) = (read(false), read(true));
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        self.show_report(id, LogData::from_report(&value), text, anonymized);
    }

    fn show_report(&mut self, id: &str, mut data: LogData, text: String, anonymized: String) {
        if let Some(record) = self.log.records.iter().find(|r| r.id == id)
            && let Some(error) = &record.error
        {
            // Why the run stopped, as its own entry at the end.
            data.rows.push(
                Row {
                    status: Status::Error,
                    area: "photos".into(),
                    stage: "run".into(),
                    item: String::new(),
                    message: if record.cancelled {
                        "Run cancelled".into()
                    } else {
                        "The run stopped before it was finished".into()
                    },
                    code: None,
                    reason: Some(error.clone()),
                    recommendation: None,
                    source: None,
                    result: None,
                    photo: None,
                    search: String::new(),
                }
                .indexed(),
            );
            data.recount();
        }
        self.log.shown = Some(id.to_owned());
        self.log.data = data;
        self.log.report_text = text;
        self.log.anonymized_text = anonymized;
        self.log.selected = None;
        self.log.selected_photo = None;
        self.log.cached_for = None;
        self.log.copied = false;
        self.log.saved = None;
    }

    /// Starts a dry run or a transfer.
    pub fn start(&mut self, host: &mut dyn Host, apply: bool) {
        let job = match self.settings.job(apply) {
            Ok(job) => job,
            Err(invalid) => {
                self.problem = Some(invalid.text());
                return;
            }
        };
        self.problem = None;
        self.run = Some(RunState {
            kind: if apply { Kind::Transfer } else { Kind::DryRun },
            started: Local::now(),
            catalog: self.settings.catalog.clone(),
            library: self.settings.library.clone(),
            reading: None,
            stage_totals: None,
            phase_total: 0,
            phase_done: 0,
            checking_library: false,
            library_checked: false,
            question: None,
            reading_library: false,
            library_read: false,
            written: 0,
            writing: false,
            counts: BTreeMap::new(),
            stage_entries: BTreeMap::new(),
            photos_seen: Default::default(),
            tail: VecDeque::new(),
            cancelling: false,
            confirm_cancel: false,
            rolling_back: false,
            last_event: Local::now(),
            finished: false,
            error: None,
            photo_line: None,
        });
        self.tab = Tab::Transfer;
        host.start(job);
    }

    fn poll(&mut self, ui: &Ui, host: &mut dyn Host) {
        let now = ui.input(|i| i.time);
        for event in host.poll() {
            self.on_event(event);
        }
        if let Some(result) = host.poll_scan()
            && self.scan.pending
        {
            self.scan.pending = false;
            self.scan.error = result.error.filter(|(k, _)| *k != ErrorKind::Cancelled);
            self.scan.summary = result.summary;
        }
        if let Some((port, answer)) = host.poll_probe() {
            if port == self.settings.port {
                self.app.answer = Some(answer);
                self.recheck_library = true;
            } else {
                // Asked before the port changed: ask the new one now.
                self.app.asked_at = None;
            }
        }
        if let Some(answer) = host.poll_library_action() {
            self.library_busy = false;
            match answer {
                Ok(probe) => {
                    self.library_error = None;
                    if let Some(path) = &probe.library {
                        self.settings.library = path.display().to_string();
                        self.changed = true;
                    }
                    // A library created here is the one to follow.
                    if std::mem::take(&mut self.follow_after_action) {
                        self.settings.follow_app = true;
                    }
                    self.app.answer = Some(Ok(probe));
                    self.recheck_library = true;
                }
                Err(e) => {
                    self.follow_after_action = false;
                    self.library_error = Some(e);
                }
            }
        }
        // Following LightCraft: the target is the library it has open.
        if self.settings.follow_app
            && !self.is_running()
            && let Some(Ok(probe)) = &self.app.answer
            && probe.persistent
            && let Some(path) = &probe.library
        {
            let open = path.display().to_string();
            if self.settings.library != open {
                self.settings.library = open;
                self.changed = true;
            }
        }
        // Does LightCraft have the chosen library open? Compared when either
        // side changed, not every frame (it resolves both paths on disk).
        if self.recheck_library || self.checked_library != self.settings.library {
            self.recheck_library = false;
            self.checked_library = self.settings.library.clone();
            self.library_matches = match &self.app.answer {
                Some(Ok(probe)) if !self.settings.library.is_empty() => {
                    Some(probe.has_open(std::path::Path::new(&self.settings.library)))
                }
                _ => None,
            };
        }
        // LightCraft's state: every 5 s, and at once when the port changes.
        // Not during a run: it holds its own connection and checks itself.
        if self.app.port != self.settings.port {
            self.app.port = self.settings.port;
            self.app.answer = None;
            self.app.asked_at = None;
            self.recheck_library = true;
        }
        if !self.is_running() && self.app.asked_at.is_none_or(|at| now - at > 5.0) {
            self.app.asked_at = Some(now);
            host.probe(self.settings.port);
        }
        // The catalog's files: every 2 s (Lightroom may be opened or closed).
        if self.files_checked_at.is_none_or(|at| now - at > 2.0) {
            self.files_checked_at = Some(now);
            self.files = if self.settings.catalog.is_empty() {
                CatalogFiles::default()
            } else {
                catalog_files(std::path::Path::new(&self.settings.catalog))
            };
        }
        // A new scan when the catalog, library, mapping or labels change. The
        // old numbers go at once; the scan starts when typing has paused.
        let key = self.scan_key();
        if key != self.scan.key {
            let first = self.scan.key.is_empty();
            self.scan = Scan {
                key,
                due: Some(if first { now } else { now + 0.5 }),
                ..Scan::default()
            };
        }
        if let Some(due) = self.scan.due
            && now >= due
            && !self.is_running()
        {
            self.scan.due = None;
            if !self.settings.catalog.is_empty()
                && let Ok(job) = self.settings.job(false)
            {
                self.scan.pending = true;
                host.scan(job);
            }
        }
        if self.is_running() || self.scan.pending || self.scan.due.is_some() || self.library_busy {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs(1));
        }
        // Dropping a catalog on the window chooses it.
        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !self.is_running()
            && let Some(path) = dropped.into_iter().find(|p| {
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("lrcat"))
            })
        {
            self.set_catalog(path.display().to_string());
        }
    }

    pub fn set_catalog(&mut self, path: String) {
        self.settings.catalog = path;
        self.files_checked_at = None;
        self.changed = true;
    }

    fn scan_key(&self) -> String {
        format!(
            "{}\n{}\n{:?}\n{:?}",
            self.settings.catalog,
            self.settings.library,
            self.settings.rows(),
            self.settings.labels
        )
    }

    fn on_event(&mut self, event: Event) {
        let Some(run) = &mut self.run else {
            return;
        };
        run.last_event = Local::now();
        match event {
            Event::Phase(Phase::CheckingLibrary, total) => {
                run.checking_library = true;
                run.phase_total = total;
                run.phase_done = 0;
            }
            Event::Question(question) => {
                run.checking_library = false;
                run.library_checked = true;
                run.question = Some(crate::existing::QuestionState::new(*question));
            }
            Event::Phase(Phase::ReadingLibrary, total) => {
                run.library_checked |= run.checking_library;
                run.checking_library = false;
                run.reading_library = true;
                run.phase_total = total;
                run.phase_done = 0;
            }
            Event::Phase(Phase::Writing, total) => {
                run.library_checked |= run.checking_library;
                run.checking_library = false;
                run.library_read = run.reading_library;
                run.reading_library = false;
                run.writing = true;
                run.phase_total = total;
                run.phase_done = 0;
            }
            Event::Phase(phase, total) => {
                run.reading = Some(phase);
                run.phase_total = total;
                run.phase_done = 0;
            }
            Event::Advance(done) => {
                run.phase_done = done;
                if run.writing {
                    run.written = done;
                }
            }
            Event::Planned(totals) => {
                run.reading = None;
                run.stage_totals = Some(totals);
            }
            Event::Entry(entry) => {
                let row = Row::from_entry(&entry);
                *run.counts.entry(row.status).or_insert(0) += 1;
                if let Some(photo) = row.photo {
                    run.photos_seen.insert(photo);
                }
                let stage = match row.stage.as_str() {
                    "keywords" => Stage::Keywords,
                    "organize" => Stage::Organize,
                    "read" => Stage::Read,
                    _ => Stage::Photos,
                };
                *run.stage_entries
                    .entry(stage)
                    .or_default()
                    .entry(row.status)
                    .or_insert(0) += 1;
                run.tail.push_back((Local::now(), row));
                while run.tail.len() > 8 {
                    run.tail.pop_front();
                }
            }
            Event::RollingBack => run.rolling_back = true,
            Event::Done(result) => self.finish(*result),
        }
    }

    fn finish(&mut self, result: RunResult) {
        let Some(run) = &mut self.run else {
            return;
        };
        let RunResult {
            report_text,
            anonymized_text,
            data,
            error,
            ..
        } = result;
        if !data.rows.is_empty() {
            run.counts = Status::ALL.iter().map(|s| (*s, data.count(*s))).collect();
        }
        // Only a run that reached its end has done every step.
        if error.is_none() && run.stage_totals.is_some() {
            run.written = run.stage_totals.iter().flat_map(|t| t.values()).sum();
        }
        run.photo_line = data.photo_counts.map(|c| c.line(data.dry_run));
        let cancelled = error
            .as_ref()
            .is_some_and(|(k, _)| *k == ErrorKind::Cancelled);
        let started = run.started;
        let id = new_id(started, &self.log.records);
        let record = Record {
            id: id.clone(),
            kind: run.kind,
            started: started.timestamp(),
            finished: Local::now().timestamp(),
            catalog: run.catalog.clone(),
            library: run.library.clone(),
            cancelled,
            error: error.as_ref().map(|(_, m)| m.clone()),
            has_report: !report_text.is_empty(),
        };
        let kind = run.kind;
        run.error = error;
        run.finished = true;
        self.log.records.insert(0, record.clone());
        self.log.records.truncate(crate::history::KEEP);
        if let Some(history) = &self.history {
            // Large reports are written off the UI thread.
            let history = history.clone();
            let (named, anonymized) = (report_text.clone(), anonymized_text.clone());
            let failed = self.history_failed.clone();
            std::thread::spawn(move || {
                if history.add(&record, &named, &anonymized).is_err() {
                    failed.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            });
        }
        self.show_report(&id, data, report_text, anonymized_text);
        // A transfer changes what the next dry run plans.
        if kind == Kind::Transfer {
            self.scan.key.clear();
        }
        if kind == Kind::DryRun {
            self.run = None;
            self.tab = Tab::Log;
        }
    }

    /// Leaves the finished run's screen.
    pub fn close_run(&mut self) {
        if !self.is_running() {
            self.run = None;
        }
    }
}

/// A 2 px focus ring in the accent color, 2 px outside.
pub fn focus_ring(ui: &Ui, response: &egui::Response, t: &Tokens) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            CornerRadius::same(6),
            Stroke::new(2.0, t.accent),
            egui::StrokeKind::Outside,
        );
    }
}

/// `1 error`, `2 errors`.
pub fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}
