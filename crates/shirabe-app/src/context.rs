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
}
