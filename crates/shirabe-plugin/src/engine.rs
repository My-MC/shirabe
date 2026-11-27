//! Measurement engine shared by the plugin and the standalone application.
//!
//! The engine owns every meter, reads the parameters and publishes read-outs through
//! [`SharedMeters`]. It does not allocate in [`MeterEngine::process`].

use std::sync::atomic::Ordering;
use std::sync::Arc;

use shirabe_dsp::spectrum::mono_sum;
use shirabe_dsp::stereo::StereoStatistics;
use shirabe_dsp::{
    db_to_gain, ChannelLayout, LoudnessMeter, PeakBallistics, PeakMeter, PpmMeter, PpmType,
    SineOscillator, TruePeakMeter, VuMeter, MINUS_INFINITY_DB,
};

use crate::denormal::DenormalGuard;
use crate::params::{LevelMode, ShirabeParams};
use crate::shared::{SharedMeters, MAX_CHANNELS};
use crate::timeline::{ChannelSample, Snapshot};

/// Frames copied into the spectrum ring per step (no allocation on the audio thread).
const SPECTRUM_BLOCK: usize = 512;
const SNAPSHOTS_PER_SECOND: f32 = 200.0;

/// All meters of one Shirabe instance.
pub struct MeterEngine {
    params: Arc<ShirabeParams>,
    shared: Arc<SharedMeters>,
    sample_rate: f32,
    peak: Vec<PeakMeter>,
    // Sample peak release/hold are block-based. Keep the measured host-block meter
    // intact and run a separate display envelope on the fixed snapshot grid.
    display_peak: [PeakMeter; MAX_CHANNELS],
    pending_peak: [f32; MAX_CHANNELS],
    display_frame: u64,
    display_interval: usize,
    display_pending: usize,
    true_peak: Vec<TruePeakMeter>,
    programme_peak: f32,
    vu: Vec<VuMeter>,
    ppm_type_i: Vec<PpmMeter>,
    ppm_type_ii: Vec<PpmMeter>,
    level_mode: LevelMode,
    loudness: LoudnessMeter,
    /// Independent loudness measurement of the sidechain reference. `None` while the host's
    /// layout has no auxiliary input, so the plug-in pays nothing for a feature it cannot use.
    reference: Option<LoudnessMeter>,
    /// Correlation and balance of the stereo input; fed only while both channels are present.
    stereo: StereoStatistics,
    stereo_active: bool,
    /// Mono sum for the spectrum capture, filled in blocks without allocating.
    spectrum_scratch: [f32; SPECTRUM_BLOCK],
    /// Channel count the spectrum capture was last fed with; a change clears the ring.
    spectrum_channels: usize,
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
            display_peak: std::array::from_fn(|_| PeakMeter::new(sr, PeakBallistics::default())),
            pending_peak: [0.0; MAX_CHANNELS],
            display_frame: 0,
            display_interval: (sr / SNAPSHOTS_PER_SECOND).round() as usize,
            display_pending: 0,
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
            reference: None,
            stereo: StereoStatistics::new(sr),
            stereo_active: false,
            spectrum_scratch: [0.0; SPECTRUM_BLOCK],
            spectrum_channels: 0,
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

    /// Prepares every meter for a sample rate, a channel count and a sidechain channel count.
    /// Allocates; call it outside the audio callback.
    ///
    /// A non-zero `reference_channels` creates the independent reference meter; zero leaves it
    /// out, so a layout without an auxiliary input costs nothing extra.
    pub fn prepare(&mut self, sample_rate: f32, channels: usize, reference_channels: usize) {
        let channels = channels.min(MAX_CHANNELS);
        self.sample_rate = sample_rate;
        self.display_interval = ((sample_rate / SNAPSHOTS_PER_SECOND).round() as usize).max(1);
        for meter in &mut self.display_peak {
            meter.prepare(sample_rate);
        }
        self.restart_capture();
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
        self.reference = (reference_channels > 0).then(|| {
            let roles = ChannelLayout::from_channel_count(reference_channels)
                .unwrap_or(ChannelLayout::Stereo)
                .roles();
            LoudnessMeter::new(sample_rate, roles)
        });
        self.stereo.prepare(sample_rate);
        self.stereo_active = false;
        self.spectrum_channels = 0;
        self.programme_peak = MINUS_INFINITY_DB;
        self.osc.reset();
        self.shared
            .configure(channels, sample_rate, self.reference.is_some());
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
        if let Some(reference) = &mut self.reference {
            reference.reset();
        }
        self.stereo.reset();
        self.stereo_active = false;
        self.spectrum_channels = 0;
        self.programme_peak = MINUS_INFINITY_DB;
        self.osc.reset();
        self.restart_capture();
        self.shared.clear();
    }

    fn restart_capture(&mut self) {
        self.display_frame = 0;
        self.display_pending = 0;
        self.pending_peak.fill(0.0);
        for meter in &mut self.display_peak {
            meter.reset();
        }
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
        // The comparison is part of the programme display, so it starts over with it.
        if let Some(reference) = &mut self.reference {
            reference.reset_programme();
        }
        // The graph records the programme, so it starts over with it.
        self.shared.history.clear();
        self.shared.timeline.clear();
        // The spectrum is display-only but shows the same programme, so Reset clears it too.
        self.shared.spectrum.clear_at(self.display_frame);
    }

    /// Measures one block. `channels` holds one slice per channel, all of the same length.
    /// When the test oscillator is on, the channels are overwritten with the test tone first.
    /// `reference` holds the sidechain channels; it is read only and never mixed into the
    /// output. `playing` is the host transport state (used for the reset-on-play option).
    pub fn process(
        &mut self,
        channels: &mut [&mut [f32]],
        reference: &mut [&mut [f32]],
        playing: bool,
    ) {
        let _denormals = DenormalGuard::new();
        let frames = channels.first().map_or(0, |channel| channel.len());
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

        // Capture the mono sum for the spectrum tile. The ring is bounded and the copy uses a
        // fixed scratch block, so the audio thread neither locks nor allocates.
        if channels.len() != self.spectrum_channels {
            // A layout change must not leave frames from the previous input in the display.
            self.shared.clear_display_capture();
            self.restart_capture();
            self.spectrum_channels = channels.len();
        }
        let spectrum_frames = channels.first().map_or(0, |channel| channel.len());
        let mut offset = 0;
        while offset < spectrum_frames {
            let count = (spectrum_frames - offset).min(SPECTRUM_BLOCK);
            if channels.len() >= 2 {
                mono_sum(
                    &channels[0][offset..offset + count],
                    &channels[1][offset..offset + count],
                    &mut self.spectrum_scratch[..count],
                );
            } else {
                self.spectrum_scratch[..count]
                    .copy_from_slice(&channels[0][offset..offset + count]);
            }
            self.shared.spectrum.push(&self.spectrum_scratch[..count]);
            offset += count;
        }

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
            self.shared.timeline.clear();
        }
        let integrating = self.shared.integrating();
        let resuming = integrating && !self.loudness.is_integrating();
        self.loudness.set_integrating(integrating);
        if channels.len() < 2 && self.stereo_active {
            self.stereo.reset();
        }
        self.stereo_active = channels.len() >= 2;
        self.measure_display_segments(channels, frames, vu_reference);
        self.shared.stereo.store(&self.stereo.readout());

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
                    readout.vu.store(self.vu[index].vu(), Ordering::Relaxed);
                }
                LevelMode::PpmTypeI
                | LevelMode::PpmTypeII
                | LevelMode::PpmTypeIIBbc
                | LevelMode::PpmIeee => {
                    let ppm = if level_mode == LevelMode::PpmTypeI {
                        &self.ppm_type_i[index]
                    } else {
                        &self.ppm_type_ii[index]
                    };
                    readout.ppm_db.store(ppm.level_db(), Ordering::Relaxed);
                }
            }
        }
        self.shared
            .programme_seconds
            .store(self.loudness.programme_seconds() as f32, Ordering::Relaxed);
        let loudness = self.loudness.readout();
        self.shared.loudness.store(&loudness);

        // The reference is a second, independent measurement with the same windows and gating.
        // It shares the transport, so Pause and Reset act on both programmes together.
        if let Some(reference_meter) = &mut self.reference {
            reference_meter.set_integrating(integrating);
            let reference_frames = reference.first().map_or(0, |channel| channel.len());
            // The host fills an unconnected sidechain with silence; only a buffer of the
            // expected length and uniform channel lengths is processed.
            if reference_frames > 0
                && reference
                    .iter()
                    .all(|channel| channel.len() == reference_frames)
            {
                reference_meter.process(&*reference);
            }
            self.shared.reference.store(&reference_meter.readout());
        }
        // The graph records the programme measurement: it advances only while that measurement
        // runs, so it freezes with Pause and starts over with Reset.
        if integrating {
            self.shared.history.record(
                frames,
                loudness.momentary.map(|v| v as f32),
                loudness.short_term.map(|v| v as f32),
            );
        }
        self.shared.loudness.plr.store(
            loudness
                .integrated
                .filter(|_| self.programme_peak > MINUS_INFINITY_DB)
                .map(|i| f64::from(self.programme_peak) - i),
        );
        self.shared.blocks.fetch_add(1, Ordering::Relaxed);
    }

    fn measure_display_segments(&mut self, channels: &[&mut [f32]], frames: usize, reference: f32) {
        let channels = &channels[..channels.len().min(MAX_CHANNELS)];
        if self.level_mode == LevelMode::Vu {
            for meter in &mut self.vu[..channels.len()] {
                meter.set_reference(reference);
            }
        }
        let mut start = 0;
        // Keep the old empty-block VU/PPM cleanup as well as the non-empty host boundaries.
        loop {
            let count = (frames - start).min(self.display_interval - self.display_pending);
            let end = start + count;
            let mut slices = [&[][..]; MAX_CHANNELS];
            for (index, channel) in channels.iter().enumerate() {
                let segment = &channel[start..end];
                slices[index] = segment;
                self.pending_peak[index] = segment
                    .iter()
                    .fold(self.pending_peak[index], |peak, sample| {
                        peak.max(sample.abs())
                    });
                match self.level_mode {
                    LevelMode::Vu => self.vu[index].process_segment(segment, end == frames),
                    LevelMode::PpmTypeI => {
                        self.ppm_type_i[index].process_segment(segment, end == frames);
                    }
                    _ => self.ppm_type_ii[index].process_segment(segment, end == frames),
                }
            }
            self.loudness.process(&slices[..channels.len()]);
            if self.stereo_active {
                self.shared.scope.push(slices[0], slices[1]);
                self.stereo.process(slices[0], slices[1]);
            }
            self.display_pending += count;
            self.display_frame += count as u64;
            if self.display_pending == self.display_interval {
                self.publish_snapshot(channels.len());
                self.display_pending = 0;
                self.pending_peak.fill(0.0);
            }
            if end == frames {
                break;
            }
            start = end;
        }
    }

    fn publish_snapshot(&mut self, channels: usize) {
        let loudness = self.loudness.readout();
        let stereo = self.stereo.readout();
        let mut point = Snapshot {
            frame: self.display_frame,
            scope_epoch: self.shared.scope.epoch(),
            spectrum_epoch: self.shared.spectrum.epoch(),
            momentary: loudness.momentary.map(|x| x as f32),
            short_term: loudness.short_term.map(|x| x as f32),
            correlation: stereo.correlation,
            balance: stereo.balance,
            ..Snapshot::default()
        };
        for index in 0..channels {
            let peak = &mut self.display_peak[index];
            peak.process_peak(self.pending_peak[index], self.display_interval);
            let mut channel = ChannelSample {
                level_db: peak.level_db(),
                hold_db: peak.hold_db(),
                ..ChannelSample::default()
            };
            match self.level_mode {
                LevelMode::Vu => channel.vu = self.vu[index].vu(),
                LevelMode::PpmTypeI => channel.ppm_db = self.ppm_type_i[index].level_db(),
                _ => channel.ppm_db = self.ppm_type_ii[index].level_db(),
            }
            point.channels[index] = channel;
        }
        self.shared.timeline.push(point);
    }
}
