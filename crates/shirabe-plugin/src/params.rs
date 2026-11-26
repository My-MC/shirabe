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

/// Meter arrangement shown by the editor. Variant IDs are stored in projects: never change
/// them. Display-only, so it is not automatable.
#[derive(Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutPreset {
    /// The arrangement used before presets existed.
    #[id = "standard"]
    #[name = "Standard"]
    Standard,
    /// Peak and level stacked, with a larger scope and the read-out on the right.
    #[id = "stereo"]
    #[name = "Stereo"]
    Stereo,
    /// The Standard meters with the loudness history graph along the bottom. Appended after
    /// `stereo`, so saved projects and `standalone.json` keep their existing values.
    #[id = "history"]
    #[name = "History"]
    History,
    /// The Standard meters with the FFT / 1/3-octave spectrum along the bottom. Appended after
    /// `history`, so every stored layout value keeps its meaning.
    #[id = "spectrum"]
    #[name = "Spectrum"]
    Spectrum,
}

impl LayoutPreset {
    /// The geometry preset of the same name.
    pub fn geometry(self) -> shirabe_ui::layout::LayoutPreset {
        match self {
            Self::Standard => shirabe_ui::layout::LayoutPreset::Standard,
            Self::Stereo => shirabe_ui::layout::LayoutPreset::Stereo,
            Self::History => shirabe_ui::layout::LayoutPreset::History,
            Self::Spectrum => shirabe_ui::layout::LayoutPreset::Spectrum,
        }
    }
}

/// Spectrum display of the spectrum tile. Display-only, so it is not automatable.
#[derive(Enum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpectrumMode {
    /// One line per FFT bin.
    #[id = "fft"]
    #[name = "FFT"]
    #[default]
    Fft,
    /// One bar per 1/3-octave band.
    #[id = "rta"]
    #[name = "1/3 octave"]
    Rta,
}

impl SpectrumMode {
    /// The display mode of the same name.
    pub fn display(self) -> shirabe_ui::spectrum::Mode {
        match self {
            Self::Fft => shirabe_ui::spectrum::Mode::Fft,
            Self::Rta => shirabe_ui::spectrum::Mode::Rta,
        }
    }
}

/// Interface language. Variant IDs are stored in projects and standalone.json: never
/// change them. Display-only, so it is not automatable.
#[derive(Enum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UiLanguage {
    /// English, the default.
    #[id = "en"]
    #[name = "English"]
    #[default]
    English,
    /// Japanese.
    #[id = "ja"]
    #[name = "Japanese"]
    Japanese,
}

impl UiLanguage {
    /// The text catalogue language of the same name.
    pub fn lang(self) -> shirabe_ui::i18n::Lang {
        match self {
            Self::English => shirabe_ui::i18n::Lang::English,
            Self::Japanese => shirabe_ui::i18n::Lang::Japanese,
        }
    }

    /// The stored setting for a catalogue language.
    pub fn from_lang(lang: shirabe_ui::i18n::Lang) -> Self {
        match lang {
            shirabe_ui::i18n::Lang::English => Self::English,
            shirabe_ui::i18n::Lang::Japanese => Self::Japanese,
        }
    }
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

    /// Draw a fading trail behind the Lissajous figure.
    #[id = "scp_trail"]
    pub scope_trail: BoolParam,

    /// Fit the Lissajous figure to the signal instead of using the fixed ±1 scale.
    #[id = "scp_auto"]
    pub scope_auto_gain: BoolParam,

    /// Plot left on the horizontal and right on the vertical axis (vectorscope) instead of
    /// the rotated mid/side Lissajous figure. Display-only, so it is not automatable.
    #[id = "scp_lr"]
    pub scope_view_lr: BoolParam,

    /// Arrangement of the meter tiles. Display-only, so it is not automatable.
    #[id = "lay_preset"]
    pub layout_preset: EnumParam<LayoutPreset>,

    /// Interface language. Display-only, so it is not automatable. Appended after every
    /// older parameter so the host automation indices already in use do not move.
    #[id = "ui_lang"]
    pub ui_language: EnumParam<UiLanguage>,

    /// Spectrum display: FFT line or 1/3-octave bars. Display-only and appended last.
    #[id = "spc_mode"]
    pub spectrum_mode: EnumParam<SpectrumMode>,
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

            scope_trail: BoolParam::new("Lissajous Trail", true).non_automatable(),

            scope_auto_gain: BoolParam::new("Lissajous Auto Gain", false).non_automatable(),

            scope_view_lr: BoolParam::new("Scope L/R View", false).non_automatable(),

            layout_preset: EnumParam::new("Layout Preset", LayoutPreset::Standard)
                .non_automatable(),

            ui_language: EnumParam::new("UI Language", UiLanguage::English).non_automatable(),

            spectrum_mode: EnumParam::new("Spectrum Display", SpectrumMode::Fft).non_automatable(),
        }
    }
}

impl ShirabeParams {
    /// The text catalogue for the current setting and the available glyphs.
    pub fn text(&self, japanese_available: bool) -> shirabe_ui::i18n::Text {
        shirabe_ui::i18n::Text::new(self.ui_language.value().lang(), japanese_available)
    }

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

    #[test]
    fn the_vectorscope_switch_is_opt_in_non_automatable_and_keeps_its_index() {
        let params = ShirabeParams::default();
        assert!(!params.scope_view_lr.value());
        assert!(params
            .scope_view_lr
            .flags()
            .contains(ParamFlags::NON_AUTOMATABLE));
        let ids: Vec<String> = params
            .param_map()
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();
        // Appending keeps every host automation index already in use unchanged.
        let scp_lr = ids.iter().position(|id| id == "scp_lr").unwrap();
        assert_eq!(scp_lr, ids.len() - 4);
        assert_eq!(ids[ids.len() - 3], "lay_preset");
        assert_eq!(ids[ids.len() - 2], "ui_lang");
        assert_eq!(ids.last().map(String::as_str), Some("spc_mode"));
    }

    #[test]
    fn ui_language_is_english_by_default_non_automatable_and_appended() {
        let params = ShirabeParams::default();
        assert_eq!(params.ui_language.value(), UiLanguage::English);
        assert!(params
            .ui_language
            .flags()
            .contains(ParamFlags::NON_AUTOMATABLE));
        // The stored variant ids are permanent: projects and standalone.json hold them.
        assert_eq!(UiLanguage::ids(), Some(&["en", "ja"][..]));
        let ids: Vec<String> = params
            .param_map()
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();
        assert_eq!(ids[ids.len() - 2], "ui_lang");
        // Every older parameter kept its position; the new ID is only appended.
        assert_eq!(ids[ids.len() - 3], "lay_preset");
    }

    #[test]
    fn the_text_catalogue_follows_the_setting_and_the_available_glyphs() {
        let params = ShirabeParams::default();
        unsafe {
            params
                .ui_language
                .as_ptr()
                .set_normalized_value(params.ui_language.preview_normalized(UiLanguage::Japanese));
        }
        assert_eq!(params.text(true).lang(), shirabe_ui::i18n::Lang::Japanese);
        assert_eq!(params.text(true).reset(), "リセット");
        // Without a CJK font the stored preference is kept but English is drawn.
        assert_eq!(params.ui_language.value(), UiLanguage::Japanese);
        assert_eq!(params.text(false).lang(), shirabe_ui::i18n::Lang::English);
        assert_eq!(params.text(false).reset(), "Reset");
    }

    #[test]
    fn layout_preset_is_a_non_automatable_setting_defaulting_to_standard() {
        let params = ShirabeParams::default();
        assert_eq!(params.layout_preset.value(), LayoutPreset::Standard);
        assert!(params
            .layout_preset
            .flags()
            .contains(ParamFlags::NON_AUTOMATABLE));
        // The geometry enum lives in the UI crate and must stay in step with the stored one.
        assert_eq!(
            params.layout_preset.value().geometry(),
            shirabe_ui::layout::LayoutPreset::Standard
        );
        let ids: Vec<String> = params
            .param_map()
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();
        assert_eq!(ids[ids.len() - 3], "lay_preset");
        assert_eq!(ids[ids.len() - 2], "ui_lang");
    }

    #[test]
    fn the_history_layout_is_appended_with_a_stable_id() {
        let params = ShirabeParams::default();
        assert_eq!(params.layout_preset.value(), LayoutPreset::Standard);
        assert!(params
            .layout_preset
            .flags()
            .contains(ParamFlags::NON_AUTOMATABLE));
        // Appending keeps the stored IDs of the existing layouts; nih-plug stores enum
        // parameters by ID, and `standalone.json` stores the plain index.
        assert_eq!(
            LayoutPreset::ids(),
            Some(&["standard", "stereo", "history", "spectrum"][..])
        );
        assert_eq!(LayoutPreset::from_index(1), LayoutPreset::Stereo);
        assert_eq!(LayoutPreset::from_index(2), LayoutPreset::History);
        assert_eq!(
            LayoutPreset::History.geometry(),
            shirabe_ui::layout::LayoutPreset::History
        );
        // No new parameter: the existing IDs keep their positions.
        let ids: Vec<String> = params
            .param_map()
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();
        assert_eq!(ids[ids.len() - 3], "lay_preset");
        assert_eq!(ids[ids.len() - 2], "ui_lang");
    }

    #[test]
    fn the_spectrum_layout_and_display_are_appended_non_automatable_settings() {
        let params = ShirabeParams::default();
        // The stored variant ids are permanent, and the new preset is only appended.
        assert_eq!(
            LayoutPreset::ids(),
            Some(&["standard", "stereo", "history", "spectrum"][..])
        );
        assert_eq!(LayoutPreset::from_index(3), LayoutPreset::Spectrum);
        assert_eq!(
            LayoutPreset::Spectrum.geometry(),
            shirabe_ui::layout::LayoutPreset::Spectrum
        );

        assert_eq!(params.spectrum_mode.value(), SpectrumMode::Fft);
        assert!(params
            .spectrum_mode
            .flags()
            .contains(ParamFlags::NON_AUTOMATABLE));
        assert_eq!(SpectrumMode::ids(), Some(&["fft", "rta"][..]));

        let ids: Vec<String> = params
            .param_map()
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();
        assert_eq!(ids.last().map(String::as_str), Some("spc_mode"));
        assert_eq!(ids[ids.len() - 2], "ui_lang");
    }
}
