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
    engine.prepare(RATE, channels);
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
        engine.process(&mut slices, false);
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
    engine.process(&mut slices, false);
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
    engine.process(&mut slices, false);
    assert_eq!(shared.active_channels(), 1);
    assert!(figure(&shared).is_empty());
}

#[test]
fn switching_to_mono_clears_the_previous_stereo_figure() {
    let (mut engine, shared) = engine(2);
    run(&mut engine, 0.0, 4);
    assert!(!figure(&shared).is_empty());
    engine.prepare(RATE, 1);
    let mut mono = [0.0f32; 64];
    let mut slices: [&mut [f32]; 1] = [&mut mono];
    engine.process(&mut slices, false);
    assert!(figure(&shared).is_empty());
}
