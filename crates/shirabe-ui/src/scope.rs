//! Lissajous (mid/side) display of the stereo signal.
//!
//! The widget only draws: it takes points already rotated to mid/side coordinates and never
//! touches audio state. See `shirabe_dsp::scope` for the capture buffer and the rotation.

use std::collections::VecDeque;

use crate::egui::{
    pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind,
};
use crate::theme::Theme;

/// Points read from the capture buffer for one displayed frame. The oldest frames of the
/// capture are dropped, so the nicest figures come from signals that are periodic within
/// this window (about 43 ms at 48 kHz).
pub const MAX_POINTS: usize = 2048;

/// How long a point stays visible in the trail, in seconds.
pub const TRAIL_SECONDS: f32 = 0.30;

/// Upper bound of stored trail points (about 341 ms at 48 kHz).
///
/// The trail is a display history, not a measurement: at very high sample rates the sample
/// count caps the effective duration before [`TRAIL_SECONDS`] does.
pub const TRAIL_CAPACITY: usize = 16_384;

/// Largest automatic boost in dB. A louder signal is never attenuated, so the fixed-scale
/// overload indication keeps its meaning.
pub const MAX_GAIN_DB: f32 = 20.0;

/// Signals below this level (dBFS) never raise the automatic gain.
pub const GAIN_FLOOR_DBFS: f32 = -60.0;

/// Headroom left below full scale when the automatic gain fits the figure.
const GAIN_HEADROOM_DB: f32 = 1.0;
/// Time constant for the peak envelope to fall, in seconds.
const PEAK_RELEASE_SECONDS: f32 = 2.0;
/// Time constant for lowering the gain (input got louder), in seconds.
const GAIN_ATTACK_SECONDS: f32 = 0.15;
/// Time constant for raising the gain (input got quieter), in seconds.
const GAIN_RELEASE_SECONDS: f32 = 0.8;
/// Changes smaller than this are ignored, so a steady tone settles.
const GAIN_DEADBAND_DB: f32 = 0.1;
/// Largest time step accepted by one update, in seconds.
const MAX_UPDATE_SECONDS: f32 = 0.1;

/// Height reserved for the title line above the square.
const TITLE_HEIGHT: f32 = 14.0;

/// One point of the fading trail, already scaled by the gain in use when it was captured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimedPoint {
    /// Mid/side coordinates after the display gain.
    pub mid: f32,
    pub side: f32,
    /// Seconds on the editor clock (monotonic within one editor session).
    pub time: f64,
}

/// Bounded, time-stamped history of the Lissajous figure for the fading trail.
///
/// Points are never re-added while the source is paused: the editor only appends samples it
/// has not seen, and [`prune`](Self::prune) removes points as their age passes the trail
/// length. Memory is capped by [`TRAIL_CAPACITY`].
pub struct Trail {
    points: VecDeque<TimedPoint>,
}

impl Trail {
    /// Creates an empty trail with capacity reserved up front.
    pub fn new() -> Self {
        Self {
            points: VecDeque::with_capacity(TRAIL_CAPACITY),
        }
    }

    /// Removes every point.
    pub fn clear(&mut self) {
        self.points.clear();
    }

    /// Appends one point, dropping the oldest point when the capacity is reached.
    pub fn push(&mut self, mid: f32, side: f32, time: f64) {
        if self.points.len() == TRAIL_CAPACITY {
            self.points.pop_front();
        }
        self.points.push_back(TimedPoint { mid, side, time });
    }

    /// Removes points older than `seconds` at time `now`.
    pub fn prune(&mut self, now: f64, seconds: f32) {
        if seconds <= 0.0 {
            self.clear();
            return;
        }
        let limit = now - f64::from(seconds);
        while self.points.front().is_some_and(|p| p.time < limit) {
            self.points.pop_front();
        }
    }

    /// Number of stored points.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Whether the trail holds no points.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Iterates from the oldest point to the newest.
    pub fn iter(&self) -> impl Iterator<Item = &TimedPoint> {
        self.points.iter()
    }
}

impl Default for Trail {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Trail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Trail").field("len", &self.len()).finish()
    }
}

/// Opacity of a trail point of `age` seconds with a trail length of `seconds`.
///
/// The newest points are strongest and the opacity reaches zero exactly at the limit.
pub fn alpha_at(age: f32, seconds: f32) -> f32 {
    if !age.is_finite() || !seconds.is_finite() || seconds <= 0.0 || age < 0.0 {
        return 0.0;
    }
    let t = (age / seconds).clamp(0.0, 1.0);
    (1.0 - t).powf(1.3) * 0.85
}

/// Text for the right side of the Lissajous title: the scale actually applied.
pub fn scale_label(auto: bool, gain_db: f32, holding: bool, clipped: bool) -> String {
    let gain = {
        let rounded = (gain_db * 10.0).round() / 10.0;
        if rounded == 0.0 {
            0.0
        } else {
            rounded
        }
    };
    match (auto, clipped, holding) {
        (false, true, _) => "CLIP  ±1".to_owned(),
        (false, false, _) => "FIXED  ±1".to_owned(),
        (true, true, _) => format!("CLIP  {gain:+.1} dB"),
        (true, false, true) => format!("AUTO  {gain:+.1} HOLD"),
        (true, false, false) => format!("AUTO  {gain:+.1} dB"),
    }
}

/// Automatic display gain for the Lissajous view.
///
/// The gain only ever boosts (0 to [`MAX_GAIN_DB`]) and follows a peak envelope with a slow
/// release and a faster attack. Below [`GAIN_FLOOR_DBFS`] it holds the last value instead of
/// winding up on silence or noise, which would make the scale label misleading.
#[derive(Clone, Copy, Debug)]
pub struct AutoGain {
    gain_db: f32,
    peak: f32,
    holding: bool,
}

impl AutoGain {
    /// Creates a gain of 0 dB, holding until the first usable signal.
    pub fn new() -> Self {
        Self {
            gain_db: 0.0,
            peak: 0.0,
            holding: true,
        }
    }

    /// Returns to 0 dB and forgets the peak envelope.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Current gain in dB.
    pub fn gain_db(&self) -> f32 {
        self.gain_db
    }

    /// Whether the gain is being held (silence, below the floor, or no new samples).
    pub fn is_holding(&self) -> bool {
        self.holding
    }

    /// Linear factor applied to the displayed points.
    pub fn factor(&self) -> f32 {
        10.0f32.powf(self.gain_db / 20.0)
    }

    /// Feeds mid/side points captured during `dt` seconds.
    pub fn update(&mut self, points: &[(f32, f32)], dt: f32) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        let dt = dt.min(MAX_UPDATE_SECONDS);
        let decay = (-dt / PEAK_RELEASE_SECONDS).exp();
        let block_peak = points
            .iter()
            .filter(|(mid, side)| mid.is_finite() && side.is_finite())
            .map(|(mid, side)| mid.abs().max(side.abs()))
            .fold(0.0f32, f32::max);

        let floor = 10.0f32.powf(GAIN_FLOOR_DBFS / 20.0);
        if block_peak < floor {
            // Do not wind up on silence or noise; let the envelope fall for the next signal.
            self.peak *= decay;
            self.holding = true;
            return;
        }

        self.peak = block_peak.max(self.peak * decay);
        self.holding = false;
        let target = (-20.0 * self.peak.log10() - GAIN_HEADROOM_DB).clamp(0.0, MAX_GAIN_DB);
        let delta = target - self.gain_db;
        if delta.abs() < GAIN_DEADBAND_DB {
            return;
        }
        let tau = if delta < 0.0 {
            GAIN_ATTACK_SECONDS
        } else {
            GAIN_RELEASE_SECONDS
        };
        let alpha = 1.0 - (-dt / tau).exp();
        self.gain_db += delta * alpha;
        if (target - self.gain_db).abs() < 0.05 {
            self.gain_db = target;
        }
    }
}

impl Default for AutoGain {
    fn default() -> Self {
        Self::new()
    }
}

/// Maps mid/side coordinates onto a centred square.
///
/// `mid` is the vertical axis (L + R, positive up) and `side` the horizontal axis
/// (L − R, positive right). Values outside ±1 are drawn on the border; use
/// [`is_clipped`] to flag them.
#[derive(Clone, Copy, Debug)]
pub struct ScopeGeometry {
    square: Rect,
}

impl ScopeGeometry {
    /// Fits the largest centred square inside `rect`.
    pub fn new(rect: Rect) -> Self {
        let side = rect.width().min(rect.height()).max(0.0);
        Self {
            square: Rect::from_center_size(rect.center(), vec2(side, side)),
        }
    }

    /// The square the figure is drawn in.
    pub fn square(&self) -> Rect {
        self.square
    }

    /// Position of a mid/side pair. Full scale (±1) lies on the border of the square.
    pub fn point(&self, mid: f32, side: f32) -> Pos2 {
        let half = self.square.width() * 0.5;
        let mid = unit(mid);
        let side = unit(side);
        pos2(
            self.square.center().x + side * half,
            self.square.center().y - mid * half,
        )
    }
}

/// Clamps to full scale, passing non-finite values through as the centre.
fn unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// Whether any point lies beyond full scale (0 dBFS on either axis).
pub fn is_clipped(points: &[(f32, f32)]) -> bool {
    points.iter().any(|&(mid, side)| {
        (mid.is_finite() && mid.abs() > 1.0) || (side.is_finite() && side.abs() > 1.0)
    })
}

/// Everything the Lissajous panel draws in one frame.
pub struct ScopeRender<'a> {
    /// Newest points, already scaled by the display gain.
    pub points: &'a [(f32, f32)],
    /// Fading history when the trail is on; `None` draws only `points`.
    pub trail: Option<&'a Trail>,
    /// Editor clock in seconds, used to age the trail.
    pub now: f64,
    /// Trail length in seconds (normally [`TRAIL_SECONDS`]).
    pub trail_seconds: f32,
    /// Whether automatic gain is selected.
    pub auto: bool,
    /// Gain in dB applied to `points` and stored in the trail.
    pub gain_db: f32,
    /// Whether the automatic gain is holding its last value.
    pub holding: bool,
    /// Mono input: no figure is drawn because a duplicated channel would suggest perfect
    /// correlation.
    pub mono: bool,
}

/// Draws the Lissajous panel in `rect`.
pub fn draw(painter: &Painter, rect: Rect, view: &ScopeRender<'_>, theme: &Theme) {
    painter.text(
        pos2(rect.left(), rect.top()),
        Align2::LEFT_TOP,
        "LISSAJOUS  M/S",
        FontId::proportional(11.0),
        theme.text_dim,
    );

    let body = Rect::from_min_max(pos2(rect.left(), rect.top() + TITLE_HEIGHT), rect.max);
    if body.width() < 24.0 || body.height() < 24.0 {
        return;
    }
    let geometry = ScopeGeometry::new(body);
    let square = geometry.square();
    let clipped = !view.mono && is_clipped(view.points);

    painter.rect_filled(square, 3.0, theme.panel);
    let grid = theme.text_dim.gamma_multiply(0.35);
    let centre = square.center();
    painter.line_segment(
        [
            pos2(square.left(), centre.y),
            pos2(square.right(), centre.y),
        ],
        Stroke::new(1.0_f32, grid),
    );
    painter.line_segment(
        [
            pos2(centre.x, square.top()),
            pos2(centre.x, square.bottom()),
        ],
        Stroke::new(1.0_f32, grid),
    );
    let border = if clipped { theme.over } else { theme.text_dim };
    painter.rect_stroke(
        square,
        3.0,
        Stroke::new(1.0_f32, border),
        StrokeKind::Inside,
    );

    // The right end of the title states the scale actually applied, or the clip state.
    if !view.mono {
        painter.text(
            pos2(rect.right(), rect.top()),
            Align2::RIGHT_TOP,
            scale_label(view.auto, view.gain_db, view.holding, clipped),
            FontId::proportional(10.0),
            if clipped { theme.over } else { theme.text_dim },
        );
    }

    if view.mono {
        painter.text(
            centre,
            Align2::CENTER_CENTER,
            "MONO",
            FontId::proportional(12.0),
            theme.text_dim,
        );
        painter.text(
            pos2(centre.x, centre.y + 15.0),
            Align2::CENTER_CENTER,
            "no stereo content",
            FontId::proportional(9.0),
            theme.text_dim.gamma_multiply(0.8),
        );
        return;
    }

    let colour = if clipped {
        Color32::from_rgb(0xf0, 0x8a, 0x3c)
    } else {
        theme.on_target
    };

    // The trail fades with time, so a paused source leaves a decaying figure rather than
    // stacking copies of the same samples.
    if let Some(trail) = view.trail {
        draw_trail(
            painter,
            &geometry,
            trail,
            view.now,
            view.trail_seconds,
            colour,
        );
    }

    // Only finite points are drawn; a host passing a NaN must not break the trace.
    let visible: Vec<Pos2> = view
        .points
        .iter()
        .filter(|(mid, side)| mid.is_finite() && side.is_finite())
        .map(|&(mid, side)| geometry.point(mid, side))
        .collect();
    match visible.as_slice() {
        [] => {}
        [point] => {
            painter.circle_filled(*point, 1.5, colour);
        }
        _ => {
            painter.add(Shape::line(visible, Stroke::new(1.2_f32, colour)));
        }
    }
}

/// Draws the aged trail in a small number of opacity layers, oldest first.
fn draw_trail(
    painter: &Painter,
    geometry: &ScopeGeometry,
    trail: &Trail,
    now: f64,
    seconds: f32,
    colour: Color32,
) {
    const LAYERS: usize = 12;
    if seconds <= 0.0 {
        return;
    }
    let mut chunk: Vec<Pos2> = Vec::with_capacity(256);
    let mut layer = usize::MAX;
    let flush = |chunk: &mut Vec<Pos2>, layer: usize| {
        if chunk.is_empty() || layer == usize::MAX {
            return;
        }
        // Alpha of the middle of this layer; fresh points are near 0.7 and old ones near 0.
        let centre_age = (layer as f32 + 0.5) / LAYERS as f32 * seconds;
        let alpha = alpha_at(centre_age, seconds);
        let faded = Color32::from_rgba_unmultiplied(
            colour.r(),
            colour.g(),
            colour.b(),
            (alpha * 255.0).round().clamp(0.0, 255.0) as u8,
        );
        match chunk.as_slice() {
            [] => {}
            [point] => {
                painter.circle_filled(*point, 0.8, faded);
            }
            _ => {
                painter.add(Shape::line(
                    std::mem::take(chunk),
                    Stroke::new(1.0_f32, faded),
                ));
            }
        }
        chunk.clear();
    };
    for point in trail.iter() {
        if !point.mid.is_finite() || !point.side.is_finite() {
            continue;
        }
        let age = (now - point.time) as f32;
        if alpha_at(age, seconds) <= 0.0 {
            continue;
        }
        let t = (age / seconds).clamp(0.0, 1.0);
        // 0 is the newest layer (brightest), LAYERS - 1 the oldest.
        let this_layer = ((t * LAYERS as f32) as usize).min(LAYERS - 1);
        if this_layer != layer {
            flush(&mut chunk, layer);
            layer = this_layer;
        }
        chunk.push(geometry.point(point.mid, point.side));
    }
    flush(&mut chunk, layer);
}
