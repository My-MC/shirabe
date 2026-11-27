//! Shirabe Meter Suite.
//!
//! The plugin is strictly pass-through: audio is never modified unless the test oscillator is
//! switched on, and no latency is added. Measurement runs on the audio thread and publishes
//! read-outs through lock-free atomics in [`shared::SharedMeters`]; the editor only reads them.

// The VST3 bindings behind `nih_export_vst3!` expand to an expression with a trailing
// semicolon (rust-lang/rust#79813). Remove this once the bindings are updated.
#![allow(semicolon_in_expressions_from_non_local_macros)]

use std::sync::Arc;

use nih_plug::prelude::*;

mod denormal;
pub mod editor;
pub mod engine;
pub mod fonts;
pub mod params;
pub mod playout;
pub mod shared;
mod timeline;
#[cfg(test)]
mod timeline_tests;

use engine::MeterEngine;
pub use params::ShirabeParams;
use shared::{SharedMeters, MAX_CHANNELS};

/// The plugin instance.
pub struct Shirabe {
    params: Arc<ShirabeParams>,
    shared: Arc<SharedMeters>,
    engine: MeterEngine,
}

impl Default for Shirabe {
    fn default() -> Self {
        let params = Arc::new(ShirabeParams::default());
        let shared = Arc::new(SharedMeters::new());
        Self {
            engine: MeterEngine::new(params.clone(), shared.clone()),
            params,
            shared,
        }
    }
}

impl Plugin for Shirabe {
    const NAME: &'static str = "Shirabe Meter Suite";
    const VENDOR: &'static str = "Shirabe";
    const URL: &'static str = env!("CARGO_PKG_REPOSITORY");
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // The first layout is the default. It and the second entry are the layouts that shipped
    // before the sidechain existed: they must keep their bus configuration, or every existing
    // project would be loaded with different inputs. Sidechain-capable layouts are appended.
    //
    // Surround layouts are planned for a later release; the channel roles in
    // `shirabe_dsp::channel` already model them.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            names: PortNames::const_default(),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
        // Appended: stereo main bus plus a stereo sidechain (reference) input.
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[new_nonzero_u32(2)],
            names: PortNames {
                aux_inputs: &["Reference"],
                ..PortNames::const_default()
            },
            ..AudioIOLayout::const_default()
        },
        // Appended: mono main bus plus a mono sidechain input.
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            aux_input_ports: &[new_nonzero_u32(1)],
            names: PortNames {
                aux_inputs: &["Reference"],
                ..PortNames::const_default()
            },
            ..AudioIOLayout::const_default()
        },
    ];

    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.shared.clone())
    }

    fn initialize(
        &mut self,
        audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        let channels = audio_io_layout
            .main_input_channels
            .map_or(0, |c| c.get() as usize)
            .min(MAX_CHANNELS);
        let reference_channels = audio_io_layout
            .aux_input_ports
            .first()
            .map_or(0, |c| c.get() as usize);
        self.engine
            .prepare(buffer_config.sample_rate, channels, reference_channels);
        true
    }

    fn reset(&mut self) {
        self.engine.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let playing = context.transport().playing;
        // The sidechain is measured by the engine and never written, so the main bus stays the
        // only path to the output. An empty slice means this layout has no auxiliary input.
        let mut none: [&mut [f32]; 0] = [];
        let reference: &mut [&mut [f32]] = match aux.inputs.first_mut() {
            Some(reference) => reference.as_slice(),
            None => &mut none[..],
        };
        self.engine.process(buffer.as_slice(), reference, playing);
        ProcessStatus::Normal
    }
}

impl ClapPlugin for Shirabe {
    const CLAP_ID: &'static str = "io.github.my-mc.shirabe.meter-suite";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Audio metering suite");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Analyzer,
        ClapFeature::Utility,
        ClapFeature::Mono,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for Shirabe {
    // Never change this: hosts use it to find the plugin in saved projects.
    const VST3_CLASS_ID: [u8; 16] = [
        155, 219, 185, 163, 158, 228, 74, 180, 189, 126, 131, 244, 155, 212, 132, 82,
    ];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::Analyzer,
        Vst3SubCategory::Tools,
    ];
}

nih_export_clap!(Shirabe);
nih_export_vst3!(Shirabe);
