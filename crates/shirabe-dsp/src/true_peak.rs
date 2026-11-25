//! True-peak level (ITU-R BS.1770 Annex 2): the peak of the signal reconstructed between
//! samples, estimated by oversampling with a polyphase interpolation filter.
//!
//! The recommendation requires at least 4x oversampling at 48 kHz. This implementation uses
//! 4x below 96 kHz, 2x up to 192 kHz and no oversampling above, with a 32-tap-per-phase
//! Kaiser-windowed sinc filter designed at construction time.

use crate::level::gain_to_db;
use std::f64::consts::PI;

const TAPS_PER_PHASE: usize = 32;
const MAX_FACTOR: usize = 4;
const KAISER_BETA: f64 = 8.0;

/// Oversampling factor used for a sample rate.
pub fn oversampling_factor(sample_rate: f32) -> usize {
    if sample_rate < 96_000.0 {
        4
    } else if sample_rate < 192_000.0 {
        2
    } else {
        1
    }
}

fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let half = x / 2.0;
    for k in 1..50 {
        term *= (half / k as f64) * (half / k as f64);
        sum += term;
        if term < 1e-12 * sum {
            break;
        }
    }
    sum
}

/// Polyphase coefficients: `phases[p][t]`.
fn design(factor: usize) -> Vec<[f64; TAPS_PER_PHASE]> {
    let total = factor * TAPS_PER_PHASE;
    let centre = (total - 1) as f64 / 2.0;
    // Cut-off slightly below the original Nyquist frequency.
    let cutoff = 0.5 / factor as f64 * 0.98;
    let norm = bessel_i0(KAISER_BETA);
    let mut proto = vec![0.0; total];
    for (n, h) in proto.iter_mut().enumerate() {
        let x = n as f64 - centre;
        let sinc = if x == 0.0 {
            2.0 * cutoff
        } else {
            (2.0 * PI * cutoff * x).sin() / (PI * x)
        };
        let r = x / centre;
        let window = bessel_i0(KAISER_BETA * (1.0 - r * r).max(0.0).sqrt()) / norm;
        *h = sinc * window * factor as f64;
    }
    let mut phases = vec![[0.0; TAPS_PER_PHASE]; factor];
    for (n, h) in proto.iter().enumerate() {
        phases[n % factor][n / factor] = *h;
    }
    // Normalize every phase to unity DC gain so a constant signal reads exactly.
    for phase in &mut phases {
        let sum: f64 = phase.iter().sum();
        for h in phase.iter_mut() {
            *h /= sum;
        }
    }
    phases
}

/// True-peak meter for one channel.
#[derive(Clone, Debug)]
pub struct TruePeakMeter {
    factor: usize,
    phases: Vec<[f64; TAPS_PER_PHASE]>,
    history: [f64; TAPS_PER_PHASE],
    pos: usize,
    block_peak: f32,
    max: f32,
}

impl TruePeakMeter {
    /// Creates a meter for a sample rate.
    pub fn new(sample_rate: f32) -> Self {
        let mut meter = Self {
            factor: 1,
            phases: Vec::with_capacity(MAX_FACTOR),
            history: [0.0; TAPS_PER_PHASE],
            pos: 0,
            block_peak: 0.0,
            max: 0.0,
        };
        meter.prepare(sample_rate);
        meter
    }

    /// Changes the sample rate and clears the state.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.factor = oversampling_factor(sample_rate);
        self.phases.clear();
        self.phases.extend(design(self.factor));
        self.reset();
    }

    /// Clears the filter history and the maximum.
    pub fn reset(&mut self) {
        self.history = [0.0; TAPS_PER_PHASE];
        self.pos = 0;
        self.block_peak = 0.0;
        self.max = 0.0;
    }

    /// Clears only the maximum.
    pub fn reset_max(&mut self) {
        self.max = 0.0;
    }

    /// Processes a block and returns its true peak (linear).
    pub fn process(&mut self, samples: &[f32]) -> f32 {
        let mut peak = 0.0f64;
        for &s in samples {
            self.history[self.pos] = s as f64;
            self.pos = (self.pos + 1) % TAPS_PER_PHASE;
            if self.factor == 1 {
                peak = peak.max((s as f64).abs());
                continue;
            }
            for phase in &self.phases {
                let mut acc = 0.0;
                // history[pos - 1 - t] is x[n - t]
                for (t, h) in phase.iter().enumerate() {
                    let idx = (self.pos + TAPS_PER_PHASE - 1 - t) % TAPS_PER_PHASE;
                    acc += h * self.history[idx];
                }
                peak = peak.max(acc.abs());
            }
        }
        // The interpolated signal can never be below the sample peak of the same samples, but
        // the interpolation is delayed by the filter; include the raw samples explicitly.
        for &s in samples {
            peak = peak.max((s as f64).abs());
        }
        self.block_peak = peak as f32;
        self.max = self.max.max(self.block_peak);
        self.block_peak
    }

    /// True peak of the last block in dBTP.
    pub fn block_dbtp(&self) -> f32 {
        gain_to_db(self.block_peak)
    }

    /// Highest true peak since the last reset in dBTP.
    pub fn max_dbtp(&self) -> f32 {
        gain_to_db(self.max)
    }

    /// The oversampling factor in use.
    pub fn factor(&self) -> usize {
        self.factor
    }
}
