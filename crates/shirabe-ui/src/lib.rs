//! Meter widgets, scales and themes for the Shirabe meter suite.
//!
//! Widgets only draw: they take plain values (levels in dB) and never touch audio state, so the
//! same widget can be used by the plugin editor, the standalone application and tests.

pub mod bar;
pub mod history;
pub mod i18n;
pub mod layout;
pub mod scale;
pub mod scope;
pub mod spectrum;
pub mod theme;
pub mod tile;
pub mod view;
pub mod vu_dial;

pub use nih_plug_egui::egui;

pub use bar::{draw_level_bar, draw_marker, draw_scale_labels};
pub use scale::{MeterScale, ScaleLaw, TickLabels};
pub use theme::{zone_at, Theme, Zone};
pub use tile::TileId;
pub use view::{MeterView, MeterViewRegistry, RegistryError};

/// Rounds to one decimal and turns `-0.0` into `0.0`, so read-outs never show "-0.0".
fn round_tenth(v: f32) -> f32 {
    let r = (v * 10.0).round() / 10.0;
    if r == 0.0 {
        0.0
    } else {
        r
    }
}

/// Formats a level for read-outs, with `-inf` for silence.
pub fn format_db(db: f32) -> String {
    if !db.is_finite() || db <= -100.0 {
        "-inf".to_owned()
    } else {
        format!("{:+.1}", round_tenth(db))
    }
}

/// Formats an optional loudness value (LUFS / LU) with one decimal, `--` when unavailable.
pub fn format_lufs(value: Option<f32>) -> String {
    match value {
        Some(v) if v.is_finite() && v > -100.0 => format!("{:.1}", round_tenth(v)),
        Some(_) => "-inf".to_owned(),
        None => "--".to_owned(),
    }
}

/// Formats a loudness difference in LU with its sign, or `--` when either side is
/// unavailable. Silence and other non-finite values are not differences, so they read as
/// dashes instead of a huge boost.
pub fn format_delta_lu(value: Option<f32>) -> String {
    match value {
        Some(v) if v.is_finite() => format!("{:+.*}", 1, round_tenth(v)),
        _ => "--".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readouts_never_show_negative_zero() {
        assert_eq!(format_db(-0.01), "+0.0");
        assert_eq!(format_db(-0.04), "+0.0");
        assert_eq!(format_db(-0.06), "-0.1");
        assert_eq!(format_lufs(Some(-0.02)), "0.0");
    }

    #[test]
    fn silence_and_missing_values() {
        assert_eq!(format_db(-150.0), "-inf");
        assert_eq!(format_lufs(None), "--");
    }

    #[test]
    fn loudness_differences_keep_their_sign_and_read_dashes_when_absent() {
        assert_eq!(format_delta_lu(Some(6.02)), "+6.0");
        assert_eq!(format_delta_lu(Some(-1.24)), "-1.2");
        // Rounded to zero must not print "-0.0".
        assert_eq!(format_delta_lu(Some(-0.02)), "+0.0");
        assert_eq!(format_delta_lu(None), "--");
        // Silence on either side is not a measurement, so it must not read as a huge boost.
        assert_eq!(format_delta_lu(Some(f32::NEG_INFINITY)), "--");
        assert_eq!(format_delta_lu(Some(f32::INFINITY)), "--");
        assert_eq!(format_delta_lu(Some(f32::NAN)), "--");
    }
}
