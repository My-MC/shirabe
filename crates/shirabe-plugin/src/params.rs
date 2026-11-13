//! Host-visible parameters and persisted state.
//!
//! Parameter IDs (`#[id = ...]`) and persist keys are part of saved projects and presets.
//! Never rename or reuse them; add new ones instead.

use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;

/// All parameters of the plugin.
#[derive(Params)]
pub struct ShirabeParams {
    /// Editor size, restored with the project.
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// Replaces the input with a sine tone for calibration and checks.
    #[id = "osc_on"]
    pub osc_enabled: BoolParam,

    /// Test oscillator level in dBFS (peak).
    #[id = "osc_lvl"]
    pub osc_level: FloatParam,

    /// Test oscillator frequency in Hz.
    #[id = "osc_frq"]
    pub osc_frequency: FloatParam,
}

impl Default for ShirabeParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(560, 380),

            osc_enabled: BoolParam::new("Test Oscillator", false),

            osc_level: FloatParam::new(
                "Oscillator Level",
                -18.0,
                FloatRange::Linear {
                    min: -60.0,
                    max: 0.0,
                },
            )
            .with_step_size(0.5)
            .with_unit(" dBFS"),

            osc_frequency: FloatParam::new(
                "Oscillator Frequency",
                1000.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 20_000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_value_to_string(formatters::v2s_f32_hz_then_khz(1))
            .with_string_to_value(formatters::s2v_f32_hz_then_khz()),
        }
    }
}
