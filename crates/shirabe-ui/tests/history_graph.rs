//! Geometry and drawing of the loudness time-history graph.
//!
//! The graph maps measured time to the horizontal axis (newest at the right edge) and
//! loudness relative to the target to the vertical axis, using the loudness scale of the
//! bars. A tile too small for a graph draws its heading only, so a compact window keeps the
//! numeric read-out intact instead of squeezing the graph.

use shirabe_ui::egui::{self, pos2, vec2, LayerId, Rect};
use shirabe_ui::history::{
    draw, loudness_y, plot_rect, time_x, Graph, Point, AXIS_HEIGHT, MIN_PLOT_HEIGHT,
    MIN_PLOT_WIDTH, TITLE_HEIGHT,
};
use shirabe_ui::i18n::{Lang, Text};
use shirabe_ui::MeterScale;
use shirabe_ui::Theme;

fn area(width: f32, height: f32) -> Rect {
    Rect::from_min_size(pos2(0.0, 0.0), vec2(width, height))
}

fn graph(points: &[Point]) -> Graph<'_> {
    Graph {
        points,
        points_per_second: 10.0,
        span_seconds: 180.0,
        target: -23.0,
        tolerance: Some(1.0),
        scale: MeterScale::LOUDNESS_EBU_9,
        label_offset: -23.0,
        paused: false,
        text: Text::new(Lang::English, true),
    }
}

#[test]
fn a_tile_too_small_has_no_plot() {
    assert!(plot_rect(area(300.0, 70.0)).is_none(), "too short");
    assert!(plot_rect(area(80.0, 200.0)).is_none(), "too narrow");
    let plot = plot_rect(area(300.0, 140.0)).expect("a usable tile");
    assert!(plot.width() >= MIN_PLOT_WIDTH);
    assert!(plot.height() >= MIN_PLOT_HEIGHT);
    // The heading, the labels and the time axis keep their space.
    assert!((plot.top() - TITLE_HEIGHT).abs() < 0.01);
    assert!((140.0 - plot.bottom() - AXIS_HEIGHT).abs() < 0.01);
}

#[test]
fn the_newest_point_sits_on_the_right_edge() {
    let plot = plot_rect(area(300.0, 140.0)).expect("plot");
    assert!((time_x(plot, 0.0, 180.0) - plot.right()).abs() < 0.01);
    assert!((time_x(plot, 180.0, 180.0) - plot.left()).abs() < 0.01);
    assert!((time_x(plot, 90.0, 180.0) - plot.center().x).abs() < 0.01);
    // Past the span the line stops at the left edge instead of wrapping.
    assert!((time_x(plot, 400.0, 180.0) - plot.left()).abs() < 0.01);
}

#[test]
fn louder_values_are_drawn_higher_and_the_target_is_the_reference() {
    let plot = plot_rect(area(300.0, 140.0)).expect("plot");
    let target = -23.0;
    let scale = MeterScale::LOUDNESS_EBU_9;
    let at_target = loudness_y(plot, target, target, &scale);
    assert!((at_target - (plot.bottom() - scale.position(0.0) * plot.height())).abs() < 0.01);
    let louder = loudness_y(plot, -20.0, target, &scale);
    let quieter = loudness_y(plot, -26.0, target, &scale);
    assert!(louder < at_target && at_target < quieter);
    // Values past the scale stop at the border instead of leaving the tile.
    assert!((loudness_y(plot, 0.0, target, &scale) - plot.top()).abs() < 0.01);
    assert!((loudness_y(plot, -90.0, target, &scale) - plot.bottom()).abs() < 0.01);
}

/// Runs `draw` in a frame and returns the labels it painted.
fn labels(rect: Rect, graph: &Graph<'_>) -> Vec<String> {
    let theme = Theme::dark();
    let ctx = egui::Context::default();
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        },
        |ctx| {
            let painter = ctx.layer_painter(LayerId::background());
            draw(&painter, rect, graph, &theme);
        },
    );
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_graph_names_both_curves_its_span_and_the_target_window() {
    let points: Vec<Point> = (0..300)
        .map(|i| Point::new(Some(-23.0 + (i % 6) as f32), Some(-24.0)))
        .collect();
    let rect = area(320.0, 180.0);
    let painted = labels(rect, &graph(&points));
    for expected in ["LOUDNESS HISTORY", "3 min", "s ago", "M", "S"] {
        assert!(
            painted.iter().any(|l| l == expected),
            "{expected}: {painted:?}"
        );
    }
    // The y labels are absolute loudness around the target, and the time axis is labelled
    // in seconds before now.
    assert!(painted.iter().any(|l| l == "-14"), "{painted:?}");
    assert!(painted.iter().any(|l| l == "-60"), "{painted:?}");
}

#[test]
fn an_empty_history_says_so_instead_of_drawing_a_flat_line() {
    let rect = area(320.0, 180.0);
    let painted = labels(rect, &graph(&[]));
    assert!(
        painted.iter().any(|l| l == "waiting for audio"),
        "{painted:?}"
    );
    let japanese = Graph {
        text: Text::new(Lang::Japanese, true),
        ..graph(&[])
    };
    let painted = labels(rect, &japanese);
    assert!(
        painted.iter().any(|l| l == "音声を待っています"),
        "{painted:?}"
    );
    assert!(painted.iter().any(|l| l == "ラウドネス履歴"), "{painted:?}");
}

#[test]
fn a_paused_graph_says_so() {
    let points = [Point::new(Some(-23.0), Some(-23.0)); 4];
    let paused = Graph {
        paused: true,
        ..graph(&points)
    };
    let painted = labels(area(320.0, 180.0), &paused);
    assert!(
        painted.iter().any(|l| l == "LOUDNESS HISTORY  PAUSED"),
        "{painted:?}"
    );
}

#[test]
fn a_compact_tile_draws_the_heading_only() {
    let points = [Point::new(Some(-23.0), Some(-23.0)); 4];
    let rect = area(300.0, 60.0);
    assert!(plot_rect(rect).is_none());
    let painted = labels(rect, &graph(&points));
    assert_eq!(painted, ["LOUDNESS HISTORY", "3 min"], "{painted:?}");
}

#[test]
fn a_curve_is_only_drawn_where_values_exist() {
    // Silence (absent values) must leave a gap rather than a level at the bottom of the
    // scale, so each run between gaps becomes its own path.
    let mut points = vec![Point::new(Some(-24.0), Some(-24.0)); 40];
    for point in &mut points[18..22] {
        *point = Point::new(None, Some(-24.0));
    }
    let theme = Theme::dark();
    let rect = area(320.0, 180.0);
    let ctx = egui::Context::default();
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        },
        |ctx| {
            let painter = ctx.layer_painter(LayerId::background());
            draw(&painter, rect, &graph(&points), &theme);
        },
    );
    let moment = shirabe_ui::history::momentary_colour();
    let momentary_paths = output
        .shapes
        .iter()
        .filter(|shape| match &shape.shape {
            egui::Shape::Path(path) => path.stroke.color == egui::epaint::ColorMode::Solid(moment),
            _ => false,
        })
        .count();
    assert_eq!(
        momentary_paths, 2,
        "the missing points must split the curve"
    );
    // The short-term curve has no gaps at all: one path, drawn under the momentary one.
    let short_term_paths = output
        .shapes
        .iter()
        .filter(|shape| match &shape.shape {
            egui::Shape::Path(path) => {
                path.stroke.color == egui::epaint::ColorMode::Solid(theme.text)
            }
            _ => false,
        })
        .count();
    assert_eq!(short_term_paths, 1);
}
