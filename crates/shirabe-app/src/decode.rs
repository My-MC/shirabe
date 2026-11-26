//! Decoding of local audio files for playback measurement.
//!
//! Phase 1 supports WAV (PCM 8/16/24/32 bit, IEEE float 32 bit) and FLAC (8 to 32
//! bit, lossless). The format is recognised from the file header, not the file name.
//! Samples are returned as interleaved `f32` in the file's own sample rate; the
//! meters read them unchanged.
//!
//! Other formats (MP3, AAC, Ogg, ...) are not decoded in phase 1. They are reported
//! as unsupported instead of being guessed.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// Largest single read, in frames.
pub const CHUNK_FRAMES: usize = 1024;

/// Properties of an opened file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    /// Path of the file.
    pub path: PathBuf,
    /// Sample rate in Hz.
    pub rate: u32,
    /// Channels stored in the file.
    pub channels: usize,
    /// Frames in the file, when known.
    pub frames: Option<u64>,
}

impl Track {
    /// Duration in seconds, when the length is known.
    pub fn seconds(&self) -> Option<f64> {
        Some(self.frames? as f64 / self.rate as f64)
    }
}

/// Formats seconds as `m:ss`.
pub fn format_time(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// An open file, ready to be read.
pub struct Decoder {
    inner: Inner,
}

enum Inner {
    Wav(WavDecoder),
    Flac(FlacDecoder),
}

struct WavDecoder {
    reader: hound::WavReader<BufReader<File>>,
    /// 1 / full scale of the stored format.
    scale: f32,
    float: bool,
}

struct FlacDecoder {
    reader: claxon::FlacReader<File>,
    scale: f32,
    /// Frames still to be delivered, when the length is known.
    left: Option<u64>,
    /// The block being consumed, with the next frame index in `pos`.
    block: Option<claxon::Block>,
    pos: usize,
}

impl Decoder {
    /// Opens a WAV or FLAC file and reads the properties of the stream.
    pub fn open(path: impl AsRef<Path>) -> Result<(Track, Decoder)> {
        let path = path.as_ref().to_path_buf();
        let kind = sniff(&path)?;
        let (track, inner) = match kind {
            Kind::Wav => WavDecoder::open(&path)?,
            Kind::Flac => FlacDecoder::open(&path)?,
        };
        Ok((track, Decoder { inner }))
    }

    /// Appends up to `max_frames` frames of interleaved samples to `out` and returns
    /// the number of frames read. Returns 0 at the end of the file.
    pub fn read(&mut self, max_frames: usize, out: &mut Vec<f32>) -> Result<usize> {
        let max_frames = max_frames.max(1);
        match &mut self.inner {
            Inner::Wav(w) => w.read(max_frames, out),
            Inner::Flac(f) => f.read(max_frames, out),
        }
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Wav,
    Flac,
}

/// Recognises the container from its first bytes.
fn sniff(path: &Path) -> Result<Kind> {
    let mut file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut head = [0u8; 12];
    let n = file
        .read(&mut head)
        .with_context(|| format!("cannot read {}", path.display()))?;
    if n >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WAVE" {
        return Ok(Kind::Wav);
    }
    if n >= 4 && &head[0..4] == b"fLaC" {
        return Ok(Kind::Flac);
    }
    if n >= 4 && &head[0..4] == b"RF64" {
        bail!("RF64 (over 4 GB WAV) is not supported yet");
    }
    bail!(
        "unsupported audio file (only WAV and FLAC are supported): {}",
        path.display()
    )
}

impl WavDecoder {
    fn open(path: &Path) -> Result<(Track, Inner)> {
        let reader = hound::WavReader::open(path)
            .with_context(|| format!("cannot read WAV file {}", path.display()))?;
        let spec = reader.spec();
        if spec.channels == 0 || spec.sample_rate == 0 {
            bail!("the WAV file has no channels or no sample rate");
        }
        let float = spec.sample_format == hound::SampleFormat::Float;
        let bits = match (float, spec.bits_per_sample) {
            (true, 32) => 32,
            (false, 8 | 16 | 24 | 32) => spec.bits_per_sample,
            _ => bail!(
                "unsupported WAV sample format ({:?}, {} bit)",
                spec.sample_format,
                spec.bits_per_sample
            ),
        };
        let frames = reader.duration() as u64;
        let track = Track {
            path: path.to_path_buf(),
            rate: spec.sample_rate,
            channels: spec.channels as usize,
            frames: Some(frames),
        };
        let inner = WavDecoder {
            reader,
            scale: 1.0 / (1u64 << (bits - 1)) as f32,
            float,
        };
        Ok((track, Inner::Wav(inner)))
    }

    fn read(&mut self, max_frames: usize, out: &mut Vec<f32>) -> Result<usize> {
        let channels = self.reader.spec().channels as usize;
        let start = out.len();
        let want = max_frames * channels;
        if self.float {
            let mut samples = self.reader.samples::<f32>();
            for _ in 0..want {
                match samples.next() {
                    Some(value) => out.push(value.context("cannot read WAV samples")?),
                    None => break,
                }
            }
        } else {
            let scale = self.scale;
            let mut samples = self.reader.samples::<i32>();
            for _ in 0..want {
                match samples.next() {
                    Some(value) => {
                        out.push(value.context("cannot read WAV samples")? as f32 * scale)
                    }
                    None => break,
                }
            }
        }
        // An incomplete last frame is not played.
        let extra = (out.len() - start) % channels;
        out.truncate(out.len() - extra);
        Ok((out.len() - start) / channels)
    }
}

impl FlacDecoder {
    fn open(path: &Path) -> Result<(Track, Inner)> {
        let reader = claxon::FlacReader::open(path)
            .with_context(|| format!("cannot read FLAC file {}", path.display()))?;
        let info = reader.streaminfo();
        if info.channels == 0 || info.sample_rate == 0 {
            bail!("the FLAC file has no channels or no sample rate");
        }
        if !(8..=32).contains(&info.bits_per_sample) {
            bail!("unsupported FLAC bit depth {}", info.bits_per_sample);
        }
        let track = Track {
            path: path.to_path_buf(),
            rate: info.sample_rate,
            channels: info.channels as usize,
            frames: info.samples,
        };
        let inner = FlacDecoder {
            reader,
            scale: 1.0 / (1u64 << (info.bits_per_sample - 1)) as f32,
            left: info.samples,
            block: None,
            pos: 0,
        };
        Ok((track, Inner::Flac(inner)))
    }

    fn read(&mut self, max_frames: usize, out: &mut Vec<f32>) -> Result<usize> {
        let channels = self.reader.streaminfo().channels as usize;
        let mut frames = 0;
        // Blocks hold a few thousand frames; keep the unconsumed part between calls
        // so that a block is never decoded twice or dropped half-read.
        while frames < max_frames {
            if self.block.is_none() {
                let buffer = self
                    .block
                    .take()
                    .map(|b| b.into_buffer())
                    .unwrap_or_default();
                let mut blocks = self.reader.blocks();
                match blocks
                    .read_next_or_eof(buffer)
                    .context("cannot decode FLAC samples")?
                {
                    Some(block) => {
                        self.block = Some(block);
                        self.pos = 0;
                    }
                    None => break,
                }
            }
            let block = self.block.as_ref().expect("block was just decoded");
            let duration = block.duration() as usize;
            let available = duration.saturating_sub(self.pos);
            if available == 0 {
                self.block = None;
                continue;
            }
            let n = available.min(max_frames - frames);
            for i in 0..n {
                for channel in 0..channels {
                    let value = block.channel(channel as u32)[self.pos + i];
                    out.push(value as f32 * self.scale);
                }
            }
            self.pos += n;
            frames += n;
            if self.pos >= duration {
                self.block = None;
            }
        }
        if let Some(left) = &mut self.left {
            *left = left.saturating_sub(frames as u64);
        }
        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn temp_path(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
        fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn write_wav(name: &str, spec: hound::WavSpec, samples: &[i32]) -> PathBuf {
        let path = temp_path(name);
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for s in samples {
            match spec.sample_format {
                hound::SampleFormat::Int => writer.write_sample(*s).unwrap(),
                hound::SampleFormat::Float => writer.write_sample(*s as f32 / 32768.0).unwrap(),
            }
        }
        writer.finalize().unwrap();
        path
    }

    fn read_all(path: &Path) -> (Track, Vec<f32>, usize) {
        let (track, mut decoder) = Decoder::open(path).unwrap();
        let mut out = Vec::new();
        let mut chunk = Vec::new();
        loop {
            chunk.clear();
            let n = decoder.read(CHUNK_FRAMES, &mut chunk).unwrap();
            if n == 0 {
                break;
            }
            assert!(n <= CHUNK_FRAMES);
            out.extend_from_slice(&chunk);
        }
        let frames = out.len() / track.channels;
        (track, out, frames)
    }

    #[test]
    fn reads_16_bit_stereo_wav_values_exactly() {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let path = write_wav(
            "decode_16bit.wav",
            spec,
            &[0, 16384, -16384, 32767, -32768, 0],
        );
        let (track, samples, frames) = read_all(&path);
        assert_eq!(track.rate, 48_000);
        assert_eq!(track.channels, 2);
        assert_eq!(frames, 3);
        assert_eq!(track.frames, Some(3));
        assert_eq!(samples[0], 0.0);
        assert_eq!(samples[1], 0.5);
        assert_eq!(samples[2], -0.5);
        assert!((samples[3] - 32767.0 / 32768.0).abs() < 1e-7);
        assert_eq!(samples[4], -1.0);
    }

    #[test]
    fn reads_24_bit_wav_at_full_scale() {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 96_000,
            bits_per_sample: 24,
            sample_format: hound::SampleFormat::Int,
        };
        let path = write_wav(
            "decode_24bit.wav",
            spec,
            &[0x40_0000, -0x40_0000, 0x7F_FFFF],
        );
        let (track, samples, _) = read_all(&path);
        assert_eq!(track.channels, 1);
        assert!((samples[0] - 0.5).abs() < 1e-6, "{}", samples[0]);
        assert!((samples[1] + 0.5).abs() < 1e-6, "{}", samples[1]);
        assert!((samples[2] - 0x7F_FFFF as f32 / 8_388_608.0).abs() < 1e-6);
    }

    #[test]
    fn reads_8_bit_wav_as_signed_values() {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 8,
            sample_format: hound::SampleFormat::Int,
        };
        let path = write_wav("decode_8bit.wav", spec, &[0, 64, -64, 127]);
        let (_, samples, _) = read_all(&path);
        assert_eq!(samples.len(), 4);
        assert!(samples[0].abs() < 1e-6);
        assert!((samples[1] - 0.5).abs() < 1e-6);
        assert!((samples[2] + 0.5).abs() < 1e-6);
        assert!((samples[3] - 127.0 / 128.0).abs() < 1e-6);
    }

    #[test]
    fn reads_float_wav_without_scaling() {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let path = write_wav("decode_float.wav", spec, &[0, 8192, -8192, 16384]);
        let (_, samples, frames) = read_all(&path);
        assert_eq!(frames, 2);
        assert_eq!(samples, vec![0.0, 0.25, -0.25, 0.5]);
    }

    #[test]
    fn reads_a_flac_file_losslessly() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/stereo_1k_16bit_44100.flac");
        let (track, samples, frames) = read_all(&path);
        assert_eq!(track.rate, 44_100);
        assert_eq!(track.channels, 2);
        assert_eq!(frames, 11_025);
        assert_eq!(track.frames, Some(11_025));
        assert!((track.seconds().unwrap() - 0.25).abs() < 1e-9);

        let left: Vec<f32> = samples.chunks(2).map(|f| f[0]).collect();
        let right: Vec<f32> = samples.chunks(2).map(|f| f[1]).collect();
        let peak = |v: &[f32]| v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!((peak(&left) - 0.125893).abs() < 0.002, "{}", peak(&left));
        assert!((peak(&right) - 0.0630957).abs() < 0.002, "{}", peak(&right));
        let rms = |v: &[f32]| {
            (v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
        };
        assert!((rms(&left) - 0.125893 / 2f64.sqrt()).abs() < 0.002);
        assert!((rms(&right) - 0.0630957 / 2f64.sqrt()).abs() < 0.002);
    }

    #[test]
    fn read_respects_the_requested_frame_count() {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let samples: Vec<i32> = (0..100).flat_map(|i| [i, -i]).collect();
        let path = write_wav("decode_chunks.wav", spec, &samples);
        let (_, mut decoder) = Decoder::open(&path).unwrap();
        let mut out = Vec::new();
        assert_eq!(decoder.read(64, &mut out).unwrap(), 64);
        assert_eq!(out.len(), 128);
        out.clear();
        assert_eq!(decoder.read(1024, &mut out).unwrap(), 36);
        assert_eq!(out.len(), 72);
        out.clear();
        assert_eq!(decoder.read(1024, &mut out).unwrap(), 0);
    }

    #[test]
    fn rejects_files_that_are_not_wav_or_flac() {
        let path = temp_path("decode_unknown.mp3");
        let mut f = File::create(&path).unwrap();
        f.write_all(b"ID3\x04\x00\x00\x00\x00\x00\x00").unwrap();
        drop(f);
        let err = Decoder::open(&path).err().unwrap().to_string();
        assert!(err.contains("only WAV and FLAC"), "{err}");
    }

    #[test]
    fn reports_a_missing_file() {
        let path = temp_path("decode_missing.wav");
        let _ = fs::remove_file(&path);
        let err = Decoder::open(&path).err().unwrap();
        assert!(err.to_string().starts_with("cannot open"), "{err:#}");
    }

    #[test]
    fn formats_times_for_the_window() {
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(9.6), "0:10");
        assert_eq!(format_time(225.4), "3:45");
        assert_eq!(format_time(3600.0), "60:00");
    }
}
