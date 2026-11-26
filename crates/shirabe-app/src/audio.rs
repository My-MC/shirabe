//! Audio input for the standalone application.
//!
//! The application measures what an input device records. Streams are opened with the
//! device's own sample rate, channel count and sample format, and blocks of any size are
//! accepted (fixes the fixed 48 kHz / fixed buffer size of the previous standalone wrapper).

use std::sync::atomic::{AtomicBool, Ordering};
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
/// Sample rate of the internal clock used when no device is selected.
const IDLE_SAMPLE_RATE: f32 = 48_000.0;

/// What the application measures.
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

/// Input device names of an audio API.
/// The default device comes first; some APIs (ALSA) do not list it among the devices.
pub fn input_devices(host: &str) -> Vec<String> {
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
}

fn find_input(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    host.input_devices()
        .ok()?
        .find(|d| d.name().map(|n| n == name).unwrap_or(false))
        .or_else(|| {
            host.default_input_device()
                .filter(|d| d.name().map(|n| n == name).unwrap_or(false))
        })
}

/// The default audio API and its default input device, if any.
pub fn default_input() -> Option<Source> {
    let host = cpal::default_host();
    let device = host.default_input_device()?.name().ok()?;
    Some(Source::Input {
        host: host.id().name().to_owned(),
        device,
    })
}

/// A running source. Dropping it stops the audio.
pub struct Running {
    _stream: Option<cpal::Stream>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// Human-readable stream description.
    pub description: String,
    /// The last stream error, if any.
    pub error: Arc<Mutex<Option<String>>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Per-channel scratch buffers, allocated before the stream starts.
struct Scratch {
    left: Vec<f32>,
    right: Vec<f32>,
}

impl Scratch {
    fn new() -> Self {
        Self {
            left: vec![0.0; MAX_BLOCK],
            right: vec![0.0; MAX_BLOCK],
        }
    }

    /// Runs `engine` over `frames` frames of the first one or two channels of the
    /// interleaved `data` (`stride` channels per frame).
    fn process<T>(&mut self, engine: &mut MeterEngine, data: &[T], stride: usize, channels: usize)
    where
        T: Sample,
        f32: FromSample<T>,
    {
        let frames = data.len() / stride.max(1);
        let mut start = 0;
        while start < frames {
            let n = (frames - start).min(MAX_BLOCK);
            for i in 0..n {
                let frame = &data[(start + i) * stride..];
                self.left[i] = f32::from_sample(frame[0]);
                if channels > 1 {
                    self.right[i] = f32::from_sample(frame[1]);
                }
            }
            if channels > 1 {
                let mut slices: [&mut [f32]; 2] = [&mut self.left[..n], &mut self.right[..n]];
                engine.process(&mut slices, false);
            } else {
                let mut slices: [&mut [f32]; 1] = [&mut self.left[..n]];
                engine.process(&mut slices, false);
            }
            start += n;
        }
    }
}

/// Starts measuring `source`. The engine is prepared here, outside the audio thread.
pub fn start(source: &Source, mut engine: MeterEngine) -> Result<Running> {
    let error = Arc::new(Mutex::new(None));
    match source {
        Source::None => {
            engine.prepare(IDLE_SAMPLE_RATE, 2);
            engine.reset();
            let stop = Arc::new(AtomicBool::new(false));
            let thread = {
                let stop = stop.clone();
                std::thread::Builder::new()
                    .name("shirabe-idle".into())
                    .spawn(move || idle_loop(engine, &stop))?
            };
            Ok(Running {
                _stream: None,
                stop,
                thread: Some(thread),
                description: "No input  ·  48.0 kHz (test oscillator only)".to_owned(),
                error,
            })
        }
        Source::Input { host, device } => {
            let h = host_by_name(host)?;
            let dev = find_input(&h, device)
                .ok_or_else(|| anyhow!("input device '{device}' not found"))?;
            let config = dev
                .default_input_config()
                .context("the device reports no input configuration")?;
            let stride = config.channels() as usize;
            let channels = stride.min(2);
            let rate = config.sample_rate().0 as f32;
            engine.prepare(rate, channels);
            engine.reset();
            let stream_config: cpal::StreamConfig = config.config();
            let err = error.clone();
            let on_error = move |e: cpal::StreamError| {
                *err.lock().unwrap() = Some(e.to_string());
            };
            let stream = match config.sample_format() {
                SampleFormat::F32 => {
                    build::<f32>(&dev, &stream_config, engine, stride, channels, on_error)
                }
                SampleFormat::I16 => {
                    build::<i16>(&dev, &stream_config, engine, stride, channels, on_error)
                }
                SampleFormat::I32 => {
                    build::<i32>(&dev, &stream_config, engine, stride, channels, on_error)
                }
                SampleFormat::U16 => {
                    build::<u16>(&dev, &stream_config, engine, stride, channels, on_error)
                }
                SampleFormat::F64 => {
                    build::<f64>(&dev, &stream_config, engine, stride, channels, on_error)
                }
                SampleFormat::I8 => {
                    build::<i8>(&dev, &stream_config, engine, stride, channels, on_error)
                }
                SampleFormat::U8 => {
                    build::<u8>(&dev, &stream_config, engine, stride, channels, on_error)
                }
                f => Err(anyhow!("unsupported sample format {f}")),
            }?;
            stream.play().context("cannot start the input stream")?;
            let used = if stride > 2 {
                format!(" (channels 1-2 of {stride})")
            } else {
                String::new()
            };
            Ok(Running {
                _stream: Some(stream),
                stop: Arc::new(AtomicBool::new(false)),
                thread: None,
                description: format!(
                    "{device}  ·  {:.1} kHz  ·  {} ch{used}  ·  {}",
                    rate / 1000.0,
                    channels,
                    config.sample_format()
                ),
                error,
            })
        }
    }
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut engine: MeterEngine,
    stride: usize,
    channels: usize,
    on_error: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let mut scratch = Scratch::new();
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                realtime(|| scratch.process(&mut engine, data, stride, channels));
            },
            on_error,
            None,
        )
        .context("cannot open the input stream")
}

/// Feeds silence in 10 ms blocks at real-time pace.
fn idle_loop(mut engine: MeterEngine, stop: &AtomicBool) {
    let block = (IDLE_SAMPLE_RATE / 100.0) as usize;
    let mut left = vec![0.0f32; block];
    let mut right = vec![0.0f32; block];
    let period = Duration::from_millis(10);
    let mut next = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        left.fill(0.0);
        right.fill(0.0);
        let mut slices: [&mut [f32]; 2] = [&mut left, &mut right];
        realtime(|| engine.process(&mut slices, false));
        next += period;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            next = now;
        }
    }
}

/// Runs audio-thread code. Debug builds abort if it allocates.
#[inline]
fn realtime<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(debug_assertions)]
    {
        assert_no_alloc::assert_no_alloc(f)
    }
    #[cfg(not(debug_assertions))]
    {
        f()
    }
}
