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
use shirabe_dsp::{
    db_to_gain, ChannelLayout, LoudnessMeter, PeakBallistics, PeakMeter, PpmMeter, PpmType,
    SineOscillator, TruePeakMeter, VuMeter,
};

mod denormal;
mod editor;
mod params;
pub mod shared;

use params::LevelMode;
pub use params::ShirabeParams;
use shared::{SharedMeters, MAX_CHANNELS};

/// The plugin instance.
pub struct Shirabe {
    params: Arc<ShirabeParams>,
    shared: Arc<SharedMeters>,
    sample_rate: f32,
    peak: Vec<PeakMeter>,
    true_peak: Vec<TruePeakMeter>,
    vu: Vec<VuMeter>,
    ppm_type_i: Vec<PpmMeter>,
    ppm_type_ii: Vec<PpmMeter>,
    level_mode: LevelMode,
    loudness: LoudnessMeter,
    osc: SineOscillator,
    osc_was_enabled: bool,
    was_playing: bool,
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
            true_peak: (0..MAX_CHANNELS)
                .map(|_| TruePeakMeter::new(48_000.0))
                .collect(),
            vu: (0..MAX_CHANNELS)
                .map(|_| VuMeter::new(48_000.0, -18.0))
                .collect(),
            ppm_type_i: (0..MAX_CHANNELS)
                .map(|_| PpmMeter::new(48_000.0, PpmType::TypeI))
                .collect(),
            ppm_type_ii: (0..MAX_CHANNELS)
                .map(|_| PpmMeter::new(48_000.0, PpmType::TypeII))
                .collect(),
            level_mode: LevelMode::Vu,
            loudness: LoudnessMeter::new(48_000.0, ChannelLayout::Stereo.roles()),
            osc: SineOscillator::new(1000.0, 48_000.0),
            osc_was_enabled: false,
            was_playing: false,
        }
    }
}

impl Plugin for Shirabe {
    const NAME: &'static str = "Shirabe Meter Suite";
    const VENDOR: &'static str = "Shirabe";
    const URL: &'static str = env!("CARGO_PKG_REPOSITORY");
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // The first layout is the default. Surround layouts are planned for a later release; the
    // channel roles in `shirabe_dsp::channel` already model them.
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
        for meter in &mut self.true_peak {
            meter.prepare(self.sample_rate);
        }
        for meter in &mut self.vu {
            meter.prepare(self.sample_rate);
        }
        for meter in self.ppm_type_i.iter_mut().chain(&mut self.ppm_type_ii) {
            meter.prepare(self.sample_rate);
        }
        let layout = ChannelLayout::from_channel_count(channels).unwrap_or(ChannelLayout::Stereo);
        self.loudness.prepare(self.sample_rate, layout.roles());
        self.osc.reset();
        self.shared.configure(channels, self.sample_rate);
        true
    }

    fn reset(&mut self) {
        for meter in &mut self.peak {
            meter.reset();
        }
        for meter in &mut self.true_peak {
            meter.reset();
        }
        for meter in &mut self.vu {
            meter.reset();
        }
        for meter in self.ppm_type_i.iter_mut().chain(&mut self.ppm_type_ii) {
            meter.reset();
        }
        self.loudness.reset();
        self.osc.reset();
        self.shared.clear();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let _denormals = denormal::DenormalGuard::new();
        let playing = context.transport().playing;
        let transport_started = playing && !self.was_playing;
        self.was_playing = playing;
        let auto_reset = transport_started && self.params.auto_reset.value();
        if self.shared.take_reset_request() || auto_reset {
            self.reset_maxima();
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

        let vu_reference = self.params.vu_reference.value();
        let level_mode = self.params.level_mode.value();
        if level_mode != self.level_mode {
            // Start the newly selected meter from rest instead of showing a stale reading.
            for meter in self.ppm_type_i.iter_mut().chain(&mut self.ppm_type_ii) {
                meter.reset();
            }
            for meter in &mut self.vu {
                meter.reset();
            }
            self.level_mode = level_mode;
        }
        let channels = buffer.as_slice_immutable();
        for (index, channel) in channels.iter().take(MAX_CHANNELS).enumerate() {
            let readout = self.shared.channel(index);

            let peak = &mut self.peak[index];
            peak.process(channel);
            readout.store(peak);

            let true_peak = &mut self.true_peak[index];
            true_peak.process(channel);
            readout
                .true_peak_max_db
                .store(true_peak.max_dbtp(), Ordering::Relaxed);

            match level_mode {
                LevelMode::Vu => {
                    let vu = &mut self.vu[index];
                    vu.set_reference(vu_reference);
                    vu.process(channel);
                    readout.vu.store(vu.vu(), Ordering::Relaxed);
                }
                LevelMode::PpmTypeI | LevelMode::PpmTypeII => {
                    let ppm = if level_mode == LevelMode::PpmTypeI {
                        &mut self.ppm_type_i[index]
                    } else {
                        &mut self.ppm_type_ii[index]
                    };
                    ppm.process(channel);
                    readout.ppm_db.store(ppm.level_db(), Ordering::Relaxed);
                }
            }
        }
        self.loudness.process(channels);
        self.shared.loudness.store(&self.loudness.readout());
        self.shared.blocks.fetch_add(1, Ordering::Relaxed);

        ProcessStatus::Normal
    }
}

impl Shirabe {
    /// Clears maxima, integrated loudness and loudness range (keeps the running meters).
    fn reset_maxima(&mut self) {
        for meter in &mut self.peak {
            meter.reset_max();
        }
        for meter in &mut self.true_peak {
            meter.reset_max();
        }
        self.loudness.reset_programme();
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
