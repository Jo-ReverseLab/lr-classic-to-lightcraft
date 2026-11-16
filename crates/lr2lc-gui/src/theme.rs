//! Colors, type and spacing. One set of tokens per theme; both themes share
//! every size and layout, only the colors change. The theme follows the
//! system unless the person picks one.

use egui::epaint::text::{FontData, FontDefinitions, FontFamily};
use egui::{Color32, FontId, Stroke, TextStyle, Theme, ThemePreference, Visuals};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The theme the person chose.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    /// System → Light → Dark → System.
    pub fn next(self) -> ThemeMode {
        match self {
            ThemeMode::System => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::System,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeMode::System => "Theme: System",
            ThemeMode::Light => "Theme: Light",
            ThemeMode::Dark => "Theme: Dark",
        }
    }

    pub fn preference(self) -> ThemePreference {
        match self {
            ThemeMode::System => ThemePreference::System,
            ThemeMode::Light => ThemePreference::Light,
            ThemeMode::Dark => ThemePreference::Dark,
        }
    }
}

/// The color tokens of one theme.
#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub bg_app: Color32,
    pub bg_bar: Color32,
    pub bg_panel: Color32,
    pub bg_group: Color32,
    pub bg_raised: Color32,
    pub bg_field: Color32,
    pub bg_selected: Color32,
    pub border: Color32,
    pub border_control: Color32,
    pub divider: Color32,
    pub text: Color32,
    pub text_strong: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub ok: Color32,
    pub adjusted: Color32,
    pub skipped: Color32,
    pub conflict: Color32,
    pub error: Color32,
    pub error_badge_bg: Color32,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub const DARK: Tokens = Tokens {
    bg_app: hex(0x17181a),
    bg_bar: hex(0x1d1e21),
    bg_panel: hex(0x202124),
    bg_group: hex(0x1d1e21),
    bg_raised: hex(0x2a2b30),
    bg_field: hex(0x151618),
    bg_selected: hex(0x2f3136),
    border: hex(0x34363b),
    border_control: hex(0x3d3f45),
    divider: hex(0x2a2b30),
    text: hex(0xdcdde0),
    text_strong: hex(0xffffff),
    // Slightly lighter than the spec's #9b9ea6 so muted text keeps 4.5:1
    // on the raised and selected backgrounds.
    text_muted: hex(0xa3a6ae),
    accent: hex(0xe4e5e8),
    on_accent: hex(0x17181a),
    ok: hex(0x6cb6ff),
    adjusted: hex(0xc2aaff),
    skipped: hex(0xa3a6ae),
    conflict: hex(0xf0c46a),
    error: hex(0xff9c8a),
    error_badge_bg: hex(0x3a2422),
};

pub const LIGHT: Tokens = Tokens {
    bg_app: hex(0xf3f4f6),
    bg_bar: hex(0xffffff),
    bg_panel: hex(0xffffff),
    bg_group: hex(0xf3f4f6),
    bg_raised: hex(0xeceef1),
    bg_field: hex(0xffffff),
    bg_selected: hex(0xe6e9ef),
    border: hex(0xd3d6dc),
    border_control: hex(0xb9bdc5),
    divider: hex(0xe4e6ea),
    text: hex(0x1b1c1f),
    text_strong: hex(0x000000),
    // Slightly darker than the spec's #5b5f68 for 4.5:1 on raised/selected.
    text_muted: hex(0x555962),
    accent: hex(0x1b1c1f),
    on_accent: hex(0xf3f4f6),
    ok: hex(0x1a62b0),
    adjusted: hex(0x6b49c9),
    skipped: hex(0x555962),
    conflict: hex(0x8a5a00),
    error: hex(0xb83a26),
    error_badge_bg: hex(0xfbe4df),
};

/// The tokens of the theme egui shows now.
pub fn tokens(ctx: &egui::Context) -> &'static Tokens {
    match ctx.theme() {
        Theme::Dark => &DARK,
        Theme::Light => &LIGHT,
    }
}

pub const MEDIUM: &str = "Inter-Medium";
pub const SEMIBOLD: &str = "Inter-SemiBold";

/// Inter (regular, medium, semibold) as the proportional font, the
/// built-in monospace font for monospace.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add(
        "Inter-Regular",
        include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    );
    add(MEDIUM, include_bytes!("../assets/fonts/Inter-Medium.ttf"));
    add(
        SEMIBOLD,
        include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    );
    if let Some(list) = fonts.families.get_mut(&FontFamily::Proportional) {
        list.insert(0, "Inter-Regular".to_owned());
    }
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for name in [MEDIUM, SEMIBOLD] {
        let mut list = vec![name.to_owned()];
        list.extend(fallback.iter().cloned());
        fonts.families.insert(FontFamily::Name(name.into()), list);
    }
    ctx.set_fonts(fonts);
}

/// A font of `size` in `weight` (`None`: regular).
pub fn font(size: f32, weight: Option<&'static str>) -> FontId {
    match weight {
        Some(name) => FontId::new(size, FontFamily::Name(name.into())),
        None => FontId::proportional(size),
    }
}

/// Sets sizes, spacing and colors for both themes and the chosen mode.
pub fn apply(ctx: &egui::Context, mode: ThemeMode) {
    for (theme, t) in [(Theme::Dark, &DARK), (Theme::Light, &LIGHT)] {
        ctx.style_mut_of(theme, |style| {
            style.text_styles = [
                (
                    TextStyle::Heading,
                    FontId::new(16.0, FontFamily::Name(SEMIBOLD.into())),
                ),
                (TextStyle::Body, FontId::proportional(13.0)),
                (
                    TextStyle::Button,
                    FontId::new(13.0, FontFamily::Name(MEDIUM.into())),
                ),
                (TextStyle::Small, FontId::proportional(12.0)),
                (TextStyle::Monospace, FontId::monospace(12.0)),
            ]
            .into();
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 6.0);
            style.spacing.interact_size.y = 40.0;
            style.visuals = visuals(theme, t);
        });
    }
    ctx.set_theme(mode.preference());
}

fn visuals(theme: Theme, t: &Tokens) -> Visuals {
    let mut v = match theme {
        Theme::Dark => Visuals::dark(),
        Theme::Light => Visuals::light(),
    };
    v.panel_fill = t.bg_app;
    v.window_fill = t.bg_panel;
    v.extreme_bg_color = t.bg_field;
    v.faint_bg_color = t.bg_group;
    v.override_text_color = Some(t.text);
    v.hyperlink_color = t.text;
    v.selection.bg_fill = t.bg_selected;
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.window_stroke = Stroke::new(1.0, t.border);
    let radius = egui::CornerRadius::same(4);
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.bg_fill = t.bg_raised;
        w.weak_bg_fill = t.bg_raised;
        w.bg_stroke = Stroke::new(1.0, t.border_control);
        w.fg_stroke = Stroke::new(1.5, t.text);
        w.corner_radius = radius;
    }
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.text_muted);
    v.widgets.active.bg_stroke = Stroke::new(2.0, t.accent);
    v.widgets.noninteractive.bg_fill = t.bg_panel;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.divider);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.noninteractive.corner_radius = radius;
    // Focus ring: 2 px in the accent color.
    v.selection.stroke = Stroke::new(2.0, t.accent);
    v
}

/// Relative luminance contrast of two colors (WCAG).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    fn channel(c: u8) -> f32 {
        let c = f32::from(c) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    let l =
        |c: Color32| 0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b());
    let (x, y) = (l(a), l(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_status_colors_keep_4_5_to_1() {
        for t in [&DARK, &LIGHT] {
            for bg in [t.bg_panel, t.bg_app, t.bg_raised, t.bg_selected, t.bg_group] {
                for fg in [
                    t.text,
                    t.text_muted,
                    t.ok,
                    t.adjusted,
                    t.skipped,
                    t.conflict,
                    t.error,
                ] {
                    let c = contrast(fg, bg);
                    assert!(c >= 4.5, "{fg:?} on {bg:?}: {c:.2}");
                }
            }
            assert!(contrast(t.on_accent, t.accent) >= 4.5);
            assert!(contrast(t.error, t.error_badge_bg) >= 4.5);
        }
    }

    #[test]
    fn the_theme_button_cycles() {
        let m = ThemeMode::System;
        assert_eq!(m.next().next().next(), ThemeMode::System);
        assert_eq!(m.next(), ThemeMode::Light);
    }
}
