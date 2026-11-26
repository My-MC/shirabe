//! Band definitions, capture ring and spectral analysis.
//!
//! The capture side is lock-free and allocation-free: the audio thread writes mono frames into a
//! fixed ring, the editor reads the newest window and runs the FFT on the UI thread.
//!
//! Frequencies of the 1/3-octave display follow the base-ten series of IEC 61260-1 §5.4.1:
//! `f = 1000 * 10^(n/10)` with the octave ratio `G = 10^(3/10)`. The bands are **FFT-bin
//! aggregations for display**; they are not class 1 or class 2 band-pass filters (see
//! `docs/standards.md`).

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};

use crate::MINUS_INFINITY_DB;

/// FFT window length in samples (16384). At 48 kHz one bin is about 2.93 Hz.
pub const FFT_SIZE: usize = 16_384;
/// Frames kept by the capture ring (four windows, 256 KiB).
pub const SPECTRUM_CAPACITY: usize = 65_536;
/// Number of 1/3-octave bands from 20 Hz to 20 kHz.
pub const BAND_COUNT: usize = 31;
/// Band step of the lowest band: `1000 * 10^(-17/10)` ≈ 19.95 Hz.
pub const FIRST_BAND_STEP: i32 = -17;
/// Default exponential averaging time constant in seconds.
pub const SMOOTHING_SECONDS: f32 = 0.125;

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

/// Configuration of a [`SpectrumAnalyzer`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpectrumConfig {
    /// FFT length; must be a power of two and at most [`SPECTRUM_CAPACITY`].
    pub fft_size: usize,
    /// Exponential averaging time constant in seconds; `0` keeps every frame as it is.
    pub smoothing_seconds: f32,
}

impl Default for SpectrumConfig {
    fn default() -> Self {
        Self {
            fft_size: FFT_SIZE,
            smoothing_seconds: SMOOTHING_SECONDS,
        }
    }
}

/// A bounded, lock-free mono sample ring.
///
/// One producer (the audio thread) and any number of readers. No method allocates or blocks.
pub struct SpectrumCapture {
    /// Fixed capacity, allocated once when the capture is created. Keeping it on the heap
    /// keeps the size of a meter engine (and of a stack frame that builds one) small.
    slots: Box<[AtomicU32]>,
    written: AtomicU64,
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
            epoch: AtomicU64::new(0),
        }
    }

    /// Discards every captured frame.
    pub fn clear(&self) {
        self.written.store(0, Ordering::Relaxed);
        self.epoch.fetch_add(1, Ordering::Release);
    }

    /// Appends mono frames (audio thread). Older frames are overwritten past the capacity.
    pub fn push(&self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        // The counter is advanced once, after the samples, so a reader never sees a slot that
        // has been announced but not yet written.
        let start = self.written.load(Ordering::Relaxed);
        for (offset, sample) in samples.iter().enumerate() {
            let index = ((start + offset as u64) % SPECTRUM_CAPACITY as u64) as usize;
            self.slots[index].store(sample.to_bits(), Ordering::Relaxed);
        }
        self.written
            .fetch_add(samples.len() as u64, Ordering::Release);
    }

    /// Copies the newest frames into `out`, oldest first, and returns how many were copied.
    pub fn read_latest(&self, out: &mut [f32]) -> usize {
        let written = self.written.load(Ordering::Acquire);
        let available = written.min(SPECTRUM_CAPACITY as u64) as usize;
        let count = out.len().min(available);
        if count == 0 {
            return 0;
        }
        let first = written - count as u64;
        for (k, slot) in out[..count].iter_mut().enumerate() {
            let index = ((first + k as u64) % SPECTRUM_CAPACITY as u64) as usize;
            *slot = f32::from_bits(self.slots[index].load(Ordering::Relaxed));
        }
        count
    }

    /// Frames written since the last [`clear`](Self::clear).
    pub fn written(&self) -> u64 {
        self.written.load(Ordering::Acquire)
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
/// The FFT plan, window and buffers are allocated in [`SpectrumAnalyzer::new`]; [`analyze`]
/// only reuses them, so it can run on the UI thread without affecting the audio callback.
///
/// `0 dBFS` is a full-scale sine at a bin centre (peak amplitude 1.0). A band shows the power in
/// its bins with the same reference, so a full-scale sine reads 0 dB in its band while broadband
/// noise reads its RMS level about 3 dB above the sine-equivalent reference.
pub struct SpectrumAnalyzer {
    sample_rate: f32,
    fft_size: usize,
    window: Vec<f32>,
    /// `sum(w) / 2`: the peak amplitude of a bin-centred sine divided by its FFT magnitude.
    amplitude_scale: f32,
    reference_power: f32,
    /// Band integration factor `sum(w)^2 / (N * sum(w^2))`.
    band_scale: f32,
    input: Vec<f32>,
    output: Vec<realfft::num_complex::Complex32>,
    fft: Arc<dyn RealToComplex<f32>>,
    smoothing_seconds: f32,
    power: Vec<f32>,
    bins_db: Vec<f32>,
    bands_db: [f32; BAND_COUNT],
    band_bins: [(usize, usize); BAND_COUNT],
    last_written: u64,
    last_epoch: u64,
    analyzed: u64,
    ready: bool,
}

impl SpectrumAnalyzer {
    /// Prepares a plan for `sample_rate` and `config`. Allocates; call it outside the audio thread.
    ///
    /// # Panics
    /// Panics if `config.fft_size` is not a power of two between 2 and [`SPECTRUM_CAPACITY`].
    pub fn new(sample_rate: f32, config: SpectrumConfig) -> Self {
        let fft_size = config.fft_size;
        assert!(
            fft_size.is_power_of_two() && (2..=SPECTRUM_CAPACITY).contains(&fft_size),
            "fft_size must be a power of two between 2 and {SPECTRUM_CAPACITY}"
        );
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(fft_size);
        let output = fft.make_output_vec();

        // Periodic Hann window: 0.5 - 0.5 cos(2 pi n / N).
        let window: Vec<f32> = (0..fft_size)
            .map(|n| 0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / fft_size as f32).cos())
            .collect();
        let window_sum: f32 = window.iter().sum();
        let amplitude_scale = window_sum / 2.0;
        let reference_power = amplitude_scale * amplitude_scale;
        let window_square_sum: f32 = window.iter().map(|w| w * w).sum();
        let band_scale = window_sum * window_sum / (fft_size as f32 * window_square_sum);

        let bin_hz = sample_rate / fft_size as f32;
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
            amplitude_scale,
            reference_power,
            band_scale,
            input: vec![0.0; fft_size],
            output,
            fft,
            smoothing_seconds: config.smoothing_seconds,
            power: vec![0.0; bins],
            bins_db: vec![MINUS_INFINITY_DB; bins],
            bands_db: [MINUS_INFINITY_DB; BAND_COUNT],
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

    /// Analyses a new window when at least half a window of new audio has arrived.
    ///
    /// Returns `true` when the display values were updated. A clear of `capture` resets the
    /// analyser, so stale frames never survive a source change or Reset.
    pub fn analyze(&mut self, capture: &SpectrumCapture) -> bool {
        let epoch = capture.epoch();
        if epoch != self.last_epoch {
            self.reset();
            self.last_epoch = epoch;
        }
        let written = capture.written();
        if written < self.fft_size as u64 {
            return false;
        }
        let hop = (self.fft_size / 2) as u64;
        // A clear can land between the epoch check and this read; never underflow.
        let new_frames = written.saturating_sub(self.last_written);
        if self.analyzed > 0 && new_frames < hop {
            return false;
        }
        if capture.read_latest(&mut self.input) < self.fft_size {
            return false;
        }
        // A clear can race the copy; drop the window instead of mixing two runs.
        if capture.epoch() != epoch {
            return false;
        }
        self.last_written = written;

        for (sample, &window) in self.input.iter_mut().zip(&self.window) {
            *sample *= window;
        }
        if self.fft.process(&mut self.input, &mut self.output).is_err() {
            return false;
        }

        let alpha = self.smoothing_alpha(new_frames);
        let first = self.analyzed == 0;
        for (smoothed, bin) in self.power.iter_mut().zip(&self.output) {
            let ratio = bin.norm_sqr() / self.reference_power;
            if first {
                *smoothed = ratio;
            } else {
                *smoothed += alpha * (ratio - *smoothed);
            }
        }
        for (level, power) in self.bins_db.iter_mut().zip(&self.power) {
            *level = power_to_db(*power);
        }
        for (index, (start, end)) in self.band_bins.iter().copied().enumerate() {
            let sum: f32 = self.power[start..end].iter().sum();
            // Integrate the one-sided bin power over the band, normalised by the window's
            // power gain, so a bin-centred full-scale sine reads 0 dB in its band.
            let ratio = sum * self.band_scale;
            self.bands_db[index] = power_to_db(ratio);
        }
        self.analyzed += 1;
        self.ready = true;
        true
    }

    /// Clears every displayed value and starts averaging from scratch.
    pub fn reset(&mut self) {
        self.power.fill(0.0);
        self.bins_db.fill(MINUS_INFINITY_DB);
        self.bands_db.fill(MINUS_INFINITY_DB);
        self.last_written = 0;
        self.analyzed = 0;
        self.ready = false;
    }

    /// Window sum used for the level reference (exposed for tests of the normalisation).
    pub fn window_sum(&self) -> f32 {
        self.amplitude_scale * 2.0
    }

    fn smoothing_alpha(&self, new_frames: u64) -> f32 {
        if self.smoothing_seconds <= 0.0 || self.sample_rate <= 0.0 {
            return 1.0;
        }
        let dt = new_frames as f32 / self.sample_rate;
        (1.0 - (-dt / self.smoothing_seconds).exp()).clamp(0.0, 1.0)
    }
}

/// Linear power ratio to dBFS with the meter's silence floor.
fn power_to_db(ratio: f32) -> f32 {
    if ratio <= 0.0 {
        MINUS_INFINITY_DB
    } else {
        (10.0 * ratio.log10()).max(MINUS_INFINITY_DB)
    }
}
