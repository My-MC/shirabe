//! True-peak level (ITU-R BS.1770 Annex 2): the peak of the signal reconstructed between
//! samples, estimated by oversampling with a polyphase interpolation filter.
//!
//! The recommendation requires at least 4x oversampling at 48 kHz. This implementation uses
//! 4x below 96 kHz, 2x up to 192 kHz and no oversampling above, with a 32-tap-per-phase
//! Kaiser-windowed sinc filter designed at construction time.

use crate::level::gain_to_db;
pub use crate::oversample::oversampling_factor;
use crate::oversample::Oversampler;

/// True-peak meter for one channel.
#[derive(Clone, Debug)]
pub struct TruePeakMeter {
    os: Oversampler,
    block_peak: f32,
    max: f32,
}

impl TruePeakMeter {
    /// Creates a meter for a sample rate.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            os: Oversampler::new(sample_rate),
            block_peak: 0.0,
            max: 0.0,
        }
    }

    /// Changes the sample rate and clears the state.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.os.prepare(sample_rate);
        self.reset();
    }

    /// Clears the filter history and the maximum.
    pub fn reset(&mut self) {
        self.os.reset();
        self.block_peak = 0.0;
        self.max = 0.0;
    }

    /// Discards interpolation history at a programme boundary, preserving the held maximum.
    pub fn clear_history(&mut self) {
        self.os.reset();
    }

    /// Clears only the maximum.
    pub fn reset_max(&mut self) {
        self.max = 0.0;
    }

    /// Processes a block and returns its true peak (linear).
    pub fn process(&mut self, samples: &[f32]) -> f32 {
        let mut peak = 0.0f64;
        for &s in samples {
            self.os.push(s as f64, |y| peak = peak.max(y.abs()));
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
        self.os.factor()
    }
}
