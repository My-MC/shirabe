//! Lock-free state shared between the audio thread (writer) and the editor (reader).
//!
//! Every value is an independent atomic, so the editor may see values from adjacent blocks
//! mixed together; for meters this is harmless and avoids any locking on the audio thread.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use nih_plug::prelude::AtomicF32;
use shirabe_dsp::{ChannelLayout, LoudnessReadout, PeakMeter, MINUS_INFINITY_DB};

/// Maximum number of channels the meters are pre-allocated for.
pub const MAX_CHANNELS: usize = 8;

/// Read-outs of one channel.
pub struct ChannelReadout {
    /// Displayed sample peak level in dBFS.
    pub level_db: AtomicF32,
    /// Peak-hold marker in dBFS.
    pub hold_db: AtomicF32,
    /// Highest sample peak since the last reset in dBFS.
    pub max_db: AtomicF32,
    /// Highest true peak since the last reset in dBTP.
    pub true_peak_max_db: AtomicF32,
    /// VU needle position in VU.
    pub vu: AtomicF32,
    /// Quasi-peak programme meter reading in dBFS.
    pub ppm_db: AtomicF32,
}

impl ChannelReadout {
    fn new() -> Self {
        Self {
            level_db: AtomicF32::new(MINUS_INFINITY_DB),
            hold_db: AtomicF32::new(MINUS_INFINITY_DB),
            max_db: AtomicF32::new(MINUS_INFINITY_DB),
            true_peak_max_db: AtomicF32::new(MINUS_INFINITY_DB),
            vu: AtomicF32::new(MINUS_INFINITY_DB),
            ppm_db: AtomicF32::new(MINUS_INFINITY_DB),
        }
    }

    /// Publishes the current state of a sample peak meter (audio thread).
    pub fn store(&self, meter: &PeakMeter) {
        self.level_db.store(meter.level_db(), Ordering::Relaxed);
        self.hold_db.store(meter.hold_db(), Ordering::Relaxed);
        self.max_db.store(meter.max_db(), Ordering::Relaxed);
    }

    fn clear(&self) {
        for v in [
            &self.level_db,
            &self.hold_db,
            &self.max_db,
            &self.true_peak_max_db,
            &self.vu,
            &self.ppm_db,
        ] {
            v.store(MINUS_INFINITY_DB, Ordering::Relaxed);
        }
    }
}

/// An optional value stored as `NaN` when absent.
pub struct OptionalF32(AtomicF32);

impl OptionalF32 {
    fn new() -> Self {
        Self(AtomicF32::new(f32::NAN))
    }

    /// Stores a value (`None` becomes `NaN`).
    pub fn store(&self, value: Option<f64>) {
        self.0
            .store(value.map_or(f32::NAN, |v| v as f32), Ordering::Relaxed);
    }

    /// Loads the value.
    pub fn load(&self) -> Option<f32> {
        let v = self.0.load(Ordering::Relaxed);
        (!v.is_nan()).then_some(v)
    }
}

/// Loudness read-outs.
pub struct LoudnessShared {
    /// Momentary loudness in LUFS.
    pub momentary: OptionalF32,
    /// Short-term loudness in LUFS.
    pub short_term: OptionalF32,
    /// Integrated loudness in LUFS.
    pub integrated: OptionalF32,
    /// Loudness range in LU.
    pub range: OptionalF32,
    /// Maximum momentary loudness in LUFS.
    pub max_momentary: OptionalF32,
    /// Maximum short-term loudness in LUFS.
    pub max_short_term: OptionalF32,
}

impl LoudnessShared {
    fn new() -> Self {
        Self {
            momentary: OptionalF32::new(),
            short_term: OptionalF32::new(),
            integrated: OptionalF32::new(),
            range: OptionalF32::new(),
            max_momentary: OptionalF32::new(),
            max_short_term: OptionalF32::new(),
        }
    }

    /// Publishes a loudness read-out (audio thread).
    pub fn store(&self, r: &LoudnessReadout) {
        self.momentary.store(r.momentary);
        self.short_term.store(r.short_term);
        self.integrated.store(r.integrated);
        self.range.store(r.range);
        self.max_momentary.store(r.max_momentary);
        self.max_short_term.store(r.max_short_term);
    }
}

/// Everything the editor can read.
pub struct SharedMeters {
    channels: [ChannelReadout; MAX_CHANNELS],
    /// Programme loudness of all channels.
    pub loudness: LoudnessShared,
    active_channels: AtomicUsize,
    sample_rate: AtomicF32,
    reset_requested: AtomicBool,
    /// Whether the integrated loudness and loudness range are running (editor writes).
    integrating: AtomicBool,
    /// Measured programme duration in seconds since the last reset (audio thread writes).
    pub programme_seconds: AtomicF32,
    /// Number of processed blocks; lets the editor tell whether audio is running.
    pub blocks: AtomicU64,
}

impl SharedMeters {
    /// Creates empty read-outs.
    pub fn new() -> Self {
        Self {
            channels: std::array::from_fn(|_| ChannelReadout::new()),
            loudness: LoudnessShared::new(),
            active_channels: AtomicUsize::new(2),
            sample_rate: AtomicF32::new(0.0),
            reset_requested: AtomicBool::new(false),
            integrating: AtomicBool::new(true),
            programme_seconds: AtomicF32::new(0.0),
            blocks: AtomicU64::new(0),
        }
    }

    /// Sets the active channel count and sample rate (called from `initialize`).
    pub fn configure(&self, channels: usize, sample_rate: f32) {
        self.active_channels
            .store(channels.min(MAX_CHANNELS), Ordering::Relaxed);
        self.sample_rate.store(sample_rate, Ordering::Relaxed);
        self.clear();
    }

    /// Clears all read-outs.
    pub fn clear(&self) {
        for channel in &self.channels {
            channel.clear();
        }
        self.loudness.store(&LoudnessReadout::default());
    }

    /// Read-outs of channel `index` (must be below [`MAX_CHANNELS`]).
    pub fn channel(&self, index: usize) -> &ChannelReadout {
        &self.channels[index]
    }

    /// Number of channels currently processed.
    pub fn active_channels(&self) -> usize {
        self.active_channels.load(Ordering::Relaxed)
    }

    /// The layout matching the active channel count, if it is a known one.
    pub fn layout(&self) -> Option<ChannelLayout> {
        ChannelLayout::from_channel_count(self.active_channels())
    }

    /// Current sample rate in Hz (0 before the first initialisation).
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate.load(Ordering::Relaxed)
    }

    /// Asks the audio thread to clear the maxima and the programme loudness (editor).
    pub fn request_reset(&self) {
        self.reset_requested.store(true, Ordering::Relaxed);
    }

    /// Starts or pauses the integrated loudness and loudness range (editor).
    pub fn set_integrating(&self, integrating: bool) {
        self.integrating.store(integrating, Ordering::Relaxed);
    }

    /// Whether the integrated loudness and loudness range are running.
    pub fn integrating(&self) -> bool {
        self.integrating.load(Ordering::Relaxed)
    }

    /// Consumes a pending reset request (audio thread).
    pub fn take_reset_request(&self) -> bool {
        self.reset_requested.swap(false, Ordering::Relaxed)
    }
}

impl Default for SharedMeters {
    fn default() -> Self {
        Self::new()
    }
}
