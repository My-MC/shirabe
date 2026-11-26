//! Lock-free single-producer / single-consumer ring buffer of interleaved frames.
//!
//! The decode thread produces frames, the output callback consumes them. Neither side
//! locks or allocates after construction: the buffer is a preallocated `Vec<f32>` and
//! the positions are atomics with acquire/release ordering.
//!
//! The producer and the consumer are separate types, so the compiler enforces the
//! single-producer / single-consumer contract.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// State shared by one producer and one consumer.
struct Shared {
    /// Interleaved samples, `capacity_frames * channels` long. Each sample sits in its
    /// own cell, so neither side ever holds a reference to memory the other writes.
    buf: Box<[UnsafeCell<f32>]>,
    /// Channels per frame.
    channels: usize,
    /// Samples written since the start.
    head: AtomicU64,
    /// Samples read since the start.
    tail: AtomicU64,
    /// Frames consumed since the start, for the position display.
    played: Arc<AtomicU64>,
}

// The buffer is split by the head and tail positions: the producer writes only between
// head and tail (modulo the capacity), the consumer reads only between tail and head,
// and both exchange positions with acquire/release. Each sample has its own cell, so
// the two sides never alias a `f32`.
unsafe impl Sync for Shared {}

impl Shared {
    fn frames(&self) -> usize {
        self.buf.len() / self.channels
    }

    /// Copies samples starting at absolute sample `from` into `dst`.
    fn copy_out(&self, from: u64, dst: &mut [f32]) {
        let cap = self.buf.len();
        let start = (from % cap as u64) as usize;
        for (i, value) in dst.iter_mut().enumerate() {
            // Safety: the head and tail positions give this side ownership of these
            // samples; the other side only writes cells outside this range.
            *value = unsafe { *self.buf[(start + i) % cap].get() };
        }
    }

    /// Copies `src` into the ring starting at absolute sample `from`.
    fn copy_in(&self, from: u64, src: &[f32]) {
        let cap = self.buf.len();
        let start = (from % cap as u64) as usize;
        for (i, value) in src.iter().enumerate() {
            // Safety: as in `copy_out`; the cells written here are in the free region.
            unsafe { *self.buf[(start + i) % cap].get() = *value };
        }
    }
}

/// The writing side, used by the decode thread.
pub struct Producer {
    shared: Arc<Shared>,
    head: u64,
}

/// The reading side, used by the audio callback.
pub struct Consumer {
    shared: Arc<Shared>,
    tail: u64,
}

/// Creates a ring buffer of `capacity_frames` frames of `channels` channels.
pub fn ring(capacity_frames: usize, channels: usize) -> (Producer, Consumer) {
    assert!(channels >= 1, "at least one channel");
    assert!(capacity_frames >= 2, "at least two frames");
    let samples: Vec<UnsafeCell<f32>> = (0..capacity_frames * channels)
        .map(|_| UnsafeCell::new(0.0))
        .collect();
    let shared = Arc::new(Shared {
        buf: samples.into_boxed_slice(),
        channels,
        head: AtomicU64::new(0),
        tail: AtomicU64::new(0),
        played: Arc::new(AtomicU64::new(0)),
    });
    (
        Producer {
            shared: shared.clone(),
            head: 0,
        },
        Consumer { shared, tail: 0 },
    )
}

impl Producer {
    /// Frames that can be pushed without waiting.
    pub fn free_frames(&self) -> usize {
        let tail = self.shared.tail.load(Ordering::Acquire);
        let buffered = (self.head - tail) / self.shared.channels as u64;
        self.shared.frames() - buffered as usize
    }

    /// Pushes whole frames and returns how many were written (fewer than `frames`
    /// when the buffer is nearly full).
    pub fn push(&mut self, frames: &[f32]) -> usize {
        let ch = self.shared.channels;
        let want = frames.len() / ch;
        let n = want.min(self.free_frames());
        if n == 0 {
            return 0;
        }
        self.shared.copy_in(self.head, &frames[..n * ch]);
        self.head += (n * ch) as u64;
        self.shared.head.store(self.head, Ordering::Release);
        n
    }
}

impl Consumer {
    /// Pops whole frames into `out` and returns how many were written. Frames that
    /// are not available are left untouched (the caller fills silence).
    pub fn pop(&mut self, out: &mut [f32]) -> usize {
        let ch = self.shared.channels;
        let want = out.len() / ch;
        let head = self.shared.head.load(Ordering::Acquire);
        let available = ((head - self.tail) / ch as u64) as usize;
        let n = want.min(available);
        if n == 0 {
            return 0;
        }
        self.shared.copy_out(self.tail, &mut out[..n * ch]);
        self.tail += (n * ch) as u64;
        self.shared.tail.store(self.tail, Ordering::Release);
        self.shared.played.fetch_add(n as u64, Ordering::Relaxed);
        n
    }

    /// Frames consumed since the ring was created (used by the tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn frames_played(&self) -> u64 {
        self.shared.played.load(Ordering::Relaxed)
    }

    /// The played counter, readable from the user interface.
    pub fn played_handle(&self) -> Arc<AtomicU64> {
        self.shared.played.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::{Duration, Instant};

    fn seq(from: usize, frames: usize, ch: usize) -> Vec<f32> {
        (from..from + frames)
            .flat_map(|i| std::iter::repeat_n(i as f32, ch))
            .collect()
    }

    #[test]
    fn pushes_and_pops_the_same_frames() {
        let (mut p, mut c) = ring(8, 2);
        let data = seq(0, 3, 2);
        assert_eq!(p.push(&data), 3);
        let mut out = vec![0.0; 4 * 2];
        assert_eq!(c.pop(&mut out), 3);
        assert_eq!(&out[..6], &data[..]);
        assert_eq!(c.frames_played(), 3);
    }

    #[test]
    fn a_full_ring_accepts_only_what_fits() {
        let (mut p, mut c) = ring(4, 2);
        let data = seq(0, 6, 2);
        assert_eq!(p.push(&data), 4);
        assert_eq!(p.free_frames(), 0);
        assert_eq!(p.push(&data), 0);

        let mut out = vec![0.0; 2 * 2];
        assert_eq!(c.pop(&mut out), 2);
        assert_eq!(p.push(&seq(4, 2, 2)), 2);
        assert_eq!(p.free_frames(), 0);

        let mut rest = vec![0.0; 4 * 2];
        assert_eq!(c.pop(&mut rest), 4);
        assert_eq!(&rest[..], &[2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 5.0, 5.0]);
    }

    #[test]
    fn wraps_around_without_losing_order() {
        let (mut p, mut c) = ring(8, 2);
        let mut next_in = 0usize;
        let mut next_out = 0usize;
        let mut out = vec![0.0; 8 * 2];
        for round in 0..200 {
            let n = 1 + round % 5;
            // Make room, checking every frame that comes out.
            while p.free_frames() < n {
                let got = c.pop(&mut out);
                assert!(got > 0, "ring reported no free space but nothing to pop");
                let expect = seq(next_out, got, 2);
                assert_eq!(&out[..got * 2], &expect[..], "round {round}");
                next_out += got;
            }
            assert_eq!(p.push(&seq(next_in, n, 2)), n);
            next_in += n;

            let take = 1 + (round * 3) % 4;
            let got = c.pop(&mut out[..take * 2]);
            let expect = seq(next_out, got, 2);
            assert_eq!(&out[..got * 2], &expect[..], "round {round}");
            next_out += got;
        }
        // Everything that was written but not yet read is still in order.
        loop {
            let got = c.pop(&mut out);
            if got == 0 {
                break;
            }
            let expect = seq(next_out, got, 2);
            assert_eq!(&out[..got * 2], &expect[..]);
            next_out += got;
        }
        assert_eq!(next_out, next_in);
    }

    #[test]
    fn pops_only_whole_frames() {
        let (mut p, mut c) = ring(8, 2);
        assert_eq!(p.push(&seq(0, 1, 2)), 1);
        // A buffer that is not a whole number of frames still yields whole frames.
        let mut out = vec![0.0; 3];
        assert_eq!(c.pop(&mut out), 1);
    }

    #[test]
    fn a_producer_thread_and_a_consumer_thread_exchange_every_frame() {
        let (mut p, mut c) = ring(64, 2);
        const FRAMES: usize = 20_000;
        let producer = thread::spawn(move || {
            let mut sent = 0;
            while sent < FRAMES {
                let n = (FRAMES - sent).min(7);
                let data = seq(sent, n, 2);
                let mut written = p.push(&data);
                while written < n {
                    thread::sleep(Duration::from_micros(50));
                    written += p.push(&data[written * 2..]);
                }
                sent += n;
            }
        });

        let mut received = Vec::with_capacity(FRAMES);
        let mut out = vec![0.0; 5 * 2];
        let start = Instant::now();
        while received.len() < FRAMES {
            let got = c.pop(&mut out);
            for i in 0..got {
                received.push(out[i * 2]);
            }
            if got == 0 {
                assert!(
                    start.elapsed() < Duration::from_secs(10),
                    "consumer stalled"
                );
                thread::yield_now();
            }
        }
        producer.join().unwrap();
        assert_eq!(received.len(), FRAMES);
        for (i, v) in received.iter().enumerate() {
            assert_eq!(*v, i as f32, "frame {i} out of order");
        }
        assert_eq!(c.frames_played(), FRAMES as u64);
    }
}
