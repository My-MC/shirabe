//! Bounded history of momentary and short-term loudness for the time-history graph.
//!
//! The audio thread appends one point per 100 ms of measured programme time into a fixed,
//! pre-allocated ring; the editor reads the newest points without a lock and without
//! allocating. The history is display data only: it never feeds back into a measurement.
//!
//! Pausing the programme measurement passes no measured frames, so the recording freezes and
//! continues from the same point on resume. A reset or a sample-rate change clears it.

use std::sync::atomic::{AtomicU64, Ordering};

/// Interval between two points, in seconds of measured programme time.
pub const HISTORY_INTERVAL_SECONDS: f64 = 0.1;

/// Number of points kept: 1800 points, three minutes at ten points per second.
pub const HISTORY_CAPACITY: usize = 1_800;

/// Packs two loudness values into one atomic word so a point is never torn.
fn pack(momentary: Option<f32>, short_term: Option<f32>) -> u64 {
    let m = momentary.unwrap_or(f32::NAN);
    let s = short_term.unwrap_or(f32::NAN);
    u64::from(m.to_bits()) | (u64::from(s.to_bits()) << 32)
}

fn unpack(word: u64) -> (f32, f32) {
    (
        f32::from_bits(word as u32),
        f32::from_bits((word >> 32) as u32),
    )
}

/// Ring of loudness points shared by the audio thread (writer) and the editor (readers).
///
/// Every method is lock-free and allocation-free. [`prepare`](Self::prepare) only changes the
/// interval and clears the ring, so it may run on the audio thread as well.
pub struct LoudnessHistory {
    slots: [AtomicU64; HISTORY_CAPACITY],
    /// Points written since the last clear; wraps only after 2^64 points.
    written: AtomicU64,
    /// Incremented by every clear, so a reader can tell a restart from a full ring.
    epoch: AtomicU64,
    /// Frames per point; 0 before the first [`prepare`](Self::prepare).
    step_samples: AtomicU64,
    /// Frames accumulated toward the next point.
    pending: AtomicU64,
}

impl LoudnessHistory {
    /// Creates an empty history. It records nothing until [`prepare`](Self::prepare) sets the
    /// sample rate.
    pub fn new() -> Self {
        Self {
            slots: std::array::from_fn(|_| AtomicU64::new(pack(None, None))),
            written: AtomicU64::new(0),
            epoch: AtomicU64::new(0),
            step_samples: AtomicU64::new(0),
            pending: AtomicU64::new(0),
        }
    }

    /// Sets the interval for `sample_rate` and clears the history.
    ///
    /// Allocates nothing, so the engine may call it from `prepare`. A sample-rate change must
    /// clear the history: the old points no longer describe the current measurement.
    pub fn prepare(&self, sample_rate: f32) {
        let rate = f64::from(sample_rate).max(1.0);
        let step = (rate * HISTORY_INTERVAL_SECONDS).round().max(1.0) as u64;
        self.step_samples.store(step, Ordering::Relaxed);
        self.clear();
    }

    /// Discards every point and restarts the interval.
    pub fn clear(&self) {
        self.written.store(0, Ordering::Relaxed);
        self.pending.store(0, Ordering::Relaxed);
        self.epoch.fetch_add(1, Ordering::Release);
    }

    /// Accounts for `frames` of measured programme time and appends a point for every whole
    /// interval that they complete (audio thread).
    ///
    /// Pass `0` while the programme measurement is paused: the recording then freezes. Returns
    /// the number of points appended, which is more than one only when a single block is longer
    /// than the interval.
    pub fn record(&self, frames: usize, momentary: Option<f32>, short_term: Option<f32>) -> usize {
        let step = self.step_samples.load(Ordering::Relaxed);
        if step == 0 || frames == 0 {
            return 0;
        }
        let mut pending = self.pending.load(Ordering::Relaxed) + frames as u64;
        let mut appended = 0;
        while pending >= step {
            pending -= step;
            self.push(momentary, short_term);
            appended += 1;
        }
        self.pending.store(pending, Ordering::Relaxed);
        appended
    }

    fn push(&self, momentary: Option<f32>, short_term: Option<f32>) {
        // The counter is advanced after the slot, so a reader never sees a slot that has been
        // announced but not yet written.
        let written = self.written.load(Ordering::Relaxed);
        let index = (written % HISTORY_CAPACITY as u64) as usize;
        self.slots[index].store(pack(momentary, short_term), Ordering::Relaxed);
        self.written.fetch_add(1, Ordering::Release);
    }

    /// Copies the newest points into `out`, oldest first, and returns how many were copied.
    ///
    /// At most [`HISTORY_CAPACITY`] points and at most `out.len()` points are returned.
    /// Missing values are `NaN`.
    pub fn read_recent(&self, out: &mut [(f32, f32)]) -> usize {
        let written = self.written.load(Ordering::Acquire);
        let available = written.min(HISTORY_CAPACITY as u64) as usize;
        let count = out.len().min(available);
        if count == 0 {
            return 0;
        }
        let first = written - count as u64;
        for (k, slot) in out[..count].iter_mut().enumerate() {
            let index = ((first + k as u64) % HISTORY_CAPACITY as u64) as usize;
            *slot = unpack(self.slots[index].load(Ordering::Relaxed));
        }
        count
    }

    /// Number of points written since the last clear.
    pub fn written(&self) -> u64 {
        self.written.load(Ordering::Acquire)
    }

    /// Counter that changes on every [`clear`](Self::clear).
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
}

impl Default for LoudnessHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for LoudnessHistory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoudnessHistory")
            .field("capacity", &HISTORY_CAPACITY)
            .field("written", &self.written.load(Ordering::Relaxed))
            .field("epoch", &self.epoch.load(Ordering::Relaxed))
            .finish()
    }
}
