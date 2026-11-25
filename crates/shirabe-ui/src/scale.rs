//! Mapping from decibels to a position on a meter.

/// A meter scale: a piecewise-linear mapping from dB to a normalized position (0 = bottom,
/// 1 = top) plus the tick marks to label.
#[derive(Clone, Debug, PartialEq)]
pub struct MeterScale {
    /// Break points as `(dB, position)`, sorted by ascending dB and position.
    pub breakpoints: &'static [(f32, f32)],
    /// Tick marks in dB.
    pub ticks: &'static [f32],
    /// Label ticks with their sign (`+3`, `-20`) instead of the magnitude only.
    pub signed_labels: bool,
}

impl MeterScale {
    /// Sample peak scale in dBFS, -60 to 0, with the upper range expanded.
    pub const PEAK_DBFS: MeterScale = MeterScale {
        breakpoints: &[(-60.0, 0.0), (-40.0, 0.15), (-20.0, 0.45), (0.0, 1.0)],
        ticks: &[
            0.0, -3.0, -6.0, -9.0, -12.0, -18.0, -24.0, -30.0, -40.0, -50.0, -60.0,
        ],
        signed_labels: false,
    };

    /// VU scale, -20 to +3 VU, with the classic expanded region around 0 VU.
    pub const VU: MeterScale = MeterScale {
        breakpoints: &[
            (-20.0, 0.0),
            (-10.0, 0.30),
            (-5.0, 0.55),
            (0.0, 0.85),
            (3.0, 1.0),
        ],
        ticks: &[
            3.0, 2.0, 1.0, 0.0, -1.0, -2.0, -3.0, -5.0, -7.0, -10.0, -20.0,
        ],
        signed_labels: true,
    };

    /// Type I programme meter scale: dB relative to the permitted maximum level, -50 to +5.
    pub const PPM_TYPE_I: MeterScale = MeterScale {
        breakpoints: &[
            (-50.0, 0.0),
            (-40.0, 0.1),
            (-30.0, 0.25),
            (-20.0, 0.45),
            (-10.0, 0.7),
            (0.0, 0.92),
            (5.0, 1.0),
        ],
        ticks: &[5.0, 0.0, -5.0, -10.0, -20.0, -30.0, -40.0, -50.0],
        signed_labels: true,
    };

    /// Type II programme meter scale: dB relative to the alignment level, -12 to +12.
    pub const PPM_TYPE_II: MeterScale = MeterScale {
        breakpoints: &[(-12.0, 0.0), (12.0, 1.0)],
        ticks: &[12.0, 8.0, 4.0, 0.0, -4.0, -8.0, -12.0],
        signed_labels: true,
    };

    /// Loudness scale in LUFS, -60 to 0, expanded between -40 and -10.
    pub const LOUDNESS: MeterScale = MeterScale {
        breakpoints: &[(-60.0, 0.0), (-40.0, 0.15), (-10.0, 0.9), (0.0, 1.0)],
        ticks: &[0.0, -10.0, -14.0, -18.0, -23.0, -28.0, -33.0, -40.0, -60.0],
        signed_labels: false,
    };

    /// Formats a tick label.
    pub fn label(&self, tick: f32) -> String {
        if self.signed_labels && tick > 0.0 {
            format!("+{}", tick as i32)
        } else if self.signed_labels {
            format!("{}", tick as i32)
        } else {
            format!("{}", tick.abs() as i32)
        }
    }

    /// Lowest dB on the scale.
    pub fn min_db(&self) -> f32 {
        self.breakpoints.first().map_or(-60.0, |b| b.0)
    }

    /// Highest dB on the scale.
    pub fn max_db(&self) -> f32 {
        self.breakpoints.last().map_or(0.0, |b| b.0)
    }

    /// Maps dB to a position in 0.0..=1.0, clamped at both ends.
    pub fn position(&self, db: f32) -> f32 {
        let points = self.breakpoints;
        if points.is_empty() {
            return 0.0;
        }
        if db <= points[0].0 {
            return points[0].1;
        }
        for pair in points.windows(2) {
            let (db0, p0) = pair[0];
            let (db1, p1) = pair[1];
            if db <= db1 {
                let t = (db - db0) / (db1 - db0);
                return p0 + t * (p1 - p0);
            }
        }
        points[points.len() - 1].1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_and_clamping() {
        let s = MeterScale::PEAK_DBFS;
        assert_eq!(s.position(-60.0), 0.0);
        assert_eq!(s.position(-200.0), 0.0);
        assert_eq!(s.position(0.0), 1.0);
        assert_eq!(s.position(12.0), 1.0);
    }

    #[test]
    fn monotonic() {
        for s in [
            MeterScale::PEAK_DBFS,
            MeterScale::VU,
            MeterScale::PPM_TYPE_I,
            MeterScale::PPM_TYPE_II,
            MeterScale::LOUDNESS,
        ] {
            let mut last = -1.0;
            for i in 0..=800 {
                let p = s.position(-70.0 + i as f32 * 0.1);
                assert!(p >= last);
                last = p;
            }
        }
    }

    #[test]
    fn vu_labels_are_signed() {
        assert_eq!(MeterScale::VU.label(3.0), "+3");
        assert_eq!(MeterScale::VU.label(-20.0), "-20");
        assert_eq!(MeterScale::PEAK_DBFS.label(-18.0), "18");
    }
}
