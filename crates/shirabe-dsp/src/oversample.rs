//! Polyphase interpolation used to reconstruct the signal between samples.
//!
//! The true-peak meter needs it (ITU-R BS.1770 Annex 2), and so do the rectifying meters
//! (PPM, VU): an analogue rectifier sees the continuous waveform, while rectifying only the
//! samples under-reads tones whose frequency divides the sample rate (for example 12 kHz at
//! 48 kHz reads up to 3 dB low). Oversampling first keeps them within the IEEE Std 152-1991
//! frequency-response tolerances up to 16 kHz.
//!
//! The factor is 4 below 96 kHz, 2 up to 192 kHz and 1 above, with a 32-tap-per-phase
//! Kaiser-windowed sinc filter designed at preparation time.

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

/// Streaming interpolator for one channel.
#[derive(Clone, Debug)]
pub struct Oversampler {
    factor: usize,
    phases: Vec<[f64; TAPS_PER_PHASE]>,
    history: [f64; TAPS_PER_PHASE],
    pos: usize,
}

impl Oversampler {
    /// Creates an interpolator for a sample rate.
    pub fn new(sample_rate: f32) -> Self {
        let mut o = Self {
            factor: 1,
            phases: Vec::with_capacity(MAX_FACTOR),
            history: [0.0; TAPS_PER_PHASE],
            pos: 0,
        };
        o.prepare(sample_rate);
        o
    }

    /// Changes the sample rate and clears the history. Allocates at most once per factor
    /// change (the capacity is reserved for the largest factor).
    pub fn prepare(&mut self, sample_rate: f32) {
        self.factor = oversampling_factor(sample_rate);
        self.phases.clear();
        self.phases.extend(design(self.factor));
        self.reset();
    }

    /// Clears the history.
    pub fn reset(&mut self) {
        self.history = [0.0; TAPS_PER_PHASE];
        self.pos = 0;
    }

    /// The oversampling factor in use.
    pub fn factor(&self) -> usize {
        self.factor
    }

    /// Pushes one input sample and calls `f` with each of the `factor` output samples.
    #[inline]
    pub fn push(&mut self, s: f64, mut f: impl FnMut(f64)) {
        self.history[self.pos] = s;
        self.pos = (self.pos + 1) % TAPS_PER_PHASE;
        if self.factor == 1 {
            f(s);
            return;
        }
        for phase in &self.phases {
            let mut acc = 0.0;
            // history[pos - 1 - t] is x[n - t]
            for (t, h) in phase.iter().enumerate() {
                let idx = (self.pos + TAPS_PER_PHASE - 1 - t) % TAPS_PER_PHASE;
                acc += h * self.history[idx];
            }
            f(acc);
        }
    }
}
