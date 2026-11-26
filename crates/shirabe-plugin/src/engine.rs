//! Measurement engine shared by the plugin and the standalone application.
//!
//! The engine owns every meter, reads the parameters and publishes read-outs through
//! [`SharedMeters`]. It does not allocate in [`MeterEngine::process`].

use std::sync::atomic::Ordering;
use std::sync::Arc;

use shirabe_dsp::stereo::StereoStatistics;
use shirabe_dsp::{
    db_to_gain, ChannelLayout, LoudnessMeter, PeakBallistics, PeakMeter, PpmMeter, PpmType,
    SineOscillator, TruePeakMeter, VuMeter, MINUS_INFINITY_DB,
};

use crate::denormal::DenormalGuard;
use crate::params::{LevelMode, ShirabeParams};
use crate::shared::{SharedMeters, MAX_CHANNELS};

/// All meters of one Shirabe instance.
pub struct MeterEngine {
    params: Arc<ShirabeParams>,
    shared: Arc<SharedMeters>,
    sample_rate: f32,
    peak: Vec<PeakMeter>,
    true_peak: Vec<TruePeakMeter>,
    programme_peak: f32,
    vu: Vec<VuMeter>,
    ppm_type_i: Vec<PpmMeter>,
    ppm_type_ii: Vec<PpmMeter>,
    level_mode: LevelMode,
    loudness: LoudnessMeter,
    /// Correlation and balance of the stereo input; fed only while both channels are present.
    stereo: StereoStatistics,
    stereo_active: bool,
    osc: SineOscillator,
    osc_was_enabled: bool,
    was_playing: bool,
}

impl MeterEngine {
    /// Creates an engine prepared for 48 kHz stereo.
    pub fn new(params: Arc<ShirabeParams>, shared: Arc<SharedMeters>) -> Self {
        let sr = 48_000.0;
        Self {
            params,
            shared,
            sample_rate: sr,
            peak: (0..MAX_CHANNELS)
                .map(|_| PeakMeter::new(sr, PeakBallistics::default()))
                .collect(),
            true_peak: (0..MAX_CHANNELS).map(|_| TruePeakMeter::new(sr)).collect(),
            programme_peak: MINUS_INFINITY_DB,
            vu: (0..MAX_CHANNELS).map(|_| VuMeter::new(sr, -18.0)).collect(),
            ppm_type_i: (0..MAX_CHANNELS)
                .map(|_| PpmMeter::new(sr, PpmType::TypeI))
                .collect(),
            ppm_type_ii: (0..MAX_CHANNELS)
                .map(|_| PpmMeter::new(sr, PpmType::TypeII))
                .collect(),
            level_mode: LevelMode::Vu,
            loudness: LoudnessMeter::new(sr, ChannelLayout::Stereo.roles()),
            stereo: StereoStatistics::new(sr),
            stereo_active: false,
            osc: SineOscillator::new(1000.0, sr),
            osc_was_enabled: false,
            was_playing: false,
        }
    }

    /// The parameters read by the engine.
    pub fn params(&self) -> &Arc<ShirabeParams> {
        &self.params
    }

    /// The read-outs written by the engine.
    pub fn shared(&self) -> &Arc<SharedMeters> {
        &self.shared
    }

    /// Prepares every meter for a sample rate and channel count. Allocates; call it outside
    /// the audio callback.
    pub fn prepare(&mut self, sample_rate: f32, channels: usize) {
        let channels = channels.min(MAX_CHANNELS);
        self.sample_rate = sample_rate;
        for meter in &mut self.peak {
            meter.prepare(sample_rate);
        }
        for meter in &mut self.true_peak {
            meter.prepare(sample_rate);
        }
        for meter in &mut self.vu {
            meter.prepare(sample_rate);
        }
        for meter in self.ppm_type_i.iter_mut().chain(&mut self.ppm_type_ii) {
            meter.prepare(sample_rate);
        }
        let layout = ChannelLayout::from_channel_count(channels).unwrap_or(ChannelLayout::Stereo);
        self.loudness.prepare(sample_rate, layout.roles());
        self.stereo.prepare(sample_rate);
        self.stereo_active = false;
        self.programme_peak = MINUS_INFINITY_DB;
        self.osc.reset();
        self.shared.configure(channels, sample_rate);
    }

    /// Returns every meter to rest and clears the read-outs.
    pub fn reset(&mut self) {
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
        self.stereo.reset();
        self.stereo_active = false;
        self.programme_peak = MINUS_INFINITY_DB;
        self.osc.reset();
        self.shared.clear();
    }

    /// Clears maxima, integrated loudness and loudness range (keeps the running meters).
    pub fn reset_maxima(&mut self) {
        for meter in &mut self.peak {
            meter.reset_max();
        }
        for meter in &mut self.true_peak {
            meter.reset();
        }
        self.programme_peak = MINUS_INFINITY_DB;
        self.loudness.reset_programme();
    }

    /// Measures one block. `channels` holds one slice per channel, all of the same length.
    /// When the test oscillator is on, the channels are overwritten with the test tone first.
    /// `playing` is the host transport state (used for the reset-on-play option).
    pub fn process(&mut self, channels: &mut [&mut [f32]], playing: bool) {
        let _denormals = DenormalGuard::new();
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
            let frames = channels.first().map_or(0, |c| c.len());
            for i in 0..frames {
                let value = self.osc.next_sample() * amplitude;
                for channel in channels.iter_mut() {
                    channel[i] = value;
                }
            }
        }
        self.osc_was_enabled = osc_enabled;

        // Feed the Lissajous view and the stereo statistics with the stereo signal as processed
        // (test oscillator included). Mono inputs publish nothing: a duplicated channel would
        // look like a perfectly correlated stereo signal.
        if channels.len() >= 2 {
            let left: &[f32] = channels[0];
            let right: &[f32] = channels[1];
            self.shared.scope.push(left, right);
            self.stereo.process(left, right);
            self.stereo_active = true;
        } else if self.stereo_active {
            // Back to mono: drop the window so a stale stereo reading does not linger.
            self.stereo.reset();
            self.stereo_active = false;
        }
        self.shared.stereo.store(&self.stereo.readout());

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
        let integrating = self.shared.integrating();
        let resuming = integrating && !self.loudness.is_integrating();
        for (index, channel) in channels.iter().take(MAX_CHANNELS).enumerate() {
            let channel: &[f32] = channel;
            let readout = self.shared.channel(index);

            let peak = &mut self.peak[index];
            peak.process(channel);
            readout.store(peak);

            let true_peak = &mut self.true_peak[index];
            if resuming {
                // The FIR has a delay: otherwise the first block could contain paused peaks.
                true_peak.clear_history();
            }
            true_peak.process(channel);
            if integrating {
                self.programme_peak = self.programme_peak.max(true_peak.block_dbtp());
            }
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
                LevelMode::PpmTypeI
                | LevelMode::PpmTypeII
                | LevelMode::PpmTypeIIBbc
                | LevelMode::PpmIeee => {
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
        self.loudness.set_integrating(integrating);
        self.loudness.process(channels);
        self.shared
            .programme_seconds
            .store(self.loudness.programme_seconds() as f32, Ordering::Relaxed);
        let loudness = self.loudness.readout();
        self.shared.loudness.store(&loudness);
        self.shared.loudness.plr.store(
            loudness
                .integrated
                .filter(|_| self.programme_peak > MINUS_INFINITY_DB)
                .map(|i| f64::from(self.programme_peak) - i),
        );
        self.shared.blocks.fetch_add(1, Ordering::Relaxed);
    }
}
