//! The Running screen: overall progress, the run's phases in the order
//! lr2lc really works, counters so far and the live log tail.

use crate::app::{App, RunState, Tab};
use crate::history::Kind;
use crate::host::Host;
use crate::icons::{self, Icon};
use crate::theme::{MEDIUM, SEMIBOLD, Tokens, font};
use crate::transfer::columns;
use crate::widgets::{
    card, divider, header, primary, secondary, status_color, status_label, text_link,
};
use egui::{Align, CornerRadius, Frame, Layout, Margin, RichText, Sense, Ui, vec2};
use lr2lc_core::Phase;
use model::{Stage, Status};

/// How a phase stands.
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Waiting,
    Running,
    Done,
    Problems,
    NotNeeded,
}

struct PhaseRow {
    name: &'static str,
    state: State,
    note: String,
    fraction: f32,
}

fn phases(run: &RunState) -> Vec<PhaseRow> {
    let finished = run.finished();
    let mut out = Vec::new();
    // 1. Reading the catalog.
    let read_done = run.stage_totals.is_some();
    let failed_reading = finished && !read_done;
    out.push(PhaseRow {
        name: "Read catalog",
        state: match (read_done, failed_reading) {
            (_, true) => State::Problems,
            (true, _) => State::Done,
            _ => State::Running,
        },
        note: match (read_done, run.reading) {
            (true, _) => "read and planned".to_owned(),
            (false, Some(Phase::Checksum)) => "checking the catalog file".to_owned(),
            (false, Some(Phase::CheckingOriginals)) => "looking for the original files".to_owned(),
            (false, _) if failed_reading => "stopped".to_owned(),
            _ => "reading".to_owned(),
        },
        fraction: if read_done { 1.0 } else { 0.0 },
    });
    if run.kind == Kind::DryRun {
        return out;
    }
    // 2. Reading the library (only when a known photo changed).
    let library = if run.reading_library {
        State::Running
    } else if run.library_read && run.phase_total > 0 && !run.writing {
        State::Done
    } else if run.writing || finished {
        if run.library_read {
            State::Done
        } else {
            State::NotNeeded
        }
    } else {
        State::Waiting
    };
    out.push(PhaseRow {
        name: "Read LightCraft library",
        state: library,
        note: match library {
            State::Running => format!("{} of {} photos", run.phase_done, run.phase_total),
            State::NotNeeded => "not needed".to_owned(),
            State::Done => "done".to_owned(),
            _ => "waiting".to_owned(),
        },
        fraction: match library {
            State::Running if run.phase_total > 0 => run.phase_done as f32 / run.phase_total as f32,
            State::Done | State::NotNeeded => 1.0,
            _ => 0.0,
        },
    });
    // 3–5. Writing, by stage, in plan order.
    let totals = run.stage_totals.clone().unwrap_or_default();
    let mut offset = 0;
    for (stage, name) in [
        (Stage::Keywords, "Keywords"),
        (
            Stage::Photos,
            "Photos with their metadata and develop settings",
        ),
        (Stage::Organize, "Albums, stacks and smart albums"),
    ] {
        let total = totals.get(&stage).copied().unwrap_or(0);
        let start = offset;
        offset += total;
        let done = run.written.saturating_sub(start).min(total);
        let entries = run.stage_entries.get(&stage);
        let count = |s: Status| entries.and_then(|e| e.get(&s)).copied().unwrap_or(0);
        let errors = count(Status::Error);
        let problems = errors + count(Status::Conflict);
        // After a stop, phases keep the state they really reached.
        let active = run.writing || (finished && run.error.is_none());
        let state = if total == 0 && active {
            State::NotNeeded
        } else if active && run.written >= start + total {
            if problems > 0 {
                State::Problems
            } else {
                State::Done
            }
        } else if run.writing && run.written >= start {
            State::Running
        } else {
            State::Waiting
        };
        let mut note = match state {
            State::NotNeeded => "nothing to do".to_owned(),
            State::Waiting => "waiting".to_owned(),
            _ => format!("{done} of {total} steps"),
        };
        if errors > 0 {
            note.push_str(&format!(", {}", crate::app::plural(errors, "error")));
        }
        out.push(PhaseRow {
            name,
            state,
            note,
            fraction: if total == 0 {
                1.0
            } else {
                done as f32 / total as f32
            },
        });
    }
    out
}

pub fn ui(app: &mut App, ui: &mut Ui, host: &mut dyn Host, t: &Tokens) {
    let Some(run) = &app.run else {
        return;
    };
    let finished = run.finished();
    let stopped = run.error.is_some();
    let title = match (run.kind, finished) {
        (Kind::Transfer, false) => "Transfer running",
        (Kind::DryRun, false) => "Dry run running",
        (_, true) if run.cancelled() => "Run cancelled",
        (Kind::Transfer, true) if stopped => "Transfer stopped",
        (Kind::DryRun, true) if stopped => "Dry run stopped",
        (Kind::Transfer, true) => "Transfer finished",
        (Kind::DryRun, true) => "Dry run finished",
    };
    let rows = phases(run);
    let current = rows
        .iter()
        .position(|r| r.state == State::Running)
        .unwrap_or(rows.len().saturating_sub(1));
    let total_steps: usize = run.stage_totals.iter().flat_map(|t| t.values()).sum();
    let percent = if finished && !stopped {
        100.0
    } else if total_steps > 0 {
        run.written as f32 / total_steps as f32 * 100.0
    } else {
        0.0
    };
    let mut action = None;
    columns(ui, &[2.0, 1.0], 900.0, |ui, column| match column {
        0 => {
            card(ui, t, None, |ui| {
                Frame::new().inner_margin(Margin::same(16)).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 12.0;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).font(font(16.0, Some(SEMIBOLD))));
                        ui.label(
                            RichText::new(format!(
                                "Phase {} of {} · {}",
                                current + 1,
                                rows.len(),
                                rows.get(current).map_or("", |r| r.name)
                            ))
                            .color(t.text_muted),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(format!("{percent:.0} %")).font(font(13.0, Some(MEDIUM))));
                        });
                    });
                    let fill = if stopped { t.conflict } else { t.ok };
                    bar(ui, t, percent / 100.0, 8.0, fill, "Overall progress");
                    // LightCraft answers nothing: most often a dialog is open there.
                    let quiet = (chrono::Local::now() - run.last_event).num_seconds();
                    if !finished && run.writing && quiet >= 15 {
                        ui.horizontal(|ui| {
                            icons::show(ui, Icon::Exclamation, t.conflict, 16.0);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!(
                                        "LightCraft has not answered for {quiet} s. Is a dialog open in LightCraft? The transfer goes on as soon as it answers."
                                    ))
                                    .color(t.text_muted),
                                )
                                .wrap(),
                            );
                        });
                        if secondary(ui, t, "Show LightCraft", true, 0.0, 40.0).clicked() {
                            action = Some(Action::ShowLightCraft);
                        }
                    }
                    if let Some((_, message)) = &run.error {
                        // Why it stopped, where the person is looking.
                        ui.horizontal(|ui| {
                            icons::show(ui, Icon::Cross, t.error, 16.0);
                            ui.add(
                                egui::Label::new(RichText::new(message).color(t.error)).wrap(),
                            );
                        });
                    }
                    ui.horizontal(|ui| {
                        if finished {
                            if run.kind == Kind::Transfer
                                && primary(ui, t, "View log", true, 120.0).clicked()
                            {
                                action = Some(Action::ViewLog(None));
                            }
                            if secondary(ui, t, "Back to Transfer", true, 0.0, 40.0).clicked() {
                                action = Some(Action::Back);
                            }
                        } else if run.rolling_back {
                            ui.label(RichText::new("Taking the run back…").color(t.text_muted));
                        } else if run.cancelling {
                            ui.label(RichText::new("Stopping at the next photo…").color(t.text_muted));
                        } else {
                            if secondary(ui, t, "Stop", true, 0.0, 40.0).clicked() {
                                action = Some(Action::Stop);
                            }
                            if run.kind == Kind::Transfer
                                && secondary(ui, t, "Cancel…", true, 0.0, 40.0).clicked()
                            {
                                action = Some(Action::AskCancel);
                            }
                        }
                    });
                    if !finished && !run.cancelling {
                        ui.add(
                            egui::Label::new(
                                RichText::new(if run.kind == Kind::Transfer {
                                    "Stop keeps what is transferred so far; a new run continues and imports nothing twice. Cancel takes everything this run did back."
                                } else {
                                    "Stop ends the dry run; nothing is written either way."
                                })
                                .font(font(12.0, None))
                                .color(t.text_muted),
                            )
                            .wrap(),
                        );
                    }
                    if run.confirm_cancel && !run.cancelling {
                        egui::Frame::new()
                            .fill(t.bg_group)
                            .stroke(egui::Stroke::new(1.0, t.border_control))
                            .corner_radius(egui::CornerRadius::same(4))
                            .inner_margin(Margin::same(12))
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        "Cancel the transfer? Everything this run added to the LightCraft library is removed again and every field it changed is set back. Original files are not touched. Development settings written to photos that were already in the library cannot be set back; the log lists them.",
                                    )
                                    .wrap(),
                                );
                                ui.horizontal(|ui| {
                                    if primary(ui, t, "Cancel and take back", true, 0.0).clicked() {
                                        action = Some(Action::Cancel);
                                    }
                                    if secondary(ui, t, "Keep running", true, 0.0, 44.0).clicked() {
                                        action = Some(Action::KeepRunning);
                                    }
                                });
                            });
                    }
                });
            });
            card(ui, t, Some("Phases"), |ui| {
                for (i, row) in rows.iter().enumerate() {
                    if i > 0 {
                        divider(ui, t);
                    }
                    phase_row(ui, t, row);
                }
            });
        }
        _ => {
            counters(ui, t, run, finished, &mut action);
            live_log(ui, t, run, &mut action);
        }
    });
    match action {
        Some(Action::ShowLightCraft) => host.show_lightcraft(),
        Some(Action::Stop) => {
            if let Some(run) = &mut app.run {
                run.cancelling = true;
            }
            host.cancel(false);
        }
        Some(Action::AskCancel) => {
            if let Some(run) = &mut app.run {
                run.confirm_cancel = true;
            }
        }
        Some(Action::KeepRunning) => {
            if let Some(run) = &mut app.run {
                run.confirm_cancel = false;
            }
        }
        Some(Action::Cancel) => {
            if let Some(run) = &mut app.run {
                run.cancelling = true;
                run.confirm_cancel = false;
            }
            host.cancel(true);
        }
        Some(Action::Back) => app.close_run(),
        Some(Action::ViewLog(status)) => {
            app.log.filter = crate::logdata::Filter {
                status,
                ..Default::default()
            };
            app.tab = Tab::Log;
        }
        None => {}
    }
}

enum Action {
    ShowLightCraft,
    Stop,
    AskCancel,
    KeepRunning,
    Cancel,
    Back,
    ViewLog(Option<Status>),
}

/// A progress bar: field track, colored fill, with an accessible value.
fn bar(ui: &mut Ui, t: &Tokens, fraction: f32, height: f32, color: egui::Color32, name: &str) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let radius = CornerRadius::same((height / 2.0).min(4.0) as u8);
    ui.painter().rect_filled(rect, radius, t.bg_field);
    let mut fill = rect;
    fill.set_width(rect.width() * fraction.clamp(0.0, 1.0));
    ui.painter().rect_filled(fill, radius, color);
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::ProgressIndicator, true, name);
        info.value = Some(f64::from(fraction.clamp(0.0, 1.0) * 100.0));
        info
    });
}

fn phase_row(ui: &mut Ui, t: &Tokens, row: &PhaseRow) {
    Frame::new()
        .inner_margin(Margin::symmetric(16, 8))
        .show(ui, |ui| {
            ui.set_min_height(56.0 - 16.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.horizontal(|ui| {
                let (icon, color) = match row.state {
                    State::Done => (Icon::Check, t.ok),
                    State::Problems => (Icon::Exclamation, t.conflict),
                    State::Running => (Icon::Arc, t.ok),
                    State::Waiting => (Icon::Dot, t.skipped),
                    State::NotNeeded => (Icon::Dash, t.skipped),
                };
                icons::show(ui, icon, color, 18.0);
                ui.label(RichText::new(row.name).font(font(13.0, Some(MEDIUM))));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(&row.note).color(t.text_muted));
                });
            });
            let color = match row.state {
                State::Problems => t.conflict,
                State::Waiting | State::NotNeeded => t.skipped,
                _ => t.ok,
            };
            bar(ui, t, row.fraction, 6.0, color, row.name);
        });
}

fn counters(ui: &mut Ui, t: &Tokens, run: &RunState, finished: bool, action: &mut Option<Action>) {
    card(ui, t, Some("Log entries so far"), |ui| {
        let photos = run
            .photo_line
            .clone()
            .unwrap_or_else(|| crate::app::plural(run.photos_seen.len(), "photo") + " so far");
        Frame::new()
            .inner_margin(Margin::symmetric(16, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(photos).font(font(13.0, Some(MEDIUM))));
                ui.label(
                    RichText::new("One entry per photo, field, keyword and album.")
                        .font(font(12.0, None))
                        .color(t.text_muted),
                );
            });
        divider(ui, t);
        let count = |s: Status| run.counts.get(&s).copied().unwrap_or(0);
        let mut cells = vec![
            (Status::Transferred, "Transferred"),
            (Status::Adjusted, "Adjusted"),
            (Status::Skipped, "Skipped"),
            (Status::Error, "Errors"),
        ];
        if count(Status::Conflict) > 0 {
            cells.push((Status::Conflict, "Conflicts"));
        }
        let width = ui.available_width();
        let cell_width = (width - 1.0) / 2.0;
        for pair in cells.chunks(2) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                for (status, label) in pair {
                    let w = if pair.len() == 1 { width } else { cell_width };
                    let sense = if finished {
                        Sense::click()
                    } else {
                        Sense::hover()
                    };
                    let (rect, response) = ui.allocate_exact_size(vec2(w, 72.0), sense);
                    let painter = ui.painter();
                    if response.hovered() && finished {
                        painter.rect_filled(rect, 0.0, t.bg_raised);
                    }
                    let color = status_color(t, *status);
                    icons::paint(
                        painter,
                        egui::Rect::from_min_size(rect.min + vec2(16.0, 14.0), vec2(14.0, 14.0)),
                        Icon::for_status(*status),
                        color,
                        0.0,
                    );
                    painter.text(
                        rect.min + vec2(36.0, 13.0),
                        egui::Align2::LEFT_TOP,
                        label,
                        font(12.0, None),
                        t.text_muted,
                    );
                    painter.text(
                        rect.min + vec2(16.0, 34.0),
                        egui::Align2::LEFT_TOP,
                        count(*status).to_string(),
                        font(22.0, Some(SEMIBOLD)),
                        t.text,
                    );
                    crate::app::focus_ring(ui, &response, t);
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::Role::Link,
                            finished,
                            format!("{label}: {}", count(*status)),
                        )
                    });
                    if response.clicked() {
                        *action = Some(Action::ViewLog(Some(*status)));
                    }
                }
            });
            divider(ui, t);
        }
    });
}

fn live_log(ui: &mut Ui, t: &Tokens, run: &RunState, action: &mut Option<Action>) {
    card(ui, t, None, |ui| {
        header(ui, t, "Live log", |ui| {
            if run.finished() && text_link(ui, t, "View all").clicked() {
                *action = Some(Action::ViewLog(None));
            }
        });
        Frame::new()
            .inner_margin(Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                if run.tail.is_empty() {
                    ui.label(RichText::new("Nothing yet.").color(t.text_muted));
                }
                for (time, row) in &run.tail {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(time.format("%H:%M:%S").to_string())
                                .monospace()
                                .color(t.text_muted),
                        );
                        let tag = match row.status {
                            Status::Transferred => "OK",
                            other => status_label(other),
                        };
                        let (rect, _) = ui.allocate_exact_size(vec2(84.0, 18.0), Sense::hover());
                        ui.painter().text(
                            rect.left_center(),
                            egui::Align2::LEFT_CENTER,
                            tag.to_uppercase(),
                            egui::FontId::monospace(12.0),
                            status_color(t, row.status),
                        );
                        let text = if row.item.is_empty() {
                            row.message.clone()
                        } else {
                            format!("{} · {}", row.item, row.message)
                        };
                        ui.label(RichText::new(text).monospace());
                    });
                }
            });
    });
}
