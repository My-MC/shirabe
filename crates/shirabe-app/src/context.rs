//! Parameter access for the shared editor code.
//!
//! The editor sets parameters through nih-plug's [`ParamSetter`], which needs a
//! [`GuiContext`]. In the standalone application there is no plugin host, so this context
//! writes the values directly; the audio thread reads them like a host-driven plugin does.

use std::collections::BTreeMap;

use nih_plug::prelude::*;

/// A [`GuiContext`] without a host.
pub struct StandaloneContext;

impl GuiContext for StandaloneContext {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Standalone
    }

    fn request_resize(&self) -> bool {
        false
    }

    unsafe fn raw_begin_set_parameter(&self, _param: ParamPtr) {}

    unsafe fn raw_set_parameter_normalized(&self, param: ParamPtr, normalized: f32) {
        // SAFETY: `param` comes from the `Params` object owned by the application, which
        // outlives every editor frame.
        unsafe {
            param.set_normalized_value(normalized);
        }
    }

    unsafe fn raw_end_set_parameter(&self, _param: ParamPtr) {}

    fn get_state(&self) -> PluginState {
        PluginState {
            version: String::new(),
            params: BTreeMap::new(),
            fields: BTreeMap::new(),
        }
    }

    fn set_state(&self, _state: PluginState) {}
}

/// Plain (unnormalized) values of every parameter, keyed by parameter ID. Enum parameters
/// store the variant index, which stays valid because variants are only ever appended.
pub fn save_params(params: &dyn Params) -> BTreeMap<String, f32> {
    params
        .param_map()
        .into_iter()
        // SAFETY: the pointers come from `params`, which is alive for this call.
        .map(|(id, ptr, _)| (id, unsafe { ptr.unmodulated_plain_value() }))
        .collect()
}

/// Restores values saved by [`save_params`]. Unknown IDs are ignored.
pub fn load_params(params: &dyn Params, values: &BTreeMap<String, f32>) {
    for (id, ptr, _) in params.param_map() {
        if let Some(&plain) = values.get(&id) {
            // SAFETY: as above.
            unsafe {
                let normalized = ptr.preview_normalized(plain);
                ptr.set_normalized_value(normalized);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shirabe_plugin::ShirabeParams;

    #[test]
    fn vu_needle_is_opt_in_and_survives_settings_roundtrip() {
        let params = ShirabeParams::default();
        assert_eq!(save_params(&params).get("vu_needle"), Some(&0.0));
        load_params(&params, &BTreeMap::from([("vu_needle".to_owned(), 1.0)]));
        let restored = ShirabeParams::default();
        load_params(&restored, &save_params(&params));
        assert_eq!(save_params(&restored).get("vu_needle"), Some(&1.0));
    }

    #[test]
    fn scope_display_settings_keep_their_ids_and_survive_a_roundtrip() {
        let params = ShirabeParams::default();
        let saved = save_params(&params);
        // The trail is on, the fixed scale is the default, and the Lissajous plane is selected.
        assert_eq!(saved.get("scp_trail"), Some(&1.0));
        assert_eq!(saved.get("scp_auto"), Some(&0.0));
        assert_eq!(saved.get("scp_lr"), Some(&0.0));

        load_params(
            &params,
            &BTreeMap::from([
                ("scp_trail".to_owned(), 0.0),
                ("scp_auto".to_owned(), 1.0),
                ("scp_lr".to_owned(), 1.0),
            ]),
        );
        let restored = ShirabeParams::default();
        load_params(&restored, &save_params(&params));
        assert_eq!(save_params(&restored).get("scp_trail"), Some(&0.0));
        assert_eq!(save_params(&restored).get("scp_auto"), Some(&1.0));
        assert_eq!(save_params(&restored).get("scp_lr"), Some(&1.0));
    }

    #[test]
    fn the_layout_preset_survives_the_standalone_settings_roundtrip() {
        let params = ShirabeParams::default();
        // 0 = Standard, the default that old settings files fall back to.
        assert_eq!(save_params(&params).get("lay_preset"), Some(&0.0));

        load_params(&params, &BTreeMap::from([("lay_preset".to_owned(), 1.0)]));
        let restored = ShirabeParams::default();
        load_params(&restored, &save_params(&params));
        assert_eq!(save_params(&restored).get("lay_preset"), Some(&1.0));
        assert_eq!(
            restored.layout_preset.value(),
            shirabe_plugin::params::LayoutPreset::Stereo
        );
    }

    #[test]
    fn a_settings_map_without_a_language_keeps_the_english_default() {
        use shirabe_plugin::params::UiLanguage;
        let params = ShirabeParams::default();
        // The shape of a standalone.json written before `ui_lang` existed.
        let old = BTreeMap::from([
            ("lay_preset".to_owned(), 1.0),
            ("lvl_mode".to_owned(), 2.0),
            ("ld_tgt".to_owned(), 1.0),
        ]);
        load_params(&params, &old);
        assert_eq!(params.ui_language.value(), UiLanguage::English);
        // The older values still load; only the new ID is absent.
        assert_eq!(
            params.layout_preset.value(),
            shirabe_plugin::params::LayoutPreset::Stereo
        );
    }

    #[test]
    fn the_language_roundtrips_and_changes_nothing_else() {
        use shirabe_plugin::params::UiLanguage;
        let params = ShirabeParams::default();
        load_params(
            &params,
            &BTreeMap::from([
                // The stored value is the variant index: 0 = English, 1 = Japanese.
                ("ui_lang".to_owned(), 1.0),
                ("lay_preset".to_owned(), 1.0),
                ("osc_on".to_owned(), 1.0),
            ]),
        );
        assert_eq!(params.ui_language.value(), UiLanguage::Japanese);

        let restored = ShirabeParams::default();
        load_params(&restored, &save_params(&params));
        assert_eq!(restored.ui_language.value(), UiLanguage::Japanese);
        // A view-only setting must not disturb the layout, the oscillator or the target.
        assert_eq!(
            restored.layout_preset.value(),
            shirabe_plugin::params::LayoutPreset::Stereo
        );
        assert!(restored.osc_enabled.value());
        assert_eq!(
            restored.loudness_target.value(),
            params.loudness_target.value()
        );
    }
}
