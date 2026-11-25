//! Quasi-peak programme meter (PPM) after IEC 60268-10.
//!
//! Two coupled charge stages follow the rectified input: the first charges towards the input,
//! the second towards the remaining difference, so short bursts rise quickly while longer ones
//! settle with the slower first stage. Both stages fall at a constant rate in dB. The charge
//! rates were fitted to the standard 5 kHz tone-burst readings (see the tests); the reading
//! is normalised at preparation time so that a steady 5 kHz tone reads its peak level.

use crate::level::{gain_to_db, MINUS_INFINITY_DB};
use std::f64::consts::TAU;

/// Meter type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PpmType {
    /// Type I: integration time 5 ms, return 20 dB in 1.5 s.
    TypeI,
    /// Type II (EBU Tech 3205): a 10 ms burst reads -2 dB, return 24 dB in 2.8 s.
    TypeII,
}

impl PpmType {
    /// Charge rates of the two stages in 1/s.
    fn charge_rates(self) -> (f64, f64) {
        match self {
            Self::TypeI => (326.648, 697.108),
            Self::TypeII => (140.17, 704.062),
        }
    }

    /// Return rate in dB per second.
    pub fn return_rate(self) -> f64 {
        match self {
            Self::TypeI => 20.0 / 1.5,
            Self::TypeII => 24.0 / 2.8,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Detector {
    z1: f64,
    z2: f64,
}

impl Detector {
    #[inline]
    fn step(&mut self, x: f64, w1: f64, w2: f64, release: f64) {
        self.z1 *= release;
        self.z2 *= release;
        if x > self.z1 {
            self.z1 += w1 * (x - self.z1);
        }
        if x > self.z2 {
            self.z2 += w2 * (x - self.z1);
        }
    }

    #[inline]
    fn value(&self) -> f64 {
        self.z1 + self.z2
    }
}

/// Quasi-peak meter for one channel.
#[derive(Clone, Debug)]
pub struct PpmMeter {
    kind: PpmType,
    w1: f64,
    w2: f64,
    release: f64,
    gain: f64,
    detector: Detector,
}

impl PpmMeter {
    /// Creates a meter.
    pub fn new(sample_rate: f32, kind: PpmType) -> Self {
        let mut meter = Self {
            kind,
            w1: 0.0,
            w2: 0.0,
            release: 1.0,
            gain: 1.0,
            detector: Detector::default(),
        };
        meter.prepare(sample_rate);
        meter
    }

    /// Changes the sample rate, recalculates the ballistics and clears the reading.
    /// Runs a short simulation; call it outside the audio callback.
    pub fn prepare(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(1.0) as f64;
        let (r1, r2) = self.kind.charge_rates();
        self.w1 = (r1 / sr).min(1.0);
        self.w2 = (r2 / sr).min(1.0);
        self.release = 10f64.powf(-self.kind.return_rate() / 20.0 / sr);

        // Normalise to the steady 5 kHz reading.
        let mut d = Detector::default();
        let mut max = 0.0f64;
        let n = (0.5 * sr) as usize;
        for i in 0..n {
            let x = (TAU * 5000.0 * i as f64 / sr).sin().abs();
            d.step(x, self.w1, self.w2, self.release);
            if i > n / 2 {
                max = max.max(d.value());
            }
        }
        self.gain = if max > 0.0 { 1.0 / max } else { 1.0 };
        self.reset();
    }

    /// The meter type.
    pub fn kind(&self) -> PpmType {
        self.kind
    }

    /// Returns the reading to rest.
    pub fn reset(&mut self) {
        self.detector = Detector::default();
    }

    /// Processes a block of samples. Does not allocate.
    pub fn process(&mut self, samples: &[f32]) {
        let mut d = self.detector;
        for &s in samples {
            d.step((s as f64).abs(), self.w1, self.w2, self.release);
        }
        if d.z1.abs() < 1e-12 {
            d.z1 = 0.0;
        }
        if d.z2.abs() < 1e-12 {
            d.z2 = 0.0;
        }
        self.detector = d;
    }

    /// Reading in dBFS (peak level of a steady sine giving the same reading).
    pub fn level_db(&self) -> f32 {
        let v = self.detector.value() * self.gain;
        if v <= 0.0 {
            MINUS_INFINITY_DB
        } else {
            gain_to_db(v as f32)
        }
    }
}
