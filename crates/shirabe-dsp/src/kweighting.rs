//! K-weighting filter (ITU-R BS.1770): a high-shelf pre-filter followed by a high-pass
//! "RLB" filter. Coefficients are derived for any sample rate from the analogue prototypes,
//! which reproduce the tabulated 48 kHz coefficients of the recommendation.

use crate::biquad::{Biquad, BiquadCoefficients};
use std::f64::consts::PI;

/// Stage 1: high-shelf filter modelling the acoustic effect of the head.
pub fn shelf_coefficients(sample_rate: f64) -> BiquadCoefficients {
    let f0 = 1681.974450955533;
    let gain_db = 3.999843853973347;
    let q = 0.7071752369554196;

    let k = (PI * f0 / sample_rate).tan();
    let vh = 10f64.powf(gain_db / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    BiquadCoefficients {
        b0: (vh + vb * k / q + k * k) / a0,
        b1: 2.0 * (k * k - vh) / a0,
        b2: (vh - vb * k / q + k * k) / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    }
}

/// Stage 2: second-order high-pass filter (revised low-frequency B-weighting).
pub fn highpass_coefficients(sample_rate: f64) -> BiquadCoefficients {
    let f0 = 38.13547087602444;
    let q = 0.5003270373238773;

    let k = (PI * f0 / sample_rate).tan();
    let a0 = 1.0 + k / q + k * k;
    BiquadCoefficients {
        b0: 1.0,
        b1: -2.0,
        b2: 1.0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    }
}

/// The complete K-weighting filter for one channel.
#[derive(Clone, Debug)]
pub struct KWeighting {
    shelf: Biquad,
    highpass: Biquad,
}

impl KWeighting {
    /// Creates the filter for a sample rate.
    pub fn new(sample_rate: f64) -> Self {
        Self {
            shelf: Biquad::new(shelf_coefficients(sample_rate)),
            highpass: Biquad::new(highpass_coefficients(sample_rate)),
        }
    }

    /// Clears the filter state.
    pub fn reset(&mut self) {
        self.shelf.reset();
        self.highpass.reset();
    }

    /// Filters one sample.
    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        self.highpass.process(self.shelf.process(x))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Coefficients tabulated in ITU-R BS.1770 for 48 kHz.
    #[test]
    fn matches_bs1770_table_at_48k() {
        let s = shelf_coefficients(48_000.0);
        assert!((s.b0 - 1.53512485958697).abs() < 1e-9);
        assert!((s.b1 - -2.69169618940638).abs() < 1e-9);
        assert!((s.b2 - 1.19839281085285).abs() < 1e-9);
        assert!((s.a1 - -1.69065929318241).abs() < 1e-9);
        assert!((s.a2 - 0.73248077421585).abs() < 1e-9);

        let h = highpass_coefficients(48_000.0);
        assert!((h.a1 - -1.99004745483398).abs() < 1e-9);
        assert!((h.a2 - 0.99007225036621).abs() < 1e-9);
    }
}
