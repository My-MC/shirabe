//! Loudness time-history graph: momentary and short-term loudness over the last minutes.
//!
//! The graph only draws the points the measurement published; it never changes a measurement.
//! The horizontal axis is measured programme time (10 points per second), so pausing the
//! programme freezes the trace and a reset starts it over.
//!
//! The y axis reuses a loudness scale ([`MeterScale::LOUDNESS_EBU_9`] or
//! [`MeterScale::LOUDNESS_EBU_18`]) around the target, like the loudness bars. The axis
//! direction and the plot layout are a display convention, not part of a standard.

use crate::bar::draw_scale_labels;
use crate::egui::{pos2, Align2, Color32, FontId, Painter, Rect, Stroke, StrokeKind};
use crate::i18n::Text;
use crate::scale::MeterScale;
use crate::theme::Theme;

/// Space above the plot for the heading.
pub const TITLE_HEIGHT: f32 = 16.0;
/// Space below the plot for the time labels.
pub const AXIS_HEIGHT: f32 = 14.0;
/// Space left of the plot for the loudness labels.
pub const AXIS_WIDTH: f32 = 30.0;
/// Narrowest plot that still reads as a graph.
pub const MIN_PLOT_WIDTH: f32 = 80.0;
/// Shortest plot that still reads as a graph.
pub const MIN_PLOT_HEIGHT: f32 = 48.0;

/// Colour of the momentary curve. The short-term curve uses the theme's text colour, which
/// stays readable in both languages and on every theme.
pub fn momentary_colour() -> Color32 {
    Color32::from_rgb(0x4a, 0x9c, 0xd8)
}

/// One point of the history: loudness in LUFS, absent while the measure has no value yet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// Momentary loudness (400 ms window).
    pub momentary: Option<f32>,
    /// Short-term loudness (3 s window).
    pub short_term: Option<f32>,
}

impl Point {
    /// A point from the two published values.
    pub fn new(momentary: Option<f32>, short_term: Option<f32>) -> Self {
        Self {
            momentary,
            short_term,
        }
    }
}

/// Everything the graph draws in one frame.
pub struct Graph<'a> {
    /// Oldest first, newest last.
    pub points: &'a [Point],
    /// Points per second of measured time (10 for the capture).
    pub points_per_second: f32,
    /// Width of the time axis in seconds; older points are not drawn.
    pub span_seconds: f32,
    /// Target loudness in LUFS.
    pub target: f32,
    /// Permitted tolerance in LU, if the target has one.
    pub tolerance: Option<f32>,
    /// Loudness scale around the target.
    pub scale: MeterScale,
    /// Added to the scale values on the labels: the target, so they read in LUFS.
    pub label_offset: f32,
    /// Whether the programme measurement is paused, which freezes the trace.
    pub paused: bool,
    /// Interface language.
    pub text: Text,
}

/// The plot rectangle of a tile, or `None` when the tile is too small for a graph.
pub fn plot_rect(rect: Rect) -> Option<Rect> {
    let plot = Rect::from_min_max(
        pos2(rect.left() + AXIS_WIDTH + 2.0, rect.top() + TITLE_HEIGHT),
        pos2(rect.right() - 2.0, rect.bottom() - AXIS_HEIGHT),
    );
    (plot.width() >= MIN_PLOT_WIDTH && plot.height() >= MIN_PLOT_HEIGHT).then_some(plot)
}

/// Horizontal position of a point `age_seconds` before the newest one.
pub fn time_x(plot: Rect, age_seconds: f32, span_seconds: f32) -> f32 {
    let span = span_seconds.max(1e-3);
    plot.right() - (age_seconds / span).clamp(0.0, 1.0) * plot.width()
}

/// Vertical position of a loudness value in LUFS.
pub fn loudness_y(plot: Rect, lufs: f32, target: f32, scale: &MeterScale) -> f32 {
    plot.bottom() - scale.position(lufs - target) * plot.height()
}

/// Spacing of the vertical time grid, so a wide tile does not fill up with lines.
fn grid_step_seconds(span_seconds: f32) -> f32 {
    if span_seconds >= 120.0 {
        60.0
    } else if span_seconds >= 60.0 {
        30.0
    } else {
        10.0
    }
}

/// Draws the graph panel in `rect`.
pub fn draw(painter: &Painter, rect: Rect, graph: &Graph<'_>, theme: &Theme) {
    let title = if graph.paused {
        format!(
            "{}  {}",
            graph.text.history_title(),
            graph.text.integrating_paused()
        )
    } else {
        graph.text.history_title().to_owned()
    };
    painter.text(
        pos2(rect.left(), rect.top()),
        Align2::LEFT_TOP,
        title,
        FontId::proportional(11.0),
        if graph.paused {
            theme.over
        } else {
            theme.text_dim
        },
    );
    painter.text(
        pos2(rect.right(), rect.top()),
        Align2::RIGHT_TOP,
        graph.text.history_span_label(),
        FontId::proportional(10.0),
        theme.text_dim,
    );

    let Some(plot) = plot_rect(rect) else {
        return;
    };
    painter.rect_filled(plot, 3.0, theme.panel);

    // Target tolerance band and target line: the same reference the loudness bars use.
    if let Some(tolerance) = graph.tolerance.filter(|t| *t > 0.0) {
        let upper = loudness_y(plot, graph.target + tolerance, graph.target, &graph.scale);
        let lower = loudness_y(plot, graph.target - tolerance, graph.target, &graph.scale);
        let band = Rect::from_min_max(pos2(plot.left(), upper), pos2(plot.right(), lower));
        painter.rect_filled(band, 0.0, theme.on_target.gamma_multiply(0.16));
    }
    let grid = theme.text_dim.gamma_multiply(0.35);
    let target_y = loudness_y(plot, graph.target, graph.target, &graph.scale);
    painter.line_segment(
        [pos2(plot.left(), target_y), pos2(plot.right(), target_y)],
        Stroke::new(1.0_f32, theme.text_dim),
    );

    // Time grid, labelled in seconds before the newest point.
    let step = grid_step_seconds(graph.span_seconds);
    let mut age = step;
    while age < graph.span_seconds {
        let x = time_x(plot, age, graph.span_seconds);
        painter.line_segment(
            [pos2(x, plot.top()), pos2(x, plot.bottom())],
            Stroke::new(1.0_f32, grid),
        );
        painter.text(
            pos2(x, plot.bottom() + 1.0),
            Align2::CENTER_TOP,
            format!("-{age:.0}"),
            FontId::monospace(9.0),
            theme.text_dim,
        );
        age += step;
    }
    painter.text(
        pos2(plot.left(), plot.bottom() + 1.0),
        Align2::LEFT_TOP,
        graph.text.history_time_axis(),
        FontId::proportional(9.0),
        theme.text_dim,
    );

    // Loudness labels on the left, in the unit of the selected scale.
    draw_scale_labels(
        painter,
        Rect::from_min_max(
            pos2(plot.left() - AXIS_WIDTH, plot.top()),
            pos2(plot.left() - 2.0, plot.bottom()),
        ),
        &graph.scale,
        theme,
        graph.label_offset,
    );

    if graph.points.len() < 2 {
        painter.text(
            plot.center(),
            Align2::CENTER_CENTER,
            graph.text.history_waiting(),
            FontId::proportional(11.0),
            theme.text_dim,
        );
    } else {
        let moment = momentary_colour();
        draw_curve(painter, plot, graph, theme.text, |p| p.short_term);
        draw_curve(painter, plot, graph, moment, |p| p.momentary);
    }

    painter.rect_stroke(
        plot,
        3.0,
        Stroke::new(1.0_f32, theme.text_dim),
        StrokeKind::Inside,
    );

    // Legend under the right-hand corner of the plot.
    let legend_y = plot.bottom() + 2.0;
    painter.text(
        pos2(plot.right() - 20.0, legend_y),
        Align2::RIGHT_TOP,
        "M",
        FontId::monospace(9.0),
        momentary_colour(),
    );
    painter.text(
        pos2(plot.right(), legend_y),
        Align2::RIGHT_TOP,
        "S",
        FontId::monospace(9.0),
        theme.text,
    );
}

/// Draws one curve, breaking it wherever a value is absent or not finite (silence is a gap,
/// not a level at the bottom of the scale).
///
/// The points are joined into one path per run instead of one segment per pair: at three
/// minutes the points are less than a pixel apart, and separate sub-pixel segments leave a
/// stippled trace.
fn draw_curve(
    painter: &Painter,
    plot: Rect,
    graph: &Graph<'_>,
    colour: Color32,
    value: impl Fn(&Point) -> Option<f32>,
) {
    let stroke = Stroke::new(1.5_f32, colour);
    let count = graph.points.len();
    let seconds_per_point = 1.0 / graph.points_per_second.max(1.0);
    let mut path: Vec<crate::egui::Pos2> = Vec::with_capacity(count);
    for (index, point) in graph.points.iter().enumerate().rev() {
        let age = (count - 1 - index) as f32 * seconds_per_point;
        if age > graph.span_seconds {
            break;
        }
        let Some(lufs) = value(point).filter(|v| v.is_finite()) else {
            flush(painter, &mut path, stroke);
            continue;
        };
        path.push(pos2(
            time_x(plot, age, graph.span_seconds),
            loudness_y(plot, lufs, graph.target, &graph.scale),
        ));
    }
    flush(painter, &mut path, stroke);
}

/// Draws `path` as one line and empties it.
fn flush(painter: &Painter, path: &mut Vec<crate::egui::Pos2>, stroke: Stroke) {
    if path.len() >= 2 {
        painter.add(crate::egui::Shape::line(std::mem::take(path), stroke));
    } else {
        path.clear();
    }
}
