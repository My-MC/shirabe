//! Vertical level bar and its scale labels.

use crate::egui::{pos2, Align2, FontId, Painter, Rect, Stroke};
use crate::scale::MeterScale;
use crate::theme::Theme;

/// Draws a vertical segmented level bar with a peak-hold marker inside `rect`.
pub fn draw_level_bar(
    painter: &Painter,
    rect: Rect,
    scale: &MeterScale,
    theme: &Theme,
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
            let zone = theme.zone_at(db_at(scale, centre_pos));
            let lit = centre_pos <= level_pos && level_pos > 0.0;
            let colour = if lit { zone.lit } else { zone.unlit };
            painter.rect_filled(
                Rect::from_min_max(pos2(inner.left(), top), pos2(inner.right(), bottom)),
                0.0,
                colour,
            );
        }
    } else {
        let top = inner.bottom() - level_pos * height;
        let zone = theme.zone_at(level_db);
        painter.rect_filled(
            Rect::from_min_max(pos2(inner.left(), top), inner.right_bottom()),
            0.0,
            zone.lit,
        );
    }

    if hold_db > scale.min_db() {
        let y = inner.bottom() - scale.position(hold_db) * height;
        painter.line_segment(
            [pos2(inner.left(), y), pos2(inner.right(), y)],
            Stroke::new(2.0_f32, theme.hold),
        );
    }
}

/// Draws tick labels for `scale` right-aligned against `rect.right()`, spanning its height.
pub fn draw_scale_labels(painter: &Painter, rect: Rect, scale: &MeterScale, theme: &Theme) {
    let inner = rect.shrink2(crate::egui::vec2(0.0, 1.0));
    let font = FontId::monospace(10.0);
    for &tick in scale.ticks {
        let y = inner.bottom() - scale.position(tick) * inner.height();
        painter.text(
            pos2(rect.right() - 3.0, y),
            Align2::RIGHT_CENTER,
            format!("{}", tick.abs() as i32),
            font.clone(),
            theme.text_dim,
        );
        painter.line_segment(
            [pos2(rect.right() - 2.0, y), pos2(rect.right(), y)],
            Stroke::new(1.0_f32, theme.text_dim),
        );
    }
}

/// Inverse of [`MeterScale::position`] by bisection (only used for colouring segments).
fn db_at(scale: &MeterScale, position: f32) -> f32 {
    let (mut lo, mut hi) = (scale.min_db(), scale.max_db());
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if scale.position(mid) < position {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_at_inverts_position() {
        let s = MeterScale::PEAK_DBFS;
        for db in [-50.0f32, -20.0, -9.0, -3.0, -0.5] {
            assert!((db_at(&s, s.position(db)) - db).abs() < 0.01, "{db}");
        }
    }
}
