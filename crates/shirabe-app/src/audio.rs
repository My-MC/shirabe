//! Audio sources of the standalone application.
//!
//! The application measures an input device, what the computer is playing, or a local
//! audio file played through an output device:
//!
//! | Platform | Playing audio is captured with |
//! |---|---|
//! | Windows | WASAPI loopback of an output device (cpal) |
//! | Linux | the monitor source of a PulseAudio / PipeWire sink (`crate::pulse`) |
//! | macOS 14.2 and later | a Core Audio process tap of all system audio (`crate::tap`) |
//!
//! File playback is not a capture path: the decoder feeds both the meters (at the file
//! rate) and the output device (`crate::player`).
//!
//! Streams are opened with the device's own sample rate, channel count and sample format,
//! and blocks of any size are accepted. When a source delivers no data (a WASAPI loopback
//! stream is silent while nothing plays), silence is fed instead so the meters fall back.
//! The wait starts when the audio already received has been played out, so a source that
//! delivers in large bursts is not mistaken for a stopped one.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use serde::{Deserialize, Serialize};
use shirabe_plugin::engine::MeterEngine;

/// Largest block passed to the engine at once; longer device blocks are split.
const MAX_BLOCK: usize = 4096;
/// Sample rate used when no device is selected.
const IDLE_SAMPLE_RATE: f32 = 48_000.0;
/// Silence is fed when the audio received from the source has run out for this long.
const STALL: Duration = Duration::from_millis(100);
/// Period of the silence feeder.
const TICK: Duration = Duration::from_millis(10);

/// What the application measures. Variants are stored in the settings file: only append.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    /// No device: silence at 48 kHz, so the test oscillator still works.
    #[default]
    None,
    /// An input device of an audio API.
    Input {
        /// Audio API name as reported by cpal (for example "WASAPI" or "ALSA").
        host: String,
        /// Device name.
        device: String,
    },
    /// What an output device plays (Windows: WASAPI loopback).
    Output {
        /// Audio API name.
        host: String,
        /// Device name.
        device: String,
    },
    /// The monitor of a PulseAudio / PipeWire sink (Linux).
    Monitor {
        /// Sink name (the monitor source is `<sink>.monitor`).
        sink: String,
    },
    /// All audio the system plays (macOS 14.2 and later: Core Audio process tap).
    SystemAudio,
    /// A local WAV or FLAC file played through an output device.
    File {
        /// Path of the file.
        path: String,
    },
}

/// An entry of the source list.
#[derive(Clone, Debug)]
pub struct Choice {
    /// Text shown in the list.
    pub label: String,
    /// The source.
    pub source: Source,
}

/// Names of the audio APIs available on this system.
pub fn hosts() -> Vec<String> {
    cpal::available_hosts()
        .into_iter()
        .map(|id| id.name().to_owned())
        .collect()
}

fn host_by_name(name: &str) -> Result<cpal::Host> {
    let id = cpal::available_hosts()
        .into_iter()
        .find(|id| id.name() == name)
        .ok_or_else(|| anyhow!("audio API '{name}' is not available"))?;
    cpal::host_from_id(id).with_context(|| format!("cannot open audio API '{name}'"))
}

/// Input devices of an audio API. The default device comes first; some APIs (ALSA) do not
/// list it among the devices.
pub fn inputs(host: &str) -> Vec<Choice> {
    let Ok(h) = host_by_name(host) else {
        return Vec::new();
    };
    let mut names: Vec<String> = h
        .input_devices()
        .map(|ds| ds.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    if let Some(default) = h.default_input_device().and_then(|d| d.name().ok()) {
        if !names.contains(&default) {
            names.insert(0, default);
        }
    }
    names
        .into_iter()
        .map(|device| Choice {
            label: device.clone(),
            source: Source::Input {
                host: host.to_owned(),
                device,
            },
        })
        .collect()
}

/// Sources that capture what the computer plays. `host` is the selected audio API; only
/// WASAPI offers loopback through cpal, the other platforms have their own capture path.
pub fn playing(host: &str) -> Vec<Choice> {
    let mut list = Vec::new();
    if host == "WASAPI" {
        if let Ok(h) = host_by_name(host) {
            let default = h.default_output_device().and_then(|d| d.name().ok());
            let mut names: Vec<String> = h
                .output_devices()
                .map(|ds| ds.filter_map(|d| d.name().ok()).collect())
                .unwrap_or_default();
            // Default output first.
            if let Some(d) = &default {
                if let Some(i) = names.iter().position(|n| n == d) {
                    let n = names.remove(i);
                    names.insert(0, n);
                }
            }
            for device in names {
                list.push(Choice {
                    label: format!("{device} (playing)"),
                    source: Source::Output {
                        host: host.to_owned(),
                        device,
                    },
                });
            }
        }
    }
    #[cfg(target_os = "linux")]
    if let Ok(sinks) = crate::pulse::sinks() {
        for s in sinks {
            list.push(Choice {
                label: format!("{} (playing)", s.description),
                source: Source::Monitor { sink: s.name },
            });
        }
    }
    #[cfg(target_os = "macos")]
    if crate::tap::available() {
        list.push(Choice {
            label: "All system audio (playing)".to_owned(),
            source: Source::SystemAudio,
        });
    }
    list
}

/// The default source on first start: the default input, if any.
pub fn default_source() -> Option<Source> {
    let host = cpal::default_host();
    let device = host.default_input_device()?.name().ok()?;
    Some(Source::Input {
        host: host.id().name().to_owned(),
        device,
    })
}

/// Converts device data and runs the engine. Buffers are allocated up front.
pub struct Pump {
    engine: MeterEngine,
    left: Vec<f32>,
    right: Vec<f32>,
    /// Channels measured (1 or 2).
    channels: usize,
    /// Sample rate of the source.
    rate: f32,
    /// Frames measured from the source (silence fed by the application is not counted).
    delivered: u64,
}

impl Pump {
    /// Prepares `engine` for `rate` and `channels` (1 or 2). Allocates; call it outside the
    /// audio callback.
    pub fn new(mut engine: MeterEngine, rate: f32, channels: usize) -> Self {
        let channels = channels.clamp(1, 2);
        engine.prepare(rate, channels, 0);
        engine.reset();
        Self {
            engine,
            left: vec![0.0; MAX_BLOCK],
            right: vec![0.0; MAX_BLOCK],
            channels,
            rate,
            delivered: 0,
        }
    }

    fn run(&mut self, n: usize, playing: bool) {
        // The standalone application has no sidechain input, so the reference stays unavailable.
        if self.channels > 1 {
            let mut slices: [&mut [f32]; 2] = [&mut self.left[..n], &mut self.right[..n]];
            self.engine.process(&mut slices, &mut [], playing);
        } else {
            let mut slices: [&mut [f32]; 1] = [&mut self.left[..n]];
            self.engine.process(&mut slices, &mut [], playing);
        }
    }

    /// Measures interleaved data with `stride` channels per frame (the first one or two are
    /// used).
    pub fn interleaved<T>(&mut self, data: &[T], stride: usize)
    where
        T: Sample,
        f32: FromSample<T>,
    {
        self.interleaved_with(data, stride, false);
    }

    /// Like [`Pump::interleaved`], with the transport state given to the engine (file
    /// playback reports `true` while the file is playing).
    pub fn interleaved_with<T>(&mut self, data: &[T], stride: usize, playing: bool)
    where
        T: Sample,
        f32: FromSample<T>,
    {
        let stride = stride.max(1);
        let second = usize::from(stride > 1);
        let frames = data.len() / stride;
        self.delivered += frames as u64;
        let mut start = 0;
        while start < frames {
            let n = (frames - start).min(MAX_BLOCK);
            for i in 0..n {
                let frame = &data[(start + i) * stride..];
                self.left[i] = f32::from_sample(frame[0]);
                if self.channels > 1 {
                    self.right[i] = f32::from_sample(frame[second]);
                }
            }
            self.run(n, playing);
            start += n;
        }
    }

    /// Measures planar data (one slice per channel, all of the same length).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn planar(&mut self, first: &[f32], second: Option<&[f32]>) {
        self.delivered += first.len() as u64;
        let mut start = 0;
        while start < first.len() {
            let n = (first.len() - start).min(MAX_BLOCK);
            self.left[..n].copy_from_slice(&first[start..start + n]);
            if self.channels > 1 {
                let src = second.unwrap_or(first);
                self.right[..n].copy_from_slice(&src[start..start + n]);
            }
            self.run(n, false);
            start += n;
        }
    }

    /// Measures silence, so that the meters fall while nothing is played.
    pub(crate) fn silence(&mut self, frames: usize, playing: bool) {
        let n = frames.min(MAX_BLOCK);
        self.left[..n].fill(0.0);
        self.right[..n].fill(0.0);
        self.run(n, playing);
    }
}

/// A [`Pump`] shared between the source callback and the silence feeder.
pub struct Feed {
    pump: Mutex<Pump>,
    origin: Instant,
    /// Time, in microseconds since `origin`, at which the audio received so far has been
    /// played out.
    ///
    /// PipeWire under load moves audio in quanta of up to 8192 frames (171 ms at 48 kHz),
    /// and a reader asking for 10 ms gets them as a burst of short reads followed by a long
    /// gap. Counting from the last read fed silence in that gap, which pushed the meters
    /// down until the next burst and looked like a freeze.
    covered: AtomicU64,
}

impl Feed {
    fn new(pump: Pump) -> Arc<Self> {
        Arc::new(Self {
            pump: Mutex::new(pump),
            origin: Instant::now(),
            covered: AtomicU64::new(0),
        })
    }

    /// Runs `f` on the pump from a source callback. Debug builds abort if it allocates.
    pub fn with<R>(&self, f: impl FnOnce(&mut Pump) -> R) -> R {
        let now = self.origin.elapsed();
        let mut pump = self.pump.lock().unwrap_or_else(|e| e.into_inner());
        let before = pump.delivered;
        let result = realtime(|| f(&mut pump));
        let frames = pump.delivered - before;
        let audio = Duration::from_secs_f64(frames as f64 / f64::from(pump.rate.max(1.0)));
        // Audio arriving after the previous data ran out starts a new span at `now`.
        let start = now.max(self.covered_until());
        self.covered
            .store((start + audio).as_micros() as u64, Ordering::Relaxed);
        result
    }

    /// When the audio received so far has been played out, measured from `origin`.
    fn covered_until(&self) -> Duration {
        Duration::from_micros(self.covered.load(Ordering::Relaxed))
    }

    fn stalled(&self) -> bool {
        self.stalled_at(self.origin.elapsed())
    }

    /// Whether the source counts as stalled at `now` (measured from `origin`).
    fn stalled_at(&self, now: Duration) -> bool {
        now.saturating_sub(self.covered_until()) > STALL
    }
}

/// Feeds silence while the source delivers nothing.
fn feeder(feed: Arc<Feed>, stop: Arc<AtomicBool>) -> Result<JoinHandle<()>> {
    Ok(std::thread::Builder::new()
        .name("shirabe-silence".into())
        .spawn(move || {
            let mut next = Instant::now();
            while !stop.load(Ordering::Relaxed) {
                if feed.stalled() {
                    let mut pump = feed.pump.lock().unwrap_or_else(|e| e.into_inner());
                    let frames = (pump.rate * TICK.as_secs_f32()) as usize;
                    realtime(|| pump.silence(frames, false));
                }
                next += TICK;
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                } else {
                    next = now;
                }
            }
        })?)
}

/// A running source. Dropping it stops the audio.
pub struct Running {
    stream: Option<cpal::Stream>,
    /// Platform capture objects that must live as long as the stream.
    capture: Option<Box<dyn std::any::Any>>,
    /// File playback, when the source is a file.
    pub playback: Option<crate::player::Playback>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    /// Human-readable stream description.
    pub description: String,
    /// The last stream error, if any.
    pub error: Arc<Mutex<Option<String>>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        // Stop the capture first, then the threads that may still use the pump.
        self.stream = None;
        self.stop.store(true, Ordering::Relaxed);
        self.capture = None;
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// What a platform capture returns to [`start`].
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub struct Capture {
    /// Keeps the capture alive; dropping it stops the capture.
    pub keep: Box<dyn std::any::Any>,
    /// Thread reading the capture, if any (joined after `stop` is set).
    pub thread: Option<JoinHandle<()>>,
    /// Description shown in the window.
    pub description: String,
}

/// Starts measuring `source`. The engine is prepared here, outside the audio thread.
///
/// `output` is the device used for file playback; when it is `None` the default output
/// of the current audio API is used.
pub fn start(
    source: &Source,
    engine: MeterEngine,
    output: Option<&crate::output::DeviceRef>,
) -> Result<Running> {
    let error = Arc::new(Mutex::new(None));
    let stop = Arc::new(AtomicBool::new(false));
    match source {
        Source::None => {
            let feed = Feed::new(Pump::new(engine, IDLE_SAMPLE_RATE, 2));
            Ok(Running {
                stream: None,
                capture: None,
                playback: None,
                threads: vec![feeder(feed, stop.clone())?],
                stop,
                description: "No input  ·  48.0 kHz (test oscillator only)".to_owned(),
                error,
            })
        }
        Source::Input { host, device } | Source::Output { host, device } => {
            let loopback = matches!(source, Source::Output { .. });
            let h = host_by_name(host)?;
            let dev = if loopback {
                find(h.output_devices()?, h.default_output_device(), device)
            } else {
                find(h.input_devices()?, h.default_input_device(), device)
            }
            .ok_or_else(|| anyhow!("device '{device}' not found"))?;
            // A loopback stream uses the output device's own format.
            let config = if loopback {
                dev.default_output_config()
            } else {
                dev.default_input_config()
            }
            .context("the device reports no configuration")?;
            let stride = config.channels() as usize;
            let rate = config.sample_rate().0 as f32;
            let feed = Feed::new(Pump::new(engine, rate, stride.min(2)));
            let stream_config: cpal::StreamConfig = config.config();
            let err = error.clone();
            let on_error = move |e: cpal::StreamError| {
                *err.lock().unwrap_or_else(|e| e.into_inner()) = Some(e.to_string());
            };
            let f = feed.clone();
            let c = &stream_config;
            let stream = match config.sample_format() {
                SampleFormat::F32 => build::<f32>(&dev, c, f, stride, on_error),
                SampleFormat::I16 => build::<i16>(&dev, c, f, stride, on_error),
                SampleFormat::I32 => build::<i32>(&dev, c, f, stride, on_error),
                SampleFormat::U16 => build::<u16>(&dev, c, f, stride, on_error),
                SampleFormat::F64 => build::<f64>(&dev, c, f, stride, on_error),
                SampleFormat::I8 => build::<i8>(&dev, c, f, stride, on_error),
                SampleFormat::U8 => build::<u8>(&dev, c, f, stride, on_error),
                other => Err(anyhow!("unsupported sample format {other}")),
            }?;
            stream.play().context("cannot start the stream")?;
            let what = if loopback { "Playing on " } else { "" };
            Ok(Running {
                stream: Some(stream),
                capture: None,
                playback: None,
                threads: vec![feeder(feed, stop.clone())?],
                stop,
                description: format!(
                    "{what}{device}  ·  {}  ·  {}",
                    format_rate_channels(rate, stride),
                    config.sample_format()
                ),
                error,
            })
        }
        Source::Monitor { sink } => {
            #[cfg(target_os = "linux")]
            {
                let s = crate::pulse::sink(sink)?;
                let channels = usize::from(s.channels).min(2);
                let feed = Feed::new(Pump::new(engine, s.rate as f32, channels));
                let capture = crate::pulse::start(&s, feed.clone(), stop.clone(), error.clone())?;
                platform_running(capture, feed, stop, error)
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = (sink, engine);
                Err(anyhow!("sink monitors are only available on Linux"))
            }
        }
        Source::File { path } => start_file(path, engine, output, error, stop),
        Source::SystemAudio => {
            #[cfg(target_os = "macos")]
            {
                let tap = crate::tap::Tap::new()?;
                let (rate, channels) = tap.format();
                let feed = Feed::new(Pump::new(engine, rate, channels.min(2)));
                let capture = tap.start(feed.clone())?;
                platform_running(capture, feed, stop, error)
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = engine;
                Err(anyhow!("system audio capture is only available on macOS"))
            }
        }
    }
}

/// Starts playing a file through an output device.
fn start_file(
    path: &str,
    engine: MeterEngine,
    output: Option<&crate::output::DeviceRef>,
    error: Arc<Mutex<Option<String>>>,
    stop: Arc<AtomicBool>,
) -> Result<Running> {
    if path.trim().is_empty() {
        return Err(anyhow!("no audio file chosen"));
    }
    let (track, decoder) = crate::decode::Decoder::open(path)?;
    let file_rate = track.rate;
    let device = match output {
        Some(device) => device.clone(),
        None => {
            let host = cpal::default_host().id().name().to_owned();
            crate::output::default_device(&host).ok_or_else(|| anyhow!("no output device"))?
        }
    };
    let target = crate::output::Target::new(&device, file_rate)?;
    let (prepared, consumer) = crate::player::prepare(track, decoder, engine, target.rate)?;
    let description = format!(
        "{}  ·  {}",
        target.device_name,
        target.description(file_rate)
    );
    let stream = target.start(consumer, error.clone())?;
    let playback = prepared.start(stream, error.clone())?;
    Ok(Running {
        stream: None,
        capture: None,
        playback: Some(playback),
        threads: Vec::new(),
        stop,
        description,
        error,
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn platform_running(
    capture: Capture,
    feed: Arc<Feed>,
    stop: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
) -> Result<Running> {
    let mut threads = vec![feeder(feed, stop.clone())?];
    threads.extend(capture.thread);
    Ok(Running {
        stream: None,
        capture: Some(capture.keep),
        playback: None,
        threads,
        stop,
        description: capture.description,
        error,
    })
}

/// "48.0 kHz  ·  2 ch", noting when only two of more channels are measured.
pub fn format_rate_channels(rate: f32, channels: usize) -> String {
    let used = if channels > 2 {
        format!("2 ch (1-2 of {channels})")
    } else {
        format!("{channels} ch")
    };
    format!("{:.1} kHz  ·  {used}", rate / 1000.0)
}

fn find(
    mut devices: impl Iterator<Item = cpal::Device>,
    default: Option<cpal::Device>,
    name: &str,
) -> Option<cpal::Device> {
    let matches = |d: &cpal::Device| d.name().map(|n| n == name).unwrap_or(false);
    devices.find(matches).or_else(|| default.filter(matches))
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    feed: Arc<Feed>,
    stride: usize,
    on_error: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                feed.with(|pump| pump.interleaved(data, stride));
            },
            on_error,
            None,
        )
        .context("cannot open the stream")
}

/// Runs audio-thread code. Debug builds abort if it allocates.
#[inline]
pub(crate) fn realtime<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(debug_assertions)]
    {
        assert_no_alloc::assert_no_alloc(f)
    }
    #[cfg(not(debug_assertions))]
    {
        f()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shirabe_plugin::params::ShirabeParams;
    use shirabe_plugin::shared::SharedMeters;

    fn feed(rate: f32) -> Arc<Feed> {
        let params = Arc::new(ShirabeParams::default());
        let shared = Arc::new(SharedMeters::new());
        Feed::new(Pump::new(MeterEngine::new(params, shared), rate, 2))
    }

    fn deliver(feed: &Feed, frames: usize) {
        let block = vec![0.1f32; frames * 2];
        feed.with(|pump| pump.interleaved(&block, 2));
    }

    #[test]
    fn a_burst_of_short_reads_is_not_taken_for_a_stalled_source() {
        // An 8192-frame PipeWire quantum (171 ms at 48 kHz) read in 10 ms pieces.
        let feed = feed(48_000.0);
        let start = feed.origin.elapsed();
        for _ in 0..17 {
            deliver(&feed, 480);
        }
        // The next burst is due ~171 ms later; silence before then would push the meters
        // down between two bursts of real audio.
        assert!(!feed.stalled_at(start + Duration::from_millis(150)));
        assert!(!feed.stalled_at(start + Duration::from_millis(250)));
        // Once the received audio has run out for longer than the limit, it has stopped.
        assert!(feed.stalled_at(start + Duration::from_millis(400)));
    }

    #[test]
    fn one_large_block_counts_for_its_whole_length() {
        let feed = feed(48_000.0);
        let start = feed.origin.elapsed();
        deliver(&feed, 8192);
        assert!(!feed.stalled_at(start + Duration::from_millis(250)));
        assert!(feed.stalled_at(start + Duration::from_millis(400)));
    }

    #[test]
    fn a_source_with_small_blocks_keeps_the_short_stall_limit() {
        let feed = feed(48_000.0);
        let start = feed.origin.elapsed();
        deliver(&feed, 480);
        assert!(!feed.stalled_at(start + Duration::from_millis(100)));
        assert!(feed.stalled_at(start + Duration::from_millis(130)));
    }

    #[test]
    fn audio_after_a_pause_starts_a_new_span() {
        // Data that arrives after the previous audio ran out must not be credited to the
        // past, or a real stop would be noticed late.
        let feed = feed(48_000.0);
        deliver(&feed, 480);
        std::thread::sleep(Duration::from_millis(30));
        let resumed = feed.origin.elapsed();
        deliver(&feed, 480);
        assert!(feed.covered_until() >= resumed + Duration::from_millis(10));
        assert!(feed.stalled_at(resumed + Duration::from_millis(130)));
    }
}
