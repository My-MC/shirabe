//! Bounded loudness history captured for the time-history graph.
//!
//! The history is written by the audio thread and read by the editor. It must keep a fixed
//! cadence whatever the audio block size and must never grow beyond its capacity.

use shirabe_dsp::history::{LoudnessHistory, HISTORY_CAPACITY, HISTORY_INTERVAL_SECONDS};

fn prepared(rate: f32) -> LoudnessHistory {
    let history = LoudnessHistory::new();
    history.prepare(rate);
    history
}

fn read(history: &LoudnessHistory) -> Vec<(f32, f32)> {
    let mut buffer = vec![(f32::NAN, f32::NAN); HISTORY_CAPACITY];
    let count = history.read_recent(&mut buffer);
    buffer[..count].to_vec()
}

#[test]
fn a_point_is_taken_every_100_ms_of_measured_time() {
    assert_eq!(HISTORY_INTERVAL_SECONDS, 0.1);
    // 48 kHz: 4800 frames per point, whatever the block size.
    let history = prepared(48_000.0);
    for block in 0..9 {
        assert_eq!(history.record(480, Some(-20.0), Some(-21.0)), 0);
        assert_eq!(history.written(), 0, "block {block}");
    }
    assert_eq!(history.record(480, Some(-20.0), Some(-21.0)), 1);
    assert_eq!(history.written(), 1);

    // A long block appends every point it covers, so the cadence does not depend on the
    // host's buffer size.
    assert_eq!(history.record(48_000, Some(-20.0), Some(-21.0)), 10);
    assert_eq!(history.written(), 11);
}

#[test]
fn ten_seconds_of_audio_give_one_hundred_points_at_any_block_size() {
    for block in [1_usize, 64, 480, 512, 1024, 4800] {
        let history = prepared(48_000.0);
        let mut frames = 0;
        while frames < 480_000 {
            let n = block.min(480_000 - frames);
            history.record(n, Some(-23.0), Some(-23.0));
            frames += n;
        }
        assert_eq!(history.written(), 100, "block size {block}");
    }
}

#[test]
fn the_cadence_follows_the_sample_rate() {
    let history = prepared(44_100.0);
    assert_eq!(history.record(4_410, Some(-20.0), Some(-20.0)), 1);
    let history = prepared(96_000.0);
    assert_eq!(history.record(9_600, Some(-20.0), Some(-20.0)), 1);
    assert_eq!(history.record(4_800, Some(-20.0), Some(-20.0)), 0);
}

#[test]
fn the_ring_keeps_the_newest_points_and_drops_the_oldest() {
    let history = prepared(48_000.0);
    let extra = 25;
    for index in 0..(HISTORY_CAPACITY + extra) {
        history.record(4_800, Some(-20.0 - index as f32 * 0.01), Some(-30.0));
    }
    assert_eq!(history.written(), (HISTORY_CAPACITY + extra) as u64);
    let points = read(&history);
    assert_eq!(points.len(), HISTORY_CAPACITY);
    // The oldest surviving point is `extra` points into the recording.
    let oldest = points.first().expect("points");
    assert!(
        (oldest.0 - (-20.0 - extra as f32 * 0.01)).abs() < 1e-3,
        "{oldest:?}"
    );
    let newest = points.last().expect("points");
    let expected = -20.0 - (HISTORY_CAPACITY + extra - 1) as f32 * 0.01;
    assert!((newest.0 - expected).abs() < 1e-3, "{newest:?}");
}

#[test]
fn the_history_holds_three_minutes_at_ten_points_per_second() {
    assert_eq!(HISTORY_CAPACITY, 1_800);
    let span = HISTORY_CAPACITY as f32 * HISTORY_INTERVAL_SECONDS as f32;
    assert!((span - 180.0).abs() < 0.001, "{span}");
}

#[test]
fn clearing_restarts_the_recording_and_the_epoch() {
    let history = prepared(48_000.0);
    history.record(48_000, Some(-20.0), Some(-20.0));
    assert_eq!(history.written(), 10);
    let epoch = history.epoch();
    history.clear();
    assert_eq!(history.written(), 0);
    assert_ne!(history.epoch(), epoch);
    assert!(read(&history).is_empty());
    // The partial interval before the clear does not leak into the new recording.
    assert_eq!(history.record(4_799, Some(-20.0), Some(-20.0)), 0);
    assert_eq!(history.record(1, Some(-20.0), Some(-20.0)), 1);
}

#[test]
fn paused_audio_adds_no_points() {
    let history = prepared(48_000.0);
    history.record(48_000, Some(-20.0), Some(-20.0));
    let before = history.written();
    // A paused programme passes no measured frames, so the graph freezes.
    for _ in 0..100 {
        assert_eq!(history.record(0, Some(-20.0), Some(-20.0)), 0);
    }
    assert_eq!(history.written(), before);
    // Resuming continues from the frozen end.
    assert_eq!(history.record(4_800, Some(-20.0), Some(-20.0)), 1);
    assert_eq!(history.written(), before + 1);
}

#[test]
fn absent_values_are_stored_as_absent() {
    let history = prepared(48_000.0);
    history.record(4_800, None, Some(-34.5));
    let points = read(&history);
    assert_eq!(points.len(), 1);
    assert!(points[0].0.is_nan(), "momentary was not available");
    assert!((points[0].1 - (-34.5)).abs() < 1e-6);
}
