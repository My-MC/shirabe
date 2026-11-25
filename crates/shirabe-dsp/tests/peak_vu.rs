//! True-peak and VU meter tests.

use shirabe_dsp::{gain_to_db, TruePeakMeter, VuMeter};
use std::f64::consts::{PI, TAU};

fn sine(freq: f64, sr: f64, amp: f64, phase: f64, seconds: f64) -> Vec<f32> {
    let n = (seconds * sr) as usize;
    (0..n)
        .map(|i| (amp * (TAU * freq * i as f64 / sr + phase).sin()) as f32)
        .collect()
}

// --- True peak (ITU-R BS.1770 Annex 2) -----------------------------------------------------

#[test]
fn quarter_rate_sine_between_samples() {
    // fs/4 with a 45° phase offset: every sample sits at ±0.707 of the real peak (-3 dB).
    let sr = 48_000.0;
    let x = sine(sr / 4.0, sr, 0.5, PI / 4.0, 1.0);
    let sample_peak = x.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!((gain_to_db(sample_peak) - (-9.03)).abs() < 0.05);

    let mut tp = TruePeakMeter::new(sr as f32);
    tp.process(&x);
    let err = tp.max_dbtp() - gain_to_db(0.5);
    assert!(err.abs() < 0.2, "true peak error {err} dB");
}

#[test]
fn accuracy_across_frequencies_and_phases() {
    // BS.1770 allows the estimate to read up to 0.2 dB high and should not read far low;
    // check a sweep of frequencies (up to 20 kHz at 48 kHz) and phases.
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for &sr in &[44_100.0, 48_000.0] {
        for &freq in &[997.0, 5_000.0, 10_000.0, 15_000.0, 19_000.0, 20_000.0] {
            for k in 0..8 {
                let phase = k as f64 * PI / 8.0;
                let x = sine(freq, sr, 0.5, phase, 0.2);
                let mut tp = TruePeakMeter::new(sr as f32);
                // Skip the abrupt start: its overshoot is a real inter-sample peak.
                let warm_up = (0.05 * sr) as usize;
                tp.process(&x[..warm_up]);
                tp.reset_max();
                tp.process(&x[warm_up..]);
                let err = tp.max_dbtp() - gain_to_db(0.5);
                lo = lo.min(err);
                hi = hi.max(err);
                assert!(
                    (-0.5..=0.2).contains(&err),
                    "sr={sr} f={freq} phase={phase:.2}: error {err:.3} dB"
                );
            }
        }
    }
    println!("true-peak error range: {lo:.3} .. {hi:.3} dB");
}

#[test]
fn never_below_sample_peak() {
    let x: Vec<f32> = (0..4800)
        .map(|i| if i == 2400 { 0.9 } else { 0.0 })
        .collect();
    let mut tp = TruePeakMeter::new(48_000.0);
    tp.process(&x);
    assert!(tp.max_dbtp() >= gain_to_db(0.9) - 1e-4);
}

#[test]
fn intersample_overs_are_detected() {
    // A full-scale fs/4 sine at 45° has samples at -3 dBFS but peaks at 0 dBTP.
    let sr = 48_000.0;
    let x = sine(sr / 4.0, sr, 1.0, PI / 4.0, 0.5);
    let mut tp = TruePeakMeter::new(sr as f32);
    tp.process(&x);
    assert!(tp.max_dbtp() > -0.2, "{}", tp.max_dbtp());
}

#[test]
fn high_rates_use_less_oversampling() {
    assert_eq!(TruePeakMeter::new(48_000.0).factor(), 4);
    assert_eq!(TruePeakMeter::new(96_000.0).factor(), 2);
    assert_eq!(TruePeakMeter::new(192_000.0).factor(), 1);
}

#[test]
fn reset_max_clears() {
    let mut tp = TruePeakMeter::new(48_000.0);
    tp.process(&sine(1000.0, 48_000.0, 0.5, 0.0, 0.1));
    tp.reset_max();
    assert!(tp.max_dbtp() < -100.0);
}

// --- VU (IEC 60268-17) -----------------------------------------------------------------------

#[test]
fn reference_sine_reads_zero_vu() {
    let sr = 48_000.0;
    let mut vu = VuMeter::new(sr as f32, -18.0);
    vu.process(&sine(1000.0, sr, 10f64.powf(-18.0 / 20.0), 0.0, 3.0));
    assert!(vu.vu().abs() < 0.05, "{}", vu.vu());
}

#[test]
fn level_changes_track_in_db() {
    let sr = 48_000.0;
    let mut vu = VuMeter::new(sr as f32, -18.0);
    vu.process(&sine(1000.0, sr, 10f64.powf(-28.0 / 20.0), 0.0, 3.0));
    assert!((vu.vu() + 10.0).abs() < 0.05, "{}", vu.vu());
}

#[test]
fn ballistics_rise_time_and_overshoot() {
    let sr = 48_000.0;
    let mut vu = VuMeter::new(sr as f32, -18.0);
    let x = sine(1000.0, sr, 10f64.powf(-18.0 / 20.0), 0.0, 1.5);
    let block = 48; // 1 ms
    let mut reached_99 = None;
    let mut max_ratio: f64 = 0.0;
    for (k, chunk) in x.chunks(block).enumerate() {
        vu.process(chunk);
        let t = (k + 1) as f64 * block as f64 / sr;
        let r = vu.ratio();
        if reached_99.is_none() && r >= 0.99 {
            reached_99 = Some(t);
        }
        max_ratio = max_ratio.max(r);
    }
    let t = reached_99.expect("never reached 99 %");
    // IEC 60268-17: 99 % of the steady reading after 300 ms (±10 %).
    assert!((0.27..=0.33).contains(&t), "rise time {t:.3} s");
    // Overshoot between 1 % and 1.5 %.
    assert!(
        (1.008..=1.017).contains(&max_ratio),
        "overshoot {max_ratio:.4}"
    );
}
