//! Mapping from decibels to a position on a meter.

/// A meter scale: a piecewise-linear mapping from dB to a normalized position (0 = bottom,
/// 1 = top) plus the tick marks to label.
#[derive(Clone, Debug, PartialEq)]
pub struct MeterScale {
    /// Break points as `(dB, position)`, sorted by ascending dB and position.
    pub breakpoints: &'static [(f32, f32)],
    /// Tick marks in dB.
    pub ticks: &'static [f32],
}

impl MeterScale {
    /// Sample peak scale in dBFS, -60 to 0, with the upper range expanded.
    pub const PEAK_DBFS: MeterScale = MeterScale {
        breakpoints: &[(-60.0, 0.0), (-40.0, 0.15), (-20.0, 0.45), (0.0, 1.0)],
        ticks: &[
            0.0, -3.0, -6.0, -9.0, -12.0, -18.0, -24.0, -30.0, -40.0, -50.0, -60.0,
        ],
    };

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
        let s = MeterScale::PEAK_DBFS;
        let mut last = -1.0;
        for i in 0..=600 {
            let p = s.position(-60.0 + i as f32 * 0.1);
            assert!(p >= last);
            last = p;
        }
    }
}
