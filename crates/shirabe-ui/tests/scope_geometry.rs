//! Geometry of the scope display: plane coordinates mapped onto a square.

use shirabe_ui::egui::{pos2, vec2, Rect};
use shirabe_ui::scope::{is_clipped, ScopeGeometry};

fn square(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(pos2(x, y), vec2(w, h))
}

#[test]
fn the_centre_of_the_square_is_the_origin() {
    let geometry = ScopeGeometry::new(square(0.0, 0.0, 100.0, 100.0));
    assert_eq!(geometry.point(0.0, 0.0), pos2(50.0, 50.0));
}

#[test]
fn the_vertical_axis_is_up_and_the_horizontal_axis_is_right() {
    let geometry = ScopeGeometry::new(square(0.0, 0.0, 100.0, 100.0));
    assert_eq!(geometry.point(0.0, 1.0), pos2(50.0, 0.0));
    assert_eq!(geometry.point(0.0, -1.0), pos2(50.0, 100.0));
    assert_eq!(geometry.point(1.0, 0.0), pos2(100.0, 50.0));
    assert_eq!(geometry.point(-1.0, 0.0), pos2(0.0, 50.0));
}

#[test]
fn the_square_is_centred_in_a_wide_rectangle() {
    let geometry = ScopeGeometry::new(square(0.0, 0.0, 300.0, 100.0));
    let square = geometry.square();
    assert_eq!(square.width(), 100.0);
    assert_eq!(square.height(), 100.0);
    assert_eq!(square.left(), 100.0);
    assert_eq!(square.top(), 0.0);
    assert_eq!(geometry.point(1.0, 1.0), pos2(200.0, 0.0));
}

#[test]
fn values_beyond_full_scale_are_drawn_on_the_border() {
    let geometry = ScopeGeometry::new(square(0.0, 0.0, 100.0, 100.0));
    assert_eq!(geometry.point(0.0, 2.0), pos2(50.0, 0.0));
    assert_eq!(geometry.point(-3.0, 0.0), pos2(0.0, 50.0));
}

#[test]
fn values_inside_full_scale_are_not_clipped() {
    assert!(!is_clipped(&[(0.0, 0.0), (1.0, -1.0), (-1.0, 1.0)]));
}

#[test]
fn a_value_beyond_full_scale_is_clipped() {
    assert!(is_clipped(&[(0.0, 0.0), (1.001, 0.0)]));
    assert!(is_clipped(&[(0.0, -1.5)]));
}

#[test]
fn non_finite_points_are_ignored_rather_than_flagged_as_clipping() {
    assert!(!is_clipped(&[(f32::NAN, 0.0), (0.0, f32::INFINITY)]));
}
