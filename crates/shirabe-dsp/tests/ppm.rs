//! Quasi-peak programme meter tests (IEC 60268-10 Type I and Type II).

use shirabe_dsp::{PpmMeter, PpmType};
use std::f64::consts::TAU;

const SR: f32 = 48_000.0;

/// 5 kHz tone burst of `burst_ms` at `amp` peak, followed by silence, starting from rest.
/// Returns the highest reading in dBFS.
fn burst_reading(kind: PpmType, burst_ms: f64, sr: f32) -> f32 {
    let mut ppm = PpmMeter::new(sr, kind);
    let burst = (burst_ms / 1000.0 * sr as f64).round() as usize;
    let total = burst + (0.1 * sr as f64) as usize;
    let mut max = f32::NEG_INFINITY;
    for chunk_start in (0..total).step_by(32) {
        let end = (chunk_start + 32).min(total);
        let x: Vec<f32> = (chunk_start..end)
            .map(|i| {
                if i < burst {
                    (TAU * 5000.0 * i as f64 / sr as f64).sin() as f32
                } else {
                    0.0
                }
            })
            .collect();
        ppm.process(&x);
        max = max.max(ppm.level_db());
    }
    max
}

fn steady(kind: PpmType, amp: f64, seconds: f64) -> PpmMeter {
    let mut ppm = PpmMeter::new(SR, kind);
    let n = (seconds * SR as f64) as usize;
    let x: Vec<f32> = (0..n)
        .map(|i| (amp * (TAU * 1000.0 * i as f64 / SR as f64).sin()) as f32)
        .collect();
    ppm.process(&x);
    ppm
}

fn assert_within(name: &str, value: f32, expected: f32, tol: f32) {
    assert!(
        (value - expected).abs() <= tol,
        "{name}: got {value:.2} dB, expected {expected} ± {tol}"
    );
}

#[test]
fn steady_sine_reads_its_peak() {
    for kind in [PpmType::TypeI, PpmType::TypeII] {
        let ppm = steady(kind, 10f64.powf(-9.0 / 20.0), 1.0);
        assert_within(&format!("{kind:?}"), ppm.level_db(), -9.0, 0.1);
    }
}

// Type I: integration time 5 ms (a 5 ms burst reads -2 dB, a 10 ms burst -1 dB).
#[test]
fn type_i_burst_response() {
    for sr in [44_100.0, 48_000.0, 96_000.0] {
        assert_within("10 ms", burst_reading(PpmType::TypeI, 10.0, sr), -1.0, 0.5);
        assert_within("5 ms", burst_reading(PpmType::TypeI, 5.0, sr), -2.0, 0.5);
        assert_within("3 ms", burst_reading(PpmType::TypeI, 3.0, sr), -4.0, 1.0);
    }
}

// Type II: a 10 ms burst reads -2.5 dB, 5 ms -4 dB, 1.5 ms -9 dB.
#[test]
fn type_ii_burst_response() {
    for sr in [44_100.0, 48_000.0, 96_000.0] {
        assert_within("10 ms", burst_reading(PpmType::TypeII, 10.0, sr), -2.5, 0.5);
        assert_within("5 ms", burst_reading(PpmType::TypeII, 5.0, sr), -4.0, 0.75);
        assert_within("1.5 ms", burst_reading(PpmType::TypeII, 1.5, sr), -9.0, 1.0);
    }
}

/// Time for the reading to fall by `drop_db` after a steady tone stops.
fn return_time(kind: PpmType, drop_db: f32) -> f32 {
    let mut ppm = steady(kind, 1.0, 1.0);
    let start = ppm.level_db();
    let silence = vec![0.0f32; 48];
    let mut t = 0.0;
    while ppm.level_db() > start - drop_db {
        ppm.process(&silence);
        t += 48.0 / SR;
        assert!(t < 10.0, "never returned");
    }
    t
}

#[test]
fn type_i_return_time() {
    // 20 dB in 1.5 s (IEC 60268-10 allows 1.4 to 2.0 s).
    assert_within("return", return_time(PpmType::TypeI, 20.0), 1.5, 0.1);
}

#[test]
fn type_ii_return_time() {
    // 24 dB in 2.8 s ± 0.3 s.
    assert_within("return", return_time(PpmType::TypeII, 24.0), 2.8, 0.1);
}

#[test]
fn reset_returns_to_rest() {
    let mut ppm = steady(PpmType::TypeI, 1.0, 0.2);
    ppm.reset();
    assert!(ppm.level_db() < -100.0);
}
