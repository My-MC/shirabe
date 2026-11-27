//! Lossy, single-producer history of display-only meter samples.

use std::sync::atomic::{fence, AtomicU32, AtomicU64, Ordering::*};

use shirabe_dsp::MINUS_INFINITY_DB;

use crate::shared::MAX_CHANNELS;

/// More than one second of history at the nominal 200 Hz cadence.
pub(crate) const CAPACITY: usize = 256;
const VALUES: usize = MAX_CHANNELS * 3 + 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ChannelSample {
    pub level_db: f32,
    pub vu: f32,
    pub ppm_db: f32,
}

impl Default for ChannelSample {
    fn default() -> Self {
        Self {
            level_db: MINUS_INFINITY_DB,
            vu: MINUS_INFINITY_DB,
            ppm_db: MINUS_INFINITY_DB,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Snapshot {
    /// Exclusive end in audio frames since the last capture reset.
    pub frame: u64,
    pub scope_epoch: u64,
    pub channels: [ChannelSample; MAX_CHANNELS],
    pub momentary: Option<f32>,
    pub short_term: Option<f32>,
    pub correlation: Option<f32>,
    pub balance: Option<f32>,
}

struct Slot {
    stamp: AtomicU64,
    frame: AtomicU64,
    scope_epoch: AtomicU64,
    values: [AtomicU32; VALUES],
}

impl Slot {
    fn new() -> Self {
        Self {
            stamp: AtomicU64::new(0),
            frame: AtomicU64::new(0),
            scope_epoch: AtomicU64::new(0),
            values: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }

    fn put(&self, stamp: u64, point: Snapshot) {
        // Invalidate before overwriting. If any payload read sees the overwrite, the
        // reader's acquire fence makes the invalidation visible to its final stamp check.
        self.stamp.store(stamp - 1, Relaxed);
        fence(Release);
        self.frame.store(point.frame, Relaxed);
        self.scope_epoch.store(point.scope_epoch, Relaxed);
        for (values, channel) in self.values.chunks_exact(3).zip(point.channels) {
            for (word, value) in values
                .iter()
                .zip([channel.level_db, channel.vu, channel.ppm_db])
            {
                word.store(value.to_bits(), Relaxed);
            }
        }
        for (word, value) in self.values[MAX_CHANNELS * 3..].iter().zip([
            point.momentary,
            point.short_term,
            point.correlation,
            point.balance,
        ]) {
            word.store(value.unwrap_or(f32::NAN).to_bits(), Relaxed);
        }
        self.stamp.store(stamp, Release);
    }

    fn get(&self, stamp: u64) -> Option<Snapshot> {
        if self.stamp.load(Acquire) != stamp {
            return None;
        }
        let load = |i: usize| f32::from_bits(self.values[i].load(Relaxed));
        let optional = |i| {
            let value = load(MAX_CHANNELS * 3 + i);
            (!value.is_nan()).then_some(value)
        };
        let point = Snapshot {
            frame: self.frame.load(Relaxed),
            scope_epoch: self.scope_epoch.load(Relaxed),
            channels: std::array::from_fn(|i| ChannelSample {
                level_db: load(i * 3),
                vu: load(i * 3 + 1),
                ppm_db: load(i * 3 + 2),
            }),
            momentary: optional(0),
            short_term: optional(1),
            correlation: optional(2),
            balance: optional(3),
        };
        fence(Acquire);
        (self.stamp.load(Relaxed) == stamp).then_some(point)
    }
}

pub(crate) struct Timeline {
    slots: Box<[Slot]>,
    written: AtomicU64,
    start: AtomicU64,
    epoch: AtomicU64,
}

impl Timeline {
    pub fn new() -> Self {
        Self {
            slots: (0..CAPACITY).map(|_| Slot::new()).collect(),
            written: AtomicU64::new(0),
            start: AtomicU64::new(0),
            epoch: AtomicU64::new(0),
        }
    }

    pub fn clear(&self) {
        self.epoch.fetch_add(1, AcqRel);
        fence(Release);
        self.start.store(self.written.load(Relaxed), Relaxed);
        self.epoch.fetch_add(1, Release);
    }

    pub fn push(&self, point: Snapshot) {
        let index = self.written.load(Relaxed);
        self.slots[index as usize % CAPACITY].put((index + 1) * 2, point);
        self.written.store(index + 1, Release);
    }

    pub fn epoch(&self) -> u64 {
        self.epoch.load(Acquire)
    }

    /// Copies a single observed batch, retaining only complete snapshots. The caller commits
    /// neither the points nor the cursor if a clear races the copy.
    pub fn read(&self, cursor: u64, out: &mut Vec<Snapshot>) -> Option<(u64, u64)> {
        out.clear();
        let epoch = self.epoch();
        if epoch % 2 != 0 {
            return None;
        }
        let end = self.written.load(Acquire);
        let start = self
            .start
            .load(Relaxed)
            .max(end.saturating_sub(CAPACITY as u64));
        for index in cursor.max(start)..end {
            if let Some(point) = self.slots[index as usize % CAPACITY].get((index + 1) * 2) {
                out.push(point);
            }
        }
        fence(Acquire);
        (self.epoch.load(Relaxed) == epoch).then_some((end, epoch))
    }

    #[cfg(test)]
    pub fn points_for_test(&self) -> Vec<Snapshot> {
        let mut points = Vec::new();
        assert!(self.read(0, &mut points).is_some());
        points
    }
}
