//! Programme loudness and loudness range (ITU-R BS.1770, EBU Tech 3341 / 3342).
//!
//! All memory is allocated in [`LoudnessMeter::new`]; [`LoudnessMeter::process`] never allocates,
//! so the meter can run on the audio thread for any programme length. Gated measurements use
//! histograms with 0.01 LU bins that also accumulate the exact block energies, so the gated
//! averages are exact and only the gate thresholds are quantized to 0.01 LU.

use crate::channel::ChannelRole;
use crate::kweighting::KWeighting;

/// Maximum number of channels a meter can handle.
pub const MAX_LOUDNESS_CHANNELS: usize = 8;

/// Absolute gate in LUFS.
pub const ABSOLUTE_GATE_LUFS: f64 = -70.0;

const SUB_BLOCKS_MOMENTARY: usize = 4; // 400 ms
const SUB_BLOCKS_SHORT_TERM: usize = 30; // 3 s
const RELATIVE_GATE_INTEGRATED_LU: f64 = -10.0;
const RELATIVE_GATE_LRA_LU: f64 = -20.0;

const HIST_MIN_LUFS: f64 = -70.0;
const HIST_MAX_LUFS: f64 = 10.0;
const HIST_STEP_LU: f64 = 0.01;
const HIST_BINS: usize = ((HIST_MAX_LUFS - HIST_MIN_LUFS) / HIST_STEP_LU) as usize;

/// Converts a mean-square energy (already channel weighted) to LUFS.
#[inline]
pub fn energy_to_lufs(energy: f64) -> f64 {
    if energy > 0.0 {
        -0.691 + 10.0 * energy.log10()
    } else {
        f64::NEG_INFINITY
    }
}

#[inline]
fn lufs_to_energy(lufs: f64) -> f64 {
    10f64.powf((lufs + 0.691) / 10.0)
}

/// Histogram of gating blocks: counts and summed energies per 0.01 LU bin.
#[derive(Clone, Debug)]
struct GateHistogram {
    count: Vec<u64>,
    energy: Vec<f64>,
}

impl GateHistogram {
    fn new() -> Self {
        Self {
            count: vec![0; HIST_BINS],
            energy: vec![0.0; HIST_BINS],
        }
    }

    fn clear(&mut self) {
        self.count.fill(0);
        self.energy.fill(0.0);
    }

    fn bin(lufs: f64) -> usize {
        (((lufs - HIST_MIN_LUFS) / HIST_STEP_LU) as usize).min(HIST_BINS - 1)
    }

    /// Adds a block if it is above the absolute gate.
    fn add(&mut self, energy: f64) {
        let lufs = energy_to_lufs(energy);
        if lufs > ABSOLUTE_GATE_LUFS {
            let bin = Self::bin(lufs);
            self.count[bin] += 1;
            self.energy[bin] += energy;
        }
    }

    /// Power mean (in LUFS) of all blocks at or above `from_bin`.
    fn mean_from(&self, from_bin: usize) -> Option<f64> {
        let mut n = 0u64;
        let mut e = 0.0;
        for bin in from_bin..HIST_BINS {
            n += self.count[bin];
            e += self.energy[bin];
        }
        (n > 0).then(|| energy_to_lufs(e / n as f64))
    }

    /// Mean after applying a relative gate (in LU) to the absolute-gated blocks.
    fn gated_mean(&self, relative_gate_lu: f64) -> Option<(f64, usize)> {
        let ungated = self.mean_from(0)?;
        let gate = ungated + relative_gate_lu;
        // A block is kept if its loudness is above the gate. Blocks are binned by flooring, so
        // the first kept bin is the one whose lower edge is at or above the gate.
        let from = if gate <= HIST_MIN_LUFS {
            0
        } else {
            (((gate - HIST_MIN_LUFS) / HIST_STEP_LU).ceil() as usize).min(HIST_BINS)
        };
        self.mean_from(from).map(|m| (m, from))
    }

    /// Loudness at the given percentile (0..=1) of the blocks at or above `from_bin`.
    fn percentile_from(&self, from_bin: usize, p: f64) -> Option<f64> {
        let total: u64 = self.count[from_bin..].iter().sum();
        if total == 0 {
            return None;
        }
        let target = ((total - 1) as f64 * p).round() as u64;
        let mut seen = 0u64;
        for (offset, count) in self.count[from_bin..].iter().enumerate() {
            seen += count;
            if seen > target {
                let bin = from_bin + offset;
                return Some(HIST_MIN_LUFS + (bin as f64 + 0.5) * HIST_STEP_LU);
            }
        }
        None
    }
}

/// Read-outs of a [`LoudnessMeter`] in LUFS / LU. Unavailable values are `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LoudnessReadout {
    /// Momentary loudness (400 ms).
    pub momentary: Option<f64>,
    /// Short-term loudness (3 s).
    pub short_term: Option<f64>,
    /// Integrated (programme) loudness since the last reset.
    pub integrated: Option<f64>,
    /// Loudness range since the last reset.
    pub range: Option<f64>,
    /// Highest momentary loudness since the last reset.
    pub max_momentary: Option<f64>,
    /// Highest short-term loudness since the last reset.
    pub max_short_term: Option<f64>,
}

/// Multichannel BS.1770 loudness meter.
#[derive(Clone, Debug)]
pub struct LoudnessMeter {
    sample_rate: f64,
    channels: usize,
    weights: [f64; MAX_LOUDNESS_CHANNELS],
    filters: Vec<KWeighting>,

    /// Samples per sub-block. Sub-blocks divide the 100 ms hop exactly and are at most 2.5 ms
    /// long, so the momentary and short-term windows slide in fine steps (Max M / Max S read
    /// a short tone correctly whatever its position) while the gating blocks keep the exact
    /// 400 ms length and 100 ms hop of ITU-R BS.1770.
    sub_len: usize,
    /// Sub-blocks per 100 ms hop.
    subs_per_hop: usize,
    sub_pos: usize,
    sub_energy: f64,
    /// Ring of the most recent sub-block energy sums (channel weighted), 3 s long.
    ring: Vec<f64>,
    ring_pos: usize,
    ring_filled: usize,
    momentary_sum: f64,
    short_term_sum: f64,
    hop_pos: usize,

    integrating: bool,
    programme_samples: u64,
    integrated_hist: GateHistogram,
    range_hist: GateHistogram,
    readout: LoudnessReadout,
}

/// Largest divisor of `hop` that is at most `limit` (at least 1).
fn sub_block_length(hop: usize, limit: usize) -> usize {
    (1..=limit.max(1)).rev().find(|d| hop % d == 0).unwrap_or(1)
}

impl LoudnessMeter {
    /// Creates a meter for the given channel roles (at most [`MAX_LOUDNESS_CHANNELS`]).
    pub fn new(sample_rate: f32, roles: &[ChannelRole]) -> Self {
        let mut meter = Self {
            sample_rate: sample_rate.max(1.0) as f64,
            channels: 0,
            weights: [0.0; MAX_LOUDNESS_CHANNELS],
            filters: Vec::with_capacity(MAX_LOUDNESS_CHANNELS),
            sub_len: 1,
            subs_per_hop: 1,
            sub_pos: 0,
            sub_energy: 0.0,
            ring: Vec::new(),
            ring_pos: 0,
            ring_filled: 0,
            momentary_sum: 0.0,
            short_term_sum: 0.0,
            hop_pos: 0,
            integrating: true,
            programme_samples: 0,
            integrated_hist: GateHistogram::new(),
            range_hist: GateHistogram::new(),
            readout: LoudnessReadout::default(),
        };
        meter.prepare(sample_rate, roles);
        meter
    }

    /// Changes the sample rate or channel layout and resets everything. Allocates the history
    /// buffer for the sample rate, so call it outside the audio callback.
    pub fn prepare(&mut self, sample_rate: f32, roles: &[ChannelRole]) {
        self.sample_rate = sample_rate.max(1.0) as f64;
        self.channels = roles.len().min(MAX_LOUDNESS_CHANNELS);
        self.weights = [0.0; MAX_LOUDNESS_CHANNELS];
        for (w, role) in self.weights.iter_mut().zip(roles) {
            *w = role.loudness_weight() as f64;
        }
        self.filters.clear();
        for _ in 0..self.channels {
            self.filters.push(KWeighting::new(self.sample_rate));
        }
        let hop = (self.sample_rate * 0.1).round().max(1.0) as usize;
        self.sub_len = sub_block_length(hop, hop / 40);
        self.subs_per_hop = hop / self.sub_len;
        self.ring = vec![0.0; SUB_BLOCKS_SHORT_TERM * self.subs_per_hop];
        self.reset();
    }

    /// Clears all measurements, including the integrated loudness and loudness range.
    pub fn reset(&mut self) {
        for f in &mut self.filters {
            f.reset();
        }
        self.sub_pos = 0;
        self.sub_energy = 0.0;
        self.ring.fill(0.0);
        self.ring_pos = 0;
        self.ring_filled = 0;
        self.momentary_sum = 0.0;
        self.short_term_sum = 0.0;
        self.hop_pos = 0;
        self.readout = LoudnessReadout::default();
        self.reset_programme();
    }

    /// Clears only the integrated loudness, loudness range, maxima and programme duration
    /// (e.g. on transport start), keeping the filter state and the momentary / short-term
    /// history. EBU Tech 3341 requires I and LRA to be reset together.
    pub fn reset_programme(&mut self) {
        self.integrated_hist.clear();
        self.range_hist.clear();
        self.programme_samples = 0;
        self.readout.integrated = None;
        self.readout.range = None;
        self.readout.max_momentary = None;
        self.readout.max_short_term = None;
    }

    /// Starts (`true`) or pauses (`false`) the integrated loudness and loudness range together
    /// (EBU Tech 3341 §2.2). Momentary and short-term loudness keep running.
    pub fn set_integrating(&mut self, integrating: bool) {
        self.integrating = integrating;
    }

    /// Whether the integrated loudness and loudness range are running.
    pub fn is_integrating(&self) -> bool {
        self.integrating
    }

    /// Duration in seconds that has been integrated since the last programme reset.
    pub fn programme_seconds(&self) -> f64 {
        self.programme_samples as f64 / self.sample_rate
    }

    /// Processes one block. `channels[i]` holds the samples of channel `i`; all slices must have
    /// the same length. Extra channels beyond the prepared layout are ignored.
    pub fn process<C: AsRef<[f32]>>(&mut self, channels: &[C]) {
        let n_channels = self.channels.min(channels.len());
        let Some(first) = channels.first() else {
            return;
        };
        let len = first.as_ref().len();
        if self.integrating {
            self.programme_samples += len as u64;
        }

        for i in 0..len {
            let mut sum = 0.0;
            for ch in 0..n_channels {
                let x = channels[ch].as_ref()[i] as f64;
                let y = self.filters[ch].process(x);
                sum += self.weights[ch] * y * y;
            }
            self.sub_energy += sum;
            self.sub_pos += 1;
            if self.sub_pos == self.sub_len {
                self.finish_sub_block();
            }
        }
    }

    fn finish_sub_block(&mut self) {
        let n = self.ring.len();
        let m_len = SUB_BLOCKS_MOMENTARY * self.subs_per_hop;
        // Slide both windows: add the new sub-block, drop the ones that left each window.
        let leaving_m = self.ring[(self.ring_pos + n - m_len) % n];
        let leaving_s = self.ring[self.ring_pos];
        self.ring[self.ring_pos] = self.sub_energy;
        self.ring_pos = (self.ring_pos + 1) % n;
        self.ring_filled = (self.ring_filled + 1).min(n);
        self.momentary_sum += self.sub_energy - leaving_m;
        self.short_term_sum += self.sub_energy - leaving_s;
        self.sub_energy = 0.0;
        self.sub_pos = 0;

        self.hop_pos += 1;
        let hop_end = self.hop_pos == self.subs_per_hop;
        if hop_end {
            self.hop_pos = 0;
            // Recompute the running sums exactly to stop rounding errors from accumulating.
            self.momentary_sum = self.window_sum(m_len);
            self.short_term_sum = self.window_sum(n);
        }

        if self.ring_filled >= m_len {
            let e = (self.momentary_sum / (m_len * self.sub_len) as f64).max(0.0);
            let m = energy_to_lufs(e);
            self.readout.momentary = Some(m);
            if self.integrating {
                self.readout.max_momentary =
                    Some(self.readout.max_momentary.map_or(m, |x| x.max(m)));
                if hop_end {
                    // Gating blocks: 400 ms every 100 ms (75 % overlap).
                    self.integrated_hist.add(e);
                    self.readout.integrated = self
                        .integrated_hist
                        .gated_mean(RELATIVE_GATE_INTEGRATED_LU)
                        .map(|(m, _)| m);
                }
            }
        }
        if self.ring_filled >= n {
            let e = (self.short_term_sum / (n * self.sub_len) as f64).max(0.0);
            let s = energy_to_lufs(e);
            self.readout.short_term = Some(s);
            if self.integrating {
                self.readout.max_short_term =
                    Some(self.readout.max_short_term.map_or(s, |x| x.max(s)));
                if hop_end {
                    // Short-term values every 100 ms feed the loudness range (EBU Tech 3342).
                    self.range_hist.add(e);
                    self.readout.range = self.loudness_range();
                }
            }
        }
    }

    /// Sum of the most recent `count` sub-blocks.
    fn window_sum(&self, count: usize) -> f64 {
        let n = self.ring.len();
        (1..=count.min(n))
            .map(|k| self.ring[(self.ring_pos + n - k) % n])
            .sum()
    }

    fn loudness_range(&self) -> Option<f64> {
        let (_, from) = self.range_hist.gated_mean(RELATIVE_GATE_LRA_LU)?;
        let low = self.range_hist.percentile_from(from, 0.10)?;
        let high = self.range_hist.percentile_from(from, 0.95)?;
        Some((high - low).max(0.0))
    }

    /// Current read-outs.
    pub fn readout(&self) -> LoudnessReadout {
        self.readout
    }

    /// The sample rate the meter was prepared for.
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }
}

/// Converts a loudness to the energy domain (exposed for tests and tools).
pub fn lufs_to_mean_square(lufs: f64) -> f64 {
    lufs_to_energy(lufs)
}
