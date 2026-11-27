//! The stereo scope capture buffer and the mid/side coordinates drawn by the Lissajous view.

use shirabe_dsp::scope::{mid_side, StereoScope, SCOPE_CAPACITY};

fn read(scope: &StereoScope, count: usize) -> Vec<(f32, f32)> {
    let mut out = vec![(0.0, 0.0); count];
    let n = scope.read_latest(&mut out);
    out.truncate(n);
    out
}

#[test]
fn an_untouched_scope_has_no_points() {
    let scope = StereoScope::new();
    assert_eq!(read(&scope, 8).len(), 0);
}

#[test]
fn points_are_read_in_the_order_they_were_written() {
    let scope = StereoScope::new();
    scope.push(&[0.0, 1.0, 2.0], &[10.0, 11.0, 12.0]);
    assert_eq!(read(&scope, 8), vec![(0.0, 10.0), (1.0, 11.0), (2.0, 12.0)]);
}

#[test]
fn a_short_read_returns_only_the_newest_points() {
    let scope = StereoScope::new();
    scope.push(&[0.0, 1.0, 2.0, 3.0], &[-0.0, -1.0, -2.0, -3.0]);
    assert_eq!(read(&scope, 2), vec![(2.0, -2.0), (3.0, -3.0)]);
}

#[test]
fn the_buffer_is_bounded_and_drops_the_oldest_points() {
    let scope = StereoScope::new();
    let left: Vec<f32> = (0..SCOPE_CAPACITY + 10).map(|i| i as f32).collect();
    let right: Vec<f32> = left.iter().map(|v| -v).collect();
    scope.push(&left[..SCOPE_CAPACITY], &right[..SCOPE_CAPACITY]);
    scope.push(&left[SCOPE_CAPACITY..], &right[SCOPE_CAPACITY..]);

    let points = read(&scope, SCOPE_CAPACITY + 100);
    assert_eq!(points.len(), SCOPE_CAPACITY);
    assert_eq!(points[0], (10.0, -10.0));
    assert_eq!(
        points[SCOPE_CAPACITY - 1],
        ((SCOPE_CAPACITY + 9) as f32, -((SCOPE_CAPACITY + 9) as f32))
    );
    assert!(points.windows(2).all(|w| w[1].0 > w[0].0));
}

#[test]
fn shorter_of_the_two_channel_slices_is_used() {
    let scope = StereoScope::new();
    scope.push(&[0.25, 0.5], &[0.75]);
    assert_eq!(read(&scope, 8), vec![(0.25, 0.75)]);
    scope.push(&[], &[]);
    assert_eq!(read(&scope, 8).len(), 1);
}

#[test]
fn clearing_removes_the_points() {
    let scope = StereoScope::new();
    scope.push(&[1.0], &[1.0]);
    scope.clear();
    assert_eq!(read(&scope, 8).len(), 0);
}

#[test]
fn in_phase_signal_lies_on_the_mid_axis() {
    for level in [0.5f32, -0.25, 1.0] {
        let (mid, side) = mid_side(level, level);
        assert_eq!(side, 0.0);
        assert_eq!(mid, level);
    }
}

#[test]
fn anti_phase_signal_lies_on_the_side_axis() {
    for level in [0.5f32, -0.25, 1.0] {
        let (mid, side) = mid_side(level, -level);
        assert_eq!(mid, 0.0);
        assert_eq!(side, level);
    }
}

#[test]
fn full_scale_mono_reaches_exactly_the_top_edge() {
    assert_eq!(mid_side(1.0, 1.0), (1.0, 0.0));
    assert_eq!(mid_side(-1.0, -1.0), (-1.0, 0.0));
}

#[test]
fn quadrature_signal_traces_a_circle_of_radius_one_over_root_two() {
    // A full-scale sine and cosine draw a circle; each axis peaks at 1/sqrt(2).
    let radius = std::f32::consts::FRAC_1_SQRT_2;
    for degree in (0..360).step_by(7) {
        let angle = (degree as f32).to_radians();
        let (mid, side) = mid_side(angle.sin(), angle.cos());
        let r = (mid * mid + side * side).sqrt();
        assert!((r - radius).abs() < 1e-6, "{degree} deg: {r}");
        assert!(mid.abs() <= radius + 1e-6 && side.abs() <= radius + 1e-6);
    }
}

#[test]
fn silence_maps_to_the_centre() {
    assert_eq!(mid_side(0.0, 0.0), (0.0, 0.0));
}

#[test]
fn delayed_capture_can_read_a_large_burst_at_an_earlier_cursor() {
    let scope = StereoScope::with_capacity(32768);
    let left: Vec<_> = (0..8192).map(|i| i as f32).collect();
    let right: Vec<_> = left.iter().map(|v| -v).collect();
    scope.push(&left, &right);
    let mut out = [(0.0, 0.0); 2048];
    assert_eq!(scope.read_through(0, 800, &mut out), Some((800, 800)));
    assert_eq!(out[799], (799.0, -799.0));
    assert_eq!(scope.read_through(800, 1600, &mut out), Some((800, 1600)));
    assert_eq!(out[0], (800.0, -800.0));
    assert_eq!(out[799], (1599.0, -1599.0));
}

#[test]
fn read_since_returns_only_frames_written_after_the_cursor() {
    let scope = StereoScope::new();
    scope.push(&[1.0, 2.0], &[-1.0, -2.0]);
    let mut out = [(0.0f32, 0.0f32); 8];
    let (count, cursor) = scope.read_since(0, &mut out);
    assert_eq!(count, 2);
    assert_eq!(&out[..count], &[(1.0, -1.0), (2.0, -2.0)]);
    assert_eq!(cursor, 2);

    scope.push(&[3.0], &[-3.0]);
    let (count, cursor) = scope.read_since(cursor, &mut out);
    assert_eq!(count, 1);
    assert_eq!(&out[..count], &[(3.0, -3.0)]);
    assert_eq!(cursor, 3);
}

#[test]
fn read_since_returns_nothing_when_no_frames_arrived() {
    let scope = StereoScope::new();
    scope.push(&[1.0, 2.0], &[3.0, 4.0]);
    let mut out = [(0.0f32, 0.0f32); 8];
    let (_, cursor) = scope.read_since(0, &mut out);
    let (count, again) = scope.read_since(cursor, &mut out);
    assert_eq!(count, 0);
    assert_eq!(again, cursor);
}

#[test]
fn read_since_drops_frames_the_reader_missed() {
    let scope = StereoScope::new();
    let left: Vec<f32> = (0..SCOPE_CAPACITY as i32 + 10).map(|i| i as f32).collect();
    let right: Vec<f32> = left.iter().map(|v| -v).collect();
    scope.push(&left, &right);
    let mut out = [(0.0f32, 0.0f32); SCOPE_CAPACITY + 100];
    let (count, cursor) = scope.read_since(0, &mut out);
    assert_eq!(count, SCOPE_CAPACITY);
    assert_eq!(cursor, (SCOPE_CAPACITY + 10) as u64);
    // The oldest ten frames have been overwritten; the reader resumes at the oldest kept one.
    assert_eq!(out[0], (10.0, -10.0));
}

#[test]
fn read_since_after_clear_restarts_at_zero() {
    let scope = StereoScope::new();
    scope.push(&[1.0, 2.0, 3.0], &[0.0, 0.0, 0.0]);
    let mut out = [(0.0f32, 0.0f32); 8];
    let (_, cursor) = scope.read_since(0, &mut out);
    scope.clear();
    let (count, restarted) = scope.read_since(cursor, &mut out);
    assert_eq!(count, 0);
    assert_eq!(restarted, 0);
    assert_eq!(scope.written(), 0);
}

#[test]
fn clearing_bumps_the_epoch_so_readers_can_discard_old_points() {
    let scope = StereoScope::new();
    let before = scope.epoch();
    scope.push(&[1.0], &[1.0]);
    scope.clear();
    assert!(scope.epoch() > before);
    scope.push(&[2.0], &[2.0]);
    assert_eq!(scope.written(), 1);
    assert_eq!(scope.epoch(), before + 1);
}

#[test]
fn read_since_resumes_after_a_small_buffer() {
    let scope = StereoScope::new();
    scope.push(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]);
    let mut out = [(0.0f32, 0.0f32); 2];
    let (count, cursor) = scope.read_since(0, &mut out);
    assert_eq!(count, 2);
    assert_eq!(cursor, 2);
    let (count, cursor) = scope.read_since(cursor, &mut out);
    assert_eq!(count, 1);
    assert_eq!(out[0], (3.0, 3.0));
    assert_eq!(cursor, 3);
}
