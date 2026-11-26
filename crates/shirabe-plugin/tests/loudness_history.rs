//! Loudness time history fed by the real measurement engine.
//!
//! The capture must follow the programme measurement: it advances while the measurement runs,
//! freezes while it is paused and starts over on a reset, a source change or a sample-rate
//! change. The audio thread must not allocate for it.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use nih_plug::prelude::Param;
use shirabe_dsp::history::HISTORY_CAPACITY;
use shirabe_plugin::engine::MeterEngine;
use shirabe_plugin::shared::SharedMeters;
use shirabe_plugin::ShirabeParams;

const RATE: f32 = 48_000.0;

struct Rig {
    engine: MeterEngine,
    params: Arc<ShirabeParams>,
    shared: Arc<SharedMeters>,
}

fn rig(rate: f32, channels: usize) -> Rig {
    let params = Arc::new(ShirabeParams::default());
    let shared = Arc::new(SharedMeters::new());
    let mut engine = MeterEngine::new(params.clone(), shared.clone());
    engine.prepare(rate, channels, 0);
    Rig {
        engine,
        params,
        shared,
    }
}

/// Runs `frames` frames of silence through the engine in 512-frame blocks.
fn run(rig: &mut Rig, frames: usize) {
    let mut done = 0;
    while done < frames {
        let n = 512.min(frames - done);
        let mut left = vec![0.0f32; n];
        let mut right = vec![0.0f32; n];
        let mut slices: [&mut [f32]; 2] = [&mut left, &mut right];
        rig.engine.process(&mut slices, &mut [], false);
        done += n;
    }
}

fn points(shared: &SharedMeters) -> Vec<(f32, f32)> {
    let mut buffer = vec![(f32::NAN, f32::NAN); HISTORY_CAPACITY];
    let count = shared.history.read_recent(&mut buffer);
    buffer[..count].to_vec()
}

#[test]
fn the_history_advances_with_the_programme_measurement() {
    let mut rig = rig(RATE, 2);
    assert!(points(&rig.shared).is_empty());
    run(&mut rig, 48_000);
    assert_eq!(
        rig.shared.history.written(),
        10,
        "one point per 100 ms of measured time"
    );
    run(&mut rig, 480_000);
    assert_eq!(rig.shared.history.written(), 110);
}

#[test]
fn pausing_freezes_the_history_while_m_and_s_keep_running() {
    let mut rig = rig(RATE, 2);
    run(&mut rig, 96_000);
    assert!(rig.shared.history.written() > 0);
    let chain = vec![0.5f32; 512];
    for _ in 0..20 {
        let mut left = chain.clone();
        let mut right = chain.clone();
        let mut slices: [&mut [f32]; 2] = [&mut left, &mut right];
        rig.engine.process(&mut slices, &mut [], false);
    }
    let live = rig
        .shared
        .loudness
        .momentary
        .load()
        .expect("momentary loudness keeps running");
    let before = rig.shared.history.written();

    rig.shared.set_integrating(false);
    // 1 s of programme time passes, but the recording is paused with the programme.
    run(&mut rig, 48_000);
    assert_eq!(rig.shared.history.written(), before);
    assert!(live > -60.0);

    // Resuming continues from the frozen end without a jump back or a burst.
    rig.shared.set_integrating(true);
    run(&mut rig, 48_000);
    assert_eq!(rig.shared.history.written(), before + 10);
}

#[test]
fn a_reset_clears_the_history() {
    let mut rig = rig(RATE, 2);
    run(&mut rig, 96_000);
    assert!(rig.shared.history.written() > 0);
    let epoch = rig.shared.history.epoch();
    rig.shared.request_reset();
    run(&mut rig, 512);
    assert_eq!(rig.shared.history.written(), 0);
    assert_ne!(rig.shared.history.epoch(), epoch);
    assert!(points(&rig.shared).is_empty());
}

#[test]
fn a_sample_rate_change_clears_the_history() {
    let mut rig = rig(RATE, 2);
    run(&mut rig, 96_000);
    assert!(rig.shared.history.written() > 0);
    rig.engine.prepare(96_000.0, 2, 0);
    assert_eq!(rig.shared.history.written(), 0);
    // The new interval follows the new rate: 9600 frames per point at 96 kHz.
    run(&mut rig, 9_600);
    assert_eq!(rig.shared.history.written(), 1);
}

#[test]
fn a_channel_count_change_clears_the_history() {
    let mut rig = rig(RATE, 2);
    run(&mut rig, 96_000);
    assert!(rig.shared.history.written() > 0);
    rig.engine.prepare(RATE, 1, 0);
    assert_eq!(rig.shared.history.written(), 0);
}

#[test]
fn the_capture_does_not_allocate_on_the_audio_thread() {
    let mut rig = rig(RATE, 2);
    let mut left = vec![0.25f32; 4_800];
    let mut right = vec![0.25f32; 4_800];
    assert_no_alloc::assert_no_alloc(|| {
        let mut slices: [&mut [f32]; 2] = [&mut left, &mut right];
        rig.engine.process(&mut slices, &mut [], false);
    });
    assert_eq!(rig.shared.history.written(), 1);
    // The automatic reset on transport start also runs inside the guard.
    let mut left = vec![0.25f32; 512];
    let mut right = vec![0.25f32; 512];
    assert_no_alloc::assert_no_alloc(|| {
        let mut slices: [&mut [f32]; 2] = [&mut left, &mut right];
        rig.engine.process(&mut slices, &mut [], true);
    });
    assert_eq!(
        rig.shared.history.written(),
        0,
        "the transport start reset the programme"
    );
}

#[test]
fn the_history_records_the_published_loudness_values() {
    let mut rig = rig(RATE, 2);
    run(&mut rig, 96_000);
    let published = rig.shared.loudness.momentary.load().expect("momentary");
    let recorded = points(&rig.shared);
    let last = recorded.last().expect("a point");
    // Silence is measured, not dropped: both the live read-out and the graph hold -inf.
    assert_eq!(last.0, published);
    assert!(last.0 < -60.0, "{last:?}");

    // The oscillator raises the level; the graph must follow it.
    unsafe {
        rig.params.osc_enabled.as_ptr().set_normalized_value(1.0);
        rig.params
            .osc_level
            .as_ptr()
            .set_normalized_value(rig.params.osc_level.preview_normalized(-12.0));
    }
    run(&mut rig, 48_000);
    let recorded = points(&rig.shared);
    let last = *recorded.last().expect("a point");
    assert!(
        (last.0 - (-12.0)).abs() < 1.0,
        "followed the tone: {last:?}"
    );
    assert_eq!(
        rig.shared
            .channel(0)
            .level_db
            .load(Ordering::Relaxed)
            .round(),
        -12.0
    );
}

/// The guard is only meaningful if it is installed in this test binary. It aborts instead of
/// unwinding, so the probe runs in a child process of the same test binary.
#[test]
fn the_allocation_guard_is_installed() {
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "the_allocation_guard_fires",
            "--ignored",
            "--test-threads=1",
        ])
        .output()
        .expect("re-run the test binary");
    assert!(
        !output.status.success(),
        "an allocation inside the guard did not fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("memory allocation"), "{stderr}");
}

/// Only run by [`the_allocation_guard_is_installed`]; allocating here aborts the process.
#[test]
#[ignore = "aborts on purpose; started by the_allocation_guard_is_installed"]
fn the_allocation_guard_fires() {
    assert_no_alloc::assert_no_alloc(|| {
        let _ = vec![0u8; 1024];
    });
}
