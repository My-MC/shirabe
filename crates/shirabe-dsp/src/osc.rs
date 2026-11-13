//! Test signal generators.

use std::f64::consts::TAU;

/// A sine oscillator with a double precision phase accumulator and peak amplitude 1.0.
#[derive(Clone, Debug)]
pub struct SineOscillator {
    phase: f64,
    increment: f64,
}

impl SineOscillator {
    /// Creates an oscillator at `frequency` Hz for the given sample rate.
    pub fn new(frequency: f32, sample_rate: f32) -> Self {
        let mut osc = Self {
            phase: 0.0,
            increment: 0.0,
        };
        osc.set_frequency(frequency, sample_rate);
        osc
    }

    /// Changes the frequency without resetting the phase (no clicks).
    pub fn set_frequency(&mut self, frequency: f32, sample_rate: f32) {
        self.increment = frequency as f64 / sample_rate.max(1.0) as f64;
    }

    /// Restarts at phase zero.
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Returns the next sample in the range -1.0..=1.0.
    #[inline]
    pub fn next_sample(&mut self) -> f32 {
        let value = (self.phase * TAU).sin() as f32;
        self.phase += self.increment;
        self.phase -= self.phase.floor();
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequency_from_zero_crossings() {
        let sr = 48_000.0;
        let mut osc = SineOscillator::new(1000.0, sr);
        let samples: Vec<f32> = (0..48_000).map(|_| osc.next_sample()).collect();
        let rising = samples
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        assert!((999..=1000).contains(&rising), "{rising}");
    }

    #[test]
    fn amplitude_is_unity() {
        let mut osc = SineOscillator::new(1000.0, 48_000.0);
        let peak = (0..4800)
            .map(|_| osc.next_sample().abs())
            .fold(0.0f32, f32::max);
        assert!((peak - 1.0).abs() < 1e-3);
    }
}
