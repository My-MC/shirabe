//! Time-based persistence and automatic gain of the Lissajous display.
//!
//! Both are display-only: the points fed here are mid/side coordinates, and nothing in this
//! file changes a measured value.

use shirabe_ui::scope::{alpha_at, scale_label, AutoGain, Trail, TRAIL_CAPACITY, TRAIL_SECONDS};

/// Runs `seconds` of updates with a steady mid/side peak, at 60 updates per second.
fn hold(gain: &mut AutoGain, peak: f32, seconds: f32) {
    let dt = 1.0 / 60.0;
    let steps = (seconds / dt).round() as usize;
    for _ in 0..steps {
        gain.update(&[(peak, peak)], dt);
    }
}

#[test]
fn a_new_auto_gain_starts_at_unity_and_is_holding() {
    let gain = AutoGain::new();
    assert_eq!(gain.gain_db(), 0.0);
    assert!(gain.is_holding());
}

#[test]
fn a_full_scale_signal_keeps_unity_gain() {
    let mut gain = AutoGain::new();
    hold(&mut gain, 1.0, 2.0);
    assert!(gain.gain_db().abs() < 0.1, "{}", gain.gain_db());
    assert!(!gain.is_holding());
}

#[test]
fn a_quiet_signal_is_lifted_towards_the_target() {
    let mut gain = AutoGain::new();
    // A peak of 0.1 is -20 dBFS; the display leaves 1 dB of headroom, so the target is +19 dB.
    hold(&mut gain, 0.1, 4.0);
    assert!((gain.gain_db() - 19.0).abs() < 0.5, "{}", gain.gain_db());
    assert!(!gain.is_holding());
}

#[test]
fn auto_gain_never_boosts_more_than_twenty_db() {
    let mut gain = AutoGain::new();
    hold(&mut gain, 0.002, 6.0);
    assert!(gain.gain_db() <= 20.0 + 1e-4);
    assert!(gain.gain_db() > 19.0);
}

#[test]
fn silence_never_raises_the_gain() {
    let mut gain = AutoGain::new();
    hold(&mut gain, 0.0, 6.0);
    assert_eq!(gain.gain_db(), 0.0);
    assert!(gain.is_holding());
}

#[test]
fn a_signal_below_the_noise_floor_never_raises_the_gain() {
    let mut gain = AutoGain::new();
    // -80 dBFS is below the -60 dBFS floor used for automatic gain.
    hold(&mut gain, 0.0001, 6.0);
    assert_eq!(gain.gain_db(), 0.0);
    assert!(gain.is_holding());
}

#[test]
fn the_last_gain_is_held_when_the_signal_falls_silent() {
    let mut gain = AutoGain::new();
    hold(&mut gain, 0.01, 4.0);
    let applied = gain.gain_db();
    assert!(applied > 10.0, "{applied}");
    hold(&mut gain, 0.0, 3.0);
    assert_eq!(gain.gain_db(), applied);
    assert!(gain.is_holding());
}

#[test]
fn a_steady_tone_settles_without_oscillating() {
    let mut gain = AutoGain::new();
    hold(&mut gain, 0.2, 8.0);
    let mut min = f32::MAX;
    let mut max = f32::MIN;
    let dt = 1.0 / 60.0;
    for _ in 0..120 {
        gain.update(&[(0.2, 0.2)], dt);
        min = min.min(gain.gain_db());
        max = max.max(gain.gain_db());
    }
    assert!(max - min < 0.1, "gain moved by {} dB", max - min);
}

#[test]
fn louder_input_lowers_the_gain_faster_than_quiet_input_raises_it() {
    let mut quiet = AutoGain::new();
    hold(&mut quiet, 0.1, 4.0);
    let lifted = quiet.gain_db();
    quiet.update(&[(1.0, 1.0)], 0.1);
    let fell = lifted - quiet.gain_db();

    let mut loud = AutoGain::new();
    hold(&mut loud, 0.5, 2.0);
    let start = loud.gain_db();
    loud.update(&[(0.05, 0.05)], 0.1);
    let rose = loud.gain_db() - start;

    assert!(fell > rose, "fell {fell} dB, rose {rose} dB");
}

#[test]
fn non_finite_samples_are_ignored() {
    let mut gain = AutoGain::new();
    gain.update(&[(f32::NAN, 0.0)], 0.1);
    gain.update(&[(0.0, f32::INFINITY)], 0.1);
    assert_eq!(gain.gain_db(), 0.0);
    assert!(gain.is_holding());
}

#[test]
fn the_trail_keeps_recent_points_for_the_configured_time() {
    let mut trail = Trail::new();
    trail.push(0.1, 0.2, 10.0);
    trail.push(0.3, 0.4, 10.1);
    trail.prune(10.29, TRAIL_SECONDS);
    assert_eq!(trail.len(), 2);
    trail.prune(10.1 + TRAIL_SECONDS as f64 + 0.01, TRAIL_SECONDS);
    assert_eq!(trail.len(), 0);
}

#[test]
fn the_trail_does_not_grow_when_no_new_points_arrive() {
    let mut trail = Trail::new();
    for i in 0..10 {
        trail.push(0.0, 0.0, 20.0 + i as f64 * 0.01);
    }
    let len = trail.len();
    let mut now = 20.1;
    for _ in 0..30 {
        now += 1.0 / 60.0;
        trail.prune(now, TRAIL_SECONDS);
        assert!(trail.len() <= len);
    }
    assert!(
        trail.len() < len,
        "the trail should fade without new points"
    );
}

#[test]
fn the_trail_is_bounded_in_size() {
    let mut trail = Trail::new();
    for i in 0..TRAIL_CAPACITY * 2 {
        trail.push(0.0, 0.0, i as f64 * 1e-6);
    }
    assert_eq!(trail.len(), TRAIL_CAPACITY);
}

#[test]
fn clearing_the_trail_removes_every_point() {
    let mut trail = Trail::new();
    trail.push(0.5, -0.5, 1.0);
    trail.clear();
    assert!(trail.is_empty());
}

#[test]
fn trail_intensity_falls_with_age_and_reaches_zero_at_the_limit() {
    let fresh = alpha_at(0.0, 0.3);
    let half = alpha_at(0.15, 0.3);
    let old = alpha_at(0.3, 0.3);
    assert!(fresh > half && half > old);
    assert_eq!(old, 0.0);
    assert_eq!(alpha_at(0.4, 0.3), 0.0);
    assert_eq!(alpha_at(0.1, 0.0), 0.0);
}

#[test]
fn the_scale_label_states_the_scale_in_use() {
    assert_eq!(scale_label(false, 0.0, false, false), "FIXED  ±1");
    assert_eq!(scale_label(false, 12.4, false, false), "FIXED  ±1");
    assert_eq!(scale_label(false, 0.0, false, true), "CLIP  ±1");
    assert_eq!(scale_label(true, 12.4, false, false), "AUTO  +12.4 dB");
    assert_eq!(scale_label(true, 0.04, false, false), "AUTO  +0.0 dB");
    assert_eq!(scale_label(true, 12.4, true, false), "AUTO  +12.4 HOLD");
    assert_eq!(scale_label(true, 12.4, false, true), "CLIP  +12.4 dB");
}
