//! The Transfer screen: source and target with their checks, path mapping,
//! what to transfer, the summary and the two start buttons.

use crate::app::{App, focus_ring};
use crate::checks::version_name;
use crate::host::Host;
use crate::icons::{self, Icon};
use crate::settings::{Labels, MappingRow, Selection};
use crate::theme::{MEDIUM, SEMIBOLD, Tokens, font};
use crate::widgets::{caps, card, check_row, divider, path_field, primary, secondary};
use egui::{Align, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};
use lr2lc_core::ErrorKind;
use serde_json::Value;

/// One reason the start buttons are blocked.
pub struct Block {
    pub text: String,
    /// Blocks only the transfer, not the dry run.
    pub transfer_only: bool,
}

/// Lays out columns with relative `widths`, or stacks them when the area is
/// narrower than `wrap_below`.
pub fn columns(ui: &mut Ui, widths: &[f32], wrap_below: f32, mut add: impl FnMut(&mut Ui, usize)) {
    let gap = 16.0;
    let total = ui.available_width();
    if total >= wrap_below {
        let sum: f32 = widths.iter().sum();
        let free = total - gap * (widths.len().saturating_sub(1)) as f32;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (i, w) in widths.iter().enumerate() {
                let width = free * w / sum;
                ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing.y = gap;
                    add(ui, i);
                });
            }
        });
    } else {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = gap;
            for i in 0..widths.len() {
                add(ui, i);
            }
        });
    }
}

pub fn ui(app: &mut App, ui: &mut Ui, host: &mut dyn Host, t: &Tokens) {
    let blocks = blocks(app);
    columns(ui, &[1.0, 2.0, 1.0], 1100.0, |ui, column| match column {
        0 => {
            source(app, ui, host, t);
            target(app, ui, host, t);
            mapping(app, ui, t);
        }
        1 => what(app, ui, host, t),
        _ => {
            summary(app, ui, t);
            actions(app, ui, host, t, &blocks);
        }
    });
}

/// Everything that keeps the start buttons disabled.
pub fn blocks(app: &App) -> Vec<Block> {
    let s = &app.settings;
    let mut out = Vec::new();
    let mut block = |text: &str, transfer_only| {
        out.push(Block {
            text: text.to_owned(),
            transfer_only,
        })
    };
    if s.catalog.is_empty() {
        block("Choose a Lightroom catalog.", false);
    } else if !app.files.exists {
        block("The catalog file was not found.", false);
    }
    if app.files.open_in_lightroom {
        block("Close the catalog in Lightroom first.", false);
    }
    if let Some((ErrorKind::Catalog, _)) = &app.scan.error {
        block("The catalog cannot be read.", false);
    }
    if s.library.is_empty() {
        block("Choose the LightCraft library.", true);
    }
    match &app.app.answer {
        Some(Ok(probe)) => {
            if probe.xmp_auto_write == Some(true) && !s.allow_xmp {
                block("LightCraft writes XMP automatically.", true);
            }
            if !s.library.is_empty() && !probe.has_open(std::path::Path::new(&s.library)) {
                block("LightCraft has another library open.", true);
            }
        }
        _ => block("LightCraft is not reachable.", true),
    }
    if let Err(invalid) = s.path_mappings() {
        block(&invalid.text(), false);
    }
    out
}

fn source(app: &mut App, ui: &mut Ui, host: &mut dyn Host, t: &Tokens) {
    let running = app.is_running();
    card(ui, t, Some("Source · Lightroom catalog"), |ui| {
        file_row(
            ui,
            t,
            &app.settings.catalog.clone(),
            "No catalog chosen",
            running,
            |ui| {
                if secondary(ui, t, "Choose…", !running, 0.0, 40.0).clicked()
                    && let Some(path) = host.pick_catalog()
                {
                    app.set_catalog(path.display().to_string());
                }
            },
        );
        if app.settings.catalog.is_empty() {
            return;
        }
        let ok = t.ok;
        let warn = t.conflict;
        let (icon, color, value) = if app.files.exists {
            (Icon::Check, ok, "found")
        } else {
            (Icon::Cross, t.error, "not found")
        };
        check_row(ui, t, icon, color, "Catalog file (.lrcat)", value);
        let (icon, color, value) = if app.files.companion {
            (Icon::Check, ok, "found")
        } else {
            (Icon::Dash, t.skipped, "not present")
        };
        check_row(ui, t, icon, color, "Companion file (.lrcat-data)", value);
        let scan = &app.scan;
        let (icon, color, value) = match (&scan.report, &scan.error) {
            (_, Some((ErrorKind::Catalog, message))) if message.contains("version") => {
                (Icon::Cross, t.error, "not supported".to_owned())
            }
            (_, Some((ErrorKind::Catalog, _))) => {
                (Icon::Cross, t.error, "cannot be read".to_owned())
            }
            (Some(report), _) => (
                Icon::Check,
                ok,
                version_name(report["catalog_version"].as_str().unwrap_or("")),
            ),
            _ => (Icon::Dot, t.skipped, "…".to_owned()),
        };
        check_row(ui, t, icon, color, "Catalog version", &value);
        if app.files.open_in_lightroom {
            check_row(
                ui,
                t,
                Icon::Exclamation,
                warn,
                "Catalog open in Lightroom",
                "yes, close Lightroom first",
            );
        } else {
            check_row(ui, t, Icon::Check, ok, "Catalog open in Lightroom", "no");
        }
        match &app.app.answer {
            Some(Ok(probe)) => match probe.xmp_auto_write {
                Some(true) if app.settings.allow_xmp => check_row(
                    ui,
                    t,
                    Icon::Exclamation,
                    warn,
                    "Write XMP automatically",
                    "on, continuing as chosen",
                ),
                Some(true) => check_row(
                    ui,
                    t,
                    Icon::Exclamation,
                    warn,
                    "Write XMP automatically",
                    "on, lr2lc will stop",
                ),
                _ => check_row(ui, t, Icon::Check, ok, "Write XMP automatically", "off"),
            },
            _ => check_row(
                ui,
                t,
                Icon::Dash,
                t.skipped,
                "Write XMP automatically",
                "unknown",
            ),
        }
        if let Some(report) = &scan.report {
            let missing = n(report, &["photos", "by_code", "missing"]);
            if missing > 0 {
                let total = n(report, &["photos", "read"]);
                check_row(
                    ui,
                    t,
                    Icon::Exclamation,
                    warn,
                    "Photos without original",
                    &format!("{missing} of {total}, will be skipped"),
                );
            }
        }
    });
}

fn file_row(
    ui: &mut Ui,
    t: &Tokens,
    path: &str,
    placeholder: &str,
    _running: bool,
    button: impl FnOnce(&mut Ui),
) {
    Frame::new().inner_margin(Margin::same(16)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 100.0).max(120.0);
            path_field(ui, t, path, placeholder, width);
            button(ui);
        });
    });
}

fn target(app: &mut App, ui: &mut Ui, host: &mut dyn Host, t: &Tokens) {
    let running = app.is_running();
    card(ui, t, Some("Target · LightCraft library"), |ui| {
        file_row(
            ui,
            t,
            &app.settings.library.clone(),
            "No library chosen",
            running,
            |ui| {
                if secondary(ui, t, "Choose…", !running, 0.0, 40.0).clicked()
                    && let Some(path) = host.pick_library()
                {
                    app.settings.library = path.display().to_string();
                    app.changed = true;
                }
            },
        );
        Frame::new()
            .inner_margin(Margin {
                left: 16,
                right: 16,
                top: 0,
                bottom: 16,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (icon, color, text) = match &app.app.answer {
                        Some(Ok(probe))
                            if !app.settings.library.is_empty()
                                && !probe.has_open(std::path::Path::new(&app.settings.library)) =>
                        {
                            (
                                Icon::Exclamation,
                                t.conflict,
                                "LightCraft is running with another library open".to_owned(),
                            )
                        }
                        Some(Ok(_)) => (
                            Icon::Check,
                            t.ok,
                            "LightCraft is running, control port connected".to_owned(),
                        ),
                        Some(Err(_)) => (
                            Icon::Cross,
                            t.error,
                            format!("LightCraft not reachable on port {}", app.settings.port),
                        ),
                        None => (Icon::Dot, t.skipped, "Checking LightCraft…".to_owned()),
                    };
                    icons::show(ui, icon, color, 16.0);
                    ui.add(egui::Label::new(RichText::new(text).color(t.text_muted)).wrap());
                });
            });
    });
}

fn mapping(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let running = app.is_running();
    card(ui, t, Some("Path mapping"), |ui| {
        Frame::new().inner_margin(Margin::same(16)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(
                RichText::new("Only needed if the photos are on another drive or computer.")
                    .color(t.text_muted),
            );
            let mut remove = None;
            let rows = app.settings.rows_mut();
            for (index, row) in rows.iter_mut().enumerate() {
                ui.add_space(4.0);
                ui.horizontal_top(|ui| {
                    let width = (ui.available_width() - 48.0).max(120.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        app.changed |= ui
                            .add_enabled(
                                !running,
                                egui::TextEdit::singleline(&mut row.old)
                                    .hint_text("Path in Lightroom")
                                    .desired_width(width)
                                    .min_size(vec2(width, 40.0))
                                    .align(egui::Align2::LEFT_CENTER),
                            )
                            .changed();
                        app.changed |= ui
                            .add_enabled(
                                !running,
                                egui::TextEdit::singleline(&mut row.new)
                                    .hint_text("Path on this computer")
                                    .desired_width(width)
                                    .min_size(vec2(width, 40.0))
                                    .align(egui::Align2::LEFT_CENTER),
                            )
                            .changed();
                    });
                    if icon_button(ui, t, Icon::Cross, "Remove mapping", !running).clicked() {
                        remove = Some(index);
                    }
                });
                if !row.new.trim().is_empty() && !std::path::Path::new(row.new.trim()).exists() {
                    ui.label(
                        RichText::new("This path does not exist on this computer.")
                            .font(font(12.0, None))
                            .color(t.text_muted),
                    );
                }
            }
            if let Some(index) = remove {
                rows.remove(index);
                app.changed = true;
            }
            ui.add_space(4.0);
            if secondary(ui, t, "Add mapping", !running, 0.0, 40.0).clicked() {
                app.settings.rows_mut().push(MappingRow::default());
                app.changed = true;
            }
        });
    });
}

/// A 40 px square icon button with an accessible name.
pub fn icon_button(
    ui: &mut Ui,
    t: &Tokens,
    icon: Icon,
    name: &str,
    enabled: bool,
) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(40.0, 40.0), sense);
    ui.painter().rect(
        rect,
        CornerRadius::same(4),
        t.bg_raised,
        Stroke::new(1.0, t.border_control),
        egui::StrokeKind::Inside,
    );
    icons::paint(
        ui.painter(),
        egui::Rect::from_center_size(rect.center(), vec2(16.0, 16.0)),
        icon,
        if enabled { t.text } else { t.text_muted },
        0.0,
    );
    focus_ring(ui, &response, t);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    response.on_hover_text(name)
}

/// A full-width checkbox row: box, label and an optional hint; the whole
/// row toggles it.
fn option_row(
    ui: &mut Ui,
    t: &Tokens,
    value: &mut bool,
    label: &str,
    hint: Option<&str>,
    enabled: bool,
    indent: bool,
) -> bool {
    divider(ui, t);
    let left = if indent { 46.0 } else { 16.0 };
    let width = ui.available_width();
    let text_width = width - left - 18.0 - 10.0 - 16.0;
    let label_galley = ui.painter().layout(
        label.to_owned(),
        font(13.0, Some(MEDIUM)),
        if enabled { t.text } else { t.text_muted },
        text_width,
    );
    let hint_galley = hint.map(|h| {
        ui.painter()
            .layout(h.to_owned(), font(12.0, None), t.text_muted, text_width)
    });
    let height =
        (20.0 + label_galley.size().y + hint_galley.as_ref().map_or(0.0, |g| g.size().y + 2.0))
            .max(44.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), sense);
    let painter = ui.painter();
    if response.hovered() && enabled {
        painter.rect_filled(rect, 0.0, t.bg_raised);
    }
    let box_rect = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + left, rect.min.y + 12.0),
        vec2(18.0, 18.0),
    );
    let checked = *value;
    painter.rect(
        box_rect,
        CornerRadius::same(4),
        if checked && enabled {
            t.accent
        } else {
            t.bg_field
        },
        Stroke::new(1.0, t.border_control),
        egui::StrokeKind::Inside,
    );
    if checked {
        icons::paint(
            painter,
            box_rect.shrink(1.0),
            Icon::Check,
            if enabled { t.on_accent } else { t.text_muted },
            0.0,
        );
    }
    let text_x = box_rect.max.x + 10.0;
    let label_height = label_galley.size().y;
    painter.galley(egui::pos2(text_x, rect.min.y + 10.0), label_galley, t.text);
    if let Some(g) = hint_galley {
        painter.galley(
            egui::pos2(text_x, rect.min.y + 12.0 + label_height),
            g,
            t.text_muted,
        );
    }
    focus_ring(ui, &response, t);
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, checked, label));
    if response.clicked() && enabled {
        *value = !*value;
        return true;
    }
    false
}

fn group(ui: &mut Ui, t: &Tokens, title: &str) {
    divider(ui, t);
    Frame::new()
        .fill(t.bg_group)
        .inner_margin(Margin {
            left: 16,
            right: 16,
            top: 12,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).font(font(12.0, Some(SEMIBOLD))));
        });
}

fn what(app: &mut App, ui: &mut Ui, host: &mut dyn Host, t: &Tokens) {
    let running = app.is_running();
    let enabled = !running;
    card(ui, t, None, |ui| {
        Frame::new()
            .inner_margin(Margin::symmetric(16, 8))
            .show(ui, |ui| {
                ui.set_min_height(32.0);
                ui.horizontal(|ui| {
                    ui.label(caps("What to transfer", t.text_muted));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if secondary(ui, t, "Folders and photos only", enabled, 0.0, 32.0).clicked()
                        {
                            app.settings.selection = Selection::NOTHING;
                            app.changed = true;
                        }
                        if secondary(ui, t, "Everything", enabled, 0.0, 32.0).clicked() {
                            // Never turns on the two deliberate options.
                            app.settings.selection = Selection::EVERYTHING;
                            app.changed = true;
                        }
                    });
                });
            });
        let s = &mut app.settings.selection;
        let mut changed = false;
        group(ui, t, "Always included");
        let mut always = true;
        option_row(
            ui,
            t,
            &mut always,
            "Folders and photos",
            Some("Photos are created as references. The original files stay where they are."),
            false,
            false,
        );
        group(ui, t, "Metadata");
        changed |= option_row(
            ui,
            t,
            &mut s.keywords,
            "Keywords",
            Some(
                "With hierarchy, synonyms, export attributes and people. A keyword whose name contains “|” is skipped and listed in the log.",
            ),
            enabled,
            false,
        );
        changed |= option_row(
            ui,
            t,
            &mut s.ratings,
            "Star ratings (0–5)",
            None,
            enabled,
            false,
        );
        changed |= option_row(ui, t, &mut s.labels, "Color labels", None, enabled, false);
        changed |= option_row(ui, t, &mut s.flags, "Flags", None, enabled, false);
        changed |= option_row(
            ui,
            t,
            &mut s.texts,
            "Caption and copyright",
            Some("Title, caption, copyright and creator, from the IPTC table."),
            enabled,
            false,
        );
        group(ui, t, "Organization");
        changed |= option_row(
            ui,
            t,
            &mut s.collections,
            "Collections and collection sets",
            Some(
                "Smart collections become smart albums. So far only rules based on star ratings are supported.",
            ),
            enabled,
            false,
        );
        changed |= option_row(
            ui,
            t,
            &mut s.stacks,
            "Stacks and quick collection",
            None,
            enabled,
            false,
        );
        changed |= option_row(
            ui,
            t,
            &mut s.virtual_copies,
            "Virtual copies",
            None,
            enabled,
            false,
        );
        group(ui, t, "Develop");
        changed |= option_row(
            ui,
            t,
            &mut s.develop,
            "Develop settings",
            Some("Sliders, curves, crop, masks and spots."),
            enabled,
            false,
        );
        let develop = s.develop;
        if !develop {
            app.settings.overwrite_develop = false;
        }
        changed |= option_row(
            ui,
            t,
            &mut app.settings.overwrite_develop,
            "Overwrite photos changed in Lightroom",
            Some(
                "Otherwise the state in LightCraft is kept and the difference is listed as a conflict in the log.",
            ),
            enabled && develop,
            true,
        );
        app.changed |= changed;
        advanced(app, ui, host, t, enabled);
    });
}

fn advanced(app: &mut App, ui: &mut Ui, host: &mut dyn Host, t: &Tokens, enabled: bool) {
    divider(ui, t);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, t.bg_group);
    painter.text(
        rect.left_center() + vec2(16.0, 0.0),
        egui::Align2::LEFT_CENTER,
        "Advanced",
        font(12.0, Some(SEMIBOLD)),
        t.text,
    );
    let chevron =
        egui::Rect::from_center_size(rect.left_center() + vec2(92.0, 0.0), vec2(16.0, 16.0));
    icons::paint(painter, chevron, Icon::Chevron, t.text_muted, 0.0);
    focus_ring(ui, &response, t);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::Role::DisclosureTriangle,
            true,
            app.advanced_open,
            "Advanced",
        )
    });
    if response.clicked() {
        app.advanced_open = !app.advanced_open;
    }
    if !app.advanced_open {
        return;
    }
    Frame::new().inner_margin(Margin::same(16)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 12.0;
        ui.add_enabled_ui(enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Color label names").color(t.text_muted));
                let current = match &app.settings.labels {
                    Labels::Auto => "Automatic (like Lightroom)".to_owned(),
                    Labels::English => "English".to_owned(),
                    Labels::German => "German".to_owned(),
                    Labels::File(p) => {
                        format!("From file: {}", crate::widgets::middle_ellipsis(p, 28))
                    }
                };
                let mut pick_file = false;
                egui::ComboBox::from_id_salt("labels")
                    .selected_text(current)
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for (choice, name) in [
                            (Labels::Auto, "Automatic (like Lightroom)"),
                            (Labels::English, "English"),
                            (Labels::German, "German"),
                        ] {
                            if ui
                                .selectable_label(app.settings.labels == choice, name)
                                .clicked()
                            {
                                app.settings.labels = choice;
                                app.changed = true;
                            }
                        }
                        if ui
                            .selectable_label(
                                matches!(app.settings.labels, Labels::File(_)),
                                "From file…",
                            )
                            .clicked()
                        {
                            pick_file = true;
                        }
                    });
                if pick_file && let Some(path) = host.pick_label_file() {
                    app.settings.labels = Labels::File(path.display().to_string());
                    app.changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("LightCraft control port").color(t.text_muted));
                let mut text = app.settings.port.to_string();
                let response = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .desired_width(120.0)
                        .min_size(vec2(120.0, 40.0))
                        .align(egui::Align2::LEFT_CENTER),
                );
                if response.changed() {
                    match text.trim().parse::<u16>() {
                        Ok(port) if port > 0 => {
                            app.settings.port = port;
                            app.changed = true;
                        }
                        _ => {}
                    }
                }
                if text.trim().parse::<u16>().map_or(true, |p| p == 0) {
                    ui.label(RichText::new("1 to 65535").color(t.error));
                }
            });
        });
        let mut allow = app.settings.allow_xmp;
        if option_row(
            ui,
            t,
            &mut allow,
            "Continue despite automatic XMP writing",
            Some("Only enable this deliberately."),
            enabled,
            false,
        ) {
            app.settings.allow_xmp = allow;
            app.changed = true;
        }
    });
}

fn n(v: &Value, path: &[&str]) -> u64 {
    path.iter()
        .try_fold(v, |v, k| v.get(k))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

fn sum(v: &Value, path: &[&str], keys: &[&str]) -> u64 {
    keys.iter()
        .map(|k| {
            let mut full = path.to_vec();
            full.push(k);
            n(v, &full)
        })
        .sum()
}

/// The summary value of each area: counts from the scan.
fn summary(app: &App, ui: &mut Ui, t: &Tokens) {
    let s = &app.settings.selection;
    let report = app.scan.report.as_ref();
    let count = |f: &dyn Fn(&Value) -> String| match report {
        Some(r) => f(r),
        None if app.scan.pending => "…".to_owned(),
        None => "–".to_owned(),
    };
    let photos_n = |k: u64| crate::app::plural(k as usize, "photo");
    let rows: Vec<(&str, bool, String)> = vec![
        (
            "Photos",
            true,
            count(&|r| {
                format!(
                    "{} in {}",
                    n(r, &["photos", "converted"]),
                    crate::app::plural(n(r, &["folders", "converted"]) as usize, "folder")
                )
            }),
        ),
        (
            "Keywords",
            s.keywords,
            count(&|r| n(r, &["keywords", "converted"]).to_string()),
        ),
        (
            "Star ratings",
            s.ratings,
            count(&|r| photos_n(sum(r, &["ratings"], &["1", "2", "3", "4", "5"]))),
        ),
        (
            "Color labels",
            s.labels,
            count(&|r| {
                photos_n(sum(
                    r,
                    &["photos", "labels"],
                    &["red", "yellow", "green", "blue", "purple"],
                ))
            }),
        ),
        (
            "Flags",
            s.flags,
            count(&|r| photos_n(sum(r, &["photos", "flags"], &["pick", "reject"]))),
        ),
        (
            "Caption and copyright",
            s.texts,
            count(&|r| {
                format!(
                    "{} captions, {} copyright",
                    n(r, &["photos", "captions"]),
                    n(r, &["photos", "copyrights"])
                )
            }),
        ),
        (
            "Collections",
            s.collections,
            count(&|r| {
                format!(
                    "{} albums, {} smart",
                    n(r, &["collections", "albums"]),
                    n(r, &["collections", "smart"])
                )
            }),
        ),
        (
            "Stacks",
            s.stacks,
            count(&|r| n(r, &["stacks", "converted"]).to_string()),
        ),
        (
            "Virtual copies",
            s.virtual_copies,
            count(&|r| {
                r["entries"]
                    .as_array()
                    .map_or(0, |a| {
                        a.iter()
                            .filter(|e| {
                                e["area"] == "virtual-copies" && e["status"] == "transferred"
                            })
                            .count()
                    })
                    .to_string()
            }),
        ),
        (
            "Develop settings",
            s.develop,
            count(&|r| photos_n(n(r, &["develop", "photos"]))),
        ),
    ];
    card(ui, t, Some("Summary"), |ui| {
        for (i, (label, on, value)) in rows.into_iter().enumerate() {
            if i > 0 {
                divider(ui, t);
            }
            if on {
                crate::widgets::kv_row(ui, t, None, label, &value, Some(MEDIUM), t.text, 36.0);
            } else {
                crate::widgets::kv_row(
                    ui,
                    t,
                    None,
                    label,
                    "not transferred",
                    None,
                    t.text_muted,
                    36.0,
                );
            }
        }
    });
}

fn actions(app: &mut App, ui: &mut Ui, host: &mut dyn Host, t: &Tokens, blocks: &[Block]) {
    let running = app.is_running();
    let dry_blocked = blocks.iter().any(|b| !b.transfer_only);
    let transfer_blocked = !blocks.is_empty();
    card(ui, t, Some("Actions"), |ui| {
        Frame::new().inner_margin(Margin::same(16)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 12.0;
            let width = ui.available_width();
            if primary(ui, t, "Transfer now", !running && !transfer_blocked, width).clicked() {
                app.start(host, true);
            }
            if secondary(ui, t, "Dry run (writes nothing)", !running && !dry_blocked, width, 44.0).clicked() {
                app.start(host, false);
            }
            for b in blocks {
                ui.horizontal(|ui| {
                    icons::show(ui, Icon::Exclamation, t.conflict, 14.0);
                    let text = if b.transfer_only {
                        format!("{} (blocks the transfer)", b.text)
                    } else {
                        b.text.clone()
                    };
                    ui.label(RichText::new(text).font(font(12.0, None)).color(t.text_muted));
                });
            }
            if let Some(problem) = &app.problem {
                ui.label(RichText::new(problem).color(t.error));
            }
            ui.label(
                RichText::new(
                    "The Lightroom catalog is only read, and Lightroom must be closed. On a repeat run, changes made in LightCraft are kept. Conflicts are listed in the log.",
                )
                .font(font(12.0, None))
                .color(t.text_muted),
            );
        });
    });
}
