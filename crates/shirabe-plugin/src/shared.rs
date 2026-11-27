//! Lock-free state shared between the audio thread (writer) and the editor (reader).
//!
//! Every value is an independent atomic, so the editor may see values from adjacent blocks
//! mixed together; for meters this is harmless and avoids any locking on the audio thread.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use nih_plug::prelude::AtomicF32;
use shirabe_dsp::history::LoudnessHistory;
use shirabe_dsp::scope::StereoScope;
use shirabe_dsp::spectrum::SpectrumCapture;
use shirabe_dsp::stereo::StereoStatsReadout;
use shirabe_dsp::{ChannelLayout, LoudnessReadout, PeakMeter, MINUS_INFINITY_DB};

use crate::timeline::Timeline;

/// Maximum number of channels the meters are pre-allocated for.
pub const MAX_CHANNELS: usize = 8;

// The 250 ms playout budget at up to 384 kHz, plus the live trace and burst headroom.
const DELAYED_SCOPE_CAPACITY: usize = 131_072;

/// Read-outs of one channel.
pub struct ChannelReadout {
    /// Displayed sample peak level in dBFS.
    pub level_db: AtomicF32,
    /// Peak-hold marker in dBFS.
    pub hold_db: AtomicF32,
    /// Highest sample peak since the last reset in dBFS.
    pub max_db: AtomicF32,
    /// Highest true peak since the last reset in dBTP.
    pub true_peak_max_db: AtomicF32,
    /// VU needle position in VU.
    pub vu: AtomicF32,
    /// Quasi-peak programme meter reading in dBFS.
    pub ppm_db: AtomicF32,
}

impl ChannelReadout {
    fn new() -> Self {
        Self {
            level_db: AtomicF32::new(MINUS_INFINITY_DB),
            hold_db: AtomicF32::new(MINUS_INFINITY_DB),
            max_db: AtomicF32::new(MINUS_INFINITY_DB),
            true_peak_max_db: AtomicF32::new(MINUS_INFINITY_DB),
            vu: AtomicF32::new(MINUS_INFINITY_DB),
            ppm_db: AtomicF32::new(MINUS_INFINITY_DB),
        }
    }

    /// Publishes the current state of a sample peak meter (audio thread).
    pub fn store(&self, meter: &PeakMeter) {
        self.level_db.store(meter.level_db(), Ordering::Relaxed);
        self.hold_db.store(meter.hold_db(), Ordering::Relaxed);
        self.max_db.store(meter.max_db(), Ordering::Relaxed);
    }

    fn clear(&self) {
        for v in [
            &self.level_db,
            &self.hold_db,
            &self.max_db,
            &self.true_peak_max_db,
            &self.vu,
            &self.ppm_db,
        ] {
            v.store(MINUS_INFINITY_DB, Ordering::Relaxed);
        }
    }
}

/// An optional value stored as `NaN` when absent.
pub struct OptionalF32(AtomicF32);

impl OptionalF32 {
    fn new() -> Self {
        Self(AtomicF32::new(f32::NAN))
    }

    /// Stores a value (`None` becomes `NaN`).
    pub fn store(&self, value: Option<f64>) {
        self.0
            .store(value.map_or(f32::NAN, |v| v as f32), Ordering::Relaxed);
    }

    /// Loads the value.
    pub fn load(&self) -> Option<f32> {
        let v = self.0.load(Ordering::Relaxed);
        (!v.is_nan()).then_some(v)
    }
}

/// Loudness read-outs.
pub struct LoudnessShared {
    /// Momentary loudness in LUFS.
    pub momentary: OptionalF32,
    /// Short-term loudness in LUFS.
    pub short_term: OptionalF32,
    /// Integrated loudness in LUFS.
    pub integrated: OptionalF32,
    /// Programme peak-to-loudness ratio in LU.
    pub plr: OptionalF32,
    /// Loudness range in LU.
    pub range: OptionalF32,
    /// Maximum momentary loudness in LUFS.
    pub max_momentary: OptionalF32,
    /// Maximum short-term loudness in LUFS.
    pub max_short_term: OptionalF32,
}

impl LoudnessShared {
    fn new() -> Self {
        Self {
            momentary: OptionalF32::new(),
            short_term: OptionalF32::new(),
            integrated: OptionalF32::new(),
            plr: OptionalF32::new(),
            range: OptionalF32::new(),
            max_momentary: OptionalF32::new(),
            max_short_term: OptionalF32::new(),
        }
    }

    /// Publishes a loudness read-out (audio thread).
    pub fn store(&self, r: &LoudnessReadout) {
        self.momentary.store(r.momentary);
        self.short_term.store(r.short_term);
        self.integrated.store(r.integrated);
        self.range.store(r.range);
        self.max_momentary.store(r.max_momentary);
        self.max_short_term.store(r.max_short_term);
    }
}

/// Stereo correlation and L/R balance read-outs.
pub struct StereoShared {
    /// Correlation of the measurement window: `+1` in phase, `0` unrelated, `-1` anti-phase.
    pub correlation: OptionalF32,
    /// Relative channel energy: `-1` left only, `0` equal, `+1` right only.
    pub balance: OptionalF32,
}

impl StereoShared {
    fn new() -> Self {
        Self {
            correlation: OptionalF32::new(),
            balance: OptionalF32::new(),
        }
    }

    /// Publishes the stereo statistics (audio thread).
    pub fn store(&self, r: &StereoStatsReadout) {
        self.correlation.store(r.correlation.map(f64::from));
        self.balance.store(r.balance.map(f64::from));
    }

    fn clear(&self) {
        self.correlation.store(None);
        self.balance.store(None);
    }
}

/// Read-outs of the sidechain reference measurement.
///
/// Only the three programme values are published: the comparison shows how the main
/// measurement differs from the reference, so the reference's own range, PLR and maxima are
/// not exposed.
pub struct ReferenceShared {
    /// Reference momentary loudness in LUFS.
    pub momentary: OptionalF32,
    /// Reference short-term loudness in LUFS.
    pub short_term: OptionalF32,
    /// Reference integrated loudness in LUFS.
    pub integrated: OptionalF32,
}

impl ReferenceShared {
    fn new() -> Self {
        Self {
            momentary: OptionalF32::new(),
            short_term: OptionalF32::new(),
            integrated: OptionalF32::new(),
        }
    }

    /// Publishes a reference read-out (audio thread).
    pub fn store(&self, r: &LoudnessReadout) {
        self.momentary.store(r.momentary);
        self.short_term.store(r.short_term);
        self.integrated.store(r.integrated);
    }

    fn clear(&self) {
        self.store(&LoudnessReadout::default());
    }
}

/// Deltas of the main measurement against the reference, in LU (main − reference).
///
/// A value is only present when both sides are measurements. A silent side reads
/// `-inf` rather than a value, so the delta is unavailable and the read-out shows dashes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LoudnessDeltas {
    /// Momentary difference in LU.
    pub momentary: Option<f32>,
    /// Short-term difference in LU.
    pub short_term: Option<f32>,
    /// Integrated difference in LU.
    pub integrated: Option<f32>,
}

impl LoudnessDeltas {
    /// The difference in LU, or `None` if either side is unavailable or not finite.
    fn difference(main: Option<f32>, reference: Option<f32>) -> Option<f32> {
        match (main, reference) {
            (Some(m), Some(r)) if m.is_finite() && r.is_finite() => Some(m - r),
            _ => None,
        }
    }
}

/// Everything the editor can read.
pub struct SharedMeters {
    channels: [ChannelReadout; MAX_CHANNELS],
    /// Programme loudness of all channels.
    pub loudness: LoudnessShared,
    /// Loudness of the sidechain reference input, when the host's layout has one.
    pub reference: ReferenceShared,
    /// Correlation and balance of the stereo input (audio thread writes, editor reads).
    pub stereo: StereoShared,
    /// Recent stereo frames for the Lissajous view (audio thread writes, editor reads).
    /// Empty while the input is mono.
    pub scope: StereoScope,
    pub(crate) timeline: Timeline,
    /// Momentary and short-term loudness at 10 points per second (audio thread writes, editor
    /// reads). Display-only: it never feeds back into a measurement.
    pub history: LoudnessHistory,
    /// Mono capture for the spectrum tile (audio thread writes, editor analyses it on the UI
    /// thread). Display-only.
    pub spectrum: SpectrumCapture,
    active_channels: AtomicUsize,
    sample_rate: AtomicF32,
    reset_requested: AtomicBool,
    /// Whether the current audio IO layout has a sidechain input (audio thread writes from
    /// `prepare`, editor reads).
    reference_available: AtomicBool,
    /// Whether the integrated loudness and loudness range are running (editor writes).
    integrating: AtomicBool,
    /// Measured programme duration in seconds since the last reset (audio thread writes).
    pub programme_seconds: AtomicF32,
    /// Number of processed blocks; lets the editor tell whether audio is running.
    pub blocks: AtomicU64,
}

impl SharedMeters {
    /// Creates empty read-outs.
    pub fn new() -> Self {
        Self {
            channels: std::array::from_fn(|_| ChannelReadout::new()),
            loudness: LoudnessShared::new(),
            reference: ReferenceShared::new(),
            stereo: StereoShared::new(),
            scope: StereoScope::with_capacity(DELAYED_SCOPE_CAPACITY),
            timeline: Timeline::new(),
            history: LoudnessHistory::new(),
            spectrum: SpectrumCapture::new(),
            active_channels: AtomicUsize::new(2),
            sample_rate: AtomicF32::new(0.0),
            reset_requested: AtomicBool::new(false),
            reference_available: AtomicBool::new(false),
            integrating: AtomicBool::new(true),
            programme_seconds: AtomicF32::new(0.0),
            blocks: AtomicU64::new(0),
        }
    }

    /// Sets the active channel count, the sample rate and whether the layout has a sidechain
    /// input (called from `initialize`).
    pub fn configure(&self, channels: usize, sample_rate: f32, reference_available: bool) {
        self.active_channels
            .store(channels.min(MAX_CHANNELS), Ordering::Relaxed);
        self.sample_rate.store(sample_rate, Ordering::Relaxed);
        self.reference_available
            .store(reference_available, Ordering::Relaxed);
        // A new rate needs a new interval; the old points describe the previous rate.
        self.history.prepare(sample_rate);
        self.clear();
    }

    /// Clears all read-outs.
    pub fn clear(&self) {
        for channel in &self.channels {
            channel.clear();
        }
        self.loudness.store(&LoudnessReadout::default());
        self.loudness.plr.store(None);
        self.reference.clear();
        self.stereo.clear();
        self.clear_display_capture();
        self.history.clear();
        self.spectrum.clear();
    }

    /// Discards delayed display data at a source change, without clearing measured values.
    /// Only the producer may call this while audio is active.
    pub fn clear_display_capture(&self) {
        self.timeline.clear();
        self.scope.clear();
    }

    /// Read-outs of channel `index` (must be below [`MAX_CHANNELS`]).
    pub fn channel(&self, index: usize) -> &ChannelReadout {
        &self.channels[index]
    }

    /// Number of channels currently processed.
    pub fn active_channels(&self) -> usize {
        self.active_channels.load(Ordering::Relaxed)
    }

    /// The layout matching the active channel count, if it is a known one.
    pub fn layout(&self) -> Option<ChannelLayout> {
        ChannelLayout::from_channel_count(self.active_channels())
    }

    /// Current sample rate in Hz (0 before the first initialisation).
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate.load(Ordering::Relaxed)
    }

    /// Whether the current audio IO layout has a sidechain input.
    pub fn reference_available(&self) -> bool {
        self.reference_available.load(Ordering::Relaxed)
    }

    /// Comparison of the main loudness with the sidechain reference (editor).
    ///
    /// The values are read from independent atomics, so a delta may mix two adjacent blocks.
    pub fn deltas(&self) -> LoudnessDeltas {
        LoudnessDeltas {
            momentary: LoudnessDeltas::difference(
                self.loudness.momentary.load(),
                self.reference.momentary.load(),
            ),
            short_term: LoudnessDeltas::difference(
                self.loudness.short_term.load(),
                self.reference.short_term.load(),
            ),
            integrated: LoudnessDeltas::difference(
                self.loudness.integrated.load(),
                self.reference.integrated.load(),
            ),
        }
    }

    /// Asks the audio thread to clear the maxima and the programme loudness (editor).
    pub fn request_reset(&self) {
        self.reset_requested.store(true, Ordering::Relaxed);
    }

    /// Starts or pauses the integrated loudness and loudness range (editor).
    pub fn set_integrating(&self, integrating: bool) {
        self.integrating.store(integrating, Ordering::Relaxed);
    }

    /// Whether the integrated loudness and loudness range are running.
    pub fn integrating(&self) -> bool {
        self.integrating.load(Ordering::Relaxed)
    }

    /// Consumes a pending reset request (audio thread).
    pub fn take_reset_request(&self) -> bool {
        self.reset_requested.swap(false, Ordering::Relaxed)
    }
}

impl Default for SharedMeters {
    fn default() -> Self {
        Self::new()
    }
}
