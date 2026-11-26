//! Output device selection and the playback stream.
//!
//! A file is played at its own sample rate when the device supports it. Otherwise the
//! device's default rate is used and the file is converted on the decode thread; the
//! meters still read the file samples (see [`crate::player::Renderer`]).
//!
//! The callback drains the playback ring and converts to the device's sample format.
//! It does not allocate, lock or wait; an underrun is filled with silence.

use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use serde::{Deserialize, Serialize};

use crate::audio::realtime;
use crate::player::CHANNELS;
use crate::ring::Consumer;

/// Frames converted per ring pop inside the callback; buffers are preallocated.
const SCRATCH_FRAMES: usize = 4096;

/// An output device chosen by the user. Stored in the settings file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRef {
    /// Audio API name as reported by cpal.
    pub host: String,
    /// Device name.
    pub device: String,
}

impl DeviceRef {
    /// Text for the device list.
    pub fn label(&self) -> &str {
        &self.device
    }
}

/// Output devices of an audio API, the default device first.
pub fn outputs(host: &str) -> Vec<DeviceRef> {
    let Ok(h) = host_by_name(host) else {
        return Vec::new();
    };
    let default = h.default_output_device().and_then(|d| d.name().ok());
    let mut names: Vec<String> = h
        .output_devices()
        .map(|ds| ds.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    if let Some(name) = &default {
        if let Some(index) = names.iter().position(|n| n == name) {
            let name = names.remove(index);
            names.insert(0, name);
        } else {
            names.insert(0, name.clone());
        }
    }
    names
        .into_iter()
        .map(|device| DeviceRef {
            host: host.to_owned(),
            device,
        })
        .collect()
}

/// The default output device of an audio API, if there is one.
pub fn default_device(host: &str) -> Option<DeviceRef> {
    let h = host_by_name(host).ok()?;
    let device = h.default_output_device()?.name().ok()?;
    Some(DeviceRef {
        host: host.to_owned(),
        device,
    })
}

fn host_by_name(name: &str) -> Result<cpal::Host> {
    let id = cpal::available_hosts()
        .into_iter()
        .find(|id| id.name() == name)
        .ok_or_else(|| anyhow!("audio API '{name}' is not available"))?;
    cpal::host_from_id(id).with_context(|| format!("cannot open audio API '{name}'"))
}

/// One supported output configuration, reduced to what the choice needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Range {
    channels: usize,
    min_rate: u32,
    max_rate: u32,
    format: SampleFormat,
}

/// Chooses channels, rate and sample format for a file.
///
/// Two channels at the file rate are preferred, so nothing has to be converted. If no
/// such configuration exists, the device default is used and the file is converted.
fn pick(ranges: &[Range], default: Range, file_rate: u32) -> Range {
    let best = ranges
        .iter()
        .filter(|r| r.channels == CHANNELS && r.min_rate <= file_rate && file_rate <= r.max_rate)
        .min_by_key(|r| format_rank(r.format));
    match best {
        Some(range) => Range {
            channels: CHANNELS,
            min_rate: file_rate,
            max_rate: file_rate,
            format: range.format,
        },
        None => default,
    }
}

/// Buffer size of the output stream.
///
/// ALSA's default let cpal pick a buffer that measured hundreds of milliseconds on a
/// loopback device, which puts the meters far ahead of the sound; about 50 ms keeps them
/// together and leaves room for a slow decode block. Other platforms keep the device
/// default (10-30 ms, and a fixed size has to match the host's period rules).
fn buffer_size(rate: u32) -> cpal::BufferSize {
    #[cfg(target_os = "linux")]
    {
        cpal::BufferSize::Fixed(alsa_frames(rate))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = rate;
        cpal::BufferSize::Default
    }
}

/// Frames in the fixed ALSA buffer, about 50 ms rounded to whole periods.
///
/// cpal opens `Fixed(v)` as a period of `v / 4` and a buffer of exactly `v`, and ALSA
/// rejects a buffer that is not a multiple of the period (`EINVAL`). A plain 50 ms
/// size is not divisible by four at every rate (44.1 kHz: 2205, 88.2 kHz: 4410), so
/// it is rounded down to a multiple of four: 2204 and 4408.
#[cfg(target_os = "linux")]
fn alsa_frames(rate: u32) -> u32 {
    let frames = (rate / 20).max(256);
    frames - frames % 4
}

/// Lower is preferred when the device offers several sample formats.
fn format_rank(format: SampleFormat) -> u8 {
    match format {
        SampleFormat::F32 => 0,
        SampleFormat::I16 => 1,
        SampleFormat::I32 => 2,
        SampleFormat::F64 => 3,
        SampleFormat::U16 => 4,
        SampleFormat::I8 => 5,
        SampleFormat::U8 => 6,
        _ => 7,
    }
}

/// An opened output device, ready for a stream.
pub struct Target {
    device: cpal::Device,
    config: cpal::StreamConfig,
    format: SampleFormat,
    /// Device rate in Hz.
    pub rate: u32,
    /// Device channels.
    pub channels: usize,
    /// True when the file rate differs from the device rate.
    pub converted: bool,
    /// Device name.
    pub device_name: String,
}

impl Target {
    /// Opens the device and chooses a configuration for a file at `file_rate`.
    pub fn new(device: &DeviceRef, file_rate: u32) -> Result<Self> {
        let h = host_by_name(&device.host)?;
        let dev = find(
            h.output_devices()?,
            h.default_output_device(),
            &device.device,
        )
        .ok_or_else(|| anyhow!("output device '{}' not found", device.device))?;
        let default_config = dev
            .default_output_config()
            .context("the output device reports no configuration")?;
        let ranges: Vec<Range> = dev
            .supported_output_configs()
            .map(|it| {
                it.map(|c| Range {
                    channels: c.channels() as usize,
                    min_rate: c.min_sample_rate().0,
                    max_rate: c.max_sample_rate().0,
                    format: c.sample_format(),
                })
                .collect()
            })
            .unwrap_or_default();
        let default = Range {
            channels: default_config.channels() as usize,
            min_rate: default_config.sample_rate().0,
            max_rate: default_config.sample_rate().0,
            format: default_config.sample_format(),
        };
        let chosen = pick(&ranges, default, file_rate);
        let sample_rate = cpal::SampleRate(chosen.min_rate);
        Ok(Self {
            device: dev,
            config: cpal::StreamConfig {
                channels: chosen.channels as u16,
                sample_rate,
                buffer_size: buffer_size(chosen.min_rate),
            },
            format: chosen.format,
            rate: chosen.min_rate,
            channels: chosen.channels,
            converted: chosen.min_rate != file_rate,
            device_name: device.device.clone(),
        })
    }

    /// What the window shows: rate, channels and whether the file was converted.
    pub fn description(&self, file_rate: u32) -> String {
        let mut text = format!(
            "{:.1} kHz  ·  {} ch",
            self.rate as f32 / 1000.0,
            self.channels
        );
        if self.converted {
            text.push_str(&format!(
                "  ·  file {:.1} kHz converted",
                file_rate as f32 / 1000.0
            ));
        }
        text
    }

    /// Builds and starts the stream that drains `consumer`.
    pub fn start(
        self,
        consumer: Consumer,
        error: Arc<Mutex<Option<String>>>,
    ) -> Result<cpal::Stream> {
        let config = &self.config;
        let channels = self.channels;
        let on_error = move |e: cpal::StreamError| {
            if let Ok(mut slot) = error.lock() {
                *slot = Some(e.to_string());
            }
        };
        let stream = match self.format {
            SampleFormat::F32 => build::<f32>(&self.device, config, channels, consumer, on_error),
            SampleFormat::I16 => build::<i16>(&self.device, config, channels, consumer, on_error),
            SampleFormat::I32 => build::<i32>(&self.device, config, channels, consumer, on_error),
            SampleFormat::U16 => build::<u16>(&self.device, config, channels, consumer, on_error),
            SampleFormat::F64 => build::<f64>(&self.device, config, channels, consumer, on_error),
            SampleFormat::I8 => build::<i8>(&self.device, config, channels, consumer, on_error),
            SampleFormat::U8 => build::<u8>(&self.device, config, channels, consumer, on_error),
            other => Err(anyhow!("unsupported output sample format {other}")),
        }?;
        Ok(stream)
    }
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
    channels: usize,
    mut consumer: Consumer,
    on_error: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let mut scratch = vec![0.0f32; SCRATCH_FRAMES * CHANNELS];
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let frames = data.len() / channels;
                // Debug builds abort if the callback allocates (see `realtime`).
                let mut done = 0;
                realtime(|| {
                    while done < frames {
                        let n = (frames - done).min(SCRATCH_FRAMES);
                        let got = consumer.pop(&mut scratch[..n * CHANNELS]);
                        let block = &mut data[done * channels..(done + n) * channels];
                        for i in 0..n {
                            let (left, right) = if i < got {
                                (scratch[i * 2], scratch[i * 2 + 1])
                            } else {
                                (0.0, 0.0)
                            };
                            match channels {
                                1 => block[i] = T::from_sample((left + right) * 0.5),
                                2 => {
                                    block[i * 2] = T::from_sample(left);
                                    block[i * 2 + 1] = T::from_sample(right);
                                }
                                n => {
                                    block[i * n] = T::from_sample(left);
                                    block[i * n + 1] = T::from_sample(right);
                                    for c in 2..n {
                                        block[i * n + c] = T::from_sample(0.0);
                                    }
                                }
                            }
                        }
                        done += n;
                    }
                })
            },
            on_error,
            None,
        )
        .context("cannot open the output stream")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(channels: usize, min_rate: u32, max_rate: u32, format: SampleFormat) -> Range {
        Range {
            channels,
            min_rate,
            max_rate,
            format,
        }
    }

    fn default() -> Range {
        range(2, 48_000, 48_000, SampleFormat::F32)
    }

    #[test]
    fn prefers_two_channels_at_the_file_rate() {
        let ranges = [
            range(2, 44_100, 44_100, SampleFormat::I16),
            range(2, 48_000, 48_000, SampleFormat::F32),
        ];
        let chosen = pick(&ranges, default(), 44_100);
        assert_eq!(chosen.min_rate, 44_100);
        assert_eq!(chosen.channels, 2);
        assert_eq!(chosen.format, SampleFormat::I16);
    }

    #[test]
    fn prefers_float_when_the_rate_is_offered_twice() {
        let ranges = [
            range(2, 44_100, 96_000, SampleFormat::I16),
            range(2, 44_100, 48_000, SampleFormat::F32),
        ];
        assert_eq!(pick(&ranges, default(), 44_100).format, SampleFormat::F32);
    }

    #[test]
    fn falls_back_to_the_device_default() {
        let ranges = [range(2, 48_000, 48_000, SampleFormat::F32)];
        let chosen = pick(&ranges, default(), 44_100);
        assert_eq!(chosen.min_rate, 48_000);
        assert_eq!(chosen.format, SampleFormat::F32);
    }

    #[test]
    fn mono_devices_are_used_as_they_are() {
        let ranges = [range(1, 8_000, 96_000, SampleFormat::I16)];
        let chosen = pick(&ranges, range(1, 48_000, 48_000, SampleFormat::I16), 44_100);
        assert_eq!(chosen.channels, 1);
        assert_eq!(chosen.min_rate, 48_000);
    }

    #[test]
    fn preferred_formats_are_ordered() {
        assert!(format_rank(SampleFormat::F32) < format_rank(SampleFormat::I16));
        assert!(format_rank(SampleFormat::I16) < format_rank(SampleFormat::I32));
        assert!(format_rank(SampleFormat::I32) < format_rank(SampleFormat::U8));
    }

    /// cpal 0.15 opens `Fixed(v)` as a period of `v / 4` and a buffer of exactly `v`,
    /// and ALSA rejects a buffer that is not a multiple of the period with `EINVAL`.
    /// 44.1 kHz / 20 = 2205 would give a period of 551 (x4 = 2204).
    #[cfg(target_os = "linux")]
    #[test]
    fn alsa_buffer_is_a_whole_number_of_periods() {
        for (rate, frames) in [
            (8_000, 400),
            (22_050, 1_100),
            (32_000, 1_600),
            (44_100, 2_204),
            (48_000, 2_400),
            (88_200, 4_408),
            (96_000, 4_800),
            (192_000, 9_600),
        ] {
            let cpal::BufferSize::Fixed(size) = buffer_size(rate) else {
                panic!("{rate} Hz does not use a fixed buffer");
            };
            assert_eq!(size, frames, "{rate} Hz");
            assert_eq!(size % 4, 0, "{rate} Hz is not a whole number of periods");
            let ms = size as f64 * 1000.0 / rate as f64;
            assert!((ms - 50.0).abs() < 1.0, "{rate} Hz buffer is {ms:.2} ms");
        }
    }
}
