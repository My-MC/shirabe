//! Stereo sample capture for the Lissajous view.
//!
//! The audio thread writes stereo frames into a fixed, pre-allocated ring; the editor reads
//! the newest frames without a lock and without allocating. The ring is deliberately lossy:
//! when the editor falls behind, the oldest frames are overwritten, because a scope only ever
//! displays the recent past.

use std::sync::atomic::{AtomicU64, Ordering};

/// Number of stereo frames kept for the display (32 KiB). A single packed frame is atomic;
/// a slow reader can still copy frames from different generations if the writer laps it.
pub const SCOPE_CAPACITY: usize = 4096;

/// Packs two samples into one atomic word so that a frame is never torn.
fn pack(left: f32, right: f32) -> u64 {
    u64::from(left.to_bits()) | (u64::from(right.to_bits()) << 32)
}

fn unpack(word: u64) -> (f32, f32) {
    (
        f32::from_bits(word as u32),
        f32::from_bits((word >> 32) as u32),
    )
}

/// Mid/side coordinates of one stereo frame.
///
/// Both axes are scaled by 1/2, so a full-scale mono signal (L = R = 1.0) reaches +1 on the
/// mid axis and a full-scale anti-phase signal reaches +1 on the side axis. A full-scale sine
/// in quadrature traces a circle of radius 1/√2. This is the L/R pair rotated by 45°.
#[inline]
pub fn mid_side(left: f32, right: f32) -> (f32, f32) {
    (0.5 * (left + right), 0.5 * (left - right))
}

/// A bounded, lock-free stereo sample ring.
///
/// One producer (the audio thread) and any number of readers (the editor). No method
/// allocates or blocks.
pub struct StereoScope {
    slots: [AtomicU64; SCOPE_CAPACITY],
    /// Number of frames written since the last clear; wraps only after 2^64 frames.
    written: AtomicU64,
}

impl StereoScope {
    /// Creates an empty scope.
    pub fn new() -> Self {
        Self {
            slots: std::array::from_fn(|_| AtomicU64::new(pack(0.0, 0.0))),
            written: AtomicU64::new(0),
        }
    }

    /// Discards every captured frame. Only the producer may call this while capturing;
    /// other callers must first stop the producer.
    pub fn clear(&self) {
        self.written.store(0, Ordering::Relaxed);
    }

    /// Appends one block of stereo frames (audio thread). Extra frames beyond the capacity
    /// overwrite the oldest ones.
    pub fn push(&self, left: &[f32], right: &[f32]) {
        let frames = left.len().min(right.len());
        if frames == 0 {
            return;
        }
        // The counter is advanced once, after the samples, so a reader never sees a slot
        // that has been announced but not yet written.
        let start = self.written.load(Ordering::Relaxed);
        for i in 0..frames {
            let index = ((start + i as u64) % SCOPE_CAPACITY as u64) as usize;
            self.slots[index].store(pack(left[i], right[i]), Ordering::Relaxed);
        }
        self.written.fetch_add(frames as u64, Ordering::Release);
    }

    /// Copies the newest frames into `out`, oldest first, and returns how many were copied.
    ///
    /// At most [`SCOPE_CAPACITY`] frames and at most `out.len()` frames are returned; before
    /// any audio has been captured the result is 0.
    pub fn read_latest(&self, out: &mut [(f32, f32)]) -> usize {
        let written = self.written.load(Ordering::Acquire);
        let available = written.min(SCOPE_CAPACITY as u64) as usize;
        let count = out.len().min(available);
        if count == 0 {
            return 0;
        }
        let first = written - count as u64;
        for (k, slot) in out[..count].iter_mut().enumerate() {
            let index = ((first + k as u64) % SCOPE_CAPACITY as u64) as usize;
            *slot = unpack(self.slots[index].load(Ordering::Relaxed));
        }
        count
    }
}

impl Default for StereoScope {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for StereoScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StereoScope")
            .field("capacity", &SCOPE_CAPACITY)
            .field("written", &self.written.load(Ordering::Relaxed))
            .finish()
    }
}
