//! Display-only clock for replaying bursty audio at the UI frame rate.

use std::collections::VecDeque;

use shirabe_dsp::MINUS_INFINITY_DB;

use crate::shared::SharedMeters;
use crate::timeline::{ChannelSample, Snapshot, CAPACITY};

const MIN_DELAY: f64 = 0.010;
const MAX_DELAY: f64 = 0.250;
// Arrival is observed at paint time, so allow for roughly one 60 Hz display tick plus
// the difference between a host block end and the closest snapshot.
const JITTER: f64 = 0.025;
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
            position: 0.0,
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
        if let Some(last) = self.incoming.last() {
            let span = self.points.back().map_or_else(
                || (last.frame - self.incoming[0].frame) as f64 / rate + 0.005,
                |previous| (last.frame - previous.frame) as f64 / rate,
            );
            let gap = self.last_arrival.replace(now).map_or(0.0, |t| now - t);
            let observed = if gap <= MAX_DELAY + DRIFT_MARGIN {
                span.max(gap)
            } else {
                // A stop/resume or editor suspension isn't a recurring burst interval.
                span
            };
            let decaying = MIN_DELAY + (self.delay - MIN_DELAY) * (-gap / PEAK_DECAY_SECONDS).exp();
            self.delay = decaying.max(observed + JITTER).clamp(MIN_DELAY, MAX_DELAY);
            for point in self.incoming.drain(..) {
                if self.points.len() == CAPACITY {
                    self.points.pop_front();
                }
                self.points.push_back(point);
            }
        }

        let latest = self.points.back()?.frame as f64;
        let last_arrival = self.last_arrival?;
        let age = (now - last_arrival).min(self.delay);
        let desired = latest + (age - self.delay) * rate;
        if starting || (arrived && latest - self.position > (MAX_DELAY + DRIFT_MARGIN) * rate) {
            // Start on the delayed clock even before the first point. If fast/offline
            // rendering or a suspended editor outruns it, skip forward to the new time base.
            self.position = desired;
        } else {
            let correction =
                ((desired - self.position - dt * rate) / rate).clamp(-MAX_SLEW, MAX_SLEW);
            self.position = (self.position + dt * rate * (1.0 + correction)).min(latest);
        }
        // Never extrapolate. Retain the last point at or below the cursor for interpolation.
        while self
            .points
            .get(1)
            .is_some_and(|p| p.frame as f64 <= self.position)
        {
            self.points.pop_front();
        }
        let a = *self.points.front()?;
        let Some(&b) = self.points.get(1) else {
            return Some(a);
        };
        if self.position <= a.frame as f64 {
            return Some(a);
        }
        let t = ((self.position - a.frame as f64) / (b.frame - a.frame) as f64) as f32;
        Some(interpolate(a, b, t, self.position as u64))
    }

    fn restart(&mut self, epoch: u64, rate: f64) {
        self.points.clear();
        self.cursor = 0;
        self.epoch = epoch;
        self.rate = rate;
        self.last_tick = None;
        self.last_arrival = None;
        self.delay = MIN_DELAY;
    }
}

fn interpolate(a: Snapshot, b: Snapshot, t: f32, frame: u64) -> Snapshot {
    let linear = |a, b| a + (b - a) * t;
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
            level_db: linear(a.channels[i].level_db, b.channels[i].level_db),
            vu: linear(a.channels[i].vu, b.channels[i].vu),
            ppm_db: linear(a.channels[i].ppm_db, b.channels[i].ppm_db),
        }),
        momentary: optional(a.momentary, b.momentary),
        short_term: optional(a.short_term, b.short_term),
        correlation: optional(a.correlation, b.correlation),
        balance: optional(a.balance, b.balance),
    }
}
