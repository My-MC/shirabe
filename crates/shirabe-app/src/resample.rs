//! Band-limited sample rate conversion for the playback output path.
//!
//! The converter is a windowed-sinc polyphase resampler for one or two interleaved
//! channels. The ratio is taken as an exact rational number, `to / from`, reduced by
//! its greatest common divisor, so standard sample rates convert without drift
//! (44.1 kHz to 48 kHz is 160 / 147).
//!
//! Metering never uses this converter: the meters read the decoded file samples at
//! the file's own rate. Resampling happens between the decoder and the output ring
//! buffer, so the meters show the file as stored, not the converted signal.
//!
//! Quality: 96 taps per phase with a 4-term Blackman-Harris window and the cutoff at
//! 90 % of the lower Nyquist frequency. That is meant for monitoring playback, not
//! for offline mastering conversion. `docs/design.md` records the limits.

/// Taps per polyphase branch.
const TAPS: usize = 96;
/// Cutoff as a fraction of the lower of the two Nyquist frequencies.
const CUTOFF: f64 = 0.45;
/// Upper bound on the kernel length in samples, to bound memory for unusual rates.
const MAX_KERNEL: usize = 96 * 1024;
/// Channels handled by the converter.
pub const MAX_CHANNELS: usize = 2;

/// Converts interleaved samples from one sample rate to another.
pub struct Resampler {
    /// Upsampling factor (`to / gcd`).
    up: u64,
    /// Downsampling factor (`from / gcd`).
    down: u64,
    /// Channels per frame (1 or 2).
    channels: usize,
    /// Taps per branch.
    taps: usize,
    /// Windowed sinc, `taps * up` coefficients, branch-major.
    kernel: Vec<f32>,
    /// Input frames kept for the interpolation window, starting at `hist_start`.
    hist: Vec<f32>,
    /// Absolute index of the first frame of `hist`.
    hist_start: u64,
    /// Input frames received since the last reset.
    in_frames: u64,
    /// Output frames produced since the last reset.
    out_frames: u64,
}

impl Resampler {
    /// Creates a converter between two sample rates. Allocates.
    pub fn new(from: u32, to: u32, channels: usize) -> Self {
        assert!(from > 0 && to > 0, "sample rates must be positive");
        assert!((1..=MAX_CHANNELS).contains(&channels), "1 or 2 channels");
        let g = gcd(from as u64, to as u64);
        let up = to as u64 / g;
        let down = from as u64 / g;
        let taps = taps_for(up);
        Self {
            up,
            down,
            channels,
            taps,
            kernel: kernel(up, down, taps),
            hist: Vec::with_capacity((taps + 4096) * channels),
            hist_start: 0,
            in_frames: 0,
            out_frames: 0,
        }
    }

    /// True when no conversion is needed: samples pass through unchanged.
    pub fn is_identity(&self) -> bool {
        self.up == self.down
    }

    /// Frames the converter produces for `frames` input frames, including the tail.
    pub fn output_frames(&self, frames: u64) -> u64 {
        if self.is_identity() {
            frames
        } else {
            (frames * self.up).div_ceil(self.down)
        }
    }

    /// Converts interleaved input frames and appends the result to `out`.
    ///
    /// `input.len()` must be a multiple of the channel count. `out` grows; no other
    /// allocation takes place.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        let ch = self.channels;
        let frames = input.len() / ch;
        if frames == 0 {
            return;
        }
        if self.is_identity() {
            out.extend_from_slice(&input[..frames * ch]);
            self.in_frames += frames as u64;
            self.out_frames += frames as u64;
            return;
        }
        self.hist.extend_from_slice(&input[..frames * ch]);
        self.in_frames += frames as u64;
        self.emit(out, u64::MAX);
    }

    /// Appends the filter tail and returns the converter to its initial state.
    ///
    /// Call this at the end of a stream. The number of output frames for the whole
    /// stream is [`Resampler::output_frames`] of the input length.
    pub fn finish(&mut self, out: &mut Vec<f32>) {
        if !self.is_identity() {
            let total = self.output_frames(self.in_frames);
            self.emit(out, total);
        }
        self.reset();
    }

    /// Discards all state, keeping the filter. The next frame starts a new stream.
    pub fn reset(&mut self) {
        self.hist.clear();
        self.hist_start = 0;
        self.in_frames = 0;
        self.out_frames = 0;
    }

    /// Produces output frames while enough input is buffered, up to `limit` output
    /// frames in total.
    fn emit(&mut self, out: &mut Vec<f32>, limit: u64) {
        let ch = self.channels;
        let taps = self.taps;
        while self.out_frames < limit {
            let num = self.out_frames * self.down;
            let i0 = num / self.up;
            if i0 >= self.in_frames {
                break;
            }
            let phase = (num % self.up) as usize;
            let base = i0 as i64;
            let start = self.hist_start as i64;
            let avail = self.in_frames as i64;
            let mut acc = [0.0f32; MAX_CHANNELS];
            for k in 0..taps {
                let weight = self.kernel[phase + k * self.up as usize];
                let idx = base - k as i64;
                if idx >= start && idx < avail {
                    let off = ((idx - start) as usize) * ch;
                    for c in 0..ch {
                        acc[c] += weight * self.hist[off + c];
                    }
                }
            }
            out.extend_from_slice(&acc[..ch]);
            self.out_frames += 1;
        }
        // Drop input frames that no later output can reach.
        let next = (self.out_frames * self.down) / self.up;
        let keep = next.saturating_sub(taps as u64 - 1);
        if keep > self.hist_start {
            let drop_frames = (keep - self.hist_start) as usize;
            let drop = (drop_frames * ch).min(self.hist.len());
            self.hist.drain(..drop);
            self.hist_start += (drop / ch) as u64;
        }
    }
}

/// Taps per branch for an upsampling factor, bounding the kernel length.
fn taps_for(up: u64) -> usize {
    let max = (MAX_KERNEL / up.max(1) as usize).max(8);
    TAPS.min(max)
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// The windowed-sinc kernel, normalized so each phase has unit DC gain on average.
fn kernel(up: u64, down: u64, taps: usize) -> Vec<f32> {
    let up_usize = up as usize;
    let len = taps * up_usize;
    let fc = CUTOFF / up.max(down) as f64;
    let center = (len as f64 - 1.0) / 2.0;
    let mut k = Vec::with_capacity(len);
    for j in 0..len {
        let x = j as f64 - center;
        let u = 2.0 * fc * x;
        let sinc = if x == 0.0 {
            1.0
        } else {
            (std::f64::consts::PI * u).sin() / (std::f64::consts::PI * u)
        };
        let t = j as f64 / (len as f64 - 1.0);
        let w = blackman_harris(t);
        k.push((2.0 * fc * sinc * w) as f32);
    }
    // Normalize by the mean branch sum so DC passes at unity gain.
    let mut sum = 0.0f64;
    for p in 0..up_usize {
        let branch: f64 = (0..taps).map(|i| k[p + i * up_usize] as f64).sum();
        sum += branch;
    }
    let gain = up_usize as f64 / sum;
    for v in &mut k {
        *v = (*v as f64 * gain) as f32;
    }
    k
}

/// Four-term Blackman-Harris window on `t` in 0..=1.
fn blackman_harris(t: f64) -> f64 {
    0.35875 - 0.48829 * (std::f64::consts::TAU * t).cos()
        + 0.14128 * (2.0 * std::f64::consts::TAU * t).cos()
        - 0.01168 * (3.0 * std::f64::consts::TAU * t).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f64, amp: f32, frames: usize, channels: usize) -> Vec<f32> {
        let mut v = Vec::with_capacity(frames * channels);
        for i in 0..frames {
            let x =
                (amp as f64 * (std::f64::consts::TAU * hz * i as f64 / rate as f64).sin()) as f32;
            for _ in 0..channels {
                v.push(x);
            }
        }
        v
    }

    fn rms(x: &[f32]) -> f64 {
        let sum: f64 = x.iter().map(|v| (*v as f64) * (*v as f64)).sum();
        (sum / x.len() as f64).sqrt()
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    /// Frequency from upward zero crossings, ignoring the start of the signal.
    fn measured_hz(x: &[f32], rate: f64) -> f64 {
        let mut first = None;
        let mut last = 0usize;
        let mut count = 0usize;
        for i in 1..x.len() {
            if x[i - 1] <= 0.0 && x[i] > 0.0 {
                if first.is_none() {
                    first = Some(i);
                }
                last = i;
                count += 1;
            }
        }
        let first = first.expect("no zero crossing");
        assert!(count >= 2, "too few crossings");
        (count - 1) as f64 * rate / (last - first) as f64
    }

    /// Deterministic noise, so chunked and single-shot runs compare exactly.
    fn noise(frames: usize, channels: usize, seed: u64) -> Vec<f32> {
        let mut s = seed;
        let mut v = Vec::with_capacity(frames * channels);
        for _ in 0..frames * channels {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            v.push(((s >> 40) as f32 / 8_388_608.0) - 1.0);
        }
        v
    }

    #[test]
    fn identity_rate_passes_samples_unchanged() {
        let input = noise(1000, 2, 7);
        let mut r = Resampler::new(48_000, 48_000, 2);
        assert!(r.is_identity());
        let mut out = Vec::new();
        for chunk in input.chunks(64) {
            r.process(chunk, &mut out);
        }
        r.finish(&mut out);
        assert_eq!(out, input);
    }

    #[test]
    fn output_length_follows_the_rate_ratio() {
        let mut up = Resampler::new(44_100, 48_000, 2);
        let input = sine(44_100, 1000.0, 0.5, 4410, 2);
        let mut out = Vec::new();
        up.process(&input, &mut out);
        up.finish(&mut out);
        let frames = out.len() / 2;
        assert!(
            frames.abs_diff(4800) <= 1,
            "expected 4800 frames, got {frames}"
        );

        let mut down = Resampler::new(48_000, 44_100, 2);
        let input = sine(48_000, 1000.0, 0.5, 4800, 2);
        let mut out = Vec::new();
        down.process(&input, &mut out);
        down.finish(&mut out);
        let frames = out.len() / 2;
        assert!(
            frames.abs_diff(4410) <= 1,
            "expected 4410 frames, got {frames}"
        );
    }

    #[test]
    fn upsampling_preserves_a_midband_sine() {
        let input = sine(44_100, 1000.0, 0.5, 44_100, 2);
        let mut r = Resampler::new(44_100, 48_000, 2);
        let mut out = Vec::new();
        r.process(&input, &mut out);
        r.finish(&mut out);
        let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        let mid = &left[400..left.len() - 400];
        let got = peak(mid);
        assert!((got - 0.5).abs() < 0.005, "peak {got} differs from 0.5");
        let hz = measured_hz(mid, 48_000.0);
        assert!((hz - 1000.0).abs() < 2.0, "frequency {hz}");
        let level = rms(mid);
        assert!(
            (level - 0.35355).abs() < 0.004,
            "RMS {level} differs from 0.35355"
        );
    }

    #[test]
    fn downsampling_preserves_a_midband_sine() {
        let input = sine(48_000, 1000.0, 0.5, 48_000, 2);
        let mut r = Resampler::new(48_000, 44_100, 2);
        let mut out = Vec::new();
        r.process(&input, &mut out);
        r.finish(&mut out);
        let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        let mid = &left[400..left.len() - 400];
        assert!((peak(mid) - 0.5).abs() < 0.005, "peak {}", peak(mid));
        assert!((rms(mid) - 0.35355).abs() < 0.004, "RMS {}", rms(mid));
    }

    #[test]
    fn passband_is_flat_at_ten_kilohertz() {
        let input = sine(96_000, 10_000.0, 0.5, 96_000, 2);
        let mut r = Resampler::new(96_000, 48_000, 2);
        let mut out = Vec::new();
        r.process(&input, &mut out);
        r.finish(&mut out);
        let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        let mid = &left[400..left.len() - 400];
        assert!((rms(mid) - 0.35355).abs() < 0.007, "RMS {}", rms(mid));
    }

    #[test]
    fn downsampling_rejects_above_nyquist_content() {
        // 30 kHz at 96 kHz would alias to 18 kHz at 48 kHz; the filter must stop it.
        let input = sine(96_000, 30_000.0, 0.5, 96_000, 2);
        let mut r = Resampler::new(96_000, 48_000, 2);
        let mut out = Vec::new();
        r.process(&input, &mut out);
        r.finish(&mut out);
        let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        let mid = &left[400..left.len() - 400];
        let level = rms(mid);
        assert!(level < 0.0005, "alias leaks through: RMS {level}");
    }

    #[test]
    fn constant_input_keeps_its_value() {
        let input = vec![0.25f32; 44_100 * 2];
        let mut r = Resampler::new(44_100, 48_000, 2);
        let mut out = Vec::new();
        r.process(&input, &mut out);
        r.finish(&mut out);
        for v in &out[600..] {
            assert!((v - 0.25).abs() < 0.002, "DC became {v}");
        }
    }

    #[test]
    fn chunking_does_not_change_the_result() {
        let input = noise(10_000, 2, 42);
        let mut single = Resampler::new(44_100, 48_000, 2);
        let mut a = Vec::new();
        single.process(&input, &mut a);
        single.finish(&mut a);

        for chunk in [1usize, 7, 333, 1024] {
            let mut r = Resampler::new(44_100, 48_000, 2);
            let mut b = Vec::new();
            let mut i = 0;
            while i < input.len() {
                let n = (chunk * 2).min(input.len() - i);
                r.process(&input[i..i + n], &mut b);
                i += n;
            }
            r.finish(&mut b);
            assert_eq!(a, b, "chunk size {chunk} changed the output");
        }
    }

    #[test]
    fn finish_returns_the_converter_to_its_initial_state() {
        let input = sine(44_100, 440.0, 0.5, 2000, 2);
        let mut r = Resampler::new(44_100, 48_000, 2);
        let mut first = Vec::new();
        r.process(&input, &mut first);
        r.finish(&mut first);

        let mut fresh = Resampler::new(44_100, 48_000, 2);
        let mut second = Vec::new();
        fresh.process(&input, &mut second);
        fresh.finish(&mut second);
        assert_eq!(first, second);

        let mut again = Vec::new();
        r.process(&input, &mut again);
        r.finish(&mut again);
        assert_eq!(first, again);
    }

    #[test]
    fn stereo_channels_stay_separate() {
        let frames = 4410;
        let mut input = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let x = (0.5 * (std::f64::consts::TAU * 1000.0 * i as f64 / 44_100.0).sin()) as f32;
            input.push(x);
            input.push(0.0);
        }
        let mut r = Resampler::new(44_100, 48_000, 2);
        let mut out = Vec::new();
        r.process(&input, &mut out);
        r.finish(&mut out);
        let right: Vec<f32> = out[600..].chunks(2).map(|f| f[1]).collect();
        assert!(
            peak(&right) < 1e-6,
            "right channel leaked: {}",
            peak(&right)
        );
    }

    #[test]
    fn mono_is_converted_like_each_stereo_channel() {
        let mono = sine(44_100, 1000.0, 0.4, 4410, 1);
        let mut m = Resampler::new(44_100, 48_000, 1);
        let mut out = Vec::new();
        m.process(&mono, &mut out);
        m.finish(&mut out);
        assert!(out.len().abs_diff(4800) <= 1);
        let mid = &out[300..out.len() - 300];
        assert!((peak(mid) - 0.4).abs() < 0.005, "peak {}", peak(mid));
    }
}
