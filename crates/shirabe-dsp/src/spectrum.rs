//! Band definitions, capture ring and spectral analysis.
//!
//! The capture side is lock-free and allocation-free: the audio thread writes mono frames into a
//! fixed ring, the editor reads the newest window and runs the FFT on the UI thread.
//!
//! Frequencies of the 1/3-octave display follow the base-ten series of IEC 61260-1 §5.4.1:
//! `f = 1000 * 10^(n/10)` with the octave ratio `G = 10^(3/10)`. The bands are **FFT-bin
//! aggregations for display**; they are not class 1 or class 2 band-pass filters (see
//! `docs/standards.md`).

use std::sync::atomic::{fence, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};

use crate::MINUS_INFINITY_DB;

/// FFT window length at 44.1/48 kHz. At 48 kHz one bin is about 5.86 Hz.
pub const FFT_SIZE: usize = 8_192;
/// Frames kept by the capture ring (512 KiB): a 32768-frame window plus 250 ms at
/// 192 kHz, with room for a burst already being written while the editor copies.
pub const SPECTRUM_CAPACITY: usize = 131_072;
/// Number of 1/3-octave bands from 20 Hz to 20 kHz.
pub const BAND_COUNT: usize = 31;
/// Band step of the lowest band: `1000 * 10^(-17/10)` ≈ 19.95 Hz.
pub const FIRST_BAND_STEP: i32 = -17;
const RELEASE_DB_PER_SECOND: f32 = 25.0;
const PEAK_HOLD_SECONDS: f32 = 1.25;

/// Nominal labels of the 1/3-octave bands, low to high.
///
/// These are the customary labels of the base-ten series; the exact centre frequencies used for
/// the bin ranges come from [`exact_center_hz`].
pub const NOMINAL_LABELS: [&str; BAND_COUNT] = [
    "20", "25", "31.5", "40", "50", "63", "80", "100", "125", "160", "200", "250", "315", "400",
    "500", "630", "800", "1k", "1.25k", "1.6k", "2k", "2.5k", "3.15k", "4k", "5k", "6.3k", "8k",
    "10k", "12.5k", "16k", "20k",
];

/// Exact centre frequency of band `index` in Hz (IEC 61260-1 §5.4.1, base ten).
pub fn exact_center_hz(index: usize) -> f32 {
    let step = FIRST_BAND_STEP + index as i32;
    1000.0 * 10f32.powf(step as f32 / 10.0)
}

/// Band edges in Hz. The edges are the geometric midpoints between adjacent exact centres.
///
/// IEC 61260-1 §5.6 defines the band-edge frequencies; its text was not available in the
/// accessible preview, so these edges are derived from the exact centres and are not claimed as
/// a verified quote of that clause.
pub fn band_edges_hz(index: usize) -> (f32, f32) {
    let centre = exact_center_hz(index);
    (centre * 10f32.powf(-0.05), centre * 10f32.powf(0.05))
}

/// A bounded, lock-free mono sample ring.
///
/// One producer (the audio thread) and any number of readers. No method allocates or blocks.
pub struct SpectrumCapture {
    /// Fixed capacity, allocated once when the capture is created. Keeping it on the heap
    /// keeps the size of a meter engine (and of a stack frame that builds one) small.
    slots: Box<[AtomicU32]>,
    written: AtomicU64,
    base: AtomicU64,
    reserved: AtomicU64,
    epoch: AtomicU64,
}

impl SpectrumCapture {
    /// Creates an empty capture ring.
    pub fn new() -> Self {
        Self {
            slots: (0..SPECTRUM_CAPACITY)
                .map(|_| AtomicU32::new(0f32.to_bits()))
                .collect(),
            written: AtomicU64::new(0),
            base: AtomicU64::new(0),
            reserved: AtomicU64::new(0),
            epoch: AtomicU64::new(0),
        }
    }

    /// Discards every captured frame and restarts the frame coordinate at zero.
    pub fn clear(&self) {
        self.clear_at(0);
    }

    /// Discards every captured frame without rewinding the display clock.
    pub fn clear_at(&self, frame: u64) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        fence(Ordering::Release);
        self.base.store(frame, Ordering::Relaxed);
        self.written.store(frame, Ordering::Relaxed);
        self.reserved.store(frame, Ordering::Relaxed);
        self.epoch.fetch_add(1, Ordering::Release);
    }

    /// Appends mono frames (audio thread). Older frames are overwritten past the capacity.
    pub fn push(&self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let start = self.written.load(Ordering::Relaxed);
        let end = start + samples.len() as u64;
        // Exclude slots about to be overwritten, even while this push is still in flight.
        self.reserved.store(end, Ordering::Relaxed);
        fence(Ordering::Release);
        for (offset, sample) in samples.iter().enumerate() {
            let index = ((start + offset as u64) % SPECTRUM_CAPACITY as u64) as usize;
            self.slots[index].store(sample.to_bits(), Ordering::Relaxed);
        }
        self.written.store(end, Ordering::Release);
    }

    /// Copies the newest frames into `out`, oldest first, and returns how many were copied.
    pub fn read_latest(&self, out: &mut [f32]) -> usize {
        let epoch = self.epoch();
        let end = self.written.load(Ordering::Acquire);
        let count = out
            .len()
            .min(self.written().min(SPECTRUM_CAPACITY as u64) as usize);
        if self.read_ending_at(end, epoch, &mut out[..count]) {
            count
        } else {
            0
        }
    }

    /// Copies the exact window before exclusive audio frame `end` in `epoch`, oldest first.
    /// Returns false if the window is incomplete, overwritten, or races a reset or overwrite;
    /// callers must not use the copied data in those cases.
    pub fn read_ending_at(&self, end: u64, epoch: u64, out: &mut [f32]) -> bool {
        let Some(first) = end.checked_sub(out.len() as u64) else {
            return false;
        };
        if epoch % 2 != 0
            || self.epoch() != epoch
            || end > self.written.load(Ordering::Acquire)
            || first < self.base.load(Ordering::Relaxed)
            || first < self.oldest_unreserved()
        {
            return false;
        }
        for (k, slot) in out.iter_mut().enumerate() {
            let index = ((first + k as u64) % SPECTRUM_CAPACITY as u64) as usize;
            *slot = f32::from_bits(self.slots[index].load(Ordering::Relaxed));
        }
        // If any slot read saw a concurrent overwrite, this fence makes the reservation
        // visible to the check below (as in a seqlock payload copy).
        fence(Ordering::Acquire);
        first >= self.oldest_unreserved() && self.epoch.load(Ordering::Relaxed) == epoch
    }

    fn oldest_unreserved(&self) -> u64 {
        self.reserved
            .load(Ordering::Relaxed)
            .saturating_sub(SPECTRUM_CAPACITY as u64)
    }

    /// Frames written since the last clear.
    pub fn written(&self) -> u64 {
        self.written
            .load(Ordering::Acquire)
            .saturating_sub(self.base.load(Ordering::Relaxed))
    }

    /// Counter that changes on every clear, so a reader can drop results from an older run.
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
}

impl Default for SpectrumCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SpectrumCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpectrumCapture")
            .field("capacity", &SPECTRUM_CAPACITY)
            .field("written", &self.written.load(Ordering::Relaxed))
            .field("epoch", &self.epoch.load(Ordering::Relaxed))
            .finish()
    }
}

/// Copies the mono sum of two equal-length channels into `out` and returns the frame count.
///
/// The sum is halved so a dual-mono signal keeps the level of the mono signal. A signal present
/// in only one channel is therefore 6 dB lower than the same signal in both.
pub fn mono_sum(left: &[f32], right: &[f32], out: &mut [f32]) -> usize {
    let frames = left.len().min(right.len()).min(out.len());
    for (slot, (l, r)) in out[..frames].iter_mut().zip(left.iter().zip(right)) {
        *slot = 0.5 * (l + r);
    }
    frames
}

/// One FFT of the newest captured window, with averaging and 1/3-octave band aggregation.
///
/// The FFT plan, window and buffers are allocated in [`SpectrumAnalyzer::new`]; [`Self::analyze_at`]
/// only reuses them, so it can run on the UI thread without affecting the audio callback.
///
/// `0 dBFS` is a full-scale sine at a bin centre (peak amplitude 1.0). A band shows the power in
/// its bins with the same reference, so a full-scale sine reads 0 dB in its band while broadband
/// noise reads its RMS level about 3 dB above the sine-equivalent reference.
pub struct SpectrumAnalyzer {
    sample_rate: f32,
    fft_size: usize,
    window: Vec<f32>,
    short_window: Vec<f32>,
    long_scale: WindowScale,
    short_scale: WindowScale,
    short_mix: Vec<f32>,
    short_input: Vec<f32>,
    short_output: Vec<realfft::num_complex::Complex32>,
    scratch: Vec<realfft::num_complex::Complex32>,
    input: Vec<f32>,
    output: Vec<realfft::num_complex::Complex32>,
    fft: Arc<dyn RealToComplex<f32>>,
    bins_db: Vec<f32>,
    bands_db: [f32; BAND_COUNT],
    bin_peaks_db: Vec<f32>,
    band_peaks_db: [f32; BAND_COUNT],
    bin_hold_until: Vec<u64>,
    band_hold_until: [u64; BAND_COUNT],
    band_bins: [(usize, usize); BAND_COUNT],
    last_written: u64,
    last_epoch: u64,
    analyzed: u64,
    ready: bool,
}

impl SpectrumAnalyzer {
    /// Prepares a plan for `sample_rate`. Allocates; call it outside the audio thread.
    pub fn new(sample_rate: f32) -> Self {
        let fft_size = if sample_rate >= 144_000.0 {
            FFT_SIZE * 4
        } else if sample_rate >= 72_000.0 {
            FFT_SIZE * 2
        } else {
            FFT_SIZE
        };
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(fft_size);
        let output = fft.make_output_vec();

        let window = hann(fft_size);
        let short_window = hann(fft_size / 8);
        let long_scale = WindowScale::new(&window, fft_size);
        let short_scale = WindowScale::new(&short_window, fft_size);
        let bin_hz = sample_rate / fft_size as f32;
        let short_mix = (0..=fft_size / 2)
            .map(|bin| {
                // Crossfade power over the transition, not the complex FFT outputs (which
                // refer to windows centred at different times).
                let x = ((bin as f32 * bin_hz - 500.0) / 300.0).clamp(0.0, 1.0);
                x * x * (3.0 - 2.0 * x)
            })
            .collect();
        let nyquist = sample_rate / 2.0;
        let band_bins = std::array::from_fn(|index| {
            let (low, high) = band_edges_hz(index);
            if low >= nyquist {
                // The band lies entirely above Nyquist: an empty range keeps it at the floor.
                return (fft_size / 2, fft_size / 2);
            }
            let first = ((low / bin_hz).ceil().max(1.0) as usize).min(fft_size / 2);
            let mut last = (high / bin_hz).ceil() as usize;
            last = last.clamp(first + 1, fft_size / 2 + 1);
            // A band narrower than one bin (only the lowest bands) uses its nearest bin.
            if ((high - low) / bin_hz) < 1.0 {
                let centre = (exact_center_hz(index) / bin_hz).round() as usize;
                let centre = centre.clamp(1, fft_size / 2);
                (centre, centre + 1)
            } else {
                (first, last)
            }
        });

        let bins = fft_size / 2 + 1;
        Self {
            sample_rate,
            fft_size,
            window,
            short_window,
            long_scale,
            short_scale,
            short_mix,
            short_input: fft.make_input_vec(),
            short_output: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            input: vec![0.0; fft_size],
            output,
            fft,
            bins_db: vec![MINUS_INFINITY_DB; bins],
            bands_db: [MINUS_INFINITY_DB; BAND_COUNT],
            bin_peaks_db: vec![MINUS_INFINITY_DB; bins],
            band_peaks_db: [MINUS_INFINITY_DB; BAND_COUNT],
            bin_hold_until: vec![0; bins],
            band_hold_until: [0; BAND_COUNT],
            band_bins,
            last_written: 0,
            last_epoch: 0,
            analyzed: 0,
            ready: false,
        }
    }

    /// Sample rate this analyzer was prepared for.
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// FFT length in samples.
    pub fn fft_size(&self) -> usize {
        self.fft_size
    }

    /// Number of analysis frames taken since the last reset.
    pub fn analyzed_frames(&self) -> u64 {
        self.analyzed
    }

    /// Whether at least one window has been analysed since the last reset.
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// Frequency of bin `index` in Hz.
    pub fn bin_hz(&self, index: usize) -> f32 {
        index as f32 * self.sample_rate / self.fft_size as f32
    }

    /// Per-bin level in dBFS (index 0 is DC, the last bin is Nyquist).
    pub fn bins_db(&self) -> &[f32] {
        &self.bins_db
    }

    /// Per-band level in dBFS, low to high. See [`NOMINAL_LABELS`] for the labels.
    pub fn bands_db(&self) -> &[f32; BAND_COUNT] {
        &self.bands_db
    }

    /// Peak-hold trace in dBFS, on the same grid as [`Self::bins_db`].
    pub fn bin_peaks_db(&self) -> &[f32] {
        &self.bin_peaks_db
    }

    /// Peak-hold ticks in dBFS, on the same grid as [`Self::bands_db`].
    pub fn band_peaks_db(&self) -> &[f32; BAND_COUNT] {
        &self.band_peaks_db
    }

    /// The strongest bin as `(frequency_hz, level_db)` once a window has been analysed.
    pub fn peak(&self) -> Option<(f32, f32)> {
        if !self.ready {
            return None;
        }
        let (index, level) = self.bins_db.iter().enumerate().skip(1).fold(
            (1, MINUS_INFINITY_DB),
            |best, (index, &level)| {
                if level > best.1 {
                    (index, level)
                } else {
                    best
                }
            },
        );
        Some((self.bin_hz(index), level))
    }

    /// Analyses the latest captured window (rather than a delayed display frame).
    pub fn analyze(&mut self, capture: &SpectrumCapture) -> bool {
        let epoch = capture.epoch();
        let end = capture.written.load(Ordering::Acquire);
        self.analyze_at(capture, end, epoch)
    }

    /// Exclusive end of the most recent successful analysis.
    pub fn analyzed_end(&self) -> Option<u64> {
        self.ready.then_some(self.last_written)
    }

    /// Analyses once at `end` if at least 5 ms of audio passed since the last analysis.
    /// An epoch change clears the envelopes, even when a fresh window isn't ready yet.
    pub fn analyze_at(&mut self, capture: &SpectrumCapture, end: u64, expected_epoch: u64) -> bool {
        let epoch = capture.epoch();
        if epoch != self.last_epoch {
            self.reset();
            self.last_epoch = epoch;
        }
        if epoch != expected_epoch {
            return false;
        }
        let hop = (self.sample_rate * 0.005).round().max(1.0) as u64;
        if self.ready && end.saturating_sub(self.last_written) < hop {
            return false;
        }
        if !capture.read_ending_at(end, epoch, &mut self.input) {
            return false;
        }

        self.short_input.fill(0.0);
        let tail = &self.input[self.fft_size - self.short_window.len()..];
        for ((out, &sample), &window) in self
            .short_input
            .iter_mut()
            .zip(tail)
            .zip(&self.short_window)
        {
            *out = sample * window;
        }
        for (sample, &window) in self.input.iter_mut().zip(&self.window) {
            *sample *= window;
        }
        if self
            .fft
            .process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch)
            .is_err()
            || self
                .fft
                .process_with_scratch(
                    &mut self.short_input,
                    &mut self.short_output,
                    &mut self.scratch,
                )
                .is_err()
        {
            return false;
        }

        let step = BallisticsStep {
            previous: self.last_written,
            end,
            hold_until: end + (self.sample_rate * PEAK_HOLD_SECONDS).round() as u64,
            db_per_frame: RELEASE_DB_PER_SECOND / self.sample_rate,
        };
        self.last_written = end;
        for i in 0..self.bins_db.len() {
            let mix = self.short_mix[i];
            let ratio = self.output[i].norm_sqr() * self.long_scale.bin_power * (1.0 - mix)
                + self.short_output[i].norm_sqr() * self.short_scale.bin_power * mix;
            step.apply(
                ratio,
                &mut self.bins_db[i],
                &mut self.bin_peaks_db[i],
                &mut self.bin_hold_until[i],
            );
        }
        for (i, (start, end)) in self.band_bins.iter().copied().enumerate() {
            // Use a single power-normalised window per band, rather than summing the FFT
            // display curve, whose bin gain is calibrated for sine amplitude.
            let (spectrum, scale) = if i >= 16 {
                (&self.short_output, self.short_scale)
            } else {
                (&self.output, self.long_scale)
            };
            let sum: f32 = spectrum[start..end].iter().map(|bin| bin.norm_sqr()).sum();
            step.apply(
                sum * scale.band_power,
                &mut self.bands_db[i],
                &mut self.band_peaks_db[i],
                &mut self.band_hold_until[i],
            );
        }
        self.analyzed += 1;
        self.ready = true;
        true
    }

    /// Clears every displayed value and starts the envelopes from scratch.
    pub fn reset(&mut self) {
        self.bins_db.fill(MINUS_INFINITY_DB);
        self.bands_db.fill(MINUS_INFINITY_DB);
        self.bin_peaks_db.fill(MINUS_INFINITY_DB);
        self.band_peaks_db.fill(MINUS_INFINITY_DB);
        self.bin_hold_until.fill(0);
        self.band_hold_until.fill(0);
        self.last_written = 0;
        self.analyzed = 0;
        self.ready = false;
    }

    /// Window sum used for the level reference (exposed for tests of the normalisation).
    pub fn window_sum(&self) -> f32 {
        self.long_scale.sum
    }
}

struct BallisticsStep {
    previous: u64,
    end: u64,
    hold_until: u64,
    db_per_frame: f32,
}

impl BallisticsStep {
    fn apply(&self, power: f32, level: &mut f32, peak: &mut f32, expires: &mut u64) {
        let target = power_to_db(power);
        let decay = |from| self.end.saturating_sub(from) as f32 * self.db_per_frame;
        *level = target.max(*level - decay(self.previous));
        let falling_peak = *peak - decay(self.previous.max(*expires));
        if target >= falling_peak {
            *peak = target;
            *expires = self.hold_until;
        } else {
            *peak = falling_peak.max(*level);
        }
    }
}

#[derive(Clone, Copy)]
struct WindowScale {
    sum: f32,
    bin_power: f32,
    band_power: f32,
}

impl WindowScale {
    fn new(window: &[f32], fft_size: usize) -> Self {
        let sum: f32 = window.iter().sum();
        let square_sum: f32 = window.iter().map(|w| w * w).sum();
        Self {
            sum,
            bin_power: 4.0 / (sum * sum),
            band_power: 4.0 / (fft_size as f32 * square_sum),
        }
    }
}

fn hann(size: usize) -> Vec<f32> {
    (0..size)
        .map(|n| 0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / size as f32).cos())
        .collect()
}

/// Linear power ratio to dBFS with the meter's silence floor.
fn power_to_db(ratio: f32) -> f32 {
    if ratio <= 0.0 {
        MINUS_INFINITY_DB
    } else {
        (10.0 * ratio.log10()).max(MINUS_INFINITY_DB)
    }
}
