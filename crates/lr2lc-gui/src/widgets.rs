//! Shared pieces: cards with small uppercase headers, primary and secondary
//! buttons, status labels and pills, filter chips and check rows.

use crate::icons::{self, Icon};
use crate::theme::{MEDIUM, SEMIBOLD, Tokens, font};
use egui::text::LayoutJob;
use egui::{
    Align, Button, Color32, CornerRadius, Frame, Layout, Response, RichText, Sense, Stroke,
    TextFormat, Ui, Vec2, vec2,
};
use model::Status;

/// The color of a status.
pub fn status_color(t: &Tokens, status: Status) -> Color32 {
    match status {
        Status::Transferred => t.ok,
        Status::Adjusted => t.adjusted,
        Status::Skipped => t.skipped,
        Status::Conflict => t.conflict,
        Status::Error => t.error,
    }
}

/// The label of a status.
pub fn status_label(status: Status) -> &'static str {
    match status {
        Status::Transferred => "Transferred",
        Status::Adjusted => "Adjusted",
        Status::Skipped => "Skipped",
        Status::Conflict => "Conflict",
        Status::Error => "Error",
    }
}

/// The status of a report's `status` text.
pub fn status_of(text: &str) -> Status {
    match text {
        "adjusted" => Status::Adjusted,
        "skipped" => Status::Skipped,
        "conflict" => Status::Conflict,
        "error" => Status::Error,
        _ => Status::Transferred,
    }
}

/// A panel card: background, border, 6 px radius; an optional uppercase
/// header with a divider below.
pub fn card<R>(ui: &mut Ui, t: &Tokens, title: Option<&str>, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(t.bg_panel)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(CornerRadius::same(6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            if let Some(title) = title {
                header(ui, t, title, |_| {});
            }
            add(ui)
        })
        .inner
}

/// A card header: 11 px semibold uppercase muted, letter-spaced, with a
/// divider below; `right` adds controls at the right end.
pub fn header(ui: &mut Ui, t: &Tokens, title: &str, right: impl FnOnce(&mut Ui)) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    let galley = ui.painter().layout_job(caps(title, t.text_muted));
    ui.painter().galley(
        egui::pos2(rect.min.x + 16.0, rect.center().y - galley.size().y / 2.0),
        galley,
        t.text_muted,
    );
    let right_rect = egui::Rect::from_min_max(
        egui::pos2(rect.center().x, rect.min.y),
        egui::pos2(rect.max.x - 16.0, rect.max.y),
    );
    // A child that does not move the parent's cursor.
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(right_rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    right(&mut child);
    divider(ui, t);
}

/// A row with a label at the left and a value at the right, `height` px
/// high: the value keeps its width, the label is shortened to fit.
#[allow(clippy::too_many_arguments)]
pub fn kv_row(
    ui: &mut Ui,
    t: &Tokens,
    icon: Option<(crate::icons::Icon, Color32)>,
    label: &str,
    value: &str,
    value_weight: Option<&'static str>,
    value_color: Color32,
    height: f32,
) -> Response {
    let width = ui.available_width();
    let indent = if icon.is_some() { 24.0 } else { 0.0 };
    let inner = (width - 32.0 - indent).max(40.0);
    let layout = |text: &str, weight, color, max: f32| {
        let mut job = LayoutJob::simple_singleline(text.to_owned(), font(13.0, weight), color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(max);
        ui.painter().layout_job(job)
    };
    let lgal = layout(label, None, t.text_muted, inner);
    let vfull = layout(value, value_weight, value_color, inner);
    // Side by side when both fit; else the value goes on a second line.
    let two_lines = lgal.size().x + 12.0 + vfull.size().x > inner;
    let row_height = if two_lines { height + 18.0 } else { height };
    let (rect, response) = ui.allocate_exact_size(vec2(width, row_height), Sense::hover());
    let painter = ui.painter();
    let x = rect.min.x + 16.0 + indent;
    let first_y = if two_lines {
        rect.min.y + height / 2.0 - 2.0
    } else {
        rect.center().y
    };
    if let Some((icon, color)) = icon {
        crate::icons::paint(
            painter,
            egui::Rect::from_min_size(
                egui::pos2(rect.min.x + 16.0, first_y - 8.0),
                vec2(16.0, 16.0),
            ),
            icon,
            color,
            0.0,
        );
    }
    let value_y = if two_lines { first_y + 20.0 } else { first_y };
    let vw = vfull.size().x;
    painter.galley(
        egui::pos2(rect.max.x - 16.0 - vw, value_y - vfull.size().y / 2.0),
        vfull,
        value_color,
    );
    painter.galley(
        egui::pos2(x, first_y - lgal.size().y / 2.0),
        lgal,
        t.text_muted,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Label, true, format!("{label}: {value}"))
    });
    response
}

/// Uppercase, 11 px semibold, letter-spaced text.
pub fn caps(text: &str, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(
        &text.to_uppercase(),
        0.0,
        TextFormat {
            font_id: font(11.0, Some(SEMIBOLD)),
            color,
            extra_letter_spacing: 0.88,
            ..TextFormat::default()
        },
    );
    job
}

/// A 1 px full-width divider.
pub fn divider(ui: &mut Ui, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, t.divider);
}

/// The primary action: accent fill, on-accent text, 44 px high.
pub fn primary(ui: &mut Ui, t: &Tokens, text: &str, enabled: bool, width: f32) -> Response {
    let button = Button::new(
        RichText::new(text)
            .font(font(13.0, Some(SEMIBOLD)))
            .color(t.on_accent),
    )
    .fill(t.accent)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(4))
    .min_size(vec2(width, 44.0));
    ui.add_enabled(enabled, button)
}

/// A secondary action: raised fill, control border, `height` px high.
pub fn secondary(
    ui: &mut Ui,
    t: &Tokens,
    text: &str,
    enabled: bool,
    width: f32,
    height: f32,
) -> Response {
    let button = Button::new(
        RichText::new(text)
            .font(font(13.0, Some(MEDIUM)))
            .color(t.text),
    )
    .fill(t.bg_raised)
    .stroke(Stroke::new(1.0, t.border_control))
    .corner_radius(CornerRadius::same(4))
    .min_size(vec2(width, height));
    ui.add_enabled(enabled, button)
}

/// An underlined text button.
pub fn text_link(ui: &mut Ui, t: &Tokens, text: &str) -> Response {
    ui.add(
        Button::new(RichText::new(text).underline().color(t.text))
            .frame(false)
            .min_size(vec2(0.0, 32.0)),
    )
}

/// A status icon with its label in the status color.
pub fn status_tag(ui: &mut Ui, t: &Tokens, status: Status, icon_size: f32) {
    let color = status_color(t, status);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        icons::show(ui, Icon::for_status(status), color, icon_size)
            .on_hover_text(status_label(status));
        ui.label(
            RichText::new(status_label(status))
                .font(font(13.0, Some(MEDIUM)))
                .color(color),
        );
    });
}

/// A pill-shaped filter chip with a status dot and a count.
pub fn chip(
    ui: &mut Ui,
    t: &Tokens,
    selected: bool,
    dot: Option<Color32>,
    label: &str,
    count: Option<usize>,
    height: f32,
) -> Response {
    let (fill, text) = if selected {
        (t.accent, t.on_accent)
    } else {
        (t.bg_raised, t.text)
    };
    let mut job = LayoutJob::default();
    let format = |color, weight| TextFormat {
        font_id: font(13.0, weight),
        color,
        ..TextFormat::default()
    };
    job.append(label, 0.0, format(text, Some(MEDIUM)));
    if let Some(n) = count {
        let muted = if selected { text } else { t.text_muted };
        job.append(&format!("  {n}"), 0.0, format(muted, None));
    }
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let dot_space = if dot.is_some() { 16.0 } else { 0.0 };
    let size = vec2(galley.size().x + 24.0 + dot_space, height);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(18),
        fill,
        Stroke::new(1.0, if selected { t.accent } else { t.border_control }),
        egui::StrokeKind::Inside,
    );
    let mut x = rect.min.x + 12.0;
    if let Some(color) = dot {
        painter.circle_filled(egui::pos2(x + 4.0, rect.center().y), 4.0, color);
        x += dot_space;
    }
    painter.galley(
        egui::pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        text,
    );
    if response.has_focus() {
        painter.rect_stroke(
            rect.expand(2.0),
            CornerRadius::same(20),
            Stroke::new(2.0, t.accent),
            egui::StrokeKind::Outside,
        );
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    response
}

/// A check list row: icon, muted label, value at the right; 36 px high
/// with a divider above.
pub fn check_row(ui: &mut Ui, t: &Tokens, icon: Icon, color: Color32, label: &str, value: &str) {
    divider(ui, t);
    kv_row(
        ui,
        t,
        Some((icon, color)),
        label,
        value,
        Some(MEDIUM),
        t.text,
        36.0,
    );
}

/// `text` shortened in the middle to at most `max` characters.
pub fn middle_ellipsis(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max || max < 5 {
        return text.to_owned();
    }
    let keep = max - 1;
    let head = keep / 2;
    let tail = keep - head;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len() - tail..]);
    out
}

/// A read-only path field, 40 px high, shortened in the middle.
pub fn path_field(ui: &mut Ui, t: &Tokens, path: &str, placeholder: &str, width: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 40.0), Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(4),
        t.bg_field,
        Stroke::new(1.0, t.border_control),
        egui::StrokeKind::Inside,
    );
    let max = ((width - 24.0) / 7.0).max(8.0) as usize;
    let (text, color) = if path.is_empty() {
        (placeholder.to_owned(), t.text_muted)
    } else {
        (middle_ellipsis(path, max), t.text)
    };
    painter.text(
        rect.left_center() + Vec2::new(12.0, 0.0),
        egui::Align2::LEFT_CENTER,
        text,
        font(12.5, None),
        color,
    );
    response.on_hover_text(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_paths_are_shortened_in_the_middle() {
        assert_eq!(middle_ellipsis("short", 10), "short");
        let s = middle_ellipsis("/Users/someone/Pictures/Lightroom/Catalog.lrcat", 20);
        assert_eq!(s.chars().count(), 20);
        assert!(s.starts_with("/Users/so") && s.ends_with("og.lrcat") && s.contains('…'));
    }
}
