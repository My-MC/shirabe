//! Display-only clock for replaying bursty audio at the UI frame rate.

use std::collections::VecDeque;

use shirabe_dsp::MINUS_INFINITY_DB;

use crate::shared::{SharedMeters, MAX_CHANNELS};
use crate::timeline::{ChannelSample, Snapshot, CAPACITY};

const MIN_DELAY: f64 = 0.010;
const MAX_DELAY: f64 = 0.250;
// Arrival is observed at paint time, so allow for roughly one 60 Hz display tick plus
// the difference between a host block end and the closest snapshot.
const JITTER: f64 = 0.025;
// A first read can't distinguish a callback from accumulated history. A short batch
// can seed the clock; with more than three 60 Hz paints, wait to see an actual arrival.
const STARTUP_SPAN: f64 = 0.050;
const PEAK_DECAY_SECONDS: f64 = 4.0;
const MAX_SLEW: f64 = 0.12;
const DRIFT_MARGIN: f64 = 0.050;

/// The editor's buffered display clock. It never writes back to the measurement engine.
pub struct Playout {
    points: VecDeque<Snapshot>,
    incoming: Vec<Snapshot>,
    cursor: u64,
    epoch: u64,
    rate: f64,
    last_tick: Option<f64>,
    last_arrival: Option<f64>,
    delay: f64,
    waiting_for_cadence: bool,
    position: f64,
}

impl Default for Playout {
    fn default() -> Self {
        Self {
            points: VecDeque::with_capacity(CAPACITY),
            incoming: Vec::with_capacity(CAPACITY),
            cursor: 0,
            epoch: 0,
            rate: 0.0,
            last_tick: None,
            last_arrival: None,
            delay: MIN_DELAY,
            waiting_for_cadence: false,
            position: f64::NEG_INFINITY,
        }
    }
}

impl Playout {
    pub(crate) fn update(&mut self, shared: &SharedMeters, now: f64) -> Option<Snapshot> {
        let rate = f64::from(shared.sample_rate());
        if !now.is_finite() || !rate.is_finite() || rate <= 0.0 {
            return None;
        }
        let epoch = shared.timeline.epoch();
        if epoch != self.epoch || rate != self.rate || self.last_tick.is_some_and(|t| now < t) {
            self.restart(epoch, rate);
        }
        let (cursor, read_epoch) = shared.timeline.read(self.cursor, &mut self.incoming)?;
        if read_epoch != epoch {
            return None;
        }
        self.cursor = cursor;
        let dt = self.last_tick.replace(now).map_or(0.0, |t| now - t);
        let arrived = !self.incoming.is_empty();
        let starting = self.last_arrival.is_none();
        let mut resync = starting;
        if let Some(last) = self.incoming.last() {
            let latest = last.frame;
            let gap = self.last_arrival.replace(now).map_or(0.0, |t| now - t);
            let stalled = gap > MAX_DELAY + DRIFT_MARGIN;
            let span = self.points.back().map_or_else(
                || (latest - self.incoming[0].frame) as f64 / rate + 0.005,
                |previous| (latest - previous.frame) as f64 / rate,
            );
            if stalled || starting {
                let oldest = latest.saturating_sub((MAX_DELAY * rate) as u64);
                let first = self.incoming.partition_point(|p| p.frame < oldest);
                self.incoming.drain(..first);
                self.points.retain(|p| p.frame >= oldest);
            }
            if stalled {
                // Queued audio and wall time during a suspension do not measure burst cadence.
                resync = true;
            } else if starting && span > STARTUP_SPAN {
                self.waiting_for_cadence = true;
            } else {
                let decaying =
                    MIN_DELAY + (self.delay - MIN_DELAY) * (-gap / PEAK_DECAY_SECONDS).exp();
                // Read arrivals are quantized to paint ticks. Avoid counting all of that
                // rounding as burst time on top of the jitter margin; the audio span is exact.
                let observed = span.max(gap - dt.min(1.0 / 60.0) * 0.5);
                self.delay = decaying.max(observed + JITTER).clamp(MIN_DELAY, MAX_DELAY);
            }
            if !starting && self.waiting_for_cadence {
                resync = true;
                self.waiting_for_cadence = false;
            }
            for point in self.incoming.drain(..) {
                if self.points.len() == CAPACITY {
                    self.points.pop_front();
                }
                self.points.push_back(point);
            }
        }

        let latest = self.points.back()?.frame as f64;
        let last_arrival = self.last_arrival?;
        if self.waiting_for_cadence {
            if now - last_arrival < MAX_DELAY {
                return None;
            }
            // If no new block arrives, show the last known value rather than waiting forever.
            self.waiting_for_cadence = false;
            resync = true;
        }
        let age = (now - last_arrival).min(self.delay);
        let desired = latest + (age - self.delay) * rate;
        let jump =
            resync || (arrived && latest - self.position > (MAX_DELAY + DRIFT_MARGIN) * rate);
        if jump {
            // Start on the delayed clock even before the first point. If fast/offline
            // rendering or a suspended editor outruns it, skip forward to the new time base.
            self.position = self.position.max(desired);
        } else {
            let correction =
                ((desired - self.position - dt * rate) / rate).clamp(-MAX_SLEW, MAX_SLEW);
            self.position = (self.position + dt * rate * (1.0 + correction)).min(latest);
        }
        // A paint can cross several 5 ms snapshots. Preserve an attack crossed since
        // the last paint at full height for this frame instead of smoothing it away.
        let mut attacks = [MINUS_INFINITY_DB; MAX_CHANNELS];
        while let Some(&next) = self
            .points
            .get(1)
            .filter(|p| p.frame as f64 <= self.position)
        {
            let previous = self.points.pop_front().expect("point before next");
            for ((attack, old), new) in attacks.iter_mut().zip(previous.channels).zip(next.channels)
            {
                if !jump && new.level_db > old.level_db {
                    *attack = attack.max(new.level_db);
                }
            }
        }
        // Never extrapolate. Retain the last point at or below the cursor for interpolation.
        let a = *self.points.front()?;
        let mut point = match self.points.get(1) {
            Some(&b) if self.position > a.frame as f64 => {
                let t = ((self.position - a.frame as f64) / (b.frame - a.frame) as f64) as f32;
                interpolate(a, b, t, self.position as u64)
            }
            _ => a,
        };
        for (channel, attack) in point.channels.iter_mut().zip(attacks) {
            channel.level_db = channel.level_db.max(attack);
            channel.hold_db = channel.hold_db.max(channel.level_db);
        }
        Some(point)
    }

    fn restart(&mut self, epoch: u64, rate: f64) {
        self.points.clear();
        self.cursor = 0;
        self.epoch = epoch;
        self.rate = rate;
        self.last_tick = None;
        self.last_arrival = None;
        self.delay = MIN_DELAY;
        self.waiting_for_cadence = false;
        self.position = f64::NEG_INFINITY;
    }
}

fn interpolate(a: Snapshot, b: Snapshot, t: f32, frame: u64) -> Snapshot {
    let linear = |a, b| a + (b - a) * t;
    // Sample-peak attacks are instantaneous; do not draw them before the cursor arrives.
    let release_only = |a, b| if b > a { a } else { linear(a, b) };
    let optional = |a, b| match (a, b) {
        (Some(a), Some(b)) if a == b => Some(a),
        (Some(a), Some(b)) => Some(linear(
            f32::max(a, MINUS_INFINITY_DB),
            f32::max(b, MINUS_INFINITY_DB),
        )),
        // Readiness (e.g. the first complete loudness window) is not interpolated.
        _ => a,
    };
    Snapshot {
        frame,
        scope_epoch: a.scope_epoch,
        channels: std::array::from_fn(|i| ChannelSample {
            level_db: release_only(a.channels[i].level_db, b.channels[i].level_db),
            hold_db: release_only(a.channels[i].hold_db, b.channels[i].hold_db),
            vu: linear(a.channels[i].vu, b.channels[i].vu),
            ppm_db: linear(a.channels[i].ppm_db, b.channels[i].ppm_db),
        }),
        momentary: optional(a.momentary, b.momentary),
        short_term: optional(a.short_term, b.short_term),
        correlation: optional(a.correlation, b.correlation),
        balance: optional(a.balance, b.balance),
    }
}
