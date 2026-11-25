//! Second-order IIR section (transposed direct form II, double precision state).

/// Biquad coefficients, normalized so that `a0 == 1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BiquadCoefficients {
    /// Feed-forward coefficient b0.
    pub b0: f64,
    /// Feed-forward coefficient b1.
    pub b1: f64,
    /// Feed-forward coefficient b2.
    pub b2: f64,
    /// Feedback coefficient a1.
    pub a1: f64,
    /// Feedback coefficient a2.
    pub a2: f64,
}

/// A biquad filter with its own state.
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    c: BiquadCoefficients,
    s1: f64,
    s2: f64,
}

impl Biquad {
    /// Creates a filter with cleared state.
    pub fn new(c: BiquadCoefficients) -> Self {
        Self {
            c,
            s1: 0.0,
            s2: 0.0,
        }
    }

    /// Clears the filter state.
    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }

    /// Filters one sample.
    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let c = &self.c;
        let y = c.b0 * x + self.s1;
        self.s1 = c.b1 * x - c.a1 * y + self.s2;
        self.s2 = c.b2 * x - c.a2 * y;
        // Flush denormals so silence after a signal does not slow the audio thread down.
        if self.s1.abs() < 1e-30 {
            self.s1 = 0.0;
        }
        if self.s2.abs() < 1e-30 {
            self.s2 = 0.0;
        }
        y
    }
}
