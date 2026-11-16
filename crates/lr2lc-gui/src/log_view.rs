//! The Log screen: choose a run, filter its entries by status, area and
//! text, read one entry's details, save the report.

use crate::app::{App, focus_ring};
use crate::host::Host;
use crate::icons::{self, Icon};
use crate::logdata::{Filter, Row, area_label};
use crate::theme::{MEDIUM, SEMIBOLD, Tokens, font};
use crate::transfer::columns;
use crate::widgets::{caps, card, chip, divider, secondary, status_color, status_label, text_link};
use egui::{Align, CornerRadius, Frame, Key, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};
use model::Status;

/// Where the per-photo actions are tracked.
const SINGLE_PHOTO_ISSUE: &str = "Not available yet: lr2lc cannot run a single photo (issue #36).";

pub fn ui(app: &mut App, ui: &mut Ui, host: &mut dyn Host, t: &Tokens) {
    if app.log.records.is_empty() {
        card(ui, t, Some("Log"), |ui| {
            Frame::new().inner_margin(Margin::same(24)).show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("No runs yet. Start a dry run on the Transfer tab.")
                            .color(t.text_muted),
                    );
                });
            });
        });
        return;
    }
    keyboard(app, ui);
    columns(ui, &[2.0, 1.0], 1000.0, |ui, column| match column {
        0 => {
            filters(app, ui, host, t);
            entries(app, ui, t);
        }
        _ => details(app, ui, t),
    });
}

fn filters(app: &mut App, ui: &mut Ui, host: &mut dyn Host, t: &Tokens) {
    card(ui, t, None, |ui| {
        Frame::new().inner_margin(Margin::same(16)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 14.0;
            // Run select, meta, save.
            let shown = app.log.shown.clone().unwrap_or_default();
            let record = app.log.records.iter().find(|r| r.id == shown).cloned();
            let mut choose = None;
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Run").color(t.text_muted));
                egui::ComboBox::from_id_salt("run")
                    .selected_text(record.as_ref().map_or_else(String::new, |r| r.title()))
                    .width(240.0)
                    .height(400.0)
                    .show_ui(ui, |ui| {
                        for r in &app.log.records {
                            if ui.selectable_label(r.id == shown, r.title()).clicked() {
                                choose = Some(r.id.clone());
                            }
                        }
                    });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let can_save = !app.log.report_text.is_empty();
                    if secondary(ui, t, "Save report.json", can_save, 0.0, 40.0).clicked()
                        && let Some(path) = host.pick_report()
                    {
                        let text = if app.log.anonymize_save {
                            &app.log.anonymized_text
                        } else {
                            &app.log.report_text
                        };
                        app.log.saved = Some(save(text, &path));
                    }
                    ui.checkbox(&mut app.log.anonymize_save, "Anonymized");
                });
            });
            if let Some(r) = &record {
                let catalog = std::path::Path::new(&r.catalog)
                    .file_name()
                    .map_or_else(|| r.catalog.clone(), |n| n.to_string_lossy().into_owned());
                let mut meta = catalog;
                if !r.library.is_empty() {
                    meta.push_str(&format!(
                        " → {}",
                        crate::widgets::middle_ellipsis(&tilde(&r.library), 60)
                    ));
                }
                meta.push_str(&format!(" · Duration {}", r.duration()));
                if r.kind == crate::history::Kind::DryRun {
                    meta.push_str(" · Dry run, nothing was written");
                }
                if r.cancelled {
                    meta.push_str(" · Run cancelled");
                }
                ui.add(
                    egui::Label::new(
                        RichText::new(meta)
                            .font(font(12.0, None))
                            .color(t.text_muted),
                    )
                    .wrap(),
                );
            }
            if let Some(id) = choose {
                app.show_run(&id);
            }
            match &app.log.saved {
                Some(Ok(path)) => {
                    ui.label(
                        RichText::new(format!("Saved to {}", path.display()))
                            .font(font(12.0, None))
                            .color(t.text_muted),
                    );
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(format!("Could not save: {e}")).color(t.error));
                }
                None => {}
            }
            // Segmented status bar (decorative; the chips carry the numbers).
            let data = &app.log.data;
            let order = [
                Status::Error,
                Status::Conflict,
                Status::Skipped,
                Status::Adjusted,
                Status::Transferred,
            ];
            let total: usize = order.iter().map(|s| data.count(*s)).sum();
            if total > 0 {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::hover());
                let present: Vec<Status> = order
                    .iter()
                    .copied()
                    .filter(|s| data.count(*s) > 0)
                    .collect();
                let gaps = 2.0 * present.len().saturating_sub(1) as f32;
                let mut x = rect.min.x;
                for s in present {
                    let w = (rect.width() - gaps) * data.count(s) as f32 / total as f32;
                    let seg = egui::Rect::from_min_size(egui::pos2(x, rect.min.y), vec2(w, 8.0));
                    ui.painter()
                        .rect_filled(seg, CornerRadius::same(4), status_color(t, s));
                    x += w + 2.0;
                }
            }
            // Status chips and search.
            ui.horizontal_wrapped(|ui| {
                let all = data.rows.len();
                if chip(
                    ui,
                    t,
                    app.log.filter.status.is_none(),
                    None,
                    "All",
                    Some(all),
                    36.0,
                )
                .clicked()
                {
                    app.log.filter.status = None;
                }
                for (s, label) in [
                    (Status::Error, "Errors"),
                    (Status::Conflict, "Conflicts"),
                    (Status::Skipped, "Skipped"),
                    (Status::Adjusted, "Adjusted"),
                    (Status::Transferred, "Transferred"),
                ] {
                    let selected = app.log.filter.status == Some(s);
                    if chip(
                        ui,
                        t,
                        selected,
                        Some(status_color(t, s)),
                        label,
                        Some(data.count(s)),
                        36.0,
                    )
                    .clicked()
                    {
                        app.log.filter.status = Some(s);
                    }
                }
                let width = ui.available_width().clamp(160.0, 240.0);
                let search = ui.add(
                    egui::TextEdit::singleline(&mut app.log.filter.search)
                        .hint_text("Search…")
                        .desired_width(width)
                        .min_size(vec2(width, 40.0))
                        .align(egui::Align2::LEFT_CENTER)
                        .id_salt("search"),
                );
                app.log.search_id = Some(search.id);
            });
            // Area chips.
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Area").color(t.text_muted));
                if chip(
                    ui,
                    t,
                    app.log.filter.area.is_none(),
                    None,
                    "All",
                    None,
                    32.0,
                )
                .clicked()
                {
                    app.log.filter.area = None;
                }
                for area in data.areas() {
                    let selected = app.log.filter.area.as_deref() == Some(area);
                    if chip(ui, t, selected, None, area_label(area), None, 32.0).clicked() {
                        app.log.filter.area = Some(area.to_owned());
                    }
                }
            });
        });
    });
}

fn save(text: &str, path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    std::fs::write(path, text).map_err(|e| e.to_string())?;
    Ok(path.to_path_buf())
}

fn tilde(path: &str) -> String {
    match dirs::home_dir() {
        Some(home) => {
            let home = home.display().to_string();
            match path.strip_prefix(&home) {
                Some(rest) => format!("~{rest}"),
                None => path.to_owned(),
            }
        }
        None => path.to_owned(),
    }
}

const COLS: [f32; 2] = [150.0, 150.0];

fn entries(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let visible = app.log.data.filtered(&app.log.filter);
    // Keep the selection only while its entry is visible.
    if let Some(sel) = app.log.selected
        && !visible.contains(&sel)
    {
        app.log.selected = None;
    }
    let total = app.log.data.rows.len();
    card(ui, t, None, |ui| {
        Frame::new()
            .inner_margin(Margin::symmetric(16, 8))
            .show(ui, |ui| {
                ui.set_min_height(32.0);
                ui.horizontal(|ui| {
                    ui.label(caps(
                        &format!("Entries · {} of {total}", visible.len()),
                        t.text_muted,
                    ));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if text_link(ui, t, "Reset filters").clicked() {
                            app.log.filter = Filter::default();
                        }
                    });
                });
            });
        divider(ui, t);
        egui::ScrollArea::horizontal()
            .id_salt("entries-h")
            .show(ui, |ui| {
                let width = ui.available_width().max(640.0);
                ui.set_min_width(width);
                // Header row.
                let (rect, _) = ui.allocate_exact_size(vec2(width, 36.0), Sense::hover());
                ui.painter().rect_filled(rect, 0.0, t.bg_group);
                let x = cell_xs(rect.min.x, width);
                for (i, name) in ["Status", "Area", "Item", "Message"]
                    .into_iter()
                    .enumerate()
                {
                    ui.painter().text(
                        egui::pos2(x[i], rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        name.to_uppercase(),
                        font(11.0, Some(SEMIBOLD)),
                        t.text_muted,
                    );
                }
                if visible.is_empty() {
                    Frame::new().inner_margin(Margin::same(24)).show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            let text = if total == 0 {
                                "Nothing to report."
                            } else {
                                "No entries match these filters."
                            };
                            ui.label(RichText::new(text).color(t.text_muted));
                        });
                    });
                    return;
                }
                let height = (visible.len() as f32 * 48.0).min(440.0);
                egui::ScrollArea::vertical()
                    .id_salt("entries")
                    .max_height(440.0)
                    .min_scrolled_height(height)
                    .auto_shrink([false, true])
                    .show_rows(ui, 48.0, visible.len(), |ui, range| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for &index in &visible[range] {
                            let row = &app.log.data.rows[index];
                            let selected = app.log.selected == Some(index);
                            if entry_row(ui, t, row, selected, width).clicked() {
                                app.log.selected = Some(index);
                                app.log.copied = false;
                            }
                        }
                    });
            });
    });
}

fn cell_xs(left: f32, width: f32) -> [f32; 4] {
    let start = left + 19.0;
    let rest = width - 19.0 - COLS[0] - COLS[1] - 3.0 * 12.0 - 16.0;
    let item = rest * 1.1 / 3.1;
    [
        start,
        start + COLS[0] + 12.0,
        start + COLS[0] + COLS[1] + 24.0,
        start + COLS[0] + COLS[1] + 36.0 + item,
    ]
}

fn entry_row(ui: &mut Ui, t: &Tokens, row: &Row, selected: bool, width: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 48.0), Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 0.0, t.bg_selected);
        painter.rect_filled(
            egui::Rect::from_min_size(rect.min, vec2(3.0, rect.height())),
            0.0,
            t.accent,
        );
    } else if response.hovered() {
        painter.rect_filled(rect, 0.0, t.bg_raised);
    }
    painter.hline(
        rect.x_range(),
        rect.max.y - 0.5,
        Stroke::new(1.0, t.divider),
    );
    let x = cell_xs(rect.min.x, width);
    let color = status_color(t, row.status);
    icons::paint(
        painter,
        egui::Rect::from_min_size(egui::pos2(x[0], rect.center().y - 8.0), vec2(16.0, 16.0)),
        Icon::for_status(row.status),
        color,
        0.0,
    );
    painter.text(
        egui::pos2(x[0] + 22.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        status_label(row.status),
        font(13.0, Some(MEDIUM)),
        color,
    );
    painter.text(
        egui::pos2(x[1], rect.center().y),
        egui::Align2::LEFT_CENTER,
        area_label(&row.area),
        font(13.0, None),
        t.text_muted,
    );
    let clip = |ui: &Ui, from: f32, to: f32, text: &str, weight: Option<&'static str>, color| {
        let galley = ui
            .painter()
            .layout(text.to_owned(), font(13.0, weight), color, f32::INFINITY);
        let mut job =
            egui::text::LayoutJob::simple_singleline(text.to_owned(), font(13.0, weight), color);
        job.wrap = egui::text::TextWrapping::truncate_at_width((to - from - 8.0).max(10.0));
        let truncated = ui.painter().layout_job(job);
        let _ = galley;
        ui.painter().galley(
            egui::pos2(from, rect.center().y - truncated.size().y / 2.0),
            truncated,
            color,
        );
    };
    clip(ui, x[2], x[3] - 12.0, &row.item, Some(MEDIUM), t.text);
    clip(
        ui,
        x[3],
        rect.max.x - 16.0,
        &row.message,
        None,
        t.text_muted,
    );
    focus_ring(ui, &response, t);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::Role::Button,
            true,
            selected,
            format!(
                "{}, {}, {}, {}",
                status_label(row.status),
                area_label(&row.area),
                row.item,
                row.message
            ),
        )
    });
    response
}

fn keyboard(app: &mut App, ui: &mut Ui) {
    // Not while typing in the search field.
    let search = app.log.search_id;
    if ui
        .ctx()
        .memory(|m| m.focused().is_some_and(|id| Some(id) == search))
    {
        return;
    }
    let visible = app.log.data.filtered(&app.log.filter);
    if visible.is_empty() {
        return;
    }
    let pos = app
        .log
        .selected
        .and_then(|s| visible.iter().position(|&i| i == s));
    let next = ui.input(|i| {
        if i.key_pressed(Key::ArrowDown) {
            Some(pos.map_or(0, |p| (p + 1).min(visible.len() - 1)))
        } else if i.key_pressed(Key::ArrowUp) {
            Some(pos.map_or(0, |p| p.saturating_sub(1)))
        } else if i.key_pressed(Key::Home) {
            Some(0)
        } else if i.key_pressed(Key::End) {
            Some(visible.len() - 1)
        } else {
            None
        }
    });
    if let Some(p) = next {
        app.log.selected = Some(visible[p]);
        app.log.copied = false;
    }
}

fn details(app: &mut App, ui: &mut Ui, t: &Tokens) {
    card(ui, t, Some("Details"), |ui| {
        let Some(row) = app
            .log
            .selected
            .and_then(|i| app.log.data.rows.get(i))
            .cloned()
        else {
            Frame::new().inner_margin(Margin::same(24)).show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Select an entry to see details.").color(t.text_muted));
                });
            });
            return;
        };
        Frame::new().inner_margin(Margin::same(16)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 16.0;
            // Status pill: 28 px, field fill, icon and label in the status color.
            {
                let color = status_color(t, row.status);
                let label = status_label(row.status);
                let galley =
                    ui.painter()
                        .layout_no_wrap(label.to_owned(), font(13.0, Some(MEDIUM)), color);
                let (rect, _) =
                    ui.allocate_exact_size(vec2(galley.size().x + 46.0, 28.0), Sense::hover());
                ui.painter().rect(
                    rect,
                    CornerRadius::same(14),
                    t.bg_field,
                    Stroke::new(1.0, t.border_control),
                    egui::StrokeKind::Inside,
                );
                icons::paint(
                    ui.painter(),
                    egui::Rect::from_min_size(rect.min + vec2(12.0, 6.0), vec2(16.0, 16.0)),
                    Icon::for_status(row.status),
                    color,
                    0.0,
                );
                ui.painter().galley(
                    egui::pos2(rect.min.x + 34.0, rect.center().y - galley.size().y / 2.0),
                    galley,
                    color,
                );
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                if !row.item.is_empty() {
                    ui.add(
                        egui::Label::new(RichText::new(&row.item).font(font(16.0, Some(SEMIBOLD))))
                            .wrap(),
                    );
                }
                ui.add(egui::Label::new(&row.message).wrap());
            });
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (term, value) in [
                    ("Area", Some(area_label(&row.area).to_owned())),
                    ("From Lightroom", row.source.clone()),
                    ("In LightCraft", row.result.clone()),
                    ("Reason", row.reason.clone()),
                    ("Recommendation", row.recommendation.clone()),
                ] {
                    let Some(value) = value.filter(|v| !v.is_empty()) else {
                        continue;
                    };
                    divider(ui, t);
                    Frame::new()
                        .inner_margin(Margin::symmetric(0, 8))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(term)
                                    .font(font(12.0, None))
                                    .color(t.text_muted),
                            );
                            ui.add(egui::Label::new(value).wrap());
                        });
                }
            });
            // Technical line: stage, area, code.
            let mut tech = vec![row.stage.clone(), area_label(&row.area).to_owned()];
            if let Some(code) = &row.code {
                tech.push(code.clone());
            }
            if let Some(photo) = row.photo {
                tech.push(format!("photo {photo}"));
            }
            Frame::new()
                .fill(t.bg_field)
                .stroke(Stroke::new(1.0, t.divider))
                .corner_radius(CornerRadius::same(4))
                .inner_margin(Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new(tech.join(" · "))
                            .monospace()
                            .color(t.text_muted),
                    );
                });
            ui.horizontal_wrapped(|ui| {
                if row.status == Status::Error {
                    crate::widgets::primary(ui, t, "Retry this photo", false, 0.0)
                        .on_disabled_hover_text(SINGLE_PHOTO_ISSUE);
                }
                if row.status == Status::Conflict {
                    crate::widgets::primary(ui, t, "Overwrite with Lightroom state", false, 0.0)
                        .on_disabled_hover_text(SINGLE_PHOTO_ISSUE);
                }
                if secondary(
                    ui,
                    t,
                    if app.log.copied {
                        "Copied"
                    } else {
                        "Copy details"
                    },
                    true,
                    0.0,
                    40.0,
                )
                .clicked()
                {
                    ui.ctx().copy_text(row.details());
                    app.log.copied = true;
                }
            });
            if matches!(row.status, Status::Error | Status::Conflict) {
                ui.label(
                    RichText::new(SINGLE_PHOTO_ISSUE)
                        .font(font(12.0, None))
                        .color(t.text_muted),
                );
            }
        });
    });
}
