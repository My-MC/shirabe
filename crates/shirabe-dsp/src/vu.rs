//! VU meter (IEC 60268-17): full-wave rectified signal driving a second-order (needle)
//! system that reaches 99 % of the steady reading in 300 ms with about 1.5 % overshoot.
//!
//! Requirements are taken from IEEE Std 152-1991 §2.3 (identical to IEEE Std 152-1953 and to
//! Chinn, Gannett and Morris, Proc. IRE, Jan. 1940). The rectifier works on the oversampled
//! signal (see [`crate::oversample`]) so that tones near the Nyquist frequency read correctly.

use crate::level::gain_to_db;
use crate::oversample::Oversampler;

/// Damping ratio giving 1.5 % overshoot: exp(-pi*z/sqrt(1-z^2)) = 0.015.
const DAMPING: f64 = 0.8007;
/// Target: the step response first reaches 99 % of its final value after this time.
const RISE_TIME_99: f64 = 0.3;

/// Step response of a unit second-order system with damping `z` and natural frequency `w`.
fn step_response(z: f64, w: f64, t: f64) -> f64 {
    let wd = w * (1.0 - z * z).sqrt();
    1.0 - (-z * w * t).exp() * ((wd * t).cos() + z / (1.0 - z * z).sqrt() * (wd * t).sin())
}

/// Natural frequency (rad/s) for which the step response first crosses 0.99 at 300 ms.
fn natural_frequency() -> f64 {
    let first_crossing = |w: f64| {
        let mut t = 0.0;
        while t < 2.0 {
            if step_response(DAMPING, w, t) >= 0.99 {
                return t;
            }
            t += 1e-5;
        }
        2.0
    };
    let (mut lo, mut hi) = (1.0, 100.0);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if first_crossing(mid) > RISE_TIME_99 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// VU meter for one channel.
#[derive(Clone, Debug)]
pub struct VuMeter {
    dt: f64,
    omega: f64,
    position: f64,
    velocity: f64,
    /// Rectified average of a sine at the reference level (reads 0 VU).
    reference: f64,
    os: Oversampler,
}

impl VuMeter {
    /// Creates a meter. `reference_dbfs` is the peak level of a sine that reads 0 VU
    /// (for example -18 dBFS).
    pub fn new(sample_rate: f32, reference_dbfs: f32) -> Self {
        let os = Oversampler::new(sample_rate);
        let mut meter = Self {
            dt: 1.0 / (sample_rate.max(1.0) as f64 * os.factor() as f64),
            omega: natural_frequency(),
            position: 0.0,
            velocity: 0.0,
            reference: 1.0,
            os,
        };
        meter.set_reference(reference_dbfs);
        meter
    }

    /// Changes the sample rate and clears the needle.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.os.prepare(sample_rate);
        self.dt = 1.0 / (sample_rate.max(1.0) as f64 * self.os.factor() as f64);
        self.reset();
    }

    /// Sets the sine peak level in dBFS that reads 0 VU.
    pub fn set_reference(&mut self, reference_dbfs: f32) {
        let peak = 10f64.powf(reference_dbfs as f64 / 20.0);
        self.reference = peak * 2.0 / std::f64::consts::PI;
    }

    /// Returns the needle to rest.
    pub fn reset(&mut self) {
        self.position = 0.0;
        self.velocity = 0.0;
        self.os.reset();
    }

    /// Processes a block of samples.
    pub fn process(&mut self, samples: &[f32]) {
        self.process_segment(samples, true);
    }

    /// Processes part of a host block so the caller can sample the live detector state.
    /// Set `end_of_block` only on the last segment to preserve block-end cleanup.
    pub fn process_segment(&mut self, samples: &[f32], end_of_block: bool) {
        let w2 = self.omega * self.omega;
        let damping = 2.0 * DAMPING * self.omega;
        // Semi-implicit Euler is stable here: omega * dt is far below 1 at audio rates.
        let dt = self.dt;
        let (mut position, mut velocity) = (self.position, self.velocity);
        for &s in samples {
            self.os.push(s as f64, |y| {
                let accel = w2 * (y.abs() - position) - damping * velocity;
                velocity += accel * dt;
                position += velocity * dt;
            });
        }
        self.position = position;
        self.velocity = velocity;
        if end_of_block {
            if self.position.abs() < 1e-20 {
                self.position = 0.0;
            }
            if self.velocity.abs() < 1e-20 {
                self.velocity = 0.0;
            }
        }
    }

    /// Needle position as a linear ratio to the 0 VU deflection.
    pub fn ratio(&self) -> f64 {
        (self.position / self.reference).max(0.0)
    }

    /// Needle position in VU (dB relative to 0 VU).
    pub fn vu(&self) -> f32 {
        gain_to_db(self.ratio() as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_frequency_meets_rise_time() {
        let w = natural_frequency();
        let t = (0..200_000)
            .map(|i| i as f64 * 1e-5)
            .find(|&t| step_response(DAMPING, w, t) >= 0.99)
            .unwrap();
        assert!((t - 0.3).abs() < 1e-3, "{t}");
    }
}
