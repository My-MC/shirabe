//! Host-visible parameters and persisted state.
//!
//! Parameter IDs (`#[id = ...]`) and persist keys are part of saved projects and presets.
//! Never rename or reuse them; add new ones instead.

use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;

/// Initial value of the custom target in LUFS.
const CUSTOM_DEFAULT_LOUDNESS: f32 = -14.0;

/// Loudness targets. Variant IDs are stored in projects: never change them. Values and their
/// sources are listed in `docs/standards.md`.
#[derive(Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoudnessTarget {
    /// Japanese broadcast (ARIB TR-B32 §2.1.4, §2.1.5, §2.2.4): -24 LKFS ±1 dB, -1 dBTP.
    #[id = "arib"]
    #[name = "ARIB TR-B32 (-24 LKFS)"]
    AribTrB32,
    /// European broadcast (EBU R 128 h, m): -23 LUFS, ±1.0 LU where the target cannot be met
    /// in practice (e.g. live), -1 dBTP.
    #[id = "ebu"]
    #[name = "EBU R128 (-23 LUFS)"]
    EbuR128,
    /// North American broadcast (ATSC A/85 §6): -24 LKFS ±2 dB, true peak below -2 dBTP.
    #[id = "atsc"]
    #[name = "ATSC A/85 (-24 LKFS)"]
    AtscA85,
    /// Spotify loudness normalization (Spotify for Artists): -14 LUFS, true peak below -1 dBTP.
    /// Spotify publishes no tolerance.
    #[id = "stream14"]
    #[name = "Spotify (-14 LUFS)"]
    Streaming14,
    /// Apple Podcasts (Apple Podcasts for Creators, "Audio requirements"): -16 LKFS ±1 dB,
    /// true peak not above -1 dBFS, measured per ITU-R BS.1770-5.
    #[id = "applepod"]
    #[name = "Apple Podcasts (-16 LKFS)"]
    ApplePodcasts,
    /// User-defined target, true-peak ceiling and tolerance (the `ld_c_*` parameters). Not
    /// taken from any document: for services that publish no target.
    #[id = "custom"]
    #[name = "Custom"]
    Custom,
}

/// A loudness target with its tolerance and true-peak ceiling, resolved from the preset or the
/// custom parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetSpec {
    /// The preset.
    pub preset: LoudnessTarget,
    /// Target integrated loudness in LUFS.
    pub loudness: f32,
    /// Permitted tolerance in LU, if any.
    pub tolerance: Option<f32>,
    /// Maximum true-peak level in dBTP.
    pub true_peak_ceiling: f32,
}

impl TargetSpec {
    /// Unit name used by the document for absolute loudness values.
    pub fn unit(&self) -> &'static str {
        self.preset.unit()
    }
}

impl LoudnessTarget {
    /// Target integrated loudness in LUFS.
    pub fn loudness(self) -> f32 {
        match self {
            Self::AribTrB32 | Self::AtscA85 => -24.0,
            Self::EbuR128 => -23.0,
            Self::Streaming14 => -14.0,
            Self::ApplePodcasts => -16.0,
            Self::Custom => CUSTOM_DEFAULT_LOUDNESS,
        }
    }

    /// Maximum true-peak level in dBTP.
    pub fn true_peak_ceiling(self) -> f32 {
        match self {
            Self::AtscA85 => -2.0,
            _ => -1.0,
        }
    }

    /// Permitted tolerance around the target in LU, if the document defines one.
    pub fn tolerance(self) -> Option<f32> {
        match self {
            Self::AribTrB32 | Self::EbuR128 | Self::ApplePodcasts => Some(1.0),
            Self::AtscA85 => Some(2.0),
            Self::Streaming14 | Self::Custom => None,
        }
    }

    /// Unit name used by the document for absolute loudness values.
    pub fn unit(self) -> &'static str {
        match self {
            Self::AribTrB32 | Self::AtscA85 | Self::ApplePodcasts => "LKFS",
            Self::EbuR128 | Self::Streaming14 | Self::Custom => "LUFS",
        }
    }
}

/// Loudness scale (EBU Tech 3341 §2.7). Variant IDs are stored in projects: never change them.
#[derive(Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoudnessScale {
    /// -18 to +9 LU around the target (default).
    #[id = "ebu9"]
    #[name = "EBU +9"]
    Ebu9,
    /// -36 to +18 LU around the target.
    #[id = "ebu18"]
    #[name = "EBU +18"]
    Ebu18,
}

/// Loudness unit: absolute or relative to the target (EBU Tech 3341 §2.7, §2.8).
#[derive(Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoudnessUnit {
    /// LUFS (LKFS for presets whose document uses that name).
    #[id = "abs"]
    #[name = "Absolute"]
    Absolute,
    /// LU, 0 LU = target.
    #[id = "rel"]
    #[name = "Relative (LU)"]
    Relative,
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
    /// Type IIb quasi-peak programme meter with the EBU scale (IEC 60268-10, EBU Tech 3205).
    #[id = "ppm2"]
    #[name = "PPM Type IIb (EBU)"]
    PpmTypeII,
    /// Type IIa quasi-peak programme meter with the BBC scale 1 to 7 (IEC 60268-10).
    #[id = "ppm2bbc"]
    #[name = "PPM Type IIa (BBC)"]
    PpmTypeIIBbc,
    /// Quasi-peak programme meter with the preferred IEEE scale (IEEE Std 152-1991 §1.3.16,
    /// Fig 5). The dynamics are the same as Type II (EBU Tech 3205 normal mode).
    #[id = "ppm2ieee"]
    #[name = "PPM (IEEE 152)"]
    PpmIeee,
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

    /// Use a moving needle instead of bars when the VU meter is selected.
    #[id = "vu_needle"]
    pub vu_needle: BoolParam,

    /// Loudness target used for colouring and the target marker.
    #[id = "ld_tgt"]
    pub loudness_target: EnumParam<LoudnessTarget>,

    /// Custom target loudness in LUFS (used with `LoudnessTarget::Custom`).
    #[id = "ld_c_lvl"]
    pub custom_loudness: FloatParam,

    /// Custom tolerance in LU; 0 means no tolerance.
    #[id = "ld_c_tol"]
    pub custom_tolerance: FloatParam,

    /// Custom true-peak ceiling in dBTP.
    #[id = "ld_c_tp"]
    pub custom_true_peak: FloatParam,

    /// Loudness scale.
    #[id = "ld_scale"]
    pub loudness_scale: EnumParam<LoudnessScale>,

    /// Loudness unit.
    #[id = "ld_unit"]
    pub loudness_unit: EnumParam<LoudnessUnit>,

    /// Reset integrated loudness, loudness range and maxima when the host transport starts.
    #[id = "auto_rst"]
    pub auto_reset: BoolParam,
}

impl Default for ShirabeParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(900, 560),

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

            vu_needle: BoolParam::new("VU Needle Display", false).non_automatable(),

            level_mode: EnumParam::new("Level Meter", LevelMode::Vu),

            loudness_target: EnumParam::new("Loudness Target", LoudnessTarget::AribTrB32),

            custom_loudness: FloatParam::new(
                "Custom Target",
                CUSTOM_DEFAULT_LOUDNESS,
                FloatRange::Linear {
                    min: -40.0,
                    max: -5.0,
                },
            )
            .with_step_size(0.5)
            .with_unit(" LUFS"),

            custom_tolerance: FloatParam::new(
                "Custom Tolerance",
                0.0,
                FloatRange::Linear { min: 0.0, max: 5.0 },
            )
            .with_step_size(0.5)
            .with_value_to_string(Arc::new(|v| {
                if v <= 0.0 {
                    "none".to_owned()
                } else {
                    format!("±{v:.1} LU")
                }
            }))
            .with_string_to_value(Arc::new(|s| {
                let s = s.trim();
                if s.eq_ignore_ascii_case("none") {
                    return Some(0.0);
                }
                s.trim_start_matches(['±', '+'])
                    .trim_end_matches("LU")
                    .trim()
                    .parse()
                    .ok()
            })),

            custom_true_peak: FloatParam::new(
                "Custom True-Peak Ceiling",
                -1.0,
                FloatRange::Linear {
                    min: -10.0,
                    max: 0.0,
                },
            )
            .with_step_size(0.5)
            .with_unit(" dBTP")
            // Adding 0.0 turns -0.0 into 0.0, so the text round-trips.
            .with_value_to_string(Arc::new(|v| format!("{:.1}", v + 0.0))),

            loudness_scale: EnumParam::new("Loudness Scale", LoudnessScale::Ebu9),

            loudness_unit: EnumParam::new("Loudness Unit", LoudnessUnit::Absolute),

            auto_reset: BoolParam::new("Reset on Play", true),
        }
    }
}

impl ShirabeParams {
    /// The selected target, with the custom values applied when `Custom` is selected.
    pub fn target_spec(&self) -> TargetSpec {
        let preset = self.loudness_target.value();
        if preset == LoudnessTarget::Custom {
            let tol = self.custom_tolerance.value();
            TargetSpec {
                preset,
                loudness: self.custom_loudness.value(),
                tolerance: (tol > 0.0).then_some(tol),
                true_peak_ceiling: self.custom_true_peak.value(),
            }
        } else {
            TargetSpec {
                preset,
                loudness: preset.loudness(),
                tolerance: preset.tolerance(),
                true_peak_ceiling: preset.true_peak_ceiling(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_resolve_to_their_documented_values() {
        let params = ShirabeParams::default();
        let spec = params.target_spec();
        assert_eq!(spec.preset, LoudnessTarget::AribTrB32);
        assert_eq!(spec.loudness, -24.0);
        assert_eq!(spec.tolerance, Some(1.0));
        assert_eq!(spec.true_peak_ceiling, -1.0);
    }

    #[test]
    fn custom_defaults() {
        let params = ShirabeParams::default();
        assert_eq!(params.custom_loudness.value(), -14.0);
        assert_eq!(params.custom_tolerance.value(), 0.0);
        assert_eq!(params.custom_true_peak.value(), -1.0);
        assert_eq!(params.custom_tolerance.to_string(), "none");
        let p = &params.custom_tolerance;
        for text in ["none", "±0.5 LU", "1.5"] {
            assert!(p.string_to_normalized_value(text).is_some(), "{text}");
        }
    }
}
