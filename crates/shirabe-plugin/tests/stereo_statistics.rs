//! Stereo correlation and balance fed by the real measurement engine.

use std::sync::Arc;

use nih_plug::prelude::Param;
use shirabe_plugin::engine::MeterEngine;
use shirabe_plugin::shared::SharedMeters;
use shirabe_plugin::ShirabeParams;

const RATE: f32 = 48_000.0;
const AMPLITUDE: f32 = 0.5;
/// One 400 ms measurement window at 48 kHz.
const FULL: usize = 19_200;

struct Rig {
    engine: MeterEngine,
    params: Arc<ShirabeParams>,
    shared: Arc<SharedMeters>,
}

fn rig(channels: usize) -> Rig {
    let params = Arc::new(ShirabeParams::default());
    let shared = Arc::new(SharedMeters::new());
    let mut engine = MeterEngine::new(params.clone(), shared.clone());
    engine.prepare(RATE, channels);
    Rig {
        engine,
        params,
        shared,
    }
}

/// Runs one block through the engine under the same allocation guard the debug plugin build
/// uses, so a callback that allocates fails every test in this file.
fn process(rig: &mut Rig, slices: &mut [&mut [f32]]) {
    assert_no_alloc::assert_no_alloc(|| rig.engine.process(slices, false));
}

/// The guard is only meaningful if it is actually installed in this test binary. It aborts
/// instead of unwinding, so the probe runs in a child process of the same test binary.
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

/// Feeds `frames` frames of a 1 kHz sine in blocks of 512; `phase` is the right-channel
/// offset in cycles (0.0 in phase, 0.5 anti-phase) and `right_gain` scales the right channel.
fn run(rig: &mut Rig, phase: f32, right_gain: f32, frames: usize) {
    let block = 512;
    let mut first = 0;
    while first < frames {
        let count = block.min(frames - first);
        let mut left = vec![0.0f32; count];
        let mut right = vec![0.0f32; count];
        for i in 0..count {
            let t = (first + i) as f32 / RATE;
            let angle = std::f32::consts::TAU * 1_000.0 * t;
            left[i] = AMPLITUDE * angle.sin();
            right[i] = right_gain * AMPLITUDE * (angle + std::f32::consts::TAU * phase).sin();
        }
        first += count;
        let mut slices: [&mut [f32]; 2] = [&mut left, &mut right];
        process(rig, &mut slices);
    }
}

#[test]
fn a_stereo_engine_publishes_correlation_and_balance() {
    let mut rig = rig(2);
    assert_eq!(rig.shared.stereo.correlation.load(), None);
    assert_eq!(rig.shared.stereo.balance.load(), None);

    run(&mut rig, 0.0, 1.0, FULL);
    let correlation = rig.shared.stereo.correlation.load().expect("correlation");
    let balance = rig.shared.stereo.balance.load().expect("balance");
    assert!((correlation - 1.0).abs() < 1e-5, "{correlation}");
    assert!(balance.abs() < 1e-5, "{balance}");
}

#[test]
fn an_anti_phase_input_reads_minus_one() {
    let mut rig = rig(2);
    run(&mut rig, 0.5, 1.0, FULL);
    let correlation = rig.shared.stereo.correlation.load().expect("correlation");
    assert!((correlation + 1.0).abs() < 1e-5, "{correlation}");
    assert!(rig.shared.stereo.balance.load().expect("balance").abs() < 1e-5);
}

#[test]
fn a_quieter_right_channel_moves_the_balance_right_of_centre() {
    let mut rig = rig(2);
    // Right at half amplitude: a quarter of the left energy, so the balance is -0.6.
    run(&mut rig, 0.0, 0.5, FULL);
    let balance = rig.shared.stereo.balance.load().expect("balance");
    assert!((balance + 0.6).abs() < 1e-3, "{balance}");
    assert!((rig.shared.stereo.correlation.load().unwrap() - 1.0).abs() < 1e-5);
}

#[test]
fn a_mono_layout_publishes_nothing() {
    let mut rig = rig(1);
    let mut mono = vec![0.0f32; FULL];
    for (i, sample) in mono.iter_mut().enumerate() {
        *sample = AMPLITUDE * (i as f32 * 0.1).sin();
    }
    for chunk in mono.chunks_mut(512) {
        let mut slices: [&mut [f32]; 1] = [chunk];
        process(&mut rig, &mut slices);
    }
    assert_eq!(rig.shared.active_channels(), 1);
    assert_eq!(rig.shared.stereo.correlation.load(), None);
    assert_eq!(rig.shared.stereo.balance.load(), None);
}

#[test]
fn switching_to_mono_clears_the_previous_reading() {
    let mut rig = rig(2);
    run(&mut rig, 0.0, 1.0, FULL);
    assert!(rig.shared.stereo.correlation.load().is_some());

    rig.engine.prepare(RATE, 1);
    let mut mono = [0.0f32; 64];
    let mut slices: [&mut [f32]; 1] = [&mut mono];
    process(&mut rig, &mut slices);
    assert_eq!(rig.shared.stereo.correlation.load(), None);
    assert_eq!(rig.shared.stereo.balance.load(), None);
}

#[test]
fn a_mono_block_in_a_stereo_session_stays_unavailable() {
    // Some hosts deliver a single channel slice even though the layout is stereo.
    let mut rig = rig(2);
    run(&mut rig, 0.0, 1.0, FULL);
    assert!(rig.shared.stereo.correlation.load().is_some());

    let mut mono = vec![0.0f32; 512];
    let mut slices: [&mut [f32]; 1] = [&mut mono];
    // This block also runs the reset of the abandoned window, under the allocation guard.
    process(&mut rig, &mut slices);
    assert_eq!(rig.shared.stereo.correlation.load(), None);
    assert_eq!(rig.shared.stereo.balance.load(), None);
}

#[test]
fn a_one_sided_signal_publishes_balance_but_no_correlation() {
    let mut rig = rig(2);
    run(&mut rig, 0.0, 0.0, FULL);
    assert_eq!(rig.shared.stereo.correlation.load(), None);
    assert_eq!(rig.shared.stereo.balance.load(), Some(-1.0));
}

#[test]
fn silence_publishes_nothing() {
    let mut rig = rig(2);
    let mut left = [0.0f32; 512];
    let mut right = [0.0f32; 512];
    for _ in 0..FULL / 512 + 1 {
        let mut slices: [&mut [f32]; 2] = [&mut left, &mut right];
        process(&mut rig, &mut slices);
    }
    assert_eq!(rig.shared.stereo.correlation.load(), None);
    assert_eq!(rig.shared.stereo.balance.load(), None);
}

#[test]
fn reset_clears_the_readings() {
    let mut rig = rig(2);
    run(&mut rig, 0.0, 1.0, FULL);
    assert!(rig.shared.stereo.correlation.load().is_some());

    rig.engine.reset();
    assert_eq!(rig.shared.stereo.correlation.load(), None);
    assert_eq!(rig.shared.stereo.balance.load(), None);

    // The window has to fill again before a reading appears.
    run(&mut rig, 0.0, 1.0, FULL - 512);
    assert_eq!(rig.shared.stereo.correlation.load(), None);
    run(&mut rig, 0.0, 1.0, 512);
    assert!(rig.shared.stereo.correlation.load().is_some());
}

#[test]
fn clearing_maxima_keeps_the_instantaneous_reading() {
    // The reset button clears maxima and programme loudness; the rolling reading keeps running
    // like the momentary loudness and the level meters.
    let mut rig = rig(2);
    run(&mut rig, 0.0, 1.0, FULL);
    let before = rig.shared.stereo.correlation.load();
    rig.shared.request_reset();
    run(&mut rig, 0.0, 1.0, 512);
    assert_eq!(rig.shared.stereo.correlation.load(), before);
}

#[test]
fn the_test_oscillator_is_measured_as_a_mono_signal() {
    let mut rig = rig(2);
    // SAFETY: the parameter is owned by `rig.params`, which outlives this call, and no other
    // thread touches it while the test runs.
    unsafe {
        rig.params.osc_enabled.as_ptr().set_normalized_value(1.0);
    }
    run(&mut rig, 0.0, 1.0, FULL);
    // The engine overwrites both channels with the same tone: perfect correlation, no offset.
    let correlation = rig.shared.stereo.correlation.load().expect("correlation");
    assert!((correlation - 1.0).abs() < 1e-5, "{correlation}");
    assert!(rig.shared.stereo.balance.load().expect("balance").abs() < 1e-5);
}

#[test]
fn preparing_a_new_sample_rate_restarts_the_window() {
    let mut rig = rig(2);
    run(&mut rig, 0.0, 1.0, FULL);
    assert!(rig.shared.stereo.correlation.load().is_some());

    rig.engine.prepare(96_000.0, 2);
    assert_eq!(rig.shared.stereo.correlation.load(), None);
    assert_eq!(rig.shared.stereo.balance.load(), None);
}
