//! Geometry and drawing of the spectrum tile.
//!
//! The tile is display-only: the axis mapping and the band bars are pure functions of the plot
//! rectangle, so they can be checked without a window.

use shirabe_dsp::spectrum::{BAND_COUNT, FFT_SIZE};
use shirabe_ui::egui::{self, pos2, vec2, LayerId, Rect};
use shirabe_ui::i18n::{Lang, Text};
use shirabe_ui::spectrum::{band_rect, draw, level_y, log_x, plot_rect, Mode, SpectrumRender};
use shirabe_ui::theme::Theme;

fn area(width: f32, height: f32) -> Rect {
    Rect::from_min_size(pos2(0.0, 0.0), vec2(width, height))
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn the_frequency_axis_is_logarithmic_from_20_hz_to_20_khz() {
    let plot = area(300.0, 100.0);
    assert!(close(log_x(plot, 20.0), plot.left()));
    assert!(close(log_x(plot, 20_000.0), plot.right()));
    // 1 kHz is 1.699 decades above 20 Hz out of three decades.
    let expected = plot.left() + plot.width() * (1000.0f32 / 20.0).log10() / 1000.0f32.log10();
    assert!(close(log_x(plot, 1000.0), expected));
    assert!(log_x(plot, 200.0) < log_x(plot, 2000.0));
    // Out-of-range frequencies stay on the axis instead of drawing outside the tile.
    assert!(close(log_x(plot, 5.0), plot.left()));
    assert!(close(log_x(plot, 40_000.0), plot.right()));
}

#[test]
fn the_level_axis_runs_from_zero_to_minus_ninety_dbfs() {
    let plot = area(300.0, 100.0);
    assert!(close(level_y(plot, 0.0), plot.top()));
    assert!(close(level_y(plot, -90.0), plot.bottom()));
    assert!(close(level_y(plot, -45.0), plot.center().y));
    // Levels above full scale and below the floor stay inside the plot.
    assert!(close(level_y(plot, 6.0), plot.top()));
    assert!(close(level_y(plot, -150.0), plot.bottom()));
}

#[test]
fn the_band_bars_are_ordered_and_inside_the_plot() {
    let plot = area(600.0, 120.0);
    let mut previous_right = plot.left();
    for index in 0..BAND_COUNT {
        let band = band_rect(plot, index);
        assert!(band.left() >= previous_right - 0.01, "band {index}");
        assert!(band.left() >= plot.left() - 0.01);
        assert!(band.right() <= plot.right() + 0.01);
        assert!(band.top() >= plot.top() - 0.01);
        assert!(band.bottom() <= plot.bottom() + 0.01);
        // Every band keeps a usable width, and constant-percentage bands get wider with
        // frequency (the top band is clamped at 20 kHz).
        assert!(band.width() > 1.0, "band {index}");
        previous_right = band.right();
    }
    assert!(band_rect(plot, 20).width() > band_rect(plot, 0).width() * 2.0);
    // 1 kHz is band 17 and sits right of centre.
    let centre = band_rect(plot, 17);
    assert!(centre.center().x > plot.center().x);
}

#[test]
fn the_plot_is_dropped_from_a_tile_too_small_to_read() {
    assert!(plot_rect(area(400.0, 140.0)).is_some());
    assert!(plot_rect(area(80.0, 140.0)).is_none());
    assert!(plot_rect(area(400.0, 40.0)).is_none());
}

fn render<'a>(mode: Mode, bins: &'a [f32], bands: &'a [f32; BAND_COUNT]) -> SpectrumRender<'a> {
    SpectrumRender {
        mode,
        bins_db: bins,
        bin_peaks_db: bins,
        sample_rate: 48_000.0,
        fft_size: FFT_SIZE,
        bands_db: bands,
        band_peaks_db: bands,
        peak: Some((1000.0, -12.0)),
        ready: true,
        text: Text::new(Lang::English, true),
    }
}

fn shape_text(rect: Rect, render: &SpectrumRender<'_>) -> String {
    let ctx = egui::Context::default();
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        },
        |ctx| {
            let painter = ctx.layer_painter(LayerId::background());
            draw(&painter, rect, render, &Theme::dark());
        },
    );
    output
        .shapes
        .iter()
        .map(|clipped| format!("{:?}", clipped.shape))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn both_modes_draw_a_titled_tile_with_a_grid() {
    let bins: Vec<f32> = (0..FFT_SIZE / 2 + 1)
        .map(|index| if index == 341 { -6.0 } else { -80.0 })
        .collect();
    let bands = [-40.0f32; BAND_COUNT];
    for mode in [Mode::Fft, Mode::Rta] {
        let text = shape_text(area(640.0, 160.0), &render(mode, &bins, &bands));
        assert!(text.contains("SPECTRUM"), "{mode:?}: no title");
        assert!(text.contains("dBFS"), "{mode:?}: no unit");
        // The frequency labels and the level grid are both present.
        assert!(text.contains("100"), "{mode:?}: no frequency label");
        assert!(text.contains("-60"), "{mode:?}: no level label");
    }
    // RTA names its bands, FFT names its resolution.
    let fft = shape_text(area(640.0, 160.0), &render(Mode::Fft, &bins, &bands));
    assert!(fft.contains("FFT"));
    let rta = shape_text(area(640.0, 160.0), &render(Mode::Rta, &bins, &bands));
    assert!(rta.contains("1/3"));
}

#[test]
fn a_compact_tile_draws_the_heading_and_no_crash() {
    let bins = [-90.0f32; 16];
    let bands = [-90.0f32; BAND_COUNT];
    let text = shape_text(area(120.0, 40.0), &render(Mode::Fft, &bins, &bands));
    assert!(text.contains("SPECTRUM"));
}

#[test]
fn an_empty_analysis_says_it_is_waiting_for_audio() {
    let bins = [-150.0f32; 16];
    let bands = [-150.0f32; BAND_COUNT];
    let mut render = render(Mode::Fft, &bins, &bands);
    render.ready = false;
    render.peak = None;
    let text = shape_text(area(640.0, 160.0), &render);
    assert!(text.contains("SPECTRUM"));
    assert!(text.to_lowercase().contains("waiting"));
}
