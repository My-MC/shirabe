//! Level conversions between linear gain and decibels.

/// The floor used for "silence" in decibels. Values at or below this are treated as -inf.
pub const MINUS_INFINITY_DB: f32 = -150.0;

/// Converts a linear amplitude (1.0 = 0 dBFS) to decibels, clamped at [`MINUS_INFINITY_DB`].
#[inline]
pub fn gain_to_db(gain: f32) -> f32 {
    if gain > 0.0 {
        (20.0 * gain.log10()).max(MINUS_INFINITY_DB)
    } else {
        MINUS_INFINITY_DB
    }
}

/// Converts decibels to a linear amplitude. Values at or below [`MINUS_INFINITY_DB`] give 0.0.
#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    if db > MINUS_INFINITY_DB {
        10f32.powf(db / 20.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unity_is_zero_db() {
        assert_eq!(gain_to_db(1.0), 0.0);
        assert_eq!(db_to_gain(0.0), 1.0);
    }

    #[test]
    fn half_amplitude_is_about_minus_six_db() {
        assert!((gain_to_db(0.5) + 6.0206).abs() < 1e-3);
    }

    #[test]
    fn silence_is_clamped() {
        assert_eq!(gain_to_db(0.0), MINUS_INFINITY_DB);
        assert_eq!(gain_to_db(-1.0), MINUS_INFINITY_DB);
        assert_eq!(db_to_gain(MINUS_INFINITY_DB), 0.0);
    }

    #[test]
    fn round_trip() {
        for db in [-120.0f32, -60.0, -18.0, -1.0, 0.0, 6.0] {
            assert!((gain_to_db(db_to_gain(db)) - db).abs() < 1e-3, "{db}");
        }
    }
}
