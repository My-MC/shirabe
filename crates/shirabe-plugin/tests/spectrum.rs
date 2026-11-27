//! Spectrum capture through the shared measurement engine.
//!
//! These tests feed the engine like an audio callback would and analyse the captured mono sum on
//! the test thread, which is where the editor runs its FFT.

use std::sync::Arc;

use shirabe_dsp::spectrum::{SpectrumAnalyzer, BAND_COUNT, FFT_SIZE};
use shirabe_plugin::engine::MeterEngine;
use shirabe_plugin::params::ShirabeParams;
use shirabe_plugin::shared::SharedMeters;

const RATE: f32 = 48_000.0;

fn sine(frequency: f32, amplitude: f32, length: usize) -> Vec<f32> {
    (0..length)
        .map(|n| amplitude * (std::f32::consts::TAU * frequency * n as f32 / RATE).sin())
        .collect()
}

fn rig(channels: usize) -> (MeterEngine, Arc<SharedMeters>) {
    let params = Arc::new(ShirabeParams::default());
    let shared = Arc::new(SharedMeters::new());
    let mut engine = MeterEngine::new(params, shared.clone());
    engine.prepare(RATE, channels, 0);
    (engine, shared)
}

fn new_analyzer() -> SpectrumAnalyzer {
    SpectrumAnalyzer::new(RATE)
}

fn band_1k(analyzer: &SpectrumAnalyzer) -> f32 {
    // 1 kHz is index 17 of the 1/3-octave series.
    analyzer.bands_db()[17]
}

#[test]
fn a_stereo_signal_is_captured_as_a_mono_sum() {
    let signal = sine(1000.0, 0.5, FFT_SIZE * 2);
    let (mut engine, shared) = rig(2);
    let mut left = signal.clone();
    let mut right = signal.clone();
    engine.process(&mut [&mut left[..], &mut right[..]], &mut [], false);

    let mut analyzer = new_analyzer();
    assert!(analyzer.analyze(&shared.spectrum));
    let dual = band_1k(&analyzer);

    // The same sine on the left channel only is 6 dB lower in the mono sum.
    let (mut engine, shared) = rig(2);
    let mut left = signal.clone();
    let mut right = vec![0.0; signal.len()];
    engine.process(&mut [&mut left[..], &mut right[..]], &mut [], false);
    let mut analyzer = new_analyzer();
    assert!(analyzer.analyze(&shared.spectrum));
    let single = band_1k(&analyzer);

    assert!((dual - single - 6.02).abs() < 0.5, "{dual} vs {single} dB");
    // A bin-centred full-scale sine in both channels reads 0 dBFS.
    let full = sine(999.023, 1.0, FFT_SIZE * 2);
    let (mut engine, shared) = rig(2);
    let mut left = full.clone();
    let mut right = full;
    engine.process(&mut [&mut left[..], &mut right[..]], &mut [], false);
    let mut analyzer = new_analyzer();
    assert!(analyzer.analyze(&shared.spectrum));
    assert!(band_1k(&analyzer).abs() < 0.3, "{}", band_1k(&analyzer));
}

#[test]
fn a_mono_input_is_captured_without_duplication() {
    let signal = sine(999.023, 1.0, FFT_SIZE * 2);
    let (mut engine, shared) = rig(1);
    let mut channel = signal;
    engine.process(&mut [&mut channel[..]], &mut [], false);
    let mut analyzer = new_analyzer();
    assert!(analyzer.analyze(&shared.spectrum));
    assert!(band_1k(&analyzer).abs() < 0.3);
    assert_eq!(analyzer.bands_db().len(), BAND_COUNT);
}

#[test]
fn reset_clears_the_captured_spectrum() {
    let (mut engine, shared) = rig(2);
    let mut left = sine(1000.0, 0.5, FFT_SIZE + 32);
    let mut right = left.clone();
    engine.process(&mut [&mut left[..], &mut right[..]], &mut [], false);
    let before = shared.spectrum.epoch();
    assert!(shared.spectrum.written() > 0);
    shared.request_reset();
    let mut left = [0.0f32; 32];
    let mut right = [0.0f32; 32];
    engine.process(&mut [&mut left[..], &mut right[..]], &mut [], false);
    assert!(shared.spectrum.epoch() > before);
    assert!(shared.spectrum.written() <= 32);
}

#[test]
fn the_audio_thread_does_not_allocate_while_capturing() {
    let (mut engine, _shared) = rig(2);
    let mut left = sine(1000.0, 0.5, 512);
    let mut right = left.clone();
    assert_no_alloc::assert_no_alloc(|| {
        for _ in 0..8 {
            engine.process(&mut [&mut left[..], &mut right[..]], &mut [], false);
        }
    });
    assert!(_shared.spectrum.written() > 0);
}

#[test]
fn the_allocation_guard_is_installed() {
    let exe = std::env::current_exe().expect("test binary");
    let status = std::process::Command::new(exe)
        .args([
            "--exact",
            "the_allocation_guard_fires",
            "--ignored",
            "--test-threads=1",
        ])
        .output()
        .expect("child process");
    assert!(
        !status.status.success(),
        "an allocation inside the guard did not fail"
    );
    let stderr = String::from_utf8_lossy(&status.stderr);
    assert!(stderr.contains("memory allocation"), "{stderr}");
}

/// Only run by [`the_allocation_guard_is_installed`]; allocating here aborts the process.
#[ignore = "aborts on purpose; started by the_allocation_guard_is_installed"]
#[test]
fn the_allocation_guard_fires() {
    assert_no_alloc::assert_no_alloc(|| {
        let _ = vec![0u8; 16];
    });
}
