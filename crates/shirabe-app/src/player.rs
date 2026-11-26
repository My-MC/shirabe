//! Playback of a local audio file: decoding, metering and the output stream.
//!
//! The meters read the decoded samples at the file's own sample rate. Conversion to
//! the output device's rate happens after metering, so no reading depends on the
//! device. The decode thread feeds a bounded lock-free ring that the audio callback
//! drains; the callback neither allocates nor waits.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::Result;
use cpal::traits::StreamTrait;
use shirabe_plugin::engine::MeterEngine;

use crate::audio::{realtime, Pump};
use crate::decode::{Decoder, Track, CHUNK_FRAMES};
use crate::resample::Resampler;
use crate::ring::{self, Consumer, Producer};

/// Output frames buffered between the decode thread and the callback (milliseconds).
pub const BUFFER_MS: u32 = 100;
/// Channels of the output ring; the output stage maps them to the device.
pub const CHANNELS: usize = 2;
/// Wait of the decode thread when the ring is full.
const RETRY: Duration = Duration::from_millis(2);
/// Length of the silence blocks fed while paused or after the end.
const SILENCE_MS: u32 = 10;

/// Decodes, meters and converts one file. Runs on the decode thread.
pub struct Renderer {
    decoder: Decoder,
    track: Track,
    pump: Pump,
    resampler: Option<Resampler>,
    file_channels: usize,
    /// Decoded interleaved frames at the file rate.
    decoded: Vec<f32>,
    /// Interleaved stereo frames at the file rate.
    stereo: Vec<f32>,
    /// Interleaved stereo frames at the device rate.
    converted: Vec<f32>,
    /// Set once the decoder returned the end of the file.
    finished: bool,
}

impl Renderer {
    /// Prepares a renderer for `track`: it measures at the file rate and produces
    /// frames at `device_rate`. Allocates.
    pub fn new(track: Track, decoder: Decoder, engine: MeterEngine, device_rate: u32) -> Self {
        let channels = track.channels.clamp(1, 2);
        let pump = Pump::new(engine, track.rate as f32, channels);
        let resampler =
            (device_rate != track.rate).then(|| Resampler::new(track.rate, device_rate, CHANNELS));
        let file_channels = track.channels;
        let capacity = CHUNK_FRAMES * track.channels.max(1);
        Self {
            decoder,
            track,
            pump,
            resampler,
            file_channels,
            decoded: Vec::with_capacity(capacity),
            stereo: Vec::with_capacity(CHUNK_FRAMES * CHANNELS),
            converted: Vec::with_capacity(CHUNK_FRAMES * CHANNELS * 2),
            finished: false,
        }
    }

    /// The file being played (used by the tests; the window reads [`Playback`]).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn track(&self) -> &Track {
        &self.track
    }

    /// True once the whole file has been produced.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn ended(&self) -> bool {
        self.finished
    }

    /// Produces the next block of interleaved stereo frames at the device rate.
    ///
    /// Returns `None` when the file has been played completely. The slice is valid
    /// until the next call.
    pub fn next(&mut self) -> Result<Option<&[f32]>> {
        if self.finished {
            return Ok(None);
        }
        self.decoded.clear();
        let frames = self.decoder.read(CHUNK_FRAMES, &mut self.decoded)?;
        self.converted.clear();
        if frames == 0 {
            self.finished = true;
            // Flush the resampler tail; without one, the last block was the last audio.
            if let Some(r) = &mut self.resampler {
                r.finish(&mut self.converted);
                if !self.converted.is_empty() {
                    return Ok(Some(&self.converted));
                }
            }
            return Ok(None);
        }

        // 1. Measure the stored samples at the file's own rate.
        let channels = self.file_channels;
        self.pump.interleaved_with(&self.decoded, channels, true);

        // 2. Reduce to stereo, still at the file rate.
        self.make_stereo(frames);

        // 3. Convert to the device rate for the output only.
        match &mut self.resampler {
            Some(r) => r.process(&self.stereo, &mut self.converted),
            None => self.converted.extend_from_slice(&self.stereo),
        }
        Ok(Some(&self.converted))
    }

    /// Feeds silence to the meters while nothing is decoded (paused or ended), so
    /// that the meters fall at the right speed.
    pub fn silence(&mut self, frames: usize) {
        self.pump.silence(frames, false);
    }

    /// Frames of silence that correspond to [`SILENCE_MS`].
    pub fn silence_frames(&self) -> usize {
        (self.track.rate / 1000 * SILENCE_MS).max(1) as usize
    }

    fn make_stereo(&mut self, frames: usize) {
        self.stereo.clear();
        match self.file_channels {
            0 | 1 => {
                for i in 0..frames {
                    let value = self.decoded.get(i).copied().unwrap_or(0.0);
                    self.stereo.extend_from_slice(&[value, value]);
                }
            }
            2 => self.stereo.extend_from_slice(&self.decoded[..frames * 2]),
            channels => {
                for i in 0..frames {
                    let start = i * channels;
                    self.stereo
                        .extend_from_slice(&self.decoded[start..start + CHANNELS]);
                }
            }
        }
    }
}

/// Transport state shared with the user interface and the decode thread.
pub struct Control {
    stop: AtomicBool,
    paused: AtomicBool,
    ended: AtomicBool,
}

impl Control {
    fn new() -> Self {
        Self {
            stop: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            ended: AtomicBool::new(false),
        }
    }

    /// Asks the decode thread to stop.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Pauses or resumes playback.
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    /// Whether playback is paused.
    pub fn paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Whether the whole file has been decoded.
    pub fn ended(&self) -> bool {
        self.ended.load(Ordering::Relaxed)
    }

    fn stopping(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

/// A prepared playback: the ring producer and the renderer, before the stream exists.
pub struct Prepared {
    renderer: Renderer,
    producer: Producer,
    /// Frames consumed by the output callback.
    pub played: Arc<AtomicU64>,
    /// Transport state.
    pub control: Arc<Control>,
    track: Track,
    rate: u32,
}

impl Prepared {
    /// Starts the decode thread and the stream and returns the running playback.
    pub fn start(
        self,
        stream: cpal::Stream,
        error: Arc<Mutex<Option<String>>>,
    ) -> Result<Playback> {
        let Prepared {
            mut renderer,
            mut producer,
            played,
            control,
            track,
            rate,
        } = self;
        let worker = control.clone();
        let err = error.clone();
        let thread = thread::Builder::new()
            .name("shirabe-playback".into())
            .spawn(move || run(&mut renderer, &mut producer, &worker, &err))?;
        if let Err(e) = stream.play() {
            // Stop the decode thread so it cannot wait for a device that never started.
            control.stop();
            let _ = thread.join();
            return Err(anyhow::anyhow!("cannot start the output stream: {e}"));
        }
        Ok(Playback {
            stream,
            thread: Some(thread),
            control,
            played,
            rate,
            track,
        })
    }
}

/// Creates the ring and the renderer. The caller builds the output stream with the
/// returned consumer, then calls [`Prepared::start`].
pub(crate) fn prepare(
    track: Track,
    decoder: Decoder,
    engine: MeterEngine,
    device_rate: u32,
) -> Result<(Prepared, Consumer)> {
    let capacity = (device_rate as usize / 1000 * BUFFER_MS as usize).max(1024);
    let (producer, consumer) = ring::ring(capacity, CHANNELS);
    let played = consumer.played_handle();
    let renderer = Renderer::new(track.clone(), decoder, engine, device_rate);
    let prepared = Prepared {
        renderer,
        producer,
        played,
        control: Arc::new(Control::new()),
        track,
        rate: device_rate,
    };
    Ok((prepared, consumer))
}

/// A running file playback: the output stream and the decode thread.
pub struct Playback {
    /// Kept alive for as long as playback runs; stopping the stream is its drop.
    #[allow(dead_code)]
    stream: cpal::Stream,
    thread: Option<JoinHandle<()>>,
    control: Arc<Control>,
    /// Frames consumed by the output callback.
    played: Arc<AtomicU64>,
    rate: u32,
    track: Track,
}

impl Playback {
    /// Pauses or resumes playback.
    pub fn set_paused(&self, paused: bool) {
        self.control.set_paused(paused);
    }

    /// Whether playback is paused.
    pub fn paused(&self) -> bool {
        self.control.paused()
    }

    /// Whether the end of the file has been decoded.
    pub fn ended(&self) -> bool {
        self.control.ended()
    }

    /// Seconds played so far, as consumed by the output device.
    pub fn position(&self) -> f64 {
        self.played.load(Ordering::Relaxed) as f64 / self.rate as f64
    }

    /// Duration of the file, when known.
    pub fn duration(&self) -> Option<f64> {
        self.track.seconds()
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        // Stop the decode thread first; the stream is dropped after this method, so a
        // full ring cannot leave the thread waiting for space.
        self.control.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The decode loop: measure and convert, then wait for space in the ring.
fn run(
    renderer: &mut Renderer,
    producer: &mut Producer,
    control: &Control,
    error: &Mutex<Option<String>>,
) {
    while !control.stopping() {
        if control.paused() || control.ended() {
            let frames = renderer.silence_frames();
            realtime(|| renderer.silence(frames));
            thread::sleep(Duration::from_millis(SILENCE_MS as u64));
            continue;
        }
        match renderer.next() {
            Ok(Some(block)) => {
                if !push_all(producer, block, control) {
                    break;
                }
            }
            Ok(None) => control.ended.store(true, Ordering::Relaxed),
            Err(e) => {
                if let Ok(mut slot) = error.lock() {
                    *slot = Some(format!("{e:#}"));
                }
                control.ended.store(true, Ordering::Relaxed);
            }
        }
    }
}

/// Pushes a block, waiting while the ring is full. Returns false when stopping.
fn push_all(producer: &mut Producer, mut block: &[f32], control: &Control) -> bool {
    while !block.is_empty() {
        if control.stopping() {
            return false;
        }
        let n = producer.push(block);
        block = &block[n * CHANNELS..];
        if !block.is_empty() {
            thread::sleep(RETRY);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{SampleFormat, WavSpec, WavWriter};
    use shirabe_plugin::shared::SharedMeters;
    use shirabe_plugin::ShirabeParams;
    use std::f64::consts::TAU;
    use std::path::{Path, PathBuf};

    fn path(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    /// Writes a 1 kHz sine WAV with one amplitude per channel.
    fn sine_file(name: &str, rate: u32, amps: &[f32], seconds: f64) -> PathBuf {
        let p = path(name);
        let spec = WavSpec {
            channels: amps.len() as u16,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut w = WavWriter::create(&p, spec).unwrap();
        let frames = (seconds * rate as f64).round() as usize;
        for i in 0..frames {
            let phase = TAU * 1000.0 * i as f64 / rate as f64;
            for a in amps {
                let v = (*a as f64 * phase.sin()) as f32;
                w.write_sample((v * 32767.0).round() as i16).unwrap();
            }
        }
        w.finalize().unwrap();
        p
    }

    fn renderer(file: &Path, device_rate: u32) -> (Renderer, Arc<SharedMeters>) {
        let (track, decoder) = Decoder::open(file).unwrap();
        let params = Arc::new(ShirabeParams::default());
        let shared = Arc::new(SharedMeters::new());
        let engine = MeterEngine::new(params, shared.clone());
        (Renderer::new(track, decoder, engine, device_rate), shared)
    }

    fn render_all(r: &mut Renderer) -> Vec<f32> {
        let mut out = Vec::new();
        while let Some(block) = r.next().unwrap() {
            out.extend_from_slice(block);
        }
        out
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    fn db(x: f32) -> f32 {
        20.0 * x.log10()
    }

    #[test]
    fn meters_the_stored_samples_at_the_file_rate() {
        let file = sine_file("player_44k_stereo.wav", 44_100, &[0.125893, 0.0630957], 1.0);
        let (mut r, shared) = renderer(&file, 48_000);
        let out = render_all(&mut r);
        // The output follows the device rate.
        assert!(out.len() / 2 == 48_000, "{} output frames", out.len() / 2);
        // The meters keep the file rate and the file's own samples.
        assert_eq!(shared.sample_rate(), 44_100.0);
        assert_eq!(shared.active_channels(), 2);
        let left = shared.channel(0).max_db.load(Ordering::Relaxed);
        let right = shared.channel(1).max_db.load(Ordering::Relaxed);
        assert!((left - -18.0).abs() < 0.05, "left peak {left}");
        assert!((right - -24.0).abs() < 0.05, "right peak {right}");
    }

    #[test]
    fn the_device_rate_does_not_move_the_meters() {
        let file = sine_file("player_rates.wav", 44_100, &[0.25, 0.125], 1.0);
        let (mut a, shared_a) = renderer(&file, 44_100);
        let (mut b, shared_b) = renderer(&file, 48_000);
        let _ = render_all(&mut a);
        let _ = render_all(&mut b);
        for channel in 0..2 {
            let x = shared_a.channel(channel).max_db.load(Ordering::Relaxed);
            let y = shared_b.channel(channel).max_db.load(Ordering::Relaxed);
            assert!((x - y).abs() < 1e-3, "channel {channel}: {x} vs {y}");
        }
    }

    #[test]
    fn mono_is_played_on_both_output_channels() {
        let file = sine_file("player_mono.wav", 48_000, &[0.5], 0.5);
        let (mut r, shared) = renderer(&file, 48_000);
        let out = render_all(&mut r);
        assert_eq!(out.len() / 2, 24_000);
        assert_eq!(shared.active_channels(), 1);
        let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        let right: Vec<f32> = out.chunks(2).map(|f| f[1]).collect();
        assert_eq!(left, right);
        assert!((peak(&left) - 0.5).abs() < 0.001, "{}", peak(&left));
    }

    #[test]
    fn only_the_first_two_channels_are_played_and_metered() {
        let file = sine_file("player_quad.wav", 48_000, &[0.5, 0.25, 0.9, 0.1], 0.5);
        let (mut r, shared) = renderer(&file, 48_000);
        let out = render_all(&mut r);
        assert_eq!(out.len() / 2, 24_000);
        assert_eq!(shared.active_channels(), 2);
        let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        let right: Vec<f32> = out.chunks(2).map(|f| f[1]).collect();
        assert!((db(peak(&left)) - -6.02).abs() < 0.1, "{}", db(peak(&left)));
        assert!(
            (db(peak(&right)) - -12.04).abs() < 0.1,
            "{}",
            db(peak(&right))
        );
        let c0 = shared.channel(0).max_db.load(Ordering::Relaxed);
        let c1 = shared.channel(1).max_db.load(Ordering::Relaxed);
        assert!((c0 - -6.02).abs() < 0.1, "{c0}");
        assert!((c1 - -12.04).abs() < 0.1, "{c1}");
    }

    #[test]
    fn the_end_of_the_file_is_reported_once() {
        let file = sine_file("player_end.wav", 44_100, &[0.1, 0.1], 0.25);
        let (mut r, _) = renderer(&file, 48_000);
        let out = render_all(&mut r);
        assert!(out.len() / 2 == 12_000, "{} frames", out.len() / 2);
        assert!(r.ended());
        assert!(r.next().unwrap().is_none());
        assert!(r.next().unwrap().is_none());
    }

    #[test]
    fn silence_makes_the_meters_fall_while_paused() {
        let file = sine_file("player_pause.wav", 48_000, &[0.5, 0.5], 0.5);
        let (mut r, shared) = renderer(&file, 48_000);
        let _ = r.next().unwrap();
        let blocks = shared.blocks.load(Ordering::Relaxed);
        let before = shared.channel(0).level_db.load(Ordering::Relaxed);
        for _ in 0..100 {
            r.silence(r.silence_frames());
        }
        let after = shared.channel(0).level_db.load(Ordering::Relaxed);
        assert!(shared.blocks.load(Ordering::Relaxed) >= blocks + 100);
        assert!(after < before - 6.0, "level {before} -> {after}");
    }

    #[test]
    fn track_information_reaches_the_window() {
        let file = sine_file("player_info.wav", 44_100, &[0.1, 0.1], 0.25);
        let (r, _) = renderer(&file, 48_000);
        let track = r.track();
        assert_eq!(track.rate, 44_100);
        assert_eq!(track.channels, 2);
        assert!((track.seconds().unwrap() - 0.25).abs() < 1e-6);
        assert!(track.frames.is_some());
    }
}
