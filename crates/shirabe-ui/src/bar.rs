//! Vertical level bar and its scale labels.

use crate::egui::{pos2, Align2, FontId, Painter, Rect, Stroke};
use crate::scale::MeterScale;
use crate::theme::{zone_at, Theme, Zone};

/// Draws a vertical segmented level bar with a peak-hold marker inside `rect`.
pub fn draw_level_bar(
    painter: &Painter,
    rect: Rect,
    scale: &MeterScale,
    theme: &Theme,
    zones: &[Zone],
    level_db: f32,
    hold_db: f32,
) {
    painter.rect_filled(rect, 0.0, theme.panel);

    let inner = rect.shrink(1.0);
    let height = inner.height();
    if height <= 0.0 {
        return;
    }
    let level_pos = scale.position(level_db);

    let pitch = theme.segment_height + theme.segment_gap;
    if theme.segment_height > 0.0 && pitch > 0.0 {
        let count = (height / pitch).floor().max(1.0) as usize;
        for i in 0..count {
            let bottom = inner.bottom() - i as f32 * pitch;
            let top = bottom - theme.segment_height;
            // Colour by the dB value at the segment centre.
            let centre_pos = (i as f32 + 0.5) / count as f32;
            let zone = zone_at(zones, scale.value_at(centre_pos));
            let lit = centre_pos <= level_pos && level_db > scale.min;
            let colour = if lit { zone.lit } else { zone.unlit };
            painter.rect_filled(
                Rect::from_min_max(pos2(inner.left(), top), pos2(inner.right(), bottom)),
                0.0,
                colour,
            );
        }
    } else {
        let top = inner.bottom() - level_pos * height;
        let zone = zone_at(zones, level_db);
        painter.rect_filled(
            Rect::from_min_max(pos2(inner.left(), top), inner.right_bottom()),
            0.0,
            zone.lit,
        );
    }

    if hold_db > scale.min {
        let y = inner.bottom() - scale.position(hold_db) * height;
        painter.line_segment(
            [pos2(inner.left(), y), pos2(inner.right(), y)],
            Stroke::new(2.0_f32, theme.hold),
        );
    }
}

/// Draws tick labels for `scale` right-aligned against `rect.right()`, spanning its height.
/// `label_offset` is added to the label values (for example to show an LU scale in LUFS).
pub fn draw_scale_labels(
    painter: &Painter,
    rect: Rect,
    scale: &MeterScale,
    theme: &Theme,
    label_offset: f32,
) {
    let inner = rect.shrink2(crate::egui::vec2(0.0, 1.0));
    let font = FontId::monospace(10.0);
    for (index, &tick) in scale.ticks.iter().enumerate() {
        let y = inner.bottom() - scale.position(tick) * inner.height();
        painter.text(
            pos2(rect.right() - 3.0, y),
            Align2::RIGHT_CENTER,
            scale.label(index, label_offset),
            font.clone(),
            theme.text_dim,
        );
        painter.line_segment(
            [pos2(rect.right() - 2.0, y), pos2(rect.right(), y)],
            Stroke::new(1.0_f32, theme.text_dim),
        );
    }
}

/// Draws a horizontal marker line (for example a target level) across `rect` at `db`.
pub fn draw_marker(
    painter: &Painter,
    rect: Rect,
    scale: &MeterScale,
    db: f32,
    colour: crate::egui::Color32,
) {
    let inner = rect.shrink(1.0);
    let y = inner.bottom() - scale.position(db) * inner.height();
    painter.line_segment(
        [pos2(rect.left() - 3.0, y), pos2(rect.right() + 3.0, y)],
        Stroke::new(1.5_f32, colour),
    );
}
