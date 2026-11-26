//! Orientation of the selectable scope planes.
//!
//! The mid/side Lissajous and the left/right vectorscope plot the same captured frames; only
//! the axes differ. These tests pin down which measured channel goes where, so a sign or axis
//! swap cannot pass as a cosmetic change.

use shirabe_ui::egui::{pos2, vec2, Rect};
use shirabe_ui::scope::{ScopeGeometry, ScopePlane};

fn geometry() -> (ScopeGeometry, shirabe_ui::egui::Pos2) {
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0));
    let geometry = ScopeGeometry::new(rect);
    (geometry, geometry.square().center())
}

#[test]
fn the_vectorscope_puts_left_on_the_horizontal_axis_and_right_on_the_vertical_axis() {
    let (geometry, centre) = geometry();
    let (x, y) = ScopePlane::LeftRight.project(0.5, -0.5);
    let point = geometry.point(x, y);
    // Left positive is to the right; right negative is downwards.
    assert!(point.x > centre.x + 20.0, "{}", point.x);
    assert!(point.y > centre.y + 20.0, "{}", point.y);

    let (x, y) = ScopePlane::LeftRight.project(-0.5, 0.5);
    let point = geometry.point(x, y);
    assert!(point.x < centre.x - 20.0, "{}", point.x);
    assert!(point.y < centre.y - 20.0, "{}", point.y);
}

#[test]
fn a_centred_signal_sits_at_the_centre_of_either_plane() {
    for plane in [ScopePlane::MidSide, ScopePlane::LeftRight] {
        let (geometry, centre) = geometry();
        let (x, y) = plane.project(0.0, 0.0);
        assert_eq!(geometry.point(x, y), centre, "{plane:?}");
    }
}

#[test]
fn the_lissajous_plane_keeps_mid_on_the_vertical_axis() {
    let (geometry, centre) = geometry();
    // In-phase stereo: no side component, so the trace is a vertical line.
    let (x, y) = ScopePlane::MidSide.project(0.5, 0.5);
    let point = geometry.point(x, y);
    assert!((point.x - centre.x).abs() < 1e-3, "{}", point.x);
    assert!(point.y < centre.y - 20.0, "{}", point.y);
}

#[test]
fn only_the_vectorscope_prints_physical_axis_labels() {
    assert_eq!(ScopePlane::LeftRight.axis_labels(), Some(("L", "R")));
    assert_eq!(ScopePlane::MidSide.axis_labels(), None);
    assert_eq!(ScopePlane::LeftRight.title(), "VECTORSCOPE  L/R");
    assert_eq!(ScopePlane::MidSide.title(), "LISSAJOUS  M/S");
}
