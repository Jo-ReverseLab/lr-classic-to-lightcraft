//! The Transfer screen: source and target with their checks, path mapping,
//! what to transfer, the summary and the two start buttons.

use crate::app::{App, focus_ring};
use crate::checks::version_name;
use crate::host::Host;
use crate::icons::{self, Icon};
use crate::logdata::ScanSummary;
use crate::settings::{Labels, MappingRow, Selection};
use crate::theme::{MEDIUM, SEMIBOLD, Tokens, font};
use crate::widgets::{caps, card, check_row, divider, path_field, primary, secondary};
use egui::{Align, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};
use lr2lc_core::ErrorKind;
use lr2lc_core::probe::LibraryAction;
use std::path::PathBuf;

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
    if app.library_busy {
        block("LightCraft is switching libraries.", true);
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
            if app.library_mismatch() {
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

/// What each check of the catalog means, shown on its “i”.
fn explain(label: &str) -> &'static str {
    match label {
        "Catalog file (.lrcat)" => {
            "The Lightroom Classic catalog: one file that holds folders, keywords, ratings, collections and development settings. lr2lc only reads it."
        }
        "Companion file (.lrcat-data)" => {
            "A folder newer Lightroom versions keep next to the catalog for AI masks and similar data. lr2lc does not need it; it is fine if it is not there."
        }
        "Catalog version" => {
            "The Lightroom Classic version that last wrote the catalog. lr2lc checks that it knows this format before it reads anything."
        }
        "Catalog open in Lightroom" => {
            "Lightroom keeps unsaved changes outside the catalog file while it is open, so the catalog must be closed to be read reliably. Detected by Lightroom's lock file next to the catalog."
        }
        "Write XMP automatically" => {
            "A LightCraft setting. When it is on, LightCraft writes .xmp files next to your originals for every change, so a transfer would create or change files in your photo folders. lr2lc stops unless you allow it under Advanced."
        }
        "Photos without original" => {
            "Photos whose original file is not found at the path Lightroom recorded (after path mapping). lr2lc creates photos as references to their files, so these are skipped and listed in the log. Connect the drive or add a path mapping."
        }
        _ => "",
    }
}

fn row(ui: &mut Ui, t: &Tokens, icon: Icon, color: egui::Color32, label: &str, value: &str) {
    check_row(ui, t, icon, color, label, value, explain(label));
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
        row(ui, t, icon, color, "Catalog file (.lrcat)", value);
        let (icon, color, value) = if app.files.companion {
            (Icon::Check, ok, "found")
        } else {
            (Icon::Dash, t.skipped, "not present")
        };
        row(ui, t, icon, color, "Companion file (.lrcat-data)", value);
        let scan = &app.scan;
        let (icon, color, value) = match (&scan.summary, &scan.error) {
            (_, Some((ErrorKind::Catalog, message))) if message.contains("version") => {
                (Icon::Cross, t.error, "not supported".to_owned())
            }
            (_, Some((ErrorKind::Catalog, _))) => {
                (Icon::Cross, t.error, "cannot be read".to_owned())
            }
            (Some(report), _) => (Icon::Check, ok, version_name(&report.catalog_version)),
            _ => (Icon::Dot, t.skipped, "…".to_owned()),
        };
        row(ui, t, icon, color, "Catalog version", &value);
        if app.files.open_in_lightroom {
            row(
                ui,
                t,
                Icon::Exclamation,
                warn,
                "Catalog open in Lightroom",
                "yes, close Lightroom first",
            );
        } else {
            row(ui, t, Icon::Check, ok, "Catalog open in Lightroom", "no");
        }
        match &app.app.answer {
            Some(Ok(probe)) => match probe.xmp_auto_write {
                Some(true) if app.settings.allow_xmp => row(
                    ui,
                    t,
                    Icon::Exclamation,
                    warn,
                    "Write XMP automatically",
                    "on, continuing as chosen",
                ),
                Some(true) => row(
                    ui,
                    t,
                    Icon::Exclamation,
                    warn,
                    "Write XMP automatically",
                    "on, lr2lc will stop",
                ),
                _ => row(ui, t, Icon::Check, ok, "Write XMP automatically", "off"),
            },
            _ => row(
                ui,
                t,
                Icon::Dash,
                t.skipped,
                "Write XMP automatically",
                "unknown",
            ),
        }
        if let Some(report) = &scan.summary {
            let missing = report.missing;
            if missing > 0 {
                let total = report.photos_read;
                row(
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
    let idle = !app.is_running() && !app.library_busy;
    let connected = matches!(app.app.answer, Some(Ok(_)));
    card(ui, t, Some("Target · LightCraft library"), |ui| {
        Frame::new().inner_margin(Margin::same(16)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;
            let width = ui.available_width();
            path_field(ui, t, &app.settings.library, "No library chosen", width);
            if app.settings.follow_app {
                ui.label(
                    RichText::new("Follows the library that is open in LightCraft.")
                        .font(font(12.0, None))
                        .color(t.text_muted),
                );
            }
            ui.horizontal_wrapped(|ui| {
                if secondary(ui, t, "Open existing…", idle, 0.0, 40.0).clicked()
                    && let Some(path) = host.pick_library()
                {
                    app.settings.library = path.display().to_string();
                    app.settings.follow_app = false;
                    app.new_library = None;
                    app.changed = true;
                }
                if secondary(ui, t, "Create new…", idle && connected, 0.0, 40.0).clicked()
                    && let Some(folder) = host.pick_new_library_folder()
                {
                    app.new_library = Some((folder, "LightCraft Library".to_owned()));
                }
                if !app.settings.follow_app
                    && secondary(ui, t, "Use library open in LightCraft", idle, 0.0, 40.0).clicked()
                {
                    app.settings.follow_app = true;
                    app.new_library = None;
                    app.changed = true;
                }
            });
            new_library_confirm(app, ui, host, t, idle);
            // LightCraft's state, and a way to switch it to the chosen library.
            let mismatch = app.library_mismatch();
            let (icon, color, text) = match &app.app.answer {
                _ if app.library_busy => (
                    Icon::Arc,
                    t.ok,
                    "LightCraft is switching libraries…".to_owned(),
                ),
                Some(Ok(_)) if mismatch => (
                    Icon::Exclamation,
                    t.conflict,
                    "LightCraft has a different library open than the one chosen here".to_owned(),
                ),
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
            ui.horizontal(|ui| {
                icons::show(ui, icon, color, 16.0);
                ui.add(egui::Label::new(RichText::new(text).color(t.text_muted)).wrap());
            });
            if mismatch
                && secondary(
                    ui,
                    t,
                    "Open the chosen library in LightCraft",
                    idle,
                    0.0,
                    40.0,
                )
                .clicked()
            {
                app.library_busy = true;
                app.library_error = None;
                host.library_action(
                    app.settings.port,
                    LibraryAction::Open(PathBuf::from(&app.settings.library)),
                );
            }
            if let Some(error) = &app.library_error {
                ui.add(egui::Label::new(RichText::new(error).color(t.error)).wrap());
            }
        });
    });
}

/// The confirmation before a new library is created.
fn new_library_confirm(app: &mut App, ui: &mut Ui, host: &mut dyn Host, t: &Tokens, idle: bool) {
    let Some((folder, name)) = &mut app.new_library else {
        return;
    };
    let mut decided = None;
    Frame::new()
        .fill(t.bg_group)
        .stroke(Stroke::new(1.0, t.border_control))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(RichText::new("Create a new library").font(font(13.0, Some(SEMIBOLD))));
            ui.label(RichText::new("Name").font(font(12.0, None)).color(t.text_muted));
            ui.add(
                egui::TextEdit::singleline(name)
                    .desired_width(ui.available_width())
                    .min_size(vec2(120.0, 40.0))
                    .align(egui::Align2::LEFT_CENTER),
            );
            let valid = !name.trim().is_empty() && !name.contains(['/', '\\']);
            let target = folder.join(name.trim());
            let exists = target.exists();
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "A new LightCraft library will be created in {} and LightCraft will switch to it. Nothing is transferred yet.",
                        target.display()
                    ))
                    .color(t.text_muted),
                )
                .wrap(),
            );
            if exists {
                ui.label(RichText::new("This folder already exists. Choose another name.").color(t.error));
            }
            ui.horizontal(|ui| {
                if primary(ui, t, "Create library", idle && valid && !exists, 0.0).clicked() {
                    decided = Some(true);
                }
                if secondary(ui, t, "Cancel", true, 0.0, 44.0).clicked() {
                    decided = Some(false);
                }
            });
        });
    match decided {
        Some(true) => {
            if let Some((folder, name)) = app.new_library.take() {
                app.library_busy = true;
                app.library_error = None;
                // Followed once it exists; a failure keeps the choice made.
                app.follow_after_action = true;
                host.library_action(
                    app.settings.port,
                    LibraryAction::Create {
                        folder,
                        name: name.trim().to_owned(),
                    },
                );
            }
        }
        Some(false) => app.new_library = None,
        None => {}
    }
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
    option_row_with(ui, t, None, value, label, hint, enabled, indent)
}

/// [`option_row`] with the area's count from the catalog at the right.
#[allow(clippy::too_many_arguments)]
fn option_row_with(
    ui: &mut Ui,
    t: &Tokens,
    count: Option<&str>,
    value: &mut bool,
    label: &str,
    hint: Option<&str>,
    enabled: bool,
    indent: bool,
) -> bool {
    divider(ui, t);
    let left = if indent { 46.0 } else { 16.0 };
    let width = ui.available_width();
    let count_galley = count.map(|c| {
        ui.painter()
            .layout_no_wrap(c.to_owned(), font(12.0, None), t.text_muted)
    });
    let count_width = count_galley.as_ref().map_or(0.0, |g| g.size().x + 12.0);
    let text_width = width - left - 18.0 - 10.0 - 16.0 - count_width;
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
    if let Some(g) = count_galley {
        painter.galley(
            egui::pos2(rect.max.x - 16.0 - g.size().x, rect.min.y + 12.0),
            g,
            t.text_muted,
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
        let counts: std::collections::BTreeMap<&str, String> = summary_rows(app)
            .into_iter()
            .map(|(label, _, value)| (label, value))
            .collect();
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
        changed |= option_row_with(
            ui,
            t,
            counts.get("Keywords").map(String::as_str),
            &mut s.keywords,
            "Keywords",
            Some(
                "With hierarchy, synonyms, export attributes and people. A keyword whose name contains “|” is skipped and listed in the log.",
            ),
            enabled,
            false,
        );
        changed |= option_row_with(
            ui,
            t,
            counts.get("Star ratings").map(String::as_str),
            &mut s.ratings,
            "Star ratings (0–5)",
            None,
            enabled,
            false,
        );
        changed |= option_row_with(
            ui,
            t,
            counts.get("Color labels").map(String::as_str),
            &mut s.labels,
            "Color labels",
            None,
            enabled,
            false,
        );
        changed |= option_row_with(
            ui,
            t,
            counts.get("Flags").map(String::as_str),
            &mut s.flags,
            "Flags",
            None,
            enabled,
            false,
        );
        changed |= option_row_with(
            ui,
            t,
            counts.get("Caption and copyright").map(String::as_str),
            &mut s.texts,
            "Caption and copyright",
            Some("Title, caption, copyright and creator, from the IPTC table."),
            enabled,
            false,
        );
        group(ui, t, "Organization");
        changed |= option_row_with(
            ui,
            t,
            counts.get("Collections").map(String::as_str),
            &mut s.collections,
            "Collections and collection sets",
            Some(
                "Smart collections become smart albums. So far only rules based on star ratings are supported.",
            ),
            enabled,
            false,
        );
        changed |= option_row_with(
            ui,
            t,
            counts.get("Stacks").map(String::as_str),
            &mut s.stacks,
            "Stacks and quick collection",
            None,
            enabled,
            false,
        );
        changed |= option_row_with(
            ui,
            t,
            counts.get("Virtual copies").map(String::as_str),
            &mut s.virtual_copies,
            "Virtual copies",
            None,
            enabled,
            false,
        );
        group(ui, t, "Develop");
        changed |= option_row_with(
            ui,
            t,
            counts.get("Develop settings").map(String::as_str),
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

/// The count of each area from the scan: label, selected, value.
fn summary_rows(app: &App) -> Vec<(&'static str, bool, String)> {
    let s = &app.settings.selection;
    let scan = app.scan.summary.as_ref();
    let count = |f: &dyn Fn(&ScanSummary) -> String| match scan {
        Some(r) => f(r),
        None if app.scan.pending => "…".to_owned(),
        None => "–".to_owned(),
    };
    let photos = |k: u64| crate::app::plural(k as usize, "photo");
    vec![
        (
            "Photos",
            true,
            count(&|r| {
                format!(
                    "{} in {}",
                    r.photos,
                    crate::app::plural(r.folders as usize, "folder")
                )
            }),
        ),
        ("Keywords", s.keywords, count(&|r| r.keywords.to_string())),
        ("Star ratings", s.ratings, count(&|r| photos(r.rated))),
        ("Color labels", s.labels, count(&|r| photos(r.labelled))),
        ("Flags", s.flags, count(&|r| photos(r.flagged))),
        (
            "Caption and copyright",
            s.texts,
            count(&|r| format!("{} captions, {} copyright", r.captions, r.copyrights)),
        ),
        (
            "Collections",
            s.collections,
            count(&|r| format!("{} albums, {} smart", r.albums, r.smart)),
        ),
        ("Stacks", s.stacks, count(&|r| r.stacks.to_string())),
        (
            "Virtual copies",
            s.virtual_copies,
            count(&|r| r.virtual_copies.to_string()),
        ),
        ("Develop settings", s.develop, count(&|r| photos(r.develop))),
    ]
}

fn summary(app: &App, ui: &mut Ui, t: &Tokens) {
    let rows = summary_rows(app);
    card(ui, t, Some("Summary"), |ui| {
        for (i, (label, on, value)) in rows.into_iter().enumerate() {
            if i > 0 {
                divider(ui, t);
            }
            let (value, weight, color) = if on {
                (value.as_str(), Some(MEDIUM), t.text)
            } else {
                ("not transferred", None, t.text_muted)
            };
            crate::widgets::kv_row(ui, t, None, label, value, weight, color, 36.0, None);
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
