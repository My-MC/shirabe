//! The Lissajous capture fed by the real measurement engine.

use std::sync::Arc;

use shirabe_dsp::scope::mid_side;
use shirabe_plugin::engine::MeterEngine;
use shirabe_plugin::shared::SharedMeters;
use shirabe_plugin::ShirabeParams;
use shirabe_ui::scope::MAX_POINTS;

const RATE: f32 = 48_000.0;
const AMPLITUDE: f32 = 0.5;

fn engine(channels: usize) -> (MeterEngine, Arc<SharedMeters>) {
    let params = Arc::new(ShirabeParams::default());
    let shared = Arc::new(SharedMeters::new());
    let mut engine = MeterEngine::new(params, shared.clone());
    engine.prepare(RATE, channels, 0);
    (engine, shared)
}

/// Runs `blocks` blocks of a 1 kHz sine; `right_phase` is added to the right channel in
/// cycles (0.0 = in phase, 0.5 = anti-phase, 0.25 = quadrature).
fn run(engine: &mut MeterEngine, phase: f32, blocks: usize) {
    let frames = 512;
    let mut first = 0;
    for _ in 0..blocks {
        let mut left = vec![0.0; frames];
        let mut right = vec![0.0; frames];
        for i in 0..frames {
            let t = (first + i) as f32 / RATE;
            let angle = 2.0 * std::f32::consts::PI * 1000.0 * t;
            left[i] = AMPLITUDE * angle.sin();
            right[i] = AMPLITUDE * (angle + 2.0 * std::f32::consts::PI * phase).sin();
        }
        first += frames;
        let mut slices: [&mut [f32]; 2] = [&mut left, &mut right];
        engine.process(&mut slices, &mut [], false);
    }
}

fn figure(shared: &SharedMeters) -> Vec<(f32, f32)> {
    let mut buffer = [(0.0f32, 0.0f32); MAX_POINTS];
    let count = shared.scope.read_latest(&mut buffer);
    buffer[..count]
        .iter()
        .map(|&(l, r)| mid_side(l, r))
        .collect()
}

#[test]
fn in_phase_stereo_draws_a_vertical_line() {
    let (mut engine, shared) = engine(2);
    run(&mut engine, 0.0, 8);
    let points = figure(&shared);
    assert_eq!(points.len(), MAX_POINTS);
    assert!(points.iter().all(|(_, side)| side.abs() < 1e-4));
    let peak = points
        .iter()
        .map(|(mid, _)| *mid)
        .fold(0.0f32, |a, b| a.max(b.abs()));
    assert!((peak - AMPLITUDE).abs() < 0.01, "{peak}");
}

#[test]
fn anti_phase_stereo_draws_a_horizontal_line() {
    let (mut engine, shared) = engine(2);
    run(&mut engine, 0.5, 8);
    let points = figure(&shared);
    assert!(points.iter().all(|(mid, _)| mid.abs() < 1e-4));
    let peak = points
        .iter()
        .map(|(_, side)| *side)
        .fold(0.0f32, |a, b| a.max(b.abs()));
    assert!((peak - AMPLITUDE).abs() < 0.01, "{peak}");
}

#[test]
fn quadrature_stereo_draws_a_circle() {
    let (mut engine, shared) = engine(2);
    run(&mut engine, 0.25, 8);
    let points = figure(&shared);
    let radius = AMPLITUDE / std::f32::consts::SQRT_2;
    for (mid, side) in &points {
        let r = (mid * mid + side * side).sqrt();
        assert!((r - radius).abs() < 1e-3, "{r} vs {radius}");
    }
    // The trace covers the whole circle, not just one arc.
    let max_side = points.iter().map(|(_, s)| *s).fold(f32::MIN, f32::max);
    let min_side = points.iter().map(|(_, s)| *s).fold(f32::MAX, f32::min);
    assert!(max_side > 0.9 * radius && min_side < -0.9 * radius);
}

#[test]
fn silence_sits_at_the_centre() {
    let (mut engine, shared) = engine(2);
    let mut left = [0.0f32; 256];
    let mut right = [0.0f32; 256];
    let mut slices: [&mut [f32]; 2] = [&mut left, &mut right];
    engine.process(&mut slices, &mut [], false);
    let points = figure(&shared);
    assert!(!points.is_empty());
    assert!(points.iter().all(|&(m, s)| m == 0.0 && s == 0.0));
}

#[test]
fn a_mono_input_publishes_no_stereo_figure() {
    let (mut engine, shared) = engine(1);
    let mut mono = vec![0.0f32; 512];
    for (i, sample) in mono.iter_mut().enumerate() {
        *sample = 0.5 * (i as f32 * 0.1).sin();
    }
    let mut slices: [&mut [f32]; 1] = [&mut mono];
    engine.process(&mut slices, &mut [], false);
    assert_eq!(shared.active_channels(), 1);
    assert!(figure(&shared).is_empty());
}

#[test]
fn switching_to_mono_clears_the_previous_stereo_figure() {
    let (mut engine, shared) = engine(2);
    run(&mut engine, 0.0, 4);
    assert!(!figure(&shared).is_empty());
    engine.prepare(RATE, 1, 0);
    let mut mono = [0.0f32; 64];
    let mut slices: [&mut [f32]; 1] = [&mut mono];
    engine.process(&mut slices, &mut [], false);
    assert!(figure(&shared).is_empty());
}

use shirabe_plugin::editor::ScopeView;
use shirabe_ui::scope::{ScopePlane, TRAIL_SECONDS};

#[test]
fn the_vectorscope_puts_each_channel_on_its_physical_axis() {
    let (_, shared) = engine(2);
    shared.scope.push(&[0.5; 64], &[0.25; 64]);
    let mut view = ScopeView::new();
    view.update(&shared, 2, 0.0, true, false, ScopePlane::LeftRight);
    let (x, y) = *view.points().last().expect("the pushed frames are shown");
    assert!((x - 0.5).abs() < 1e-6, "left belongs on x: {x}");
    assert!((y - 0.25).abs() < 1e-6, "right belongs on y: {y}");

    let mut mid_side_view = ScopeView::new();
    mid_side_view.update(&shared, 2, 0.0, true, false, ScopePlane::MidSide);
    let (x, y) = *mid_side_view.points().last().unwrap();
    let (mid, side) = mid_side(0.5, 0.25);
    assert!((x - side).abs() < 1e-6 && (y - mid).abs() < 1e-6);
}

#[test]
fn switching_the_plane_discards_the_history_of_the_other_view() {
    let (mut engine, shared) = engine(2);
    run(&mut engine, 0.0, 8);
    let mut view = ScopeView::new();
    view.update(&shared, 2, 0.0, true, false, ScopePlane::MidSide);
    // In-phase stereo: a vertical line in mid/side, a 45° diagonal in L/R.
    assert!(view.points().iter().all(|(x, _)| x.abs() < 1e-4));
    let before = view.trail().len();
    assert!(before > 0);

    view.update(&shared, 2, 1.0 / 60.0, true, false, ScopePlane::LeftRight);
    assert!(!view.trail().is_empty());
    assert!(
        view.trail().iter().all(|p| (p.x - p.y).abs() < 1e-4),
        "the trail must hold left/right coordinates only"
    );
    assert!(view.points().iter().all(|(x, y)| (x - y).abs() < 1e-4));
}

#[test]
fn switching_the_plane_restarts_the_automatic_gain() {
    let (mut engine, shared) = engine(2);
    let mut view = ScopeView::new();
    let mut now = 0.0;
    for _ in 0..240 {
        run(&mut engine, 0.0, 1);
        now += 1.0 / 60.0;
        view.update(&shared, 2, now, true, true, ScopePlane::MidSide);
    }
    let applied = view.gain_db();
    assert!(applied > 3.0, "{applied}");

    view.update(
        &shared,
        2,
        now + 1.0 / 60.0,
        true,
        true,
        ScopePlane::LeftRight,
    );
    assert!(
        view.gain_db() < 1.0,
        "the gain restarted at {}, not {}",
        view.gain_db(),
        applied
    );
}

#[test]
fn the_vectorscope_flags_a_channel_past_full_scale_that_mid_side_fits() {
    let (_, shared) = engine(2);
    let mut view = ScopeView::new();
    // L = 1.25, R = 0: M and S are both 0.625, inside the fixed mid/side range.
    shared.scope.push(&[1.25], &[0.0]);
    view.update(&shared, 2, 0.0, true, false, ScopePlane::MidSide);
    assert!(!view.clipped());

    shared.scope.clear();
    shared.scope.push(&[1.25], &[0.0]);
    view.update(&shared, 2, 0.1, true, false, ScopePlane::LeftRight);
    assert!(view.clipped(), "left exceeded full scale");
}

#[test]
fn the_vectorscope_shows_no_figure_for_mono_input() {
    let (mut engine, shared) = engine(1);
    let mut mono = [0.25f32; 128];
    let mut slices: [&mut [f32]; 1] = [&mut mono];
    engine.process(&mut slices, &mut [], false);
    let mut view = ScopeView::new();
    view.update(&shared, 1, 0.0, true, false, ScopePlane::LeftRight);
    assert!(view.mono());
    assert!(view.points().is_empty());
    assert!(view.trail().is_empty());
}

#[test]
fn the_vectorscope_view_does_not_change_the_audio_or_the_meters() {
    let (mut viewed, viewed_shared) = engine(2);
    let (mut control, control_shared) = engine(2);
    let mut view = ScopeView::new();
    let mut now = 0.0;
    for block in 0..16 {
        let frames = 256;
        let mut left = vec![0.0f32; frames];
        let mut right = vec![0.0f32; frames];
        for i in 0..frames {
            let t = (block * frames + i) as f32 / RATE;
            let angle = 2.0 * std::f32::consts::PI * 1000.0 * t;
            left[i] = AMPLITUDE * angle.sin();
            right[i] = 0.25 * (angle + std::f32::consts::FRAC_PI_2).sin();
        }
        let mut control_left = left.clone();
        let mut control_right = right.clone();
        {
            let mut slices: [&mut [f32]; 2] = [&mut left, &mut right];
            viewed.process(&mut slices, &mut [], false);
        }
        {
            let mut slices: [&mut [f32]; 2] = [&mut control_left, &mut control_right];
            control.process(&mut slices, &mut [], false);
        }
        assert_eq!(left, control_left);
        assert_eq!(right, control_right);

        now += 1.0 / 60.0;
        view.update(&viewed_shared, 2, now, true, true, ScopePlane::LeftRight);
    }
    assert!(!view.points().is_empty());
    for channel in 0..2 {
        assert_eq!(
            viewed_shared
                .channel(channel)
                .level_db
                .load(std::sync::atomic::Ordering::Relaxed),
            control_shared
                .channel(channel)
                .level_db
                .load(std::sync::atomic::Ordering::Relaxed)
        );
        assert_eq!(
            viewed_shared
                .channel(channel)
                .vu
                .load(std::sync::atomic::Ordering::Relaxed),
            control_shared
                .channel(channel)
                .vu
                .load(std::sync::atomic::Ordering::Relaxed)
        );
    }
    assert_eq!(
        viewed_shared.loudness.momentary.load(),
        control_shared.loudness.momentary.load()
    );
    assert_eq!(
        viewed_shared.loudness.short_term.load(),
        control_shared.loudness.short_term.load()
    );
}

#[test]
fn a_paused_source_does_not_repeat_the_same_trace_into_the_trail() {
    let (mut engine, shared) = engine(2);
    run(&mut engine, 0.25, 8);
    let mut view = ScopeView::new();
    view.update(&shared, 2, 0.0, true, false, ScopePlane::MidSide);
    assert!(!view.trail().is_empty());
    let frozen = view.points().to_vec();
    assert!(!frozen.is_empty());
    let after_audio = view.trail().len();

    // No new audio blocks arrive (the host is paused), so the same captured frames must not
    // be appended again.
    view.update(&shared, 2, 2.0 / 60.0, true, false, ScopePlane::MidSide);
    assert_eq!(view.new_samples(), 0);
    assert_eq!(view.points(), frozen);
    assert_eq!(view.trail().len(), after_audio);

    // The trail fades out; the latest trace stays visible.
    view.update(
        &shared,
        2,
        TRAIL_SECONDS as f64 + 1.0,
        true,
        false,
        ScopePlane::MidSide,
    );
    assert!(view.trail().is_empty());
    assert_eq!(view.points(), frozen);
}

#[test]
fn clearing_the_capture_resets_the_trail_and_the_auto_gain() {
    let (mut engine, shared) = engine(2);
    run(&mut engine, 0.0, 8);
    let mut view = ScopeView::new();
    view.update(&shared, 2, 0.0, true, true, ScopePlane::MidSide);
    assert!(!view.trail().is_empty());

    shared.scope.clear();
    view.update(&shared, 2, 1.0 / 60.0, true, true, ScopePlane::MidSide);
    assert!(view.trail().is_empty());
    assert_eq!(view.gain_db(), 0.0);
}

#[test]
fn switching_to_mono_clears_the_trail() {
    let (mut engine, shared) = engine(2);
    run(&mut engine, 0.0, 4);
    let mut view = ScopeView::new();
    view.update(&shared, 2, 0.0, true, false, ScopePlane::MidSide);
    assert!(!view.trail().is_empty());

    engine.prepare(RATE, 1, 0);
    view.update(&shared, 1, 1.0 / 60.0, true, false, ScopePlane::MidSide);
    assert!(view.trail().is_empty());
    assert!(view.mono());
}

#[test]
fn fixed_scale_keeps_the_unity_gain_and_the_raw_peak() {
    let (mut engine, shared) = engine(2);
    run(&mut engine, 0.0, 8);
    let mut view = ScopeView::new();
    view.update(&shared, 2, 0.0, false, false, ScopePlane::MidSide);
    assert_eq!(view.gain_db(), 0.0);
    let peak = view
        .points()
        .iter()
        .map(|(x, y)| x.abs().max(y.abs()))
        .fold(0.0f32, f32::max);
    assert!((peak - AMPLITUDE).abs() < 0.01, "{peak}");
}

#[test]
fn automatic_gain_lifts_the_displayed_figure_without_touching_the_meter_readings() {
    let (mut control, control_shared) = engine(2);
    let (mut engine, shared) = engine(2);
    run(&mut engine, 0.0, 8);
    run(&mut control, 0.0, 8);
    let mut view = ScopeView::new();
    let mut now = 0.0;
    view.update(&shared, 2, now, true, true, ScopePlane::MidSide);
    for _ in 0..240 {
        run(&mut engine, 0.0, 1);
        run(&mut control, 0.0, 1);
        now += 1.0 / 60.0;
        view.update(&shared, 2, now, true, true, ScopePlane::MidSide);
    }
    assert!(view.gain_db() > 3.0, "{}", view.gain_db());
    let peak = view
        .points()
        .iter()
        .map(|(x, y)| x.abs().max(y.abs()))
        .fold(0.0f32, f32::max);
    assert!(peak > AMPLITUDE, "the display was not scaled: {peak}");

    // The same audio through an engine with no scope view at all must measure identically.
    for channel in 0..2 {
        assert_eq!(
            shared
                .channel(channel)
                .level_db
                .load(std::sync::atomic::Ordering::Relaxed),
            control_shared
                .channel(channel)
                .level_db
                .load(std::sync::atomic::Ordering::Relaxed)
        );
    }
    assert_eq!(
        shared.loudness.momentary.load(),
        control_shared.loudness.momentary.load()
    );
}

#[test]
fn automatic_gain_holds_the_scale_when_audio_stops() {
    let (mut engine, shared) = engine(2);
    let mut view = ScopeView::new();
    let mut now = 0.0;
    for _ in 0..120 {
        run(&mut engine, 0.0, 1);
        now += 1.0 / 60.0;
        view.update(&shared, 2, now, true, true, ScopePlane::MidSide);
    }
    let applied = view.gain_db();
    assert!(applied > 0.0, "{applied}");
    // Paused: no new frames, so the gain must stay put and be shown as held.
    now += 1.0;
    view.update(&shared, 2, now, true, true, ScopePlane::MidSide);
    assert_eq!(view.gain_db(), applied);
    assert!(view.holding());
}

#[test]
fn switching_back_to_the_fixed_scale_returns_unity_gain() {
    let (mut engine, shared) = engine(2);
    let mut view = ScopeView::new();
    let mut now = 0.0;
    for _ in 0..120 {
        run(&mut engine, 0.0, 1);
        now += 1.0 / 60.0;
        view.update(&shared, 2, now, true, true, ScopePlane::MidSide);
    }
    assert!(view.gain_db() > 0.0);

    now += 1.0 / 60.0;
    view.update(&shared, 2, now, true, false, ScopePlane::MidSide);
    assert_eq!(view.gain_db(), 0.0);
    assert!(!view.holding());
}

#[test]
fn turning_the_trail_off_stops_appending_history() {
    let (mut engine, shared) = engine(2);
    run(&mut engine, 0.25, 8);
    let mut view = ScopeView::new();
    view.update(&shared, 2, 0.0, true, false, ScopePlane::MidSide);
    assert!(!view.trail().is_empty());

    run(&mut engine, 0.25, 4);
    view.update(&shared, 2, 1.0 / 60.0, false, false, ScopePlane::MidSide);
    assert!(view.trail().is_empty());
    assert!(!view.points().is_empty());
}

#[test]
fn display_gain_does_not_report_a_legal_signal_as_clipped() {
    let (_, shared) = engine(2);
    let mut view = ScopeView::new();
    shared.scope.push(&[0.01; 256], &[0.01; 256]);
    view.update(&shared, 2, 0.0, true, true, ScopePlane::MidSide);
    for step in 1..60 {
        shared.scope.push(&[0.01; 256], &[0.01; 256]);
        view.update(
            &shared,
            2,
            f64::from(step) / 60.0,
            true,
            true,
            ScopePlane::MidSide,
        );
    }
    shared.scope.push(&[0.5; 256], &[0.5; 256]);
    view.update(&shared, 2, 1.0, true, true, ScopePlane::MidSide);
    assert!(
        shirabe_ui::scope::is_clipped(view.points()),
        "probe must exceed display bounds"
    );
    assert!(!view.clipped());
    shared.scope.push(&[1.25; 256], &[1.25; 256]);
    view.update(&shared, 2, 1.02, true, true, ScopePlane::MidSide);
    assert!(view.clipped());
}
