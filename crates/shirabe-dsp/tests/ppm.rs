//! Quasi-peak programme meter tests (IEC 60268-10 Type I and Type II).

use shirabe_dsp::{PpmMeter, PpmType};
use std::f64::consts::TAU;

const SR: f32 = 48_000.0;

/// 5 kHz tone burst of `burst_ms` at `amp` peak, followed by silence, starting from rest.
/// Returns the highest reading in dBFS.
fn burst_reading(kind: PpmType, burst_ms: f64, sr: f32) -> f32 {
    burst_reading_at(kind, burst_ms, sr, 5000.0)
}

fn burst_reading_at(kind: PpmType, burst_ms: f64, sr: f32, freq: f64) -> f32 {
    let mut ppm = PpmMeter::new(sr, kind);
    let burst = (burst_ms / 1000.0 * sr as f64).round() as usize;
    let total = burst + (0.1 * sr as f64) as usize;
    let mut max = f32::NEG_INFINITY;
    for chunk_start in (0..total).step_by(32) {
        let end = (chunk_start + 32).min(total);
        let x: Vec<f32> = (chunk_start..end)
            .map(|i| {
                if i < burst {
                    (TAU * freq * i as f64 / sr as f64).sin() as f32
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

// Type I: integration time 5 ms (a 5 ms burst reads -2 dB, a 10 ms burst -1 dB, 3 ms -4 dB).
// The values are those of IEC 60268-10 as quoted in secondary sources; the tolerances of the
// standard itself could not be checked (the full text is not freely available).
#[test]
fn type_i_burst_response() {
    for sr in [44_100.0, 48_000.0, 96_000.0] {
        assert_within("10 ms", burst_reading(PpmType::TypeI, 10.0, sr), -1.0, 0.5);
        assert_within("5 ms", burst_reading(PpmType::TypeI, 5.0, sr), -2.0, 0.5);
        assert_within("3 ms", burst_reading(PpmType::TypeI, 3.0, sr), -4.0, 1.0);
    }
}

// Type II: EBU Tech 3205 Table 2 (5 kHz bursts relative to the steady reading):
// 100 ms 0 dB ±0.5, 10 ms -2 dB ±0.5, 5 ms -4 dB ±0.75, 1.5 ms -9 dB ±1.0, 0.5 ms -17 dB ±2.0.
#[test]
fn type_ii_burst_response() {
    for sr in [44_100.0, 48_000.0, 96_000.0] {
        assert_within(
            "100 ms",
            burst_reading(PpmType::TypeII, 100.0, sr),
            0.0,
            0.5,
        );
        assert_within("10 ms", burst_reading(PpmType::TypeII, 10.0, sr), -2.0, 0.5);
        assert_within("5 ms", burst_reading(PpmType::TypeII, 5.0, sr), -4.0, 0.75);
        assert_within("1.5 ms", burst_reading(PpmType::TypeII, 1.5, sr), -9.0, 1.0);
        // Tech 3205: the 0.5 ms burst needs at least five cycles, so it uses 10 kHz.
        let r = burst_reading_at(PpmType::TypeII, 0.5, sr, 10_000.0);
        assert_within("0.5 ms", r, -17.0, 2.0);
    }
}

/// EBU Tech 3205 §3.9: overswing at most 0.5 dB for a suddenly applied steady tone.
#[test]
fn type_ii_overswing() {
    let mut ppm = PpmMeter::new(SR, PpmType::TypeII);
    let amp = 10f64.powf(-9.0 / 20.0);
    let mut max = f32::NEG_INFINITY;
    for k in 0..100 {
        let x: Vec<f32> = (k * 480..(k + 1) * 480)
            .map(|i| (amp * (TAU * 5000.0 * i as f64 / SR as f64).sin()) as f32)
            .collect();
        ppm.process(&x);
        max = max.max(ppm.level_db());
    }
    assert!(max <= -9.0 + 0.5, "overswing to {max}");
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
    // DIN 45406: 20 dB in 1.5 s (IEC 60268-10 Type I: 1.7 ± 0.3 s, so 1.5 s is inside both).
    assert_within("return", return_time(PpmType::TypeI, 20.0), 1.5, 0.1);
}

#[test]
fn type_ii_return_time() {
    // EBU Tech 3205 §3.10: +12 to -12 (24 dB) in 2.8 ± 0.3 s.
    assert_within("return", return_time(PpmType::TypeII, 24.0), 2.8, 0.1);
}

#[test]
fn reset_returns_to_rest() {
    let mut ppm = steady(PpmType::TypeI, 1.0, 0.2);
    ppm.reset();
    assert!(ppm.level_db() < -100.0);
}

// --- IEEE Std 152-1991 §1.3 (identical dynamics to EBU Tech 3205, normal mode) ------------------

/// Readings (dBFS) every 1 ms of a steady sine, after `settle` seconds.
fn steady_readings(
    kind: PpmType,
    sr: f32,
    freq: f64,
    amp: f64,
    settle: f64,
    total: f64,
) -> Vec<f32> {
    let mut ppm = PpmMeter::new(sr, kind);
    let block = (sr / 1000.0) as usize;
    let n = (total * sr as f64) as usize;
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let x: Vec<f32> = (i..i + block)
            .map(|k| (amp * (TAU * freq * k as f64 / sr as f64).sin()) as f32)
            .collect();
        ppm.process(&x);
        i += block;
        if i as f64 / sr as f64 >= settle {
            out.push(ppm.level_db());
        }
    }
    out
}

/// §1.3.1: within 25 Hz to 16 kHz the indication at test level deviates by less than 0.2 dB.
#[test]
fn ieee152_frequency_response() {
    let amp = 10f64.powf(-18.0 / 20.0);
    for sr in [44_100.0f32, 48_000.0, 96_000.0] {
        for freq in [
            25.0, 31.5, 63.0, 125.0, 1000.0, 5000.0, 10_000.0, 12_000.0, 12_500.0, 16_000.0,
        ] {
            let r = steady_readings(PpmType::TypeII, sr, freq, amp, 1.0, 1.5);
            let (lo, hi) = r
                .iter()
                .fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
            println!("sr={sr} f={freq}: {:.3} .. {:.3} dB", lo + 18.0, hi + 18.0);
            assert!(
                (lo + 18.0).abs() < 0.2 && (hi + 18.0).abs() < 0.2,
                "sr={sr} f={freq}: indication {lo:.3}..{hi:.3} dBFS"
            );
        }
    }
}

/// §1.3.10: integration time 10 ± 2 ms (burst duration that reads 2 dB below steady).
#[test]
fn ieee152_integration_time() {
    let mut lo = 1.0;
    let mut hi = 40.0;
    for _ in 0..30 {
        let mid = 0.5 * (lo + hi);
        if burst_reading(PpmType::TypeII, mid, SR) < -2.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    assert!((8.0..=12.0).contains(&lo), "integration time {lo:.2} ms");
}

/// §1.3.4: delay time below 150 ms (1 kHz at +9 dB re test reaches +8).
#[test]
fn ieee152_delay_time() {
    let test = 10f64.powf(-18.0 / 20.0);
    let amp = test * 10f64.powf(9.0 / 20.0);
    let r = steady_readings(PpmType::TypeII, SR, 1000.0, amp, 0.0, 0.3);
    let t = r
        .iter()
        .position(|&v| v >= -18.0 + 8.0)
        .expect("never reached +8") as f64
        + 1.0;
    assert!(t < 150.0, "delay {t} ms");
}

/// §1.3.11: a 1.5 ms burst at +9 increased by 10 dB raises the indication by 5 to 10 dB.
#[test]
fn ieee152_overload_characteristic() {
    let read = |gain_db: f64| {
        let mut ppm = PpmMeter::new(SR, PpmType::TypeII);
        let amp = 10f64.powf((-18.0 + 9.0 + gain_db) / 20.0);
        let n = (0.0015 * SR as f64) as usize;
        let x: Vec<f32> = (0..n)
            .map(|i| (amp * (TAU * 5000.0 * i as f64 / SR as f64).sin()) as f32)
            .collect();
        ppm.process(&x);
        ppm.level_db()
    };
    let rise = read(10.0) - read(0.0);
    assert!((5.0..=10.0 + 1e-3).contains(&rise), "rise {rise} dB");
}

/// §1.3.13: unidirectional pulses that read TEST at 1 ms read progressively less when
/// shortened, and a 100 µs pulse reads at least 10 dB below TEST.
#[test]
fn ieee152_pulse_response() {
    let pulse = |amp: f64, us: f64| {
        let mut ppm = PpmMeter::new(SR, PpmType::TypeII);
        let n = ((us * 1e-6 * SR as f64).round() as usize).max(1);
        let mut x = vec![amp as f32; n];
        x.extend(std::iter::repeat_n(0.0f32, 480));
        let mut max = f32::NEG_INFINITY;
        for c in x.chunks(16) {
            ppm.process(c);
            max = max.max(ppm.level_db());
        }
        max
    };
    // Amplitude for which a 1 ms pulse reads TEST (-18 dBFS).
    let at_1ms = pulse(1.0, 1000.0);
    let amp = 10f64.powf((-18.0 - at_1ms as f64) / 20.0);
    let mut last = f32::INFINITY;
    for us in [1000.0, 500.0, 200.0, 100.0, 50.0] {
        let r = pulse(amp, us);
        assert!(r <= last + 1e-4, "{us} µs: {r} not below {last}");
        last = r;
        if us == 100.0 {
            assert!(r <= -18.0 - 10.0, "100 µs pulse reads {r} dBFS");
        }
    }
}

/// §1.3.15 / Tech 3205 §2.4: reversibility error at most 0.5 dB for an asymmetrically
/// clipped signal (one polarity clipped to 25 %, DC removed).
#[test]
fn ieee152_reversibility_error() {
    let amp = 10f64.powf((-18.0 + 9.0) / 20.0);
    let n = SR as usize;
    let raw: Vec<f64> = (0..n)
        .map(|i| {
            let s = (TAU * 1000.0 * i as f64 / SR as f64).sin();
            if s < -0.25 {
                -0.25
            } else {
                s
            }
        })
        .collect();
    let dc = raw.iter().sum::<f64>() / n as f64;
    let read = |sign: f64| {
        let mut ppm = PpmMeter::new(SR, PpmType::TypeII);
        let x: Vec<f32> = raw.iter().map(|v| (sign * amp * (v - dc)) as f32).collect();
        ppm.process(&x);
        ppm.level_db()
    };
    let err = (read(1.0) - read(-1.0)).abs();
    assert!(err <= 0.5, "reversibility error {err} dB");
}
