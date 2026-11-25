//! Meter widgets, scales and themes for the Shirabe meter suite.
//!
//! Widgets only draw: they take plain values (levels in dB) and never touch audio state, so the
//! same widget can be used by the plugin editor, the standalone application and tests.

pub mod bar;
pub mod scale;
pub mod theme;

pub use nih_plug_egui::egui;

pub use bar::{draw_level_bar, draw_marker, draw_scale_labels};
pub use scale::MeterScale;
pub use theme::{zone_at, Theme, Zone};

/// Formats a level for read-outs, with `-inf` for silence.
pub fn format_db(db: f32) -> String {
    if !db.is_finite() || db <= -100.0 {
        "-inf".to_owned()
    } else {
        format!("{db:+.1}")
    }
}

/// Formats an optional loudness value (LUFS / LU) with one decimal, `--` when unavailable.
pub fn format_lufs(value: Option<f32>) -> String {
    match value {
        Some(v) if v.is_finite() && v > -100.0 => format!("{v:.1}"),
        Some(_) => "-inf".to_owned(),
        None => "--".to_owned(),
    }
}
