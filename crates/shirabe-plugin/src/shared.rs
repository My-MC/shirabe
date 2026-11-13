//! Lock-free state shared between the audio thread (writer) and the editor (reader).

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use nih_plug::prelude::AtomicF32;
use shirabe_dsp::{ChannelLayout, PeakMeter, MINUS_INFINITY_DB};

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
}

impl ChannelReadout {
    fn new() -> Self {
        Self {
            level_db: AtomicF32::new(MINUS_INFINITY_DB),
            hold_db: AtomicF32::new(MINUS_INFINITY_DB),
            max_db: AtomicF32::new(MINUS_INFINITY_DB),
        }
    }

    /// Publishes the current state of a meter (audio thread).
    pub fn store(&self, meter: &PeakMeter) {
        self.level_db.store(meter.level_db(), Ordering::Relaxed);
        self.hold_db.store(meter.hold_db(), Ordering::Relaxed);
        self.max_db.store(meter.max_db(), Ordering::Relaxed);
    }

    fn clear(&self) {
        self.level_db.store(MINUS_INFINITY_DB, Ordering::Relaxed);
        self.hold_db.store(MINUS_INFINITY_DB, Ordering::Relaxed);
        self.max_db.store(MINUS_INFINITY_DB, Ordering::Relaxed);
    }
}

/// Everything the editor can read.
pub struct SharedMeters {
    channels: [ChannelReadout; MAX_CHANNELS],
    active_channels: AtomicUsize,
    sample_rate: AtomicF32,
    reset_requested: AtomicBool,
    /// Number of processed blocks; lets the editor tell whether audio is running.
    pub blocks: AtomicU64,
}

impl SharedMeters {
    /// Creates empty read-outs.
    pub fn new() -> Self {
        Self {
            channels: std::array::from_fn(|_| ChannelReadout::new()),
            active_channels: AtomicUsize::new(2),
            sample_rate: AtomicF32::new(0.0),
            reset_requested: AtomicBool::new(false),
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

    /// Asks the audio thread to clear the maxima (editor).
    pub fn request_reset(&self) {
        self.reset_requested.store(true, Ordering::Relaxed);
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
