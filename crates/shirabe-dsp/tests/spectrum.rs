//! Spectrum capture and FFT / 1/3-octave analysis.
//!
//! The display reference is a bin-centred full-scale sine: a sine at 0 dBFS reads 0 dB in its
//! peak bin and in its 1/3-octave band. Tests use deterministic synthetic signals only.

use shirabe_dsp::spectrum::*;

fn sine(frequency: f32, sample_rate: f32, amplitude: f32, seconds: f32) -> Vec<f32> {
    let length = (seconds * sample_rate) as usize;
    (0..length)
        .map(|n| amplitude * (std::f32::consts::TAU * frequency * n as f32 / sample_rate).sin())
        .collect()
}

/// Pushes `samples` in small blocks, like the audio callback would.
fn push_blocks(capture: &SpectrumCapture, samples: &[f32], block: usize) {
    for chunk in samples.chunks(block) {
        capture.push(chunk);
    }
}

/// A deterministic uniform noise source in [-1, 1).
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }
}

fn white_noise(seed: u64, length: usize) -> Vec<f32> {
    let mut rng = Lcg::new(seed);
    (0..length).map(|_| rng.next()).collect()
}

/// Paul Kellet's pink filter (the customary three-pole approximation).
fn pink_noise(seed: u64, length: usize) -> Vec<f32> {
    let mut rng = Lcg::new(seed);
    let (mut b0, mut b1, mut b2, mut b3, mut b4, mut b5, mut b6) =
        (0.0f32, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    (0..length)
        .map(|_| {
            let white = rng.next();
            b0 = 0.99886 * b0 + white * 0.0555179;
            b1 = 0.99332 * b1 + white * 0.0751652;
            b2 = 0.96900 * b2 + white * 0.153_852;
            b3 = 0.86650 * b3 + white * 0.310_485_6;
            b4 = 0.55000 * b4 + white * 0.532_952_2;
            b5 = -0.7616 * b5 - white * 0.0168980;
            let pink = b0 + b1 + b2 + b3 + b4 + b5 + b6 + white * 0.5362;
            b6 = white * 0.115926;
            pink * 0.11
        })
        .collect()
}

/// Runs the analyser over `samples` and returns the last band levels (dB).
fn analyse_bands(
    sample_rate: f32,
    samples: &[f32],
    warmup_seconds: f32,
) -> Option<[f32; BAND_COUNT]> {
    let capture = SpectrumCapture::new();
    let mut analyzer = SpectrumAnalyzer::new(
        sample_rate,
        SpectrumConfig {
            smoothing_seconds: 0.5,
            ..SpectrumConfig::default()
        },
    );
    let hop = FFT_SIZE / 2;
    let mut result = None;
    for (block_index, chunk) in samples.chunks(hop).enumerate() {
        capture.push(chunk);
        if (block_index as f32) * (hop as f32 / sample_rate) >= warmup_seconds
            && analyzer.analyze(&capture)
        {
            result = Some(*analyzer.bands_db());
        }
    }
    result
}

fn band_index(nominal: &str) -> usize {
    NOMINAL_LABELS
        .iter()
        .position(|label| *label == nominal)
        .unwrap_or_else(|| panic!("no band {nominal}"))
}

#[test]
fn bands_follow_the_base_ten_one_third_octave_series() {
    assert_eq!(BAND_COUNT, 31);
    assert_eq!(NOMINAL_LABELS.len(), BAND_COUNT);
    // 1000 Hz is the reference frequency and lies exactly on a band centre.
    assert!((exact_center_hz(17) - 1000.0).abs() < 0.001);
    // Adjacent exact centres stand in the ratio G^(1/3) = 10^(1/10).
    let ratio = 10f32.powf(0.1);
    for index in 0..BAND_COUNT - 1 {
        let expected = exact_center_hz(index) * ratio;
        assert!(
            (exact_center_hz(index + 1) - expected).abs() < 0.01 * expected,
            "band {index}"
        );
    }
    // The usual audio range is covered: 20 Hz to 20 kHz.
    assert!(exact_center_hz(0) > 19.0 && exact_center_hz(0) < 20.0);
    assert!(exact_center_hz(BAND_COUNT - 1) > 19_000.0);
    // The nominal labels and the exact centres stay in step.
    assert_eq!(NOMINAL_LABELS[0], "20");
    assert_eq!(NOMINAL_LABELS[17], "1k");
    assert_eq!(NOMINAL_LABELS[30], "20k");
    // Edges are the geometric midpoints: their product is the centre squared.
    let (low, high) = band_edges_hz(17);
    assert!((low * high - 1_000_000.0).abs() < 1.0);
    assert!(low < 1000.0 && high > 1000.0);
    // Edges meet the neighbouring band's edge.
    let (_, previous_high) = band_edges_hz(16);
    assert!((previous_high - low).abs() < 0.01);
}

#[test]
fn a_bin_centred_sine_reads_its_frequency_and_level_at_several_rates() {
    for sample_rate in [44_100.0, 48_000.0, 96_000.0] {
        for target in [100.0f32, 1_000.0, 10_000.0] {
            let bin = (target * FFT_SIZE as f32 / sample_rate).round().max(1.0);
            let frequency = bin * sample_rate / FFT_SIZE as f32;
            if frequency >= sample_rate / 2.0 {
                continue;
            }
            for level_db in [-6.0, -20.0] {
                let amplitude = 10f32.powf(level_db / 20.0);
                let signal = sine(frequency, sample_rate, amplitude, 0.8);
                let capture = SpectrumCapture::new();
                push_blocks(&capture, &signal, 256);
                let mut analyzer = SpectrumAnalyzer::new(sample_rate, SpectrumConfig::default());
                assert!(
                    analyzer.analyze(&capture),
                    "{sample_rate} Hz {frequency} Hz"
                );
                let (peak_hz, peak_db) = analyzer.peak().expect("a peak");
                let bin_hz = sample_rate / FFT_SIZE as f32;
                assert!(
                    (peak_hz - frequency).abs() <= bin_hz,
                    "{peak_hz} vs {frequency} at {sample_rate}"
                );
                assert!(
                    (peak_db - level_db).abs() < 0.1,
                    "{peak_db} dB vs {level_db} dB at {frequency} Hz, {sample_rate} Hz"
                );
            }
        }
    }
}

#[test]
fn an_off_bin_sine_stays_within_one_bin_and_the_hann_scalloping_loss() {
    let sample_rate = 48_000.0;
    // Half a bin away from 1000 Hz.
    let frequency = 1000.0 + 0.5 * sample_rate / FFT_SIZE as f32;
    let signal = sine(frequency, sample_rate, 1.0, 0.8);
    let capture = SpectrumCapture::new();
    push_blocks(&capture, &signal, 256);
    let mut analyzer = SpectrumAnalyzer::new(sample_rate, SpectrumConfig::default());
    assert!(analyzer.analyze(&capture));
    let (peak_hz, peak_db) = analyzer.peak().expect("a peak");
    let bin_hz = sample_rate / FFT_SIZE as f32;
    assert!((peak_hz - frequency).abs() <= bin_hz);
    // The periodic Hann window loses at most about 1.42 dB between bins.
    assert!((-1.6..=0.1).contains(&peak_db), "{peak_db} dB");
}

#[test]
fn the_window_normalisation_puts_a_full_scale_sine_at_zero_dbfs() {
    let sample_rate = 48_000.0;
    let bin = (1000.0 * FFT_SIZE as f32 / sample_rate).round();
    let frequency = bin * sample_rate / FFT_SIZE as f32;
    for (amplitude, expected_db) in [(1.0, 0.0), (0.5, -6.0206), (0.1, -20.0)] {
        let signal = sine(frequency, sample_rate, amplitude, 0.8);
        let capture = SpectrumCapture::new();
        push_blocks(&capture, &signal, 256);
        let mut analyzer = SpectrumAnalyzer::new(sample_rate, SpectrumConfig::default());
        assert!(analyzer.analyze(&capture));
        let (_, peak_db) = analyzer.peak().expect("a peak");
        assert!(
            (peak_db - expected_db).abs() < 0.05,
            "amplitude {amplitude}: {peak_db} dB"
        );
        assert!((analyzer.window_sum() - FFT_SIZE as f32 / 2.0).abs() < 1.0);
    }
}

#[test]
fn white_noise_rises_by_three_db_per_octave_in_one_third_octave_bands() {
    // A flat power density fills a constant-percentage band in proportion to its width, so the
    // band level rises 10 dB per decade (about 3.01 dB per octave).
    let sample_rate = 48_000.0;
    let signal = white_noise(7, (sample_rate * 12.0) as usize);
    let bands = analyse_bands(sample_rate, &signal, 2.0).expect("an analysis");
    for (low, high) in [("100", "1k"), ("1k", "10k")] {
        let rise = bands[band_index(high)] - bands[band_index(low)];
        assert!((rise - 10.0).abs() < 2.0, "{low} to {high}: {rise} dB");
    }
}

#[test]
fn pink_noise_reads_flat_across_the_bands() {
    let sample_rate = 48_000.0;
    let signal = pink_noise(11, (sample_rate * 12.0) as usize);
    let bands = analyse_bands(sample_rate, &signal, 2.0).expect("an analysis");
    // Average the 100-630 Hz octave and the 4-16 kHz range.
    let low: f32 = ["100", "200", "400", "630"]
        .iter()
        .map(|f| bands[band_index(f)])
        .sum::<f32>()
        / 4.0;
    let high: f32 = ["4k", "8k", "16k"]
        .iter()
        .map(|f| bands[band_index(f)])
        .sum::<f32>()
        / 3.0;
    // 1/f power density fills every constant-percentage band with the same power.
    let slope = high - low;
    assert!(slope.abs() < 1.5, "pink slope {slope} dB");
}

#[test]
fn a_full_scale_sine_reads_zero_db_in_its_band() {
    let sample_rate = 48_000.0;
    // 1 kHz lies exactly on a band centre and close to a bin centre.
    let signal = sine(1000.0, sample_rate, 1.0, 0.8);
    let capture = SpectrumCapture::new();
    push_blocks(&capture, &signal, 256);
    let mut analyzer = SpectrumAnalyzer::new(sample_rate, SpectrumConfig::default());
    assert!(analyzer.analyze(&capture));
    let bands = analyzer.bands_db();
    let level = bands[band_index("1k")];
    println!("peak: {:?}", analyzer.peak());
    assert!((level - 0.0).abs() < 0.3, "1 kHz band: {level} dB");
}

#[test]
fn bands_above_the_nyquist_frequency_read_the_floor() {
    // At 32 kHz the 16 kHz band reaches Nyquist and the 20 kHz band has no bins at all.
    let capture = SpectrumCapture::new();
    push_blocks(&capture, &white_noise(3, 40_000), 512);
    let mut analyzer = SpectrumAnalyzer::new(32_000.0, SpectrumConfig::default());
    assert!(analyzer.analyze(&capture));
    assert_eq!(analyzer.bands_db()[band_index("20k")], -150.0);
    // The 16 kHz band is cut off at Nyquist but still holds the bins below it.
    assert!(analyzer.bands_db()[band_index("16k")].is_finite());
    assert!(analyzer.bands_db()[band_index("10k")] > -60.0);
}

#[test]
fn the_capture_ring_keeps_the_newest_frames_in_order_and_counts_clears() {
    let capture = SpectrumCapture::new();
    assert_eq!(capture.read_latest(&mut [0.0; 8]), 0);
    assert_eq!(capture.epoch(), 0);
    let samples: Vec<f32> = (0..10).map(|i| i as f32).collect();
    capture.push(&samples[..4]);
    capture.push(&samples[4..]);
    assert_eq!(capture.written(), 10);
    let mut out = [0.0f32; 4];
    assert_eq!(capture.read_latest(&mut out), 4);
    assert_eq!(out, [6.0, 7.0, 8.0, 9.0]);
    capture.clear();
    assert_eq!(capture.epoch(), 1);
    assert_eq!(capture.written(), 0);
    assert_eq!(capture.read_latest(&mut out), 0);
}

#[test]
fn a_clear_resets_the_analysis_instead_of_keeping_the_old_window() {
    let sample_rate = 48_000.0;
    let capture = SpectrumCapture::new();
    push_blocks(&capture, &sine(1000.0, sample_rate, 0.5, 0.8), 256);
    let mut analyzer = SpectrumAnalyzer::new(sample_rate, SpectrumConfig::default());
    assert!(analyzer.analyze(&capture));
    assert!(analyzer.is_ready());

    capture.clear();
    assert!(!analyzer.analyze(&capture));
    assert!(!analyzer.is_ready());
    assert_eq!(analyzer.bands_db()[band_index("1k")], -150.0);

    // Fresh silence fills a new window: the display is ready but shows the floor.
    push_blocks(&capture, &vec![0.0; FFT_SIZE + 16], 256);
    assert!(analyzer.analyze(&capture));
    assert!(analyzer.bands_db().iter().all(|&db| db <= -140.0));
}

#[test]
fn the_hop_limits_analyses_to_half_a_window_of_new_audio() {
    let sample_rate = 48_000.0;
    let capture = SpectrumCapture::new();
    let mut analyzer = SpectrumAnalyzer::new(sample_rate, SpectrumConfig::default());
    capture.push(&vec![0.1; FFT_SIZE]);
    assert!(analyzer.analyze(&capture));
    assert_eq!(analyzer.analyzed_frames(), 1);
    // No new audio: no new analysis.
    assert!(!analyzer.analyze(&capture));
    // Less than one hop: still no new analysis.
    capture.push(&vec![0.1; FFT_SIZE / 2 - 1]);
    assert!(!analyzer.analyze(&capture));
    capture.push(&[0.1]);
    assert!(analyzer.analyze(&capture));
    assert_eq!(analyzer.analyzed_frames(), 2);
}

#[test]
fn mono_sum_keeps_a_dual_mono_signal_at_its_level_and_halves_a_single_side() {
    let left = [0.5f32, -0.25, 1.0, 0.0];
    let right = [0.5f32, -0.25, 1.0, 0.0];
    let mut out = [0.0f32; 4];
    assert_eq!(mono_sum(&left, &right, &mut out), 4);
    assert_eq!(out, left);
    let silent = [0.0f32; 4];
    mono_sum(&left, &silent, &mut out);
    assert_eq!(out, [0.25, -0.125, 0.5, 0.0]);
}
