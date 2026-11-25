//! Mapping from decibels to a position on a meter.
//!
//! The scales follow the documents listed in `docs/standards.md`. Where a document does not
//! define the position of the marks, the scale says so in its comment.

/// How values map to positions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScaleLaw {
    /// Piecewise linear in dB between `(dB, position)` points, sorted by ascending dB and
    /// position. Values outside the points are clamped.
    Breakpoints(&'static [(f32, f32)]),
    /// Position proportional to voltage (as on a VU meter): `10^((dB - full_scale_db) / 20)`.
    LinearVoltage {
        /// The value at full-scale deflection.
        full_scale_db: f32,
    },
}

/// How tick marks are labelled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TickLabels {
    /// Magnitude only (`18` for -18).
    Magnitude,
    /// With sign (`+3`, `-20`, `0`).
    Signed,
    /// One text per tick, in the order of `ticks`. An empty text draws the mark only.
    Text(&'static [&'static str]),
}

/// A meter scale: a mapping from a value (dB, VU, LU) to a normalized position
/// (0 = bottom, 1 = top) plus the tick marks to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeterScale {
    /// Mapping law.
    pub law: ScaleLaw,
    /// Lowest value shown (the bottom of the scale).
    pub min: f32,
    /// Tick marks.
    pub ticks: &'static [f32],
    /// Labels of the tick marks.
    pub labels: TickLabels,
}

impl MeterScale {
    /// Sample peak scale in dBFS, -60 to 0. IEC 60268-18 does not fix the mark positions in
    /// the available sources; the upper range is expanded for readability.
    pub const PEAK_DBFS: MeterScale = MeterScale {
        law: ScaleLaw::Breakpoints(&[(-60.0, 0.0), (-40.0, 0.15), (-20.0, 0.45), (0.0, 1.0)]),
        min: -60.0,
        ticks: &[
            0.0, -3.0, -6.0, -9.0, -12.0, -18.0, -24.0, -30.0, -40.0, -50.0, -60.0,
        ],
        labels: TickLabels::Magnitude,
    };

    /// VU scale (IEC 60268-17 / ANSI C16.5): marks from -20 to +3 VU, deflection
    /// proportional to voltage, 0 VU at 71 % of full scale (+3 VU at full scale).
    pub const VU: MeterScale = MeterScale {
        law: ScaleLaw::LinearVoltage { full_scale_db: 3.0 },
        min: -20.0,
        ticks: &[
            3.0, 2.0, 1.0, 0.0, -1.0, -2.0, -3.0, -5.0, -7.0, -10.0, -20.0,
        ],
        labels: TickLabels::Signed,
    };

    /// Type I programme meter scale (IEC 60268-10 Type I / DIN 45406): -50 to +5 dB relative
    /// to the permitted maximum level, marks -50, -40, -30, -20, -10, -5, 0, +5. The available
    /// sources do not give the mark positions; these break points approximate the usual
    /// expanded upper range.
    pub const PPM_TYPE_I: MeterScale = MeterScale {
        law: ScaleLaw::Breakpoints(&[
            (-50.0, 0.0),
            (-40.0, 0.1),
            (-30.0, 0.25),
            (-20.0, 0.45),
            (-10.0, 0.7),
            (0.0, 0.92),
            (5.0, 1.0),
        ]),
        min: -50.0,
        ticks: &[5.0, 0.0, -5.0, -10.0, -20.0, -30.0, -40.0, -50.0],
        labels: TickLabels::Signed,
    };

    /// EBU programme meter scale (IEC 60268-10 Type IIb, EBU Tech 3205): -12 to +12 dB around
    /// TEST, marks every 4 dB plus +9 (permitted maximum level). Positions from EBU Tech 3205
    /// Table 6 (% of full-scale deflection); the ends are extrapolated to 0 % and 100 %.
    pub const PPM_EBU: MeterScale = MeterScale {
        law: ScaleLaw::Breakpoints(&[
            (-15.33, 0.0),
            (-12.0, 0.10),
            (-8.0, 0.22),
            (-4.0, 0.35),
            (0.0, 0.51),
            (4.0, 0.67),
            (8.0, 0.80),
            (12.0, 0.93),
            (14.15, 1.0),
        ]),
        min: -15.33,
        ticks: &[12.0, 9.0, 8.0, 4.0, 0.0, -4.0, -8.0, -12.0],
        labels: TickLabels::Text(&["+12", "", "+8", "+4", "TEST", "-4", "-8", "-12"]),
    };

    /// BBC programme meter scale (IEC 60268-10 Type IIa): marks 1 to 7, 4 dB apart except
    /// 6 dB between 1 and 2; mark 4 is the alignment level. Values are dB relative to the
    /// alignment level. The BBC describes the scale as approximately logarithmic, so it is
    /// drawn linear in dB.
    pub const PPM_BBC: MeterScale = MeterScale {
        law: ScaleLaw::Breakpoints(&[(-16.0, 0.0), (14.0, 1.0)]),
        min: -16.0,
        ticks: &[12.0, 8.0, 4.0, 0.0, -4.0, -8.0, -14.0],
        labels: TickLabels::Text(&["7", "6", "5", "4", "3", "2", "1"]),
    };

    /// EBU Mode loudness scale 'EBU +9' (EBU Tech 3341 §2.7): -18 to +9 LU relative to the
    /// target.
    pub const LOUDNESS_EBU_9: MeterScale = MeterScale {
        law: ScaleLaw::Breakpoints(&[(-18.0, 0.0), (9.0, 1.0)]),
        min: -18.0,
        ticks: &[9.0, 6.0, 3.0, 0.0, -3.0, -6.0, -9.0, -12.0, -15.0, -18.0],
        labels: TickLabels::Signed,
    };

    /// EBU Mode loudness scale 'EBU +18' (EBU Tech 3341 §2.7): -36 to +18 LU relative to the
    /// target.
    pub const LOUDNESS_EBU_18: MeterScale = MeterScale {
        law: ScaleLaw::Breakpoints(&[(-36.0, 0.0), (18.0, 1.0)]),
        min: -36.0,
        ticks: &[
            18.0, 12.0, 6.0, 0.0, -6.0, -12.0, -18.0, -24.0, -30.0, -36.0,
        ],
        labels: TickLabels::Signed,
    };

    /// Formats the label of `ticks[index]`, adding `offset` to the value (used to show an
    /// LU scale in absolute LUFS).
    pub fn label(&self, index: usize, offset: f32) -> String {
        let tick = self.ticks[index] + offset;
        match self.labels {
            TickLabels::Magnitude => format!("{}", tick.abs().round() as i32),
            TickLabels::Signed if tick > 0.0 => format!("+{}", tick.round() as i32),
            TickLabels::Signed => format!("{}", tick.round() as i32),
            TickLabels::Text(texts) => texts.get(index).copied().unwrap_or("").to_owned(),
        }
    }

    /// Maps a value to a position in 0.0..=1.0, clamped at both ends.
    pub fn position(&self, value: f32) -> f32 {
        if value <= self.min {
            return match self.law {
                ScaleLaw::Breakpoints(points) => points.first().map_or(0.0, |p| p.1),
                ScaleLaw::LinearVoltage { full_scale_db } => {
                    db_to_ratio(self.min - full_scale_db).min(1.0)
                }
            };
        }
        match self.law {
            ScaleLaw::Breakpoints(points) => {
                if points.is_empty() {
                    return 0.0;
                }
                if value <= points[0].0 {
                    return points[0].1;
                }
                for pair in points.windows(2) {
                    let (v0, p0) = pair[0];
                    let (v1, p1) = pair[1];
                    if value <= v1 {
                        let t = (value - v0) / (v1 - v0);
                        return p0 + t * (p1 - p0);
                    }
                }
                points[points.len() - 1].1
            }
            ScaleLaw::LinearVoltage { full_scale_db } => {
                db_to_ratio(value - full_scale_db).clamp(0.0, 1.0)
            }
        }
    }

    /// Maps a position back to a value (inverse of [`MeterScale::position`]).
    pub fn value_at(&self, position: f32) -> f32 {
        match self.law {
            ScaleLaw::Breakpoints(points) => {
                for pair in points.windows(2) {
                    let (v0, p0) = pair[0];
                    let (v1, p1) = pair[1];
                    if position <= p1 {
                        let t = if p1 > p0 {
                            (position - p0) / (p1 - p0)
                        } else {
                            0.0
                        };
                        return v0 + t * (v1 - v0);
                    }
                }
                points.last().map_or(0.0, |p| p.0)
            }
            ScaleLaw::LinearVoltage { full_scale_db } => {
                if position <= 0.0 {
                    self.min
                } else {
                    (full_scale_db + 20.0 * position.log10()).max(self.min)
                }
            }
        }
    }
}

fn db_to_ratio(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> [MeterScale; 7] {
        [
            MeterScale::PEAK_DBFS,
            MeterScale::VU,
            MeterScale::PPM_TYPE_I,
            MeterScale::PPM_EBU,
            MeterScale::PPM_BBC,
            MeterScale::LOUDNESS_EBU_9,
            MeterScale::LOUDNESS_EBU_18,
        ]
    }

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
        for s in all() {
            let mut last = -1.0;
            for i in 0..=1000 {
                let p = s.position(-70.0 + i as f32 * 0.1);
                assert!(p >= last, "{s:?}");
                last = p;
            }
        }
    }

    #[test]
    fn value_at_inverts_position() {
        for s in all() {
            for &tick in s.ticks {
                let back = s.value_at(s.position(tick));
                assert!((back - tick).abs() < 0.01, "{s:?}: {tick} -> {back}");
            }
        }
    }

    /// The 100 % (0 VU) mark lies at about 0.71 of full-scale deflection.
    #[test]
    fn vu_zero_at_71_percent() {
        let p = MeterScale::VU.position(0.0);
        assert!((p - 0.708).abs() < 0.001, "{p}");
        assert!((MeterScale::VU.position(-20.0) - 0.0708).abs() < 0.001);
    }

    /// EBU Tech 3205 Table 6: TEST at 51 %, -12 at 10 %, +12 at 93 % of full scale.
    #[test]
    fn ebu_ppm_marks_follow_tech_3205() {
        let s = MeterScale::PPM_EBU;
        for (db, pct) in [
            (-12.0, 0.10),
            (-8.0, 0.22),
            (-4.0, 0.35),
            (0.0, 0.51),
            (4.0, 0.67),
            (8.0, 0.80),
            (12.0, 0.93),
        ] {
            assert!((s.position(db) - pct).abs() < 1e-6);
        }
        assert_eq!(s.label(4, 0.0), "TEST");
        assert_eq!(s.label(1, 0.0), "");
    }

    #[test]
    fn bbc_marks_are_4_db_apart_except_1_to_2() {
        let s = MeterScale::PPM_BBC;
        assert_eq!(s.ticks, &[12.0, 8.0, 4.0, 0.0, -4.0, -8.0, -14.0]);
        assert_eq!(s.label(3, 0.0), "4");
    }

    /// EBU Tech 3341 §2.7: 'EBU +9' is -18..+9 LU (-41..-14 LUFS at a -23 LUFS target).
    #[test]
    fn ebu_loudness_scales() {
        let s9 = MeterScale::LOUDNESS_EBU_9;
        assert_eq!(s9.position(-18.0), 0.0);
        assert_eq!(s9.position(9.0), 1.0);
        assert_eq!(s9.label(0, -23.0), "-14");
        assert_eq!(s9.label(9, -23.0), "-41");
        let s18 = MeterScale::LOUDNESS_EBU_18;
        assert_eq!(s18.position(-36.0), 0.0);
        assert_eq!(s18.position(18.0), 1.0);
        assert_eq!(s18.label(0, -23.0), "-5");
        assert_eq!(s18.label(9, -23.0), "-59");
    }

    #[test]
    fn signed_and_magnitude_labels() {
        assert_eq!(MeterScale::VU.label(0, 0.0), "+3");
        assert_eq!(MeterScale::VU.label(10, 0.0), "-20");
        assert_eq!(MeterScale::PEAK_DBFS.label(5, 0.0), "18");
    }
}
