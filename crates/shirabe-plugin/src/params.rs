//! Host-visible parameters and persisted state.
//!
//! Parameter IDs (`#[id = ...]`) and persist keys are part of saved projects and presets.
//! Never rename or reuse them; add new ones instead.

use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;

/// Loudness targets. Variant names are stored in projects: never rename or reorder them.
#[derive(Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoudnessTarget {
    /// Japanese broadcast (ARIB TR-B32): -24 LKFS, -1 dBTP.
    #[id = "arib"]
    #[name = "ARIB TR-B32 (-24 LKFS)"]
    AribTrB32,
    /// European broadcast (EBU R128): -23 LUFS, -1 dBTP.
    #[id = "ebu"]
    #[name = "EBU R128 (-23 LUFS)"]
    EbuR128,
    /// North American broadcast (ATSC A/85): -24 LKFS, -2 dBTP.
    #[id = "atsc"]
    #[name = "ATSC A/85 (-24 LKFS)"]
    AtscA85,
    /// Typical music streaming normalization: -14 LUFS, -1 dBTP.
    #[id = "stream14"]
    #[name = "Streaming (-14 LUFS)"]
    Streaming14,
}

impl LoudnessTarget {
    /// Target integrated loudness in LUFS.
    pub fn loudness(self) -> f32 {
        match self {
            Self::AribTrB32 | Self::AtscA85 => -24.0,
            Self::EbuR128 => -23.0,
            Self::Streaming14 => -14.0,
        }
    }

    /// Maximum true-peak level in dBTP.
    pub fn true_peak_ceiling(self) -> f32 {
        match self {
            Self::AtscA85 => -2.0,
            _ => -1.0,
        }
    }

    /// Tolerance around the target in LU, for colouring read-outs.
    pub fn tolerance(self) -> f32 {
        match self {
            Self::EbuR128 => 1.0,
            Self::AtscA85 => 2.0,
            _ => 1.0,
        }
    }
}

/// Meter shown in the level section. Variant IDs are stored in projects: never change them.
#[derive(Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LevelMode {
    /// VU meter (IEC 60268-17).
    #[id = "vu"]
    #[name = "VU"]
    Vu,
    /// Type I quasi-peak programme meter (IEC 60268-10).
    #[id = "ppm1"]
    #[name = "PPM Type I"]
    PpmTypeI,
    /// Type II quasi-peak programme meter (IEC 60268-10).
    #[id = "ppm2"]
    #[name = "PPM Type II"]
    PpmTypeII,
}

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

    /// Sine peak level in dBFS that reads 0 VU.
    #[id = "vu_ref"]
    pub vu_reference: FloatParam,

    /// Meter shown in the level section.
    #[id = "lvl_mode"]
    pub level_mode: EnumParam<LevelMode>,

    /// Loudness target used for colouring and the target marker.
    #[id = "ld_tgt"]
    pub loudness_target: EnumParam<LoudnessTarget>,

    /// Reset integrated loudness, loudness range and maxima when the host transport starts.
    #[id = "auto_rst"]
    pub auto_reset: BoolParam,
}

impl Default for ShirabeParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(760, 460),

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

            vu_reference: FloatParam::new(
                "VU Reference",
                -18.0,
                FloatRange::Linear {
                    min: -24.0,
                    max: -8.0,
                },
            )
            .with_step_size(1.0)
            .with_unit(" dBFS"),

            level_mode: EnumParam::new("Level Meter", LevelMode::Vu),

            loudness_target: EnumParam::new("Loudness Target", LoudnessTarget::AribTrB32),

            auto_reset: BoolParam::new("Reset on Play", true),
        }
    }
}
