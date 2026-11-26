//! A moving-needle display for the shared VU detector.

use crate::egui::{pos2, vec2, Align2, FontId, Painter, Pos2, Rect, Shape, Stroke};
use crate::{MeterScale, Theme};

const HALF_SWEEP: f32 = 0.88;
const FULL_SCALE_VU: f32 = 3.0;

/// Voltage-proportional fraction of the needle's full angular travel.
/// Unlike the bar scale, the needle travels below the lowest printed graduation.
pub fn deflection(vu: f32) -> f32 {
    if vu.is_nan() {
        return 0.0;
    }
    10.0f32.powf((vu - FULL_SCALE_VU) / 20.0).min(1.0)
}

fn point(pivot: Pos2, radius: f32, fraction: f32) -> Pos2 {
    let angle = (2.0 * fraction - 1.0) * HALF_SWEEP;
    pivot + radius * vec2(angle.sin(), -angle.cos())
}

/// Draws one channel with the same instantaneous measurement as the bar display.
pub fn draw(painter: &Painter, rect: Rect, vu: f32, channel: &str, readout: &str, theme: &Theme) {
    let painter = painter.with_clip_rect(painter.clip_rect().intersect(rect));
    painter.rect_filled(rect, 4.0, theme.panel);

    let pivot = pos2(rect.center().x, rect.bottom() - 12.0);
    let radius = ((rect.width() - 28.0) / (2.0 * HALF_SWEEP.sin())).min(rect.height() - 28.0);
    if radius < 12.0 {
        return;
    }
    let normal = theme.vu_zones[0].lit;
    let zero = deflection(0.0);
    for (start, end, stroke) in [
        (0.0, zero, Stroke::new(1.0_f32, normal)),
        (zero, 1.0, Stroke::new(2.5_f32, theme.over)),
    ] {
        let points = (0..=24)
            .map(|i| point(pivot, radius, start + (end - start) * i as f32 / 24.0))
            .collect();
        painter.add(Shape::line(points, stroke));
    }
    let scale = MeterScale::VU;
    let mut last_label: Option<Pos2> = None;
    for (index, &vu) in scale.ticks.iter().enumerate() {
        let f = deflection(vu);
        let colour = if vu > 0.0 { theme.over } else { normal };
        let label_position = point(pivot, radius + 9.0, f);
        // Keep the reference and alternating labels on narrow faces. All ticks remain drawn.
        let major = matches!(vu as i32, -20 | -10 | -5 | -2 | 0 | 2 | 3);
        painter.line_segment(
            [
                point(pivot, radius, f),
                point(pivot, radius - if major { 6.0 } else { 3.0 }, f),
            ],
            Stroke::new(1.0_f32, colour),
        );
        if (major || radius >= 130.0)
            && last_label.is_none_or(|p| p.distance(label_position) >= 13.0)
        {
            painter.text(
                label_position,
                Align2::CENTER_CENTER,
                scale.label(index, 0.0),
                FontId::proportional(9.0),
                colour,
            );
            last_label = Some(label_position);
        }
    }
    if radius >= 75.0 {
        for percent in [20, 40, 60, 80, 100] {
            let f = zero * percent as f32 / 100.0;
            painter.text(
                point(pivot, radius - 18.0, f),
                Align2::CENTER_CENTER,
                percent,
                FontId::proportional(8.0),
                theme.text_dim,
            );
        }
    }
    painter.line_segment(
        [pivot, point(pivot, radius - 3.0, deflection(vu))],
        Stroke::new(1.5_f32, theme.hold),
    );
    painter.circle_filled(pivot, 3.0, normal);
    painter.text(
        rect.left_bottom() + vec2(7.0, -6.0),
        Align2::LEFT_BOTTOM,
        channel,
        FontId::proportional(10.0),
        theme.text_dim,
    );
    painter.text(
        rect.right_bottom() - vec2(7.0, 6.0),
        Align2::RIGHT_BOTTOM,
        readout,
        FontId::monospace(10.0),
        theme.text,
    );
}
