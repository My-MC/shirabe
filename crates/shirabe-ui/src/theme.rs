//! Colours and zones. Kept as plain data so themes can later be loaded from files.

use crate::egui::Color32;

/// A colour zone on a meter: applies from `from_db` upwards until the next zone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zone {
    /// Lower edge of the zone in dB.
    pub from_db: f32,
    /// Lit segment colour.
    pub lit: Color32,
    /// Unlit segment colour.
    pub unlit: Color32,
}

/// Visual theme shared by all meter widgets.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// Window background.
    pub background: Color32,
    /// Background of a meter panel.
    pub panel: Color32,
    /// Primary text.
    pub text: Color32,
    /// Secondary text and tick labels.
    pub text_dim: Color32,
    /// Peak-hold marker.
    pub hold: Color32,
    /// Colour zones, sorted by ascending `from_db`.
    pub zones: Vec<Zone>,
    /// Height of one bar segment in points (0 draws a continuous bar).
    pub segment_height: f32,
    /// Gap between segments in points.
    pub segment_gap: f32,
}

impl Theme {
    /// Dark theme with segmented bars: green below -9 dBFS, amber to -3 dBFS, red above.
    pub fn dark() -> Self {
        Self {
            background: Color32::from_rgb(0x12, 0x14, 0x17),
            panel: Color32::from_rgb(0x1b, 0x1e, 0x22),
            text: Color32::from_rgb(0xe6, 0xe8, 0xeb),
            text_dim: Color32::from_rgb(0x8a, 0x91, 0x99),
            hold: Color32::from_rgb(0xff, 0xff, 0xff),
            zones: vec![
                Zone {
                    from_db: f32::NEG_INFINITY,
                    lit: Color32::from_rgb(0x3d, 0xd6, 0x6b),
                    unlit: Color32::from_rgb(0x16, 0x2e, 0x1f),
                },
                Zone {
                    from_db: -9.0,
                    lit: Color32::from_rgb(0xf2, 0xc1, 0x2e),
                    unlit: Color32::from_rgb(0x33, 0x2b, 0x12),
                },
                Zone {
                    from_db: -3.0,
                    lit: Color32::from_rgb(0xf0, 0x4a, 0x3c),
                    unlit: Color32::from_rgb(0x36, 0x16, 0x14),
                },
            ],
            segment_height: 3.0,
            segment_gap: 1.0,
        }
    }

    /// The zone that applies at `db`.
    pub fn zone_at(&self, db: f32) -> &Zone {
        self.zones
            .iter()
            .rev()
            .find(|z| db >= z.from_db)
            .unwrap_or(&self.zones[0])
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_are_picked_by_level() {
        let t = Theme::dark();
        assert_eq!(t.zone_at(-40.0).from_db, f32::NEG_INFINITY);
        assert_eq!(t.zone_at(-9.0).from_db, -9.0);
        assert_eq!(t.zone_at(-0.1).from_db, -3.0);
    }
}
