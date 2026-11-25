//! Conformance tests with the synthetic test signals of EBU Tech 3341 (loudness) and
//! EBU Tech 3342 (loudness range), plus cross-checks against an independent implementation.

use shirabe_dsp::{ChannelLayout, ChannelRole, LoudnessMeter};
use std::f64::consts::TAU;

const SR: f32 = 48_000.0;

/// A sequence of 1 kHz sine segments `(seconds, dBFS peak)` on the given channel levels.
/// `levels[ch]` is an offset added to the segment level for channel `ch` (`None` = silent).
fn render(segments: &[(f64, f64)], channels: usize, offsets: &[Option<f64>]) -> Vec<Vec<f32>> {
    let total: usize = segments
        .iter()
        .map(|(s, _)| (s * SR as f64).round() as usize)
        .sum();
    let mut out = vec![Vec::with_capacity(total); channels];
    let mut n = 0usize;
    for &(seconds, db) in segments {
        let len = (seconds * SR as f64).round() as usize;
        for _ in 0..len {
            let phase = (TAU * 1000.0 * n as f64 / SR as f64).sin();
            for (ch, buf) in out.iter_mut().enumerate() {
                let v = match offsets[ch] {
                    Some(off) => phase * 10f64.powf((db + off) / 20.0),
                    None => 0.0,
                };
                buf.push(v as f32);
            }
            n += 1;
        }
    }
    out
}

fn run(meter: &mut LoudnessMeter, audio: &[Vec<f32>], block: usize) {
    let len = audio[0].len();
    let mut i = 0;
    while i < len {
        let end = (i + block).min(len);
        let chunk: Vec<&[f32]> = audio.iter().map(|c| &c[i..end]).collect();
        meter.process(&chunk);
        i = end;
    }
}

fn stereo(segments: &[(f64, f64)]) -> (LoudnessMeter, Vec<Vec<f32>>) {
    let meter = LoudnessMeter::new(SR, ChannelLayout::Stereo.roles());
    (meter, render(segments, 2, &[Some(0.0), Some(0.0)]))
}

fn assert_close(name: &str, value: Option<f64>, expected: f64, tol: f64) {
    let v = value.unwrap_or_else(|| panic!("{name}: no value"));
    assert!(
        (v - expected).abs() <= tol,
        "{name}: got {v:.3}, expected {expected} ± {tol}"
    );
}

// --- EBU Tech 3341, minimum requirements (tolerance ±0.1 LU) -----------------------------

#[test]
fn tech3341_case1_minus23() {
    let (mut m, a) = stereo(&[(20.0, -23.0)]);
    run(&mut m, &a, 512);
    let r = m.readout();
    assert_close("M", r.momentary, -23.0, 0.1);
    assert_close("S", r.short_term, -23.0, 0.1);
    assert_close("I", r.integrated, -23.0, 0.1);
}

#[test]
fn tech3341_case2_minus33() {
    let (mut m, a) = stereo(&[(20.0, -33.0)]);
    run(&mut m, &a, 441);
    let r = m.readout();
    assert_close("M", r.momentary, -33.0, 0.1);
    assert_close("S", r.short_term, -33.0, 0.1);
    assert_close("I", r.integrated, -33.0, 0.1);
}

#[test]
fn tech3341_case3_relative_gate() {
    let (mut m, a) = stereo(&[(10.0, -36.0), (60.0, -23.0), (10.0, -36.0)]);
    run(&mut m, &a, 1024);
    assert_close("I", m.readout().integrated, -23.0, 0.1);
}

#[test]
fn tech3341_case4_absolute_and_relative_gate() {
    let (mut m, a) = stereo(&[
        (10.0, -72.0),
        (10.0, -36.0),
        (60.0, -23.0),
        (10.0, -36.0),
        (10.0, -72.0),
    ]);
    run(&mut m, &a, 64);
    assert_close("I", m.readout().integrated, -23.0, 0.1);
}

#[test]
fn tech3341_case5_level_steps() {
    let (mut m, a) = stereo(&[(20.0, -26.0), (20.1, -20.0), (20.0, -26.0)]);
    run(&mut m, &a, 333);
    assert_close("I", m.readout().integrated, -23.0, 0.1);
}

#[test]
fn tech3341_case6_five_channels() {
    use ChannelRole::*;
    let roles = [Left, Right, Centre, LeftSurround, RightSurround];
    let mut m = LoudnessMeter::new(SR, &roles);
    let offsets = [
        Some(-28.0),
        Some(-28.0),
        Some(-24.0),
        Some(-30.0),
        Some(-30.0),
    ];
    let a = render(&[(20.0, 0.0)], 5, &offsets);
    run(&mut m, &a, 512);
    assert_close("I", m.readout().integrated, -23.0, 0.1);
}

#[test]
fn tech3341_case9_short_term_steady() {
    let mut segments = Vec::new();
    for _ in 0..20 {
        segments.push((1.34, -20.0));
        segments.push((1.66, -30.0));
    }
    let (mut m, a) = stereo(&segments);
    // After the first 3 s the short-term loudness must stay at -23 ± 0.1 LU.
    let block = 480; // 10 ms
    let len = a[0].len();
    let mut i = 0;
    let mut checked = 0;
    while i < len {
        let end = (i + block).min(len);
        let chunk: Vec<&[f32]> = a.iter().map(|c| &c[i..end]).collect();
        m.process(&chunk);
        if end as f32 / SR > 3.1 {
            if let Some(s) = m.readout().short_term {
                assert!(
                    (s + 23.0).abs() <= 0.1,
                    "S = {s} at {:.2} s",
                    end as f32 / SR
                );
                checked += 1;
            }
        }
        i = end;
    }
    assert!(checked > 1000);
}

// --- EBU Tech 3342, loudness range (tolerance ±1 LU) ---------------------------------------

#[test]
fn tech3342_case1() {
    let (mut m, a) = stereo(&[(20.0, -20.0), (20.0, -30.0)]);
    run(&mut m, &a, 512);
    assert_close("LRA", m.readout().range, 10.0, 1.0);
}

#[test]
fn tech3342_case2() {
    let (mut m, a) = stereo(&[(20.0, -20.0), (20.0, -15.0)]);
    run(&mut m, &a, 512);
    assert_close("LRA", m.readout().range, 5.0, 1.0);
}

#[test]
fn tech3342_case3() {
    let (mut m, a) = stereo(&[(20.0, -40.0), (20.0, -20.0)]);
    run(&mut m, &a, 512);
    assert_close("LRA", m.readout().range, 20.0, 1.0);
}

#[test]
fn tech3342_case4() {
    let (mut m, a) = stereo(&[
        (20.0, -50.0),
        (20.0, -35.0),
        (20.0, -20.0),
        (20.0, -35.0),
        (20.0, -50.0),
    ]);
    run(&mut m, &a, 512);
    assert_close("LRA", m.readout().range, 15.0, 1.0);
}

// --- Behaviour -------------------------------------------------------------------------------

#[test]
fn silence_has_no_integrated_value() {
    let (mut m, _) = stereo(&[]);
    let silence = vec![vec![0.0f32; 48_000 * 5]; 2];
    run(&mut m, &silence, 512);
    let r = m.readout();
    assert_eq!(r.integrated, None);
    assert_eq!(r.range, None);
    assert_eq!(r.momentary, Some(f64::NEG_INFINITY));
}

#[test]
fn reset_programme_restarts_integration() {
    let (mut m, a) = stereo(&[(10.0, -30.0)]);
    run(&mut m, &a, 512);
    m.reset_programme();
    let (_, b) = stereo(&[(10.0, -20.0)]);
    run(&mut m, &b, 512);
    assert_close("I", m.readout().integrated, -20.0, 0.1);
}

#[test]
fn other_sample_rates() {
    for sr in [44_100.0f32, 88_200.0, 96_000.0] {
        let mut m = LoudnessMeter::new(sr, ChannelLayout::Stereo.roles());
        let n = (sr * 10.0) as usize;
        let amp = 10f64.powf(-23.0 / 20.0);
        let a: Vec<f32> = (0..n)
            .map(|i| (amp * (TAU * 1000.0 * i as f64 / sr as f64).sin()) as f32)
            .collect();
        m.process(&[&a[..], &a[..]]);
        assert_close(&format!("I@{sr}"), m.readout().integrated, -23.0, 0.1);
    }
}

// --- Cross-check against an independent implementation (libebur128 port) -----------------

/// Deterministic "programme-like" signal: band-limited noise with slow level changes.
fn programme(seconds: f64) -> Vec<Vec<f32>> {
    let n = (seconds * SR as f64) as usize;
    let mut seed = 0x1234_5678u32;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f64 / u32::MAX as f64 - 0.5
    };
    let (mut l1, mut r1) = (0.0, 0.0);
    let mut l = Vec::with_capacity(n);
    let mut r = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / SR as f64;
        let env = 0.05 + 0.3 * (0.5 + 0.5 * (TAU * 0.07 * t).sin()).powi(3);
        l1 = 0.9 * l1 + 0.1 * rnd();
        r1 = 0.8 * r1 + 0.2 * rnd();
        let tone = 0.2 * (TAU * 220.0 * t).sin();
        l.push((env * (3.0 * l1 + tone)) as f32);
        r.push((env * (2.0 * r1 + 0.5 * tone)) as f32);
    }
    vec![l, r]
}

#[test]
fn matches_reference_implementation() {
    let audio = programme(60.0);
    let (mut m, _) = stereo(&[]);
    run(&mut m, &audio, 512);
    let ours = m.readout();

    let mut reference = ebur128::EbuR128::new(
        2,
        SR as u32,
        ebur128::Mode::I | ebur128::Mode::LRA | ebur128::Mode::S | ebur128::Mode::M,
    )
    .unwrap();
    let interleaved: Vec<f32> = audio[0]
        .iter()
        .zip(&audio[1])
        .flat_map(|(l, r)| [*l, *r])
        .collect();
    reference.add_frames_f32(&interleaved).unwrap();

    let ref_i = reference.loudness_global().unwrap();
    let ref_lra = reference.loudness_range().unwrap();
    let ref_m = reference.loudness_momentary().unwrap();
    let ref_s = reference.loudness_shortterm().unwrap();
    println!(
        "ours I={:?} LRA={:?} M={:?} S={:?}; reference I={ref_i:.3} LRA={ref_lra:.3} M={ref_m:.3} S={ref_s:.3}",
        ours.integrated, ours.range, ours.momentary, ours.short_term
    );
    assert_close("I vs reference", ours.integrated, ref_i, 0.05);
    assert_close("M vs reference", ours.momentary, ref_m, 0.05);
    assert_close("S vs reference", ours.short_term, ref_s, 0.05);
    assert_close("LRA vs reference", ours.range, ref_lra, 0.2);
}
