//! FFT spectrum and 1/3-octave RTA tile.
//!
//! Display-only: the tile receives levels that the editor computed on the UI thread and draws
//! them. It never touches the audio callback or a measurement.
//!
//! The horizontal axis is logarithmic from 20 Hz to 20 kHz; the vertical axis is dBFS from 0 at
//! the top to -90 at the bottom. A full-scale sine at a bin centre reads 0 dBFS. The 1/3-octave
//! bars aggregate FFT bins; they are not class 1 or class 2 band-pass filters (see
//! `docs/standards.md`).

use shirabe_dsp::spectrum::{band_edges_hz, BAND_COUNT, NOMINAL_LABELS};

use crate::egui::{pos2, Align2, Color32, FontId, Painter, Rect, Stroke};
use crate::i18n::Text;
use crate::theme::Theme;

/// Space above the plot for the heading.
pub const TITLE_HEIGHT: f32 = 16.0;
/// Space left of the plot for the dB labels.
pub const AXIS_WIDTH: f32 = 34.0;
/// Space below the plot for the frequency labels.
pub const AXIS_HEIGHT: f32 = 14.0;
/// Narrowest plot that still reads as a graph.
pub const MIN_PLOT_WIDTH: f32 = 90.0;
/// Shortest plot that still reads as a graph.
pub const MIN_PLOT_HEIGHT: f32 = 48.0;
/// Lowest displayed level.
pub const FLOOR_DB: f32 = -90.0;
/// Highest displayed level (full scale).
pub const CEILING_DB: f32 = 0.0;
/// Lowest displayed frequency.
pub const MIN_HZ: f32 = 20.0;
/// Highest displayed frequency.
pub const MAX_HZ: f32 = 20_000.0;

/// Which spectrum display the tile shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// One line per FFT bin.
    Fft,
    /// One bar per 1/3-octave band.
    Rta,
}

/// Everything the tile draws in one frame.
pub struct SpectrumRender<'a> {
    /// FFT line or 1/3-octave bars.
    pub mode: Mode,
    /// Per-bin levels in dBFS (index 0 is DC).
    pub bins_db: &'a [f32],
    /// Sample rate of the analysed signal in Hz.
    pub sample_rate: f32,
    /// FFT length in samples.
    pub fft_size: usize,
    /// Per-band levels in dBFS, low to high.
    pub bands_db: &'a [f32; BAND_COUNT],
    /// Strongest bin as `(frequency_hz, level_db)`.
    pub peak: Option<(f32, f32)>,
    /// Whether a window has been analysed since the last reset.
    pub ready: bool,
    /// Interface language.
    pub text: Text,
}

/// The plot rectangle, or `None` when the tile is too small to read.
pub fn plot_rect(rect: Rect) -> Option<Rect> {
    let plot = Rect::from_min_max(
        pos2(rect.left() + AXIS_WIDTH + 4.0, rect.top() + TITLE_HEIGHT),
        pos2(rect.right() - 4.0, rect.bottom() - AXIS_HEIGHT),
    );
    (plot.width() >= MIN_PLOT_WIDTH && plot.height() >= MIN_PLOT_HEIGHT).then_some(plot)
}

/// Horizontal position of `hz` on the logarithmic axis.
pub fn log_x(plot: Rect, hz: f32) -> f32 {
    let fraction = (hz.max(MIN_HZ) / MIN_HZ).log10() / (MAX_HZ / MIN_HZ).log10();
    plot.left() + fraction.clamp(0.0, 1.0) * plot.width()
}

/// Vertical position of `db` on the level axis (0 dBFS at the top).
pub fn level_y(plot: Rect, db: f32) -> f32 {
    let fraction = (db.clamp(FLOOR_DB, CEILING_DB) - FLOOR_DB) / (CEILING_DB - FLOOR_DB);
    plot.bottom() - fraction * plot.height()
}

/// Rectangle of 1/3-octave band `index`, with a small gap to its neighbour.
pub fn band_rect(plot: Rect, index: usize) -> Rect {
    let (low, high) = band_edges_hz(index);
    let left = log_x(plot, low);
    let right = log_x(plot, high);
    let gap = ((right - left) * 0.12).min(0.75);
    Rect::from_min_max(
        pos2(left + gap, plot.top()),
        pos2((right - gap).max(left + gap + 0.5), plot.bottom()),
    )
}

/// Draws the tile in `rect`.
pub fn draw(painter: &Painter, rect: Rect, render: &SpectrumRender<'_>, theme: &Theme) {
    let title = match render.mode {
        Mode::Fft => {
            let bin_hz = if render.fft_size > 0 {
                render.sample_rate / render.fft_size as f32
            } else {
                0.0
            };
            format!(
                "{}  {}  {}  ·  {:.1} Hz",
                render.text.spectrum_title(),
                render.text.spectrum_mode_fft(),
                render.fft_size,
                bin_hz
            )
        }
        Mode::Rta => format!(
            "{}  {}",
            render.text.spectrum_title(),
            render.text.spectrum_mode_rta()
        ),
    };
    painter.text(
        pos2(rect.left(), rect.top()),
        Align2::LEFT_TOP,
        title,
        FontId::proportional(11.0),
        theme.text,
    );

    if let Some((hz, db)) = render.peak {
        let peak = if hz >= 1000.0 {
            format!("{:.2} kHz  {db:.1} dBFS", hz / 1000.0)
        } else {
            format!("{hz:.0} Hz  {db:.1} dBFS")
        };
        painter.text(
            pos2(rect.right(), rect.top()),
            Align2::RIGHT_TOP,
            peak,
            FontId::proportional(10.0),
            theme.text_dim,
        );
    }

    let Some(plot) = plot_rect(rect) else {
        return;
    };

    // Level grid and labels.
    let grid = Color32::from_rgba_unmultiplied(
        theme.text_dim.r(),
        theme.text_dim.g(),
        theme.text_dim.b(),
        38,
    );
    for db in [0.0f32, -20.0, -40.0, -60.0, -80.0] {
        let y = level_y(plot, db);
        painter.line_segment(
            [pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(1.0_f32, grid),
        );
        painter.text(
            pos2(plot.left() - 4.0, y),
            Align2::RIGHT_CENTER,
            format!("{db:.0}"),
            FontId::proportional(9.0),
            theme.text_dim,
        );
    }

    // Frequency grid and labels.
    for (hz, label) in [
        (20.0f32, "20"),
        (50.0, "50"),
        (100.0, "100"),
        (200.0, "200"),
        (500.0, "500"),
        (1000.0, "1k"),
        (2000.0, "2k"),
        (5000.0, "5k"),
        (10_000.0, "10k"),
        (20_000.0, "20k"),
    ] {
        let x = log_x(plot, hz);
        painter.line_segment(
            [pos2(x, plot.top()), pos2(x, plot.bottom())],
            Stroke::new(1.0_f32, grid),
        );
        painter.text(
            pos2(x, plot.bottom() + 2.0),
            Align2::CENTER_TOP,
            label,
            FontId::proportional(9.0),
            theme.text_dim,
        );
    }

    if !render.ready {
        painter.text(
            plot.center(),
            Align2::CENTER_CENTER,
            render.text.spectrum_waiting(),
            FontId::proportional(10.0),
            theme.text_dim,
        );
        return;
    }

    match render.mode {
        Mode::Fft => draw_fft(painter, plot, render),
        Mode::Rta => draw_rta(painter, plot, render, theme),
    }
}

fn draw_fft(painter: &Painter, plot: Rect, render: &SpectrumRender<'_>) {
    if render.bins_db.len() < 2 || render.fft_size == 0 || render.sample_rate <= 0.0 {
        return;
    }
    let width = plot.width().max(1.0) as usize;
    let bins = render.bins_db.len();
    let nyquist = render.sample_rate / 2.0;
    let displayed_hz = MAX_HZ.min(nyquist);
    let max_bin = (((displayed_hz / render.sample_rate) * render.fft_size as f32).floor() as usize)
        .min(bins - 1);
    let decades = MAX_HZ / MIN_HZ;
    // One point per pixel column over the logarithmic axis, keeping the strongest bin in the
    // column so a narrow peak between two columns stays visible.
    let mut points = Vec::with_capacity(width);
    for column in 0..width {
        let x = plot.left() + (column as f32 + 0.5) / width as f32 * plot.width();
        let low_hz = MIN_HZ * decades.powf(column as f32 / width as f32);
        if low_hz >= displayed_hz {
            points.push(pos2(x, plot.bottom()));
            continue;
        }
        let high_hz = MIN_HZ * decades.powf((column + 1) as f32 / width as f32);
        let first = ((low_hz / render.sample_rate) * render.fft_size as f32).floor() as usize;
        let last = (((high_hz / render.sample_rate) * render.fft_size as f32).ceil() as usize)
            .clamp(first + 1, max_bin + 1);
        let mut best = FLOOR_DB;
        for &db in &render.bins_db[first..last] {
            if db > best {
                best = db;
            }
        }
        points.push(pos2(x, level_y(plot, best.clamp(FLOOR_DB, CEILING_DB))));
    }
    let colour = Color32::from_rgb(0x4a, 0x9c, 0xd8);
    painter.add(crate::egui::Shape::line(
        points,
        Stroke::new(1.2_f32, colour),
    ));
}

fn draw_rta(painter: &Painter, plot: Rect, render: &SpectrumRender<'_>, theme: &Theme) {
    for (index, &db) in render.bands_db.iter().enumerate() {
        let rect = band_rect(plot, index);
        let top = level_y(plot, db.clamp(FLOOR_DB, CEILING_DB));
        let bar = Rect::from_min_max(pos2(rect.left(), top), rect.max);
        if bar.height() > 0.0 {
            painter.rect_filled(bar, 1.0, theme.zone_at(db).lit);
        }
        // Label a few bands so a narrow tile does not fill up with numbers.
        if matches!(index, 2 | 7 | 12 | 17 | 22 | 27) {
            painter.text(
                pos2(rect.center().x, plot.bottom() + 2.0),
                Align2::CENTER_TOP,
                NOMINAL_LABELS[index],
                FontId::proportional(8.0),
                theme.text_dim,
            );
        }
    }
}
