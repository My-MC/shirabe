//! Meter widgets, scales and themes for the Shirabe meter suite.
//!
//! Widgets only draw: they take plain values (levels in dB) and never touch audio state, so the
//! same widget can be used by the plugin editor, the standalone application and tests.

pub mod bar;
pub mod scale;
pub mod theme;

pub use nih_plug_egui::egui;

pub use bar::{draw_level_bar, draw_scale_labels};
pub use scale::MeterScale;
pub use theme::Theme;

/// Formats a level for read-outs, with `-inf` for silence.
pub fn format_db(db: f32) -> String {
    if db <= -100.0 {
        "-inf".to_owned()
    } else {
        format!("{db:+.1}")
    }
}
