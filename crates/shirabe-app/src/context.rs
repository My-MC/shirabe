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
