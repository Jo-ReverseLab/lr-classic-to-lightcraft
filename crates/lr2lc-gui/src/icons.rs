//! Stroke icons drawn from 16 × 16 path data, in the current text color of
//! their status. Status is never shown by color alone: each status has its
//! own shape and a text label next to it.

use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};
use model::Status;

/// The icons of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Check,
    Adjusted,
    Dash,
    Cross,
    Exclamation,
    /// The running arc; turned by `angle`.
    Arc,
    Dot,
    Chevron,
    Monitor,
    Sun,
    Moon,
    /// An “i” in a circle: an explanation on hover.
    Info,
}

impl Icon {
    pub fn for_status(status: Status) -> Icon {
        match status {
            Status::Transferred => Icon::Check,
            Status::Adjusted => Icon::Adjusted,
            Status::Skipped => Icon::Dash,
            Status::Conflict => Icon::Exclamation,
            Status::Error => Icon::Cross,
        }
    }
}

/// Paints `icon` into `rect` (scaled from 16 × 16).
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32, angle: f32) {
    let s = rect.width() / 16.0;
    let p = |x: f32, y: f32| pos2(rect.min.x + x * s, rect.min.y + y * s);
    let stroke = Stroke::new(1.7 * s.max(1.0), color);
    let line = |points: Vec<Pos2>| Shape::line(points, stroke);
    let shapes: Vec<Shape> = match icon {
        // M3.5 8.5l3 3 6-7
        Icon::Check => vec![line(vec![p(3.5, 8.5), p(6.5, 11.5), p(12.5, 4.5)])],
        // M3 6h9l-2.5-2.5  M13 10H4l2.5 2.5
        Icon::Adjusted => vec![
            line(vec![p(3.0, 6.0), p(12.0, 6.0), p(9.5, 3.5)]),
            line(vec![p(13.0, 10.0), p(4.0, 10.0), p(6.5, 12.5)]),
        ],
        // M4 8h8
        Icon::Dash => vec![line(vec![p(4.0, 8.0), p(12.0, 8.0)])],
        // M4.5 4.5l7 7  M11.5 4.5l-7 7
        Icon::Cross => vec![
            line(vec![p(4.5, 4.5), p(11.5, 11.5)]),
            line(vec![p(11.5, 4.5), p(4.5, 11.5)]),
        ],
        // M8 3v6  M8 12v.5
        Icon::Exclamation => vec![
            line(vec![p(8.0, 3.0), p(8.0, 9.0)]),
            line(vec![p(8.0, 12.0), p(8.0, 12.5)]),
        ],
        // M8 2a6 6 0 1 1-6 6: three quarters of a circle, turned by `angle`.
        Icon::Arc => {
            let center = p(8.0, 8.0);
            let r = 6.0 * s;
            let points = (0..=24)
                .map(|i| {
                    let a = angle - std::f32::consts::FRAC_PI_2
                        + i as f32 / 24.0 * 1.5 * std::f32::consts::PI;
                    center + vec2(a.cos(), a.sin()) * r
                })
                .collect();
            vec![line(points)]
        }
        // M8 8v.01: a small dot.
        Icon::Dot => vec![Shape::circle_filled(p(8.0, 8.0), 1.6 * s, color)],
        // M4 6l4 4 4-4
        Icon::Chevron => vec![line(vec![p(4.0, 6.0), p(8.0, 10.0), p(12.0, 6.0)])],
        Icon::Info => vec![
            Shape::circle_stroke(p(8.0, 8.0), 6.0 * s, Stroke::new(1.3 * s.max(1.0), color)),
            line(vec![p(8.0, 7.3), p(8.0, 11.2)]),
            Shape::circle_filled(p(8.0, 5.0), 0.9 * s, color),
        ],
        Icon::Monitor => vec![
            Shape::rect_stroke(
                Rect::from_min_max(p(2.5, 3.0), p(13.5, 10.5)),
                1.0,
                stroke,
                egui::StrokeKind::Middle,
            ),
            line(vec![p(6.0, 13.0), p(10.0, 13.0)]),
            line(vec![p(8.0, 10.5), p(8.0, 13.0)]),
        ],
        Icon::Sun => {
            let mut v = vec![Shape::circle_stroke(p(8.0, 8.0), 2.8 * s, stroke)];
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let d = vec2(a.cos(), a.sin());
                v.push(line(vec![
                    p(8.0, 8.0) + d * 4.6 * s,
                    p(8.0, 8.0) + d * 6.2 * s,
                ]));
            }
            v
        }
        Icon::Moon => {
            // A crescent: the outer arc of a circle and the inner arc of a
            // second, offset circle, as one closed outline.
            let outer_c = p(8.0, 8.0);
            let inner_c = p(10.6, 6.0);
            let r = 5.5 * s;
            let ri = 4.6 * s;
            let mut points: Vec<Pos2> = (0..=28)
                .map(|i| {
                    let a = 0.35 + i as f32 / 28.0 * 4.6;
                    outer_c + vec2(a.cos(), a.sin()) * r
                })
                .collect();
            points.extend((0..=20).map(|i| {
                let a = 4.62 - i as f32 / 20.0 * 3.05;
                inner_c + vec2(a.cos(), a.sin()) * ri
            }));
            vec![Shape::closed_line(points, stroke)]
        }
    };
    painter.extend(shapes);
}

/// Adds `icon` at `size` px as a non-interactive widget.
pub fn show(ui: &mut Ui, icon: Icon, color: Color32, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let angle = if icon == Icon::Arc {
        // A slow turn: one round in two seconds.
        ui.ctx().request_repaint();
        (ui.input(|i| i.time) as f32) * std::f32::consts::PI
    } else {
        0.0
    };
    paint(ui.painter(), rect, icon, color, angle);
    response
}
