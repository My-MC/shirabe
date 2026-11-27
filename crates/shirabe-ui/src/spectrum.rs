//! FFT spectrum and 1/3-octave RTA tile.
//!
//! Display-only: the tile receives levels that the editor computed on the UI thread and draws
//! them. It never touches the audio callback or a measurement.
//!
//! The horizontal axis is logarithmic from 20 Hz to 20 kHz; the vertical axis is dBFS from 0 at
//! the top to -90 at the bottom. A full-scale sine at a bin centre reads 0 dBFS. The 1/3-octave
//! bars aggregate FFT bins; they are not class 1 or class 2 band-pass filters (see
//! `docs/standards.md`).

use shirabe_dsp::spectrum::{band_edges_hz, BAND_COUNT};

use crate::egui::{pos2, Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke};
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
    /// Interpolated FFT curve.
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
    /// Peak-hold trace in dBFS on the same bin grid.
    pub bin_peaks_db: &'a [f32],
    /// Sample rate of the analysed signal in Hz.
    pub sample_rate: f32,
    /// FFT length in samples.
    pub fft_size: usize,
    /// Per-band levels in dBFS, low to high.
    pub bands_db: &'a [f32; BAND_COUNT],
    /// Peak-hold ticks in dBFS on the same band grid.
    pub band_peaks_db: &'a [f32; BAND_COUNT],
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

    let plot_painter = painter.with_clip_rect(plot);
    match render.mode {
        Mode::Fft => draw_fft(&plot_painter, plot, render, theme),
        Mode::Rta => draw_rta(&plot_painter, plot, render, theme),
    }
}

fn draw_fft(painter: &Painter, plot: Rect, render: &SpectrumRender<'_>, theme: &Theme) {
    if render.bins_db.len() < 2 || render.fft_size == 0 || render.sample_rate <= 0.0 {
        return;
    }
    let columns = (plot.width() * painter.ctx().pixels_per_point()).ceil() as usize;
    let bin_hz = render.sample_rate / render.fft_size as f32;
    let points = fft_points(render.bins_db, bin_hz, plot, columns);
    let peaks = fft_points(render.bin_peaks_db, bin_hz, plot, columns);
    if points.len() < 2 {
        return;
    }

    let accent = theme.on_target;
    let fill_top = accent.gamma_multiply(0.39);
    let fill_bottom = accent.gamma_multiply(0.07);
    let mut fill = Mesh::default();
    fill.reserve_vertices(points.len() * 2);
    fill.reserve_triangles((points.len() - 1) * 2);
    for (i, point) in points.iter().copied().enumerate() {
        let v = (2 * i) as u32;
        fill.colored_vertex(point, fill_top);
        fill.colored_vertex(pos2(point.x, plot.bottom()), fill_bottom);
        if i > 0 {
            fill.add_triangle(v - 2, v - 1, v);
            fill.add_triangle(v, v - 1, v + 1);
        }
    }
    painter.add(Shape::mesh(fill));
    if peaks.len() > 1 {
        painter.add(Shape::line(
            peaks,
            Stroke::new(0.8_f32, theme.hold.gamma_multiply(0.36)),
        ));
    }
    painter.add(Shape::line(points, Stroke::new(1.6_f32, accent)));
}

fn fft_points(bins: &[f32], bin_hz: f32, plot: Rect, columns: usize) -> Vec<Pos2> {
    if bins.len() < 2 {
        return Vec::new();
    }
    let last_hz = MAX_HZ.min((bins.len() - 1) as f32 * bin_hz);
    if last_hz <= MIN_HZ {
        return Vec::new();
    }
    let nyquist_x = log_x(plot, last_hz);
    let mut points = Vec::with_capacity(columns + 2);
    let mut low = MIN_HZ;
    for column in 0..columns {
        let fraction = (column + 1) as f32 / columns as f32;
        let high = (MIN_HZ * (MAX_HZ / MIN_HZ).powf(fraction)).min(last_hz);
        let db = column_level(bins, low / bin_hz, high / bin_hz);
        let x = if high == last_hz {
            (log_x(plot, low) + nyquist_x) * 0.5
        } else {
            plot.left() + (column as f32 + 0.5) / columns as f32 * plot.width()
        };
        points.push(pos2(x, level_y(plot, db)));
        if high == last_hz {
            break;
        }
        low = high;
    }
    if let (Some(first), Some(last)) = (points.first().copied(), points.last().copied()) {
        points.insert(0, pos2(plot.left(), first.y));
        points.push(pos2(nyquist_x, last.y));
    }
    points
}

/// Bounds in fractional bins. When zoomed in, sample the line connecting the bin centres on
/// the log axis. When zoomed out, only take the max of bin centres inside this column.
fn column_level(bins: &[f32], low: f32, high: f32) -> f32 {
    let first = low.ceil() as usize;
    let last = (high.floor() as usize).min(bins.len() - 1);
    if high - low >= 1.0 && first <= last {
        return bins[first..=last].iter().copied().fold(FLOOR_DB, f32::max);
    }
    let centre = (low * high).sqrt().min((bins.len() - 1) as f32);
    let lower = centre.floor() as usize;
    let upper = (lower + 1).min(bins.len() - 1);
    let t = if lower == 0 || lower == upper {
        centre - lower as f32
    } else {
        (centre / lower as f32).ln() / (upper as f32 / lower as f32).ln()
    };
    let a = bins[lower].max(FLOOR_DB);
    let b = bins[upper].max(FLOOR_DB);
    a + (b - a) * t
}

fn draw_rta(painter: &Painter, plot: Rect, render: &SpectrumRender<'_>, theme: &Theme) {
    let hold = Stroke::new(1.0_f32, theme.hold.gamma_multiply(0.56));
    for (index, (&db, &peak)) in render.bands_db.iter().zip(render.band_peaks_db).enumerate() {
        let rect = band_rect(plot, index);
        let bar = Rect::from_min_max(pos2(rect.left(), level_y(plot, db)), rect.max);
        if bar.height() > 0.0 {
            painter.rect_filled(bar, 1.0, theme.zone_at(db).lit);
        }
        if peak > FLOOR_DB {
            let y = level_y(plot, peak);
            painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], hold);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractional_columns_follow_a_monotone_slope_between_bin_centres() {
        let bins = [-90.0, -90.0, -80.0, -20.0, -90.0];
        let samples: Vec<_> = (0..10)
            .map(|i| {
                let lo = 2.0 + i as f32 * 0.1;
                let hi = lo + 0.1;
                column_level(&bins, lo, hi)
            })
            .collect();
        assert!(samples.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(samples[0] > -80.0 && samples[9] < -20.0);
        assert!((column_level(&bins, 2.4, 2.5) - (-50.0)).abs() < 0.5);
    }

    #[test]
    fn a_wide_column_keeps_the_narrow_peak_without_bleeding_into_its_neighbour() {
        let bins = [-90.0, -90.0, -90.0, -3.0, -90.0, -90.0, -90.0];
        assert_eq!(column_level(&bins, 1.4, 3.4), -3.0);
        assert_eq!(column_level(&bins, 3.4, 5.4), -90.0);
    }
}
