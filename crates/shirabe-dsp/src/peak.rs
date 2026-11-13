//! Sample peak metering with display ballistics.

use crate::level::{gain_to_db, MINUS_INFINITY_DB};

/// Display ballistics for a sample peak meter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PeakBallistics {
    /// How fast the displayed level falls after the signal drops, in dB per second.
    pub release_db_per_second: f32,
    /// How long the peak-hold marker stays before it starts following the level, in seconds.
    pub hold_seconds: f32,
}

impl Default for PeakBallistics {
    /// Falls 20 dB in 1.7 s (the usual digital programme meter return time) with a 2 s hold.
    fn default() -> Self {
        Self {
            release_db_per_second: 20.0 / 1.7,
            hold_seconds: 2.0,
        }
    }
}

/// Sample peak meter for one channel.
///
/// Feed it with [`PeakMeter::process`] on the audio thread, then read the display values.
/// The instantaneous attack is sample accurate; the release and hold are applied per block,
/// which is sufficient for display purposes.
#[derive(Clone, Debug)]
pub struct PeakMeter {
    sample_rate: f32,
    ballistics: PeakBallistics,
    level_db: f32,
    hold_db: f32,
    hold_remaining_samples: u64,
    max_db: f32,
}

impl PeakMeter {
    /// Creates a meter for the given sample rate.
    pub fn new(sample_rate: f32, ballistics: PeakBallistics) -> Self {
        Self {
            sample_rate: sample_rate.max(1.0),
            ballistics,
            level_db: MINUS_INFINITY_DB,
            hold_db: MINUS_INFINITY_DB,
            hold_remaining_samples: 0,
            max_db: MINUS_INFINITY_DB,
        }
    }

    /// Changes the sample rate and clears the state.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.reset();
    }

    /// Replaces the ballistics without clearing the state.
    pub fn set_ballistics(&mut self, ballistics: PeakBallistics) {
        self.ballistics = ballistics;
    }

    /// Clears the display level, the hold marker and the maximum.
    pub fn reset(&mut self) {
        self.level_db = MINUS_INFINITY_DB;
        self.hold_db = MINUS_INFINITY_DB;
        self.hold_remaining_samples = 0;
        self.max_db = MINUS_INFINITY_DB;
    }

    /// Clears only the maximum since the last reset.
    pub fn reset_max(&mut self) {
        self.max_db = MINUS_INFINITY_DB;
    }

    /// Processes one block of samples. Does not allocate.
    pub fn process(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }

        let block_peak = samples.iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
        let block_peak_db = gain_to_db(block_peak);
        let block_len = samples.len() as u64;
        let block_seconds = samples.len() as f32 / self.sample_rate;

        let released = self.level_db - self.ballistics.release_db_per_second * block_seconds;
        self.level_db = block_peak_db.max(released).max(MINUS_INFINITY_DB);

        if block_peak_db >= self.hold_db {
            self.hold_db = block_peak_db;
            self.hold_remaining_samples = (self.ballistics.hold_seconds * self.sample_rate) as u64;
        } else if self.hold_remaining_samples > block_len {
            self.hold_remaining_samples -= block_len;
        } else {
            self.hold_remaining_samples = 0;
            self.hold_db = self.level_db;
        }

        self.max_db = self.max_db.max(block_peak_db);
    }

    /// The displayed level in dBFS, including the release ballistics.
    pub fn level_db(&self) -> f32 {
        self.level_db
    }

    /// The peak-hold marker in dBFS.
    pub fn hold_db(&self) -> f32 {
        self.hold_db
    }

    /// The highest sample peak since the last reset, in dBFS.
    pub fn max_db(&self) -> f32 {
        self.max_db
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osc::SineOscillator;

    const SR: f32 = 48_000.0;

    fn sine_block(amplitude: f32, len: usize) -> Vec<f32> {
        let mut osc = SineOscillator::new(997.0, SR);
        (0..len).map(|_| osc.next_sample() * amplitude).collect()
    }

    #[test]
    fn sine_peak_matches_amplitude() {
        let mut meter = PeakMeter::new(SR, PeakBallistics::default());
        meter.process(&sine_block(0.5, 4800));
        assert!((meter.level_db() - gain_to_db(0.5)).abs() < 0.01);
        assert!((meter.max_db() - gain_to_db(0.5)).abs() < 0.01);
    }

    #[test]
    fn release_falls_twenty_db_in_one_point_seven_seconds() {
        let mut meter = PeakMeter::new(SR, PeakBallistics::default());
        meter.process(&sine_block(1.0, 480));
        let start = meter.level_db();

        let silence = vec![0.0f32; 480];
        let blocks = (1.7 * SR / 480.0).round() as usize;
        for _ in 0..blocks {
            meter.process(&silence);
        }
        assert!((start - meter.level_db() - 20.0).abs() < 0.05);
        // The maximum is not affected by the release.
        assert!((meter.max_db() - start).abs() < 0.01);
    }

    #[test]
    fn hold_marker_stays_then_follows() {
        let ballistics = PeakBallistics {
            hold_seconds: 1.0,
            ..PeakBallistics::default()
        };
        let mut meter = PeakMeter::new(SR, ballistics);
        meter.process(&sine_block(1.0, 480));
        let peak = meter.hold_db();

        let silence = vec![0.0f32; 480];
        for _ in 0..90 {
            meter.process(&silence); // 0.9 s
        }
        assert_eq!(meter.hold_db(), peak);
        for _ in 0..20 {
            meter.process(&silence); // 1.1 s total
        }
        assert!(meter.hold_db() < peak);
        assert!((meter.hold_db() - meter.level_db()).abs() < 1.0);
    }

    #[test]
    fn reset_clears_everything() {
        let mut meter = PeakMeter::new(SR, PeakBallistics::default());
        meter.process(&sine_block(1.0, 480));
        meter.reset();
        assert_eq!(meter.level_db(), MINUS_INFINITY_DB);
        assert_eq!(meter.hold_db(), MINUS_INFINITY_DB);
        assert_eq!(meter.max_db(), MINUS_INFINITY_DB);
    }

    #[test]
    fn negative_samples_count() {
        let mut meter = PeakMeter::new(SR, PeakBallistics::default());
        meter.process(&[0.0, -0.25, 0.1]);
        assert!((meter.max_db() - gain_to_db(0.25)).abs() < 1e-4);
    }
}
