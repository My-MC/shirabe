//! Stereo sample capture for the Lissajous view.
//!
//! The audio thread writes stereo frames into a fixed, pre-allocated ring; the editor reads
//! the newest frames without a lock and without allocating. The ring is deliberately lossy:
//! when the editor falls behind, the oldest frames are overwritten, because a scope only ever
//! displays the recent past.

use std::sync::atomic::{fence, AtomicU64, Ordering};

/// Default number of stereo frames kept by a short, undelayed capture.
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
/// One producer (the audio thread) and any number of readers (the editor). After
/// construction, no method allocates or blocks.
pub struct StereoScope {
    slots: Box<[AtomicU64]>,
    /// Exclusive end reserved by the producer, including any in-flight writes.
    reserved: AtomicU64,
    /// Number of frames written since the last clear; wraps only after 2^64 frames.
    written: AtomicU64,
    /// Incremented by every clear, so a reader can discard points from before a source reset.
    epoch: AtomicU64,
}

impl StereoScope {
    /// Creates an empty scope.
    pub fn new() -> Self {
        Self::with_capacity(SCOPE_CAPACITY)
    }

    /// Creates a scope retaining `capacity` stereo frames (a nonzero power of two).
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity.is_power_of_two());
        Self {
            slots: (0..capacity)
                .map(|_| AtomicU64::new(pack(0.0, 0.0)))
                .collect(),
            reserved: AtomicU64::new(0),
            written: AtomicU64::new(0),
            epoch: AtomicU64::new(0),
        }
    }

    /// Discards every captured frame. Only the producer may call this while capturing;
    /// other callers must first stop the producer.
    pub fn clear(&self) {
        self.reserved.store(0, Ordering::Relaxed);
        self.written.store(0, Ordering::Relaxed);
        self.epoch.fetch_add(1, Ordering::Release);
    }

    /// Appends one block of stereo frames (audio thread). Extra frames beyond the capacity
    /// overwrite the oldest ones.
    pub fn push(&self, left: &[f32], right: &[f32]) {
        let frames = left.len().min(right.len());
        if frames == 0 {
            return;
        }
        let start = self.written.load(Ordering::Relaxed);
        let end = start + frames as u64;
        self.reserved.store(end, Ordering::Relaxed);
        fence(Ordering::Release);
        let mask = self.slots.len() - 1;
        // When a single callback exceeds the capacity, only the tail can remain.
        for i in frames.saturating_sub(self.slots.len())..frames {
            let index = (start as usize + i) & mask;
            self.slots[index].store(pack(left[i], right[i]), Ordering::Relaxed);
        }
        self.written.store(end, Ordering::Release);
    }

    /// Copies the newest frames into `out`, oldest first, and returns how many were copied.
    pub fn read_latest(&self, out: &mut [(f32, f32)]) -> usize {
        let end = self.written();
        self.read_through(end.saturating_sub(out.len() as u64), end, out)
            .map_or(0, |(n, _)| n)
    }

    /// Number of frames written since the last [`clear`](Self::clear).
    pub fn written(&self) -> u64 {
        self.written.load(Ordering::Acquire)
    }

    /// Counter that changes on every [`clear`](Self::clear).
    ///
    /// Readers compare it with the value they saw last frame: a different value means the
    /// captured points belong to an earlier source, even if the write counter has already
    /// passed the old cursor.
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    /// Copies frames written after `cursor` and returns them with the new cursor.
    /// If a concurrent write invalidates the copy, returns no frames without advancing.
    pub fn read_since(&self, cursor: u64, out: &mut [(f32, f32)]) -> (usize, u64) {
        self.read_through(cursor, self.written(), out)
            .unwrap_or((0, cursor))
    }

    /// Copies retained frames between `cursor` and the exclusive `end`, oldest first.
    /// Skips overwritten frames and returns the number copied and the new cursor.
    /// Returns `None` if a concurrent clear or wrap may have invalidated the copy.
    pub fn read_through(
        &self,
        cursor: u64,
        end: u64,
        out: &mut [(f32, f32)],
    ) -> Option<(usize, u64)> {
        let epoch = self.epoch();
        let written = self.written();
        let end = end.min(written);
        let len = self.slots.len();
        let oldest = self
            .reserved
            .load(Ordering::Acquire)
            .saturating_sub(len as u64);
        let start = cursor.max(oldest).min(end);
        let count = out.len().min((end - start) as usize);
        for (k, slot) in out[..count].iter_mut().enumerate() {
            let index = (start as usize + k) & (len - 1);
            *slot = unpack(self.slots[index].load(Ordering::Relaxed));
        }
        // An overwritten payload synchronizes the writer's reservation with this check.
        fence(Ordering::Acquire);
        let overwritten = count != 0
            && start
                < self
                    .reserved
                    .load(Ordering::Relaxed)
                    .saturating_sub(len as u64);
        (!overwritten && self.epoch() == epoch).then_some((count, start + count as u64))
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
            .field("capacity", &self.slots.len())
            .field("written", &self.written.load(Ordering::Relaxed))
            .field("epoch", &self.epoch.load(Ordering::Relaxed))
            .finish()
    }
}
