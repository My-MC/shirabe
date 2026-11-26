//! Stereo correlation and left/right balance over a rolling window.
//!
//! Both readings describe the same recent window (see [`WINDOW_SECONDS`]) and answer different
//! questions: correlation asks about phase, balance about level.
//!
//! # Correlation
//!
//! The normalized signed cross product of the two channels,
//!
//! ```text
//! r = Σ(l·r) / sqrt(Σl² · Σr²)
//! ```
//!
//! with `+1` for identical (mono) content, `0` for unrelated content and `-1` for anti-phase
//! content. **No DC rejection is applied**: the products are taken from the raw samples, so a DC
//! offset present on both channels reads as perfectly correlated. Programme material carries no
//! DC, and a blocking filter would change the phase of very low frequencies, which is exactly
//! what this reading is meant to show.
//!
//! # Balance
//!
//! The relative channel energy, not the phase:
//!
//! ```text
//! b = (Σr² − Σl²) / (Σr² + Σl²)
//! ```
//!
//! with `-1` for a left-only signal, `0` for equal energies and `+1` for a right-only signal.
//! [`balance_to_db`] converts it to a level difference. Because it compares energies, an
//! anti-phase signal of equal level still reads `0` while its correlation reads `-1`.
//!
//! # Availability
//!
//! Both readings are `None` while the input is mono (the caller does not feed the meter then),
//! until the window has been filled with audio, and while both channels are below the silence
//! floor. Correlation additionally needs a usable signal on each channel: a one-sided signal
//! has no correlation (one channel is at the floor) but a balance of ±1.
//!
//! # Real-time behaviour
//!
//! The ring is allocated in [`StereoStatistics::new`] and [`StereoStatistics::prepare`] only;
//! [`StereoStatistics::process`] neither allocates nor locks. The window slides one sample at a
//! time, so the read-out is independent of how the stream is split into blocks (bit-exact).

/// Length of the measurement window in seconds.
///
/// 400 ms matches the momentary loudness window, so both readings describe the same span of
/// programme. It averages 24 cycles of a 60 Hz tone, which keeps correlation steady on bass
/// material, while still following a change within the 400 ms of one window.
pub const WINDOW_SECONDS: f64 = 0.4;

/// Upper bound for the window length in samples.
///
/// 0.4 s at 384 kHz is 153 600 samples. The cap protects against a nonsensical host sample
/// rate allocating an unbounded ring; above about 655 kHz the window becomes shorter than
/// [`WINDOW_SECONDS`].
pub const MAX_WINDOW_SAMPLES: usize = 262_144;

/// Silence floor as the mean square of one channel (about −120 dBFS RMS).
///
/// 1e-12 is far below any programme material and far above the noise floor of a digital
/// signal, so content this quiet carries no usable phase or balance information: both
/// readings are reported as unavailable instead of showing random values. Balance needs the
/// two channels together to exceed the floor; correlation needs each channel on its own to
/// exceed it.
pub const SILENCE_POWER: f64 = 1e-12;

/// One measurement window: correlation in `-1..=1`, balance in `-1..=1` (left .. right).
///
/// Unavailable readings are `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StereoStatsReadout {
    /// Normalized correlation: `+1` in phase, `0` unrelated, `-1` anti-phase.
    pub correlation: Option<f32>,
    /// Relative channel energy: `-1` left only, `0` equal, `+1` right only.
    pub balance: Option<f32>,
}

/// Converts a balance to the level difference between the channels in dB (positive: right).
///
/// A balance of ±1 (one channel only) has no finite ratio and returns ±∞.
pub fn balance_to_db(balance: f32) -> f32 {
    if balance >= 1.0 {
        f32::INFINITY
    } else if balance <= -1.0 {
        f32::NEG_INFINITY
    } else {
        10.0 * ((1.0 + balance) / (1.0 - balance)).log10()
    }
}

/// Rolling stereo correlation and balance meter.
#[derive(Clone, Debug)]
pub struct StereoStatistics {
    sample_rate: f64,
    /// Products `[l², r², l·r]` of the newest samples, oldest overwritten first. Stored as
    /// `f32` to halve the memory; the running sums are `f64`, and a sample is always added and
    /// later subtracted as the same stored value, so the sums stay consistent with the ring.
    ring: Vec<[f32; 3]>,
    pos: usize,
    filled: usize,
    sum_ll: f64,
    sum_rr: f64,
    sum_lr: f64,
    readout: StereoStatsReadout,
}

impl StereoStatistics {
    /// Creates a meter for `sample_rate`. Allocates the window; call it outside the audio
    /// callback.
    pub fn new(sample_rate: f32) -> Self {
        let mut meter = Self {
            sample_rate: sample_rate.max(1.0) as f64,
            ring: Vec::new(),
            pos: 0,
            filled: 0,
            sum_ll: 0.0,
            sum_rr: 0.0,
            sum_lr: 0.0,
            readout: StereoStatsReadout::default(),
        };
        meter.prepare(sample_rate);
        meter
    }

    /// Changes the sample rate and restarts the window. Allocates if the window length changes;
    /// call it outside the audio callback.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0) as f64;
        let samples = (self.sample_rate * WINDOW_SECONDS).round().max(1.0) as usize;
        self.ring = vec![[0.0; 3]; samples.min(MAX_WINDOW_SAMPLES)];
        self.reset();
    }

    /// Discards the window and every reading.
    pub fn reset(&mut self) {
        // Old slots are overwritten before they can contribute to a new window.
        self.pos = 0;
        self.filled = 0;
        self.sum_ll = 0.0;
        self.sum_rr = 0.0;
        self.sum_lr = 0.0;
        self.readout = StereoStatsReadout::default();
    }

    /// Measures one block of stereo frames. The shorter of the two slices decides the length.
    ///
    /// Does not allocate or lock. Extra channels, a missing right channel and mono sources are
    /// the caller's concern: nothing is measured until both slices carry samples.
    pub fn process(&mut self, left: &[f32], right: &[f32]) {
        let len = self.ring.len();
        let frames = left.len().min(right.len());
        if len == 0 || frames == 0 {
            return;
        }
        for i in 0..frames {
            let l = left[i];
            let r = right[i];
            let new = [l * l, r * r, l * r];
            let previous = std::mem::replace(&mut self.ring[self.pos], new);
            let old = if self.filled == len {
                previous
            } else {
                [0.0; 3]
            };
            self.sum_ll += f64::from(new[0]) - f64::from(old[0]);
            self.sum_rr += f64::from(new[1]) - f64::from(old[1]);
            self.sum_lr += f64::from(new[2]) - f64::from(old[2]);
            self.pos += 1;
            if self.pos == len {
                self.pos = 0;
            }
            if self.filled < len {
                self.filled += 1;
            }
        }
        self.update_readout();
    }

    fn update_readout(&mut self) {
        let mut readout = StereoStatsReadout::default();
        if self.filled == self.ring.len() && !self.ring.is_empty() {
            let n = self.ring.len() as f64;
            // The sums can be a few ULPs below zero after a long series of subtractions.
            let energy_l = (self.sum_ll / n).max(0.0);
            let energy_r = (self.sum_rr / n).max(0.0);
            let total = energy_l + energy_r;
            if total.is_finite() && total > SILENCE_POWER {
                let offset = (energy_r - energy_l) / total;
                readout.balance = Some((offset as f32).clamp(-1.0, 1.0));
            }
            // The denominator may not vanish, and both channels must carry a usable signal:
            // the phase of a channel at the silence floor is noise.
            if energy_l > SILENCE_POWER && energy_r > SILENCE_POWER {
                let denominator = (self.sum_ll * self.sum_rr).sqrt();
                if denominator > 0.0 {
                    let correlation = self.sum_lr / denominator;
                    if correlation.is_finite() {
                        readout.correlation = Some((correlation as f32).clamp(-1.0, 1.0));
                    }
                }
            }
        }
        self.readout = readout;
    }

    /// The current readings.
    pub fn readout(&self) -> StereoStatsReadout {
        self.readout
    }

    /// The sample rate the meter was prepared for.
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// Number of samples in the window.
    pub fn window_samples(&self) -> usize {
        self.ring.len()
    }

    /// Length of the window in seconds (shorter than [`WINDOW_SECONDS`] above the
    /// [`MAX_WINDOW_SAMPLES`] cap).
    pub fn window_seconds(&self) -> f64 {
        self.ring.len() as f64 / self.sample_rate
    }
}

impl Default for StereoStatistics {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}
