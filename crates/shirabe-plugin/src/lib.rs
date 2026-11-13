//! Shirabe Meter Suite.
//!
//! The plugin is strictly pass-through: audio is never modified unless the test oscillator is
//! switched on, and no latency is added. Measurement runs on the audio thread and publishes
//! read-outs through lock-free atomics in [`shared::SharedMeters`]; the editor only reads them.

// The VST3 bindings behind `nih_export_vst3!` expand to an expression with a trailing
// semicolon (rust-lang/rust#79813). Remove this once the bindings are updated.
#![allow(semicolon_in_expressions_from_non_local_macros)]

use std::sync::atomic::Ordering;
use std::sync::Arc;

use nih_plug::prelude::*;
use shirabe_dsp::{db_to_gain, PeakBallistics, PeakMeter, SineOscillator};

mod editor;
mod params;
pub mod shared;

pub use params::ShirabeParams;
use shared::{SharedMeters, MAX_CHANNELS};

/// The plugin instance.
pub struct Shirabe {
    params: Arc<ShirabeParams>,
    shared: Arc<SharedMeters>,
    sample_rate: f32,
    peak: Vec<PeakMeter>,
    osc: SineOscillator,
    osc_was_enabled: bool,
}

impl Default for Shirabe {
    fn default() -> Self {
        Self {
            params: Arc::new(ShirabeParams::default()),
            shared: Arc::new(SharedMeters::new()),
            sample_rate: 48_000.0,
            peak: (0..MAX_CHANNELS)
                .map(|_| PeakMeter::new(48_000.0, PeakBallistics::default()))
                .collect(),
            osc: SineOscillator::new(1000.0, 48_000.0),
            osc_was_enabled: false,
        }
    }
}

impl Plugin for Shirabe {
    const NAME: &'static str = "Shirabe Meter Suite";
    const VENDOR: &'static str = "Shirabe";
    const URL: &'static str = env!("CARGO_PKG_REPOSITORY");
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // The first layout is the default. 7.1 is not offered yet because the host-side speaker
    // arrangement for eight channels is not the common 7.1 layout.
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
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(6),
            main_output_channels: NonZeroU32::new(6),
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

        self.sample_rate = buffer_config.sample_rate;
        for meter in &mut self.peak {
            meter.prepare(self.sample_rate);
        }
        self.osc.reset();
        self.shared.configure(channels, self.sample_rate);
        true
    }

    fn reset(&mut self) {
        for meter in &mut self.peak {
            meter.reset();
        }
        self.osc.reset();
        self.shared.clear();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        if self.shared.take_reset_request() {
            for meter in &mut self.peak {
                meter.reset_max();
            }
        }

        let osc_enabled = self.params.osc_enabled.value();
        if osc_enabled {
            if !self.osc_was_enabled {
                self.osc.reset();
            }
            self.osc
                .set_frequency(self.params.osc_frequency.value(), self.sample_rate);
            let amplitude = db_to_gain(self.params.osc_level.value());
            for frame in buffer.iter_samples() {
                let value = self.osc.next_sample() * amplitude;
                for sample in frame {
                    *sample = value;
                }
            }
        }
        self.osc_was_enabled = osc_enabled;

        for (index, channel) in buffer.as_slice_immutable().iter().enumerate() {
            let Some(meter) = self.peak.get_mut(index) else {
                break;
            };
            meter.process(channel);
            self.shared.channel(index).store(meter);
        }
        self.shared.blocks.fetch_add(1, Ordering::Relaxed);

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
        ClapFeature::Surround,
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
