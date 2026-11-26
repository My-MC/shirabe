//! Lissajous (mid/side) display of the stereo signal.
//!
//! The widget only draws: it takes points already rotated to mid/side coordinates and never
//! touches audio state. See `shirabe_dsp::scope` for the capture buffer and the rotation.

use crate::egui::{
    pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind,
};
use crate::theme::Theme;

/// Points read from the capture buffer for one displayed frame. The oldest frames of the
/// capture are dropped, so the nicest figures come from signals that are periodic within
/// this window (about 43 ms at 48 kHz).
pub const MAX_POINTS: usize = 2048;

/// Height reserved for the title line above the square.
const TITLE_HEIGHT: f32 = 14.0;

/// Maps mid/side coordinates onto a centred square.
///
/// `mid` is the vertical axis (L + R, positive up) and `side` the horizontal axis
/// (L − R, positive right). Values outside ±1 are drawn on the border; use
/// [`is_clipped`] to flag them.
#[derive(Clone, Copy, Debug)]
pub struct ScopeGeometry {
    square: Rect,
}

impl ScopeGeometry {
    /// Fits the largest centred square inside `rect`.
    pub fn new(rect: Rect) -> Self {
        let side = rect.width().min(rect.height()).max(0.0);
        Self {
            square: Rect::from_center_size(rect.center(), vec2(side, side)),
        }
    }

    /// The square the figure is drawn in.
    pub fn square(&self) -> Rect {
        self.square
    }

    /// Position of a mid/side pair. Full scale (±1) lies on the border of the square.
    pub fn point(&self, mid: f32, side: f32) -> Pos2 {
        let half = self.square.width() * 0.5;
        let mid = unit(mid);
        let side = unit(side);
        pos2(
            self.square.center().x + side * half,
            self.square.center().y - mid * half,
        )
    }
}

/// Clamps to full scale, passing non-finite values through as the centre.
fn unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// Whether any point lies beyond full scale (0 dBFS on either axis).
pub fn is_clipped(points: &[(f32, f32)]) -> bool {
    points.iter().any(|&(mid, side)| {
        (mid.is_finite() && mid.abs() > 1.0) || (side.is_finite() && side.abs() > 1.0)
    })
}

/// Draws the Lissajous panel in `rect`.
///
/// `points` are mid/side pairs in chronological order. With `mono`, no figure is drawn: a
/// mono signal carries no stereo information, and a vertical line would suggest perfect
/// correlation instead.
pub fn draw(painter: &Painter, rect: Rect, points: &[(f32, f32)], mono: bool, theme: &Theme) {
    painter.text(
        pos2(rect.left(), rect.top()),
        Align2::LEFT_TOP,
        "LISSAJOUS  M/S",
        FontId::proportional(11.0),
        theme.text_dim,
    );

    let body = Rect::from_min_max(pos2(rect.left(), rect.top() + TITLE_HEIGHT), rect.max);
    if body.width() < 24.0 || body.height() < 24.0 {
        return;
    }
    let geometry = ScopeGeometry::new(body);
    let square = geometry.square();
    let clipped = !mono && is_clipped(points);

    painter.rect_filled(square, 3.0, theme.panel);
    let grid = theme.text_dim.gamma_multiply(0.35);
    let centre = square.center();
    painter.line_segment(
        [
            pos2(square.left(), centre.y),
            pos2(square.right(), centre.y),
        ],
        Stroke::new(1.0_f32, grid),
    );
    painter.line_segment(
        [
            pos2(centre.x, square.top()),
            pos2(centre.x, square.bottom()),
        ],
        Stroke::new(1.0_f32, grid),
    );
    let border = if clipped { theme.over } else { theme.text_dim };
    painter.rect_stroke(
        square,
        3.0,
        Stroke::new(1.0_f32, border),
        StrokeKind::Inside,
    );

    // The right end of the title states the scale, or the clip state that replaces it.
    painter.text(
        pos2(rect.right(), rect.top()),
        Align2::RIGHT_TOP,
        if clipped { "CLIP  ±1" } else { "±1 FS" },
        FontId::proportional(10.0),
        if clipped { theme.over } else { theme.text_dim },
    );

    if mono {
        painter.text(
            centre,
            Align2::CENTER_CENTER,
            "MONO",
            FontId::proportional(12.0),
            theme.text_dim,
        );
        painter.text(
            pos2(centre.x, centre.y + 15.0),
            Align2::CENTER_CENTER,
            "no stereo content",
            FontId::proportional(9.0),
            theme.text_dim.gamma_multiply(0.8),
        );
        return;
    }

    // Only finite points are drawn; a host passing a NaN must not break the trace.
    let visible: Vec<Pos2> = points
        .iter()
        .filter(|(mid, side)| mid.is_finite() && side.is_finite())
        .map(|&(mid, side)| geometry.point(mid, side))
        .collect();
    let colour = if clipped {
        Color32::from_rgb(0xf0, 0x8a, 0x3c)
    } else {
        theme.on_target
    };
    match visible.as_slice() {
        [] => {}
        [point] => {
            painter.circle_filled(*point, 1.5, colour);
        }
        _ => {
            painter.add(Shape::line(visible, Stroke::new(1.2_f32, colour)));
        }
    }
}
