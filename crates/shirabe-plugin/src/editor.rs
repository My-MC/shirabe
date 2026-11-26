//! The plugin editor (also used by the standalone application).
//!
//! Layout: a header, then three meter sections side by side (sample / true peak, VU or PPM,
//! loudness), a numeric loudness panel, and the settings and test oscillator at the bottom.
//! Scales and read-outs follow the documents listed in `docs/standards.md`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, resizable_window::ResizableWindow, widgets};
use shirabe_dsp::MINUS_INFINITY_DB;
use shirabe_ui::egui::{
    self, pos2, vec2, Align, Align2, Color32, FontId, Layout, Painter, Rect, RichText, Sense,
    Stroke,
};
use shirabe_ui::{
    draw_level_bar, draw_marker, draw_scale_labels, format_db, format_lufs, MeterScale, Theme, Zone,
};

use crate::params::{
    LevelMode, LoudnessScale, LoudnessTarget, LoudnessUnit, ShirabeParams, TargetSpec,
};
use crate::shared::SharedMeters;

const SCALE_WIDTH: f32 = 30.0;
const CAPTION_HEIGHT: f32 = 16.0;
const READOUT_HEIGHT: f32 = 30.0;
const TITLE_HEIGHT: f32 = 16.0;
const MAX_BAR_WIDTH: f32 = 34.0;
const BAR_GAP: f32 = 5.0;
const CONTROLS_HEIGHT: f32 = 92.0;
/// Extra height of the custom-target row.
const CUSTOM_ROW_HEIGHT: f32 = 24.0;
const SECTION_GAP: f32 = 14.0;
const LOUDNESS_PANEL_WIDTH: f32 = 160.0;

/// Alignment level in dBFS (EBU R 68): TEST on the EBU and IEEE scales, mark 4 on the BBC scale.
const ALIGNMENT_DBFS: f32 = -18.0;
/// Permitted maximum level in dBFS: 0 dB on the Type I scale, +9 on the EBU scale.
const PERMITTED_MAX_DBFS: f32 = -9.0;
/// EBU Tech 3341 §2.4: LRA is flagged as not yet stable during the first 60 s.
const LRA_STABLE_AFTER_SECONDS: f32 = 60.0;

pub(crate) fn create(
    params: Arc<ShirabeParams>,
    shared: Arc<SharedMeters>,
) -> Option<Box<dyn Editor>> {
    let egui_state = params.editor_state.clone();
    create_egui_editor(
        params.editor_state.clone(),
        Theme::dark(),
        |ctx, theme| {
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = theme.background;
            visuals.window_fill = theme.background;
            ctx.set_visuals(visuals);
        },
        move |ctx, setter, theme| {
            // Meters move continuously, so redraw every frame while the editor is open.
            ctx.request_repaint();

            ResizableWindow::new("shirabe-main")
                .min_size(vec2(640.0, 380.0))
                .show(ctx, egui_state.as_ref(), |ui| {
                    egui::Frame::default()
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| draw(ui, &params, &shared, setter, theme));
                });
        },
    )
}

/// Draws the whole meter view (header, meters and settings). Used by the plugin editor and by
/// the standalone application.
pub fn draw(
    ui: &mut egui::Ui,
    params: &ShirabeParams,
    shared: &SharedMeters,
    setter: &ParamSetter,
    theme: &Theme,
) {
    header(ui, shared, theme);
    ui.separator();

    let controls = if params.loudness_target.value() == LoudnessTarget::Custom {
        CONTROLS_HEIGHT + CUSTOM_ROW_HEIGHT
    } else {
        CONTROLS_HEIGHT
    };
    let height = (ui.available_height() - controls).max(120.0);
    meters(ui, params, shared, theme, height);

    ui.separator();
    settings(ui, params, setter);
    oscillator_controls(ui, params, setter, theme);
}

fn header(ui: &mut egui::Ui, shared: &SharedMeters, theme: &Theme) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Shirabe")
                .strong()
                .size(18.0)
                .color(theme.text),
        );

        let layout = shared.layout().map_or_else(
            || format!("{} ch", shared.active_channels()),
            |l| l.name().to_owned(),
        );
        let rate = shared.sample_rate();
        let rate = if rate > 0.0 {
            format!("{:.1} kHz", rate / 1000.0)
        } else {
            "-".to_owned()
        };
        ui.label(RichText::new(format!("{layout}  ·  {rate}")).color(theme.text_dim));

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .button("Reset")
                .on_hover_text("Reset integrated loudness, loudness range and maxima together")
                .clicked()
            {
                shared.request_reset();
            }
            let running = shared.integrating();
            let label = if running { "Pause" } else { "Continue" };
            if ui
                .button(label)
                .on_hover_text("Pause or continue integrated loudness and loudness range together")
                .clicked()
            {
                shared.set_integrating(!running);
            }
        });
    });
}

/// Geometry of a group of vertical bars with a scale on the left.
struct BarGroup {
    rect: Rect,
    bars_top: f32,
    bars_bottom: f32,
    left: f32,
    bar_width: f32,
    count: usize,
}

impl BarGroup {
    fn new(rect: Rect, count: usize) -> Self {
        let count = count.max(1);
        let n = count as f32;
        let bars_top = rect.top() + TITLE_HEIGHT + READOUT_HEIGHT;
        let bars_bottom = rect.bottom() - CAPTION_HEIGHT;
        let usable = (rect.width() - SCALE_WIDTH).max(0.0);
        let bar_width = ((usable - BAR_GAP * n) / n).clamp(5.0, MAX_BAR_WIDTH);
        let total = n * (bar_width + BAR_GAP);
        let left = rect.left() + SCALE_WIDTH + ((usable - total) * 0.5).max(0.0);
        Self {
            rect,
            bars_top,
            bars_bottom,
            left,
            bar_width,
            count,
        }
    }

    fn bar(&self, index: usize) -> Rect {
        let x = self.left + index as f32 * (self.bar_width + BAR_GAP);
        Rect::from_min_max(
            pos2(x, self.bars_top),
            pos2(x + self.bar_width, self.bars_bottom),
        )
    }

    fn draw_frame(
        &self,
        painter: &Painter,
        title: &str,
        scale: &MeterScale,
        theme: &Theme,
        label_offset: f32,
    ) {
        painter.text(
            pos2(self.rect.left(), self.rect.top()),
            Align2::LEFT_TOP,
            title,
            FontId::proportional(12.0),
            theme.text_dim,
        );
        draw_scale_labels(
            painter,
            Rect::from_min_max(
                pos2(self.left - SCALE_WIDTH, self.bars_top),
                pos2(self.left - 2.0, self.bars_bottom),
            ),
            scale,
            theme,
            label_offset,
        );
    }

    /// Draws a secondary set of labels to the right of the last bar (for example the
    /// percentage scale of a VU meter). `marks` are `(scale value, label)`.
    fn draw_right_labels(
        &self,
        painter: &Painter,
        scale: &MeterScale,
        marks: &[(f32, &str)],
        theme: &Theme,
    ) {
        let x = self.bar(self.count - 1).right() + 3.0;
        let inner_top = self.bars_top + 1.0;
        let inner_bottom = self.bars_bottom - 1.0;
        for &(value, text) in marks {
            let y = inner_bottom - scale.position(value) * (inner_bottom - inner_top);
            painter.line_segment(
                [pos2(x - 3.0, y), pos2(x - 1.0, y)],
                Stroke::new(1.0_f32, theme.text_dim),
            );
            painter.text(
                pos2(x, y),
                Align2::LEFT_CENTER,
                text,
                FontId::monospace(9.0),
                theme.text_dim,
            );
        }
    }

    /// Draws up to two read-out lines above a bar and a caption below it.
    fn draw_texts(
        &self,
        painter: &Painter,
        bar: Rect,
        lines: [(String, Color32); 2],
        caption: &str,
        theme: &Theme,
    ) {
        let font = FontId::monospace(10.5);
        let top = self.rect.top() + TITLE_HEIGHT;
        for (i, (text, colour)) in lines.into_iter().enumerate() {
            if text.is_empty() {
                continue;
            }
            painter.text(
                pos2(bar.center().x, top + 7.0 + i as f32 * 12.0),
                Align2::CENTER_CENTER,
                text,
                font.clone(),
                colour,
            );
        }
        painter.text(
            pos2(bar.center().x, self.rect.bottom() - CAPTION_HEIGHT * 0.5),
            Align2::CENTER_CENTER,
            caption,
            FontId::proportional(12.0),
            theme.text_dim,
        );
    }
}

fn meters(
    ui: &mut egui::Ui,
    params: &ShirabeParams,
    shared: &SharedMeters,
    theme: &Theme,
    height: f32,
) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    if response
        .on_hover_text("Double-click to reset integrated loudness, loudness range and maxima")
        .double_clicked()
    {
        shared.request_reset();
    }
    let painter = ui.painter_at(rect);
    let channels = shared
        .active_channels()
        .clamp(1, crate::shared::MAX_CHANNELS);
    let target = params.target_spec();

    // The numeric loudness panel has a fixed width; the bar sections share the rest.
    let meters_width = (rect.width() - LOUDNESS_PANEL_WIDTH - 3.0 * SECTION_GAP).max(0.0);
    let peak_width = meters_width * 0.38;
    let level_width = meters_width * 0.37;
    let lufs_width = meters_width - peak_width - level_width;

    let mut x = rect.left();
    let mut take = |w: f32| {
        let r = Rect::from_min_max(pos2(x, rect.top()), pos2(x + w, rect.bottom()));
        x += w + SECTION_GAP;
        r
    };
    let peak_rect = take(peak_width);
    let level_rect = take(level_width);
    let lufs_rect = take(lufs_width);
    let panel_rect = take(LOUDNESS_PANEL_WIDTH);

    peak_section(&painter, peak_rect, shared, theme, channels, target);
    level_section(
        &painter,
        level_rect,
        shared,
        theme,
        channels,
        params.level_mode.value(),
    );
    let view = LoudnessView::new(params);
    loudness_section(&painter, lufs_rect, shared, theme, &view);
    loudness_panel(&painter, panel_rect, shared, theme, &view, channels);
}

fn caption(shared: &SharedMeters, index: usize) -> String {
    shared
        .layout()
        .and_then(|l| l.roles().get(index))
        .map_or_else(|| format!("{}", index + 1), |r| r.label().to_owned())
}

fn peak_section(
    painter: &Painter,
    rect: Rect,
    shared: &SharedMeters,
    theme: &Theme,
    channels: usize,
    target: TargetSpec,
) {
    let scale = MeterScale::PEAK_DBFS;
    let group = BarGroup::new(rect, channels);
    group.draw_frame(painter, "PEAK dBFS / TP dBTP", &scale, theme, 0.0);
    let ceiling = target.true_peak_ceiling;
    for index in 0..channels {
        let bar = group.bar(index);
        let r = shared.channel(index);
        let level = r.level_db.load(Ordering::Relaxed);
        let hold = r.hold_db.load(Ordering::Relaxed);
        let max = r.max_db.load(Ordering::Relaxed);
        let tp = r.true_peak_max_db.load(Ordering::Relaxed);

        draw_level_bar(painter, bar, &scale, theme, &theme.zones, level, hold);
        draw_marker(painter, bar, &scale, ceiling, theme.text_dim);

        let max_colour = if max >= 0.0 { theme.over } else { theme.text };
        let tp_colour = if tp > ceiling { theme.over } else { theme.text };
        group.draw_texts(
            painter,
            bar,
            [(format_db(max), max_colour), (format_db(tp), tp_colour)],
            &caption(shared, index),
            theme,
        );
    }
}

/// How the level section shows the selected meter.
struct LevelView {
    scale: MeterScale,
    title: &'static str,
    /// Added to the reading in dBFS to get the scale value (unused for VU).
    offset: f32,
    /// Scale value from which the bar is drawn red.
    over_from: f32,
}

impl LevelView {
    fn new(mode: LevelMode) -> Self {
        match mode {
            LevelMode::Vu => Self {
                scale: MeterScale::VU,
                title: "VU",
                offset: 0.0,
                over_from: 0.0,
            },
            LevelMode::PpmTypeI => Self {
                scale: MeterScale::PPM_TYPE_I,
                title: "PPM Type I  dB",
                offset: -PERMITTED_MAX_DBFS,
                over_from: 0.0,
            },
            LevelMode::PpmTypeII => Self {
                scale: MeterScale::PPM_EBU,
                title: "PPM EBU  dB",
                offset: -ALIGNMENT_DBFS,
                over_from: PERMITTED_MAX_DBFS - ALIGNMENT_DBFS,
            },
            LevelMode::PpmTypeIIBbc => Self {
                scale: MeterScale::PPM_BBC,
                title: "PPM BBC",
                offset: -ALIGNMENT_DBFS,
                // Mark 6 is the permitted maximum level (+8 dB above mark 4).
                over_from: 8.0,
            },
            LevelMode::PpmIeee => Self {
                scale: MeterScale::PPM_IEEE,
                title: "PPM IEEE  dB",
                // TEST (the alignment level) is printed at -8 on the IEEE scale.
                offset: -ALIGNMENT_DBFS - 8.0,
                // Fig 5 draws the range from 0 to +4 as a heavy band.
                over_from: 0.0,
            },
        }
    }

    /// Text read-out of a scale value.
    fn readout(&self, mode: LevelMode, value: f32) -> String {
        if value <= self.scale.min {
            return "-inf".to_owned();
        }
        match mode {
            // The BBC scale has no dB numbers; show the mark position with one decimal.
            LevelMode::PpmTypeIIBbc => format!("{:.1}", bbc_mark(value)),
            _ => format_db(value),
        }
    }
}

/// BBC scale mark (1 to 7) for a level in dB relative to the alignment level (mark 4).
fn bbc_mark(db: f32) -> f32 {
    if db >= -8.0 {
        4.0 + db / 4.0
    } else {
        // 6 dB between marks 1 and 2.
        2.0 + (db + 8.0) / 6.0
    }
}

fn level_section(
    painter: &Painter,
    rect: Rect,
    shared: &SharedMeters,
    theme: &Theme,
    channels: usize,
    mode: LevelMode,
) {
    let view = LevelView::new(mode);
    let zones = [
        Zone {
            from_db: f32::NEG_INFINITY,
            ..theme.vu_zones[0]
        },
        Zone {
            from_db: view.over_from,
            ..theme.vu_zones[theme.vu_zones.len() - 1]
        },
    ];
    let group = BarGroup::new(rect, channels);
    group.draw_frame(painter, view.title, &view.scale, theme, 0.0);
    if mode == LevelMode::Vu {
        // Percentage scale: 100 % coincides with 0 VU, the scale is linear in voltage.
        group.draw_right_labels(
            painter,
            &view.scale,
            &[
                (0.0, "100"),
                (20.0 * 0.8f32.log10(), "80"),
                (20.0 * 0.6f32.log10(), "60"),
                (20.0 * 0.4f32.log10(), "40"),
                (20.0 * 0.2f32.log10(), "20"),
            ],
            theme,
        );
    }
    for index in 0..channels {
        let bar = group.bar(index);
        let r = shared.channel(index);
        let value = match mode {
            LevelMode::Vu => r.vu.load(Ordering::Relaxed),
            _ => {
                let db = r.ppm_db.load(Ordering::Relaxed);
                if db <= MINUS_INFINITY_DB {
                    MINUS_INFINITY_DB
                } else {
                    db + view.offset
                }
            }
        };
        draw_level_bar(
            painter,
            bar,
            &view.scale,
            theme,
            &zones,
            value,
            MINUS_INFINITY_DB,
        );
        let colour = if value >= view.over_from {
            theme.over
        } else {
            theme.text
        };
        group.draw_texts(
            painter,
            bar,
            [(view.readout(mode, value), colour), (String::new(), colour)],
            &caption(shared, index),
            theme,
        );
    }
}

/// Loudness display settings resolved from the parameters.
struct LoudnessView {
    target: TargetSpec,
    scale: MeterScale,
    scale_name: &'static str,
    relative: bool,
}

impl LoudnessView {
    fn new(params: &ShirabeParams) -> Self {
        let (scale, scale_name) = match params.loudness_scale.value() {
            LoudnessScale::Ebu9 => (MeterScale::LOUDNESS_EBU_9, "+9"),
            LoudnessScale::Ebu18 => (MeterScale::LOUDNESS_EBU_18, "+18"),
        };
        Self {
            target: params.target_spec(),
            scale,
            scale_name,
            relative: params.loudness_unit.value() == LoudnessUnit::Relative,
        }
    }

    /// Unit shown next to every loudness value (EBU Tech 3341 §2.8).
    fn unit(&self) -> &'static str {
        if self.relative {
            "LU"
        } else {
            self.target.unit()
        }
    }

    /// Value to display for an absolute loudness.
    fn display(&self, lufs: Option<f32>) -> Option<f32> {
        lufs.map(|v| {
            if self.relative && v.is_finite() {
                v - self.target.loudness
            } else {
                v
            }
        })
    }

    /// Position on the scale (always relative to the target, so the target mark stays put).
    fn scale_value(&self, lufs: Option<f32>) -> f32 {
        lufs.filter(|v| v.is_finite())
            .map_or(MINUS_INFINITY_DB, |v| v - self.target.loudness)
    }

    fn colour(&self, theme: &Theme, lufs: Option<f32>) -> Color32 {
        let t = self.target.loudness;
        match (lufs, self.target.tolerance) {
            (Some(v), Some(tol)) if v > t + tol => theme.over,
            (Some(v), Some(tol)) if v >= t - tol => theme.on_target,
            _ => theme.text,
        }
    }

    fn zones(&self, theme: &Theme) -> [Zone; 3] {
        let tol = self.target.tolerance.unwrap_or(0.0);
        [
            Zone {
                from_db: f32::NEG_INFINITY,
                lit: Color32::from_rgb(0x4a, 0x9c, 0xd8),
                unlit: Color32::from_rgb(0x16, 0x26, 0x34),
            },
            Zone {
                from_db: -tol,
                lit: theme.on_target,
                unlit: Color32::from_rgb(0x14, 0x30, 0x1c),
            },
            Zone {
                from_db: tol.max(0.05),
                lit: theme.over,
                unlit: Color32::from_rgb(0x36, 0x16, 0x14),
            },
        ]
    }

    /// Name of the operating mode shown with the loudness section.
    fn mode_name(&self) -> &'static str {
        match self.target.preset {
            LoudnessTarget::EbuR128 => "EBU Mode",
            LoudnessTarget::AribTrB32 => "TR-B32",
            LoudnessTarget::AtscA85 => "A/85",
            LoudnessTarget::Streaming14 => "Spotify",
            LoudnessTarget::ApplePodcasts => "Apple Podcasts",
            LoudnessTarget::Custom => "Custom",
        }
    }
}

fn loudness_section(
    painter: &Painter,
    rect: Rect,
    shared: &SharedMeters,
    theme: &Theme,
    view: &LoudnessView,
) {
    let group = BarGroup::new(rect, 2);
    let label_offset = if view.relative {
        0.0
    } else {
        view.target.loudness
    };
    let title = format!("{} {}  {}", view.mode_name(), view.scale_name, view.unit());
    group.draw_frame(painter, &title, &view.scale, theme, label_offset);
    let zones = view.zones(theme);
    let l = &shared.loudness;
    let bars = [
        ("M", l.momentary.load(), l.max_momentary.load()),
        ("S", l.short_term.load(), l.max_short_term.load()),
    ];
    for (index, (name, value, max)) in bars.into_iter().enumerate() {
        let bar = group.bar(index);
        draw_level_bar(
            painter,
            bar,
            &view.scale,
            theme,
            &zones,
            view.scale_value(value),
            view.scale_value(max),
        );
        draw_marker(painter, bar, &view.scale, 0.0, theme.text);
        group.draw_texts(
            painter,
            bar,
            [
                (format_lufs(view.display(value)), view.colour(theme, value)),
                (view.unit().to_owned(), theme.text_dim),
            ],
            name,
            theme,
        );
    }
}

fn loudness_panel(
    painter: &Painter,
    rect: Rect,
    shared: &SharedMeters,
    theme: &Theme,
    view: &LoudnessView,
    channels: usize,
) {
    painter.rect_filled(rect, 4.0, theme.panel);
    let l = &shared.loudness;
    let integrated = l.integrated.load();
    let tp_max = (0..channels)
        .map(|i| shared.channel(i).true_peak_max_db.load(Ordering::Relaxed))
        .fold(MINUS_INFINITY_DB, f32::max);
    let seconds = shared.programme_seconds.load(Ordering::Relaxed);
    let running = shared.integrating();
    let unit = view.unit();

    let left = rect.left() + 10.0;
    let right = rect.right() - 10.0;
    let mut y = rect.top() + 10.0;

    let state = if running { "" } else { "  PAUSED" };
    painter.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        format!("INTEGRATED{state}"),
        FontId::proportional(11.0),
        if running { theme.text_dim } else { theme.over },
    );
    y += 16.0;
    painter.text(
        pos2(right, y),
        Align2::RIGHT_TOP,
        format_lufs(view.display(integrated)),
        FontId::monospace(30.0),
        view.colour(theme, integrated),
    );
    y += 34.0;
    painter.text(
        pos2(right, y),
        Align2::RIGHT_TOP,
        unit,
        FontId::proportional(11.0),
        theme.text_dim,
    );
    y += 20.0;

    let lra = l.range.load();
    let lra_unstable = lra.is_some() && seconds < LRA_STABLE_AFTER_SECONDS;
    let lra_text = if lra_unstable {
        // Not yet stable (EBU Tech 3341 §2.4).
        format!("{} LU *", format_lufs(lra))
    } else {
        format!("{} LU", format_lufs(lra))
    };
    let lra_colour = if lra_unstable {
        theme.text_dim
    } else {
        theme.text
    };
    let ceiling = view.target.true_peak_ceiling;
    let rows: [(&str, String, Color32); 6] = [
        ("LRA", lra_text, lra_colour),
        (
            "M max",
            format!(
                "{} {unit}",
                format_lufs(view.display(l.max_momentary.load()))
            ),
            view.colour(theme, l.max_momentary.load()),
        ),
        (
            "S max",
            format!(
                "{} {unit}",
                format_lufs(view.display(l.max_short_term.load()))
            ),
            view.colour(theme, l.max_short_term.load()),
        ),
        (
            "TP max",
            format!("{} dBTP", format_db(tp_max)),
            if tp_max > ceiling {
                theme.over
            } else {
                theme.text
            },
        ),
        (
            "Time",
            format!("{}:{:02}", (seconds / 60.0) as u32, (seconds % 60.0) as u32),
            theme.text_dim,
        ),
        (
            "Target",
            match view.target.tolerance {
                Some(tol) => format!(
                    "{} ±{} / {}",
                    short_number(view.target.loudness),
                    short_number(tol),
                    short_number(ceiling)
                ),
                None => format!(
                    "{} / {}",
                    short_number(view.target.loudness),
                    short_number(ceiling)
                ),
            },
            theme.text_dim,
        ),
    ];
    for (name, value, colour) in rows {
        if y + 16.0 > rect.bottom() {
            break;
        }
        painter.text(
            pos2(left, y),
            Align2::LEFT_TOP,
            name,
            FontId::proportional(12.0),
            theme.text_dim,
        );
        painter.text(
            pos2(right, y),
            Align2::RIGHT_TOP,
            value,
            FontId::monospace(12.0),
            colour,
        );
        y += 19.0;
    }
    if lra_unstable && y + 14.0 <= rect.bottom() {
        painter.text(
            pos2(left, y),
            Align2::LEFT_TOP,
            "* LRA not yet stable (< 60 s)",
            FontId::proportional(10.0),
            theme.text_dim,
        );
    }
}

fn combo<E: Enum + PartialEq + Copy + 'static>(
    ui: &mut egui::Ui,
    id: &str,
    param: &EnumParam<E>,
    setter: &ParamSetter,
    options: &[E],
    name: fn(E) -> &'static str,
) {
    let current = param.value();
    egui::ComboBox::from_id_salt(id)
        .selected_text(name(current))
        .show_ui(ui, |ui| {
            for &option in options {
                if ui
                    .selectable_label(option == current, name(option))
                    .clicked()
                {
                    setter.begin_set_parameter(param);
                    setter.set_parameter(param, option);
                    setter.end_set_parameter(param);
                }
            }
        });
}

fn settings(ui: &mut egui::Ui, params: &ShirabeParams, setter: &ParamSetter) {
    ui.horizontal(|ui| {
        ui.label("Target");
        combo(
            ui,
            "loudness-target",
            &params.loudness_target,
            setter,
            &[
                LoudnessTarget::AribTrB32,
                LoudnessTarget::EbuR128,
                LoudnessTarget::AtscA85,
                LoudnessTarget::Streaming14,
                LoudnessTarget::ApplePodcasts,
                LoudnessTarget::Custom,
            ],
            target_name,
        );
        combo(
            ui,
            "loudness-scale",
            &params.loudness_scale,
            setter,
            &[LoudnessScale::Ebu9, LoudnessScale::Ebu18],
            |s| match s {
                LoudnessScale::Ebu9 => "+9 scale",
                LoudnessScale::Ebu18 => "+18 scale",
            },
        );
        combo(
            ui,
            "loudness-unit",
            &params.loudness_unit,
            setter,
            &[LoudnessUnit::Absolute, LoudnessUnit::Relative],
            |u| match u {
                LoudnessUnit::Absolute => "Absolute",
                LoudnessUnit::Relative => "Relative (LU)",
            },
        );

        let mut auto = params.auto_reset.value();
        if ui
            .checkbox(&mut auto, "Reset on play")
            .on_hover_text("Reset the programme measurement when the host starts playback")
            .changed()
        {
            setter.begin_set_parameter(&params.auto_reset);
            setter.set_parameter(&params.auto_reset, auto);
            setter.end_set_parameter(&params.auto_reset);
        }
    });
    if params.loudness_target.value() == LoudnessTarget::Custom {
        ui.horizontal(|ui| {
            ui.label("Custom");
            ui.add(
                widgets::ParamSlider::for_param(&params.custom_loudness, setter).with_width(110.0),
            )
            .on_hover_text("Target integrated loudness");
            ui.label("±");
            ui.add(
                widgets::ParamSlider::for_param(&params.custom_tolerance, setter).with_width(90.0),
            )
            .on_hover_text("Tolerance around the target (none = no tolerance)");
            ui.label("TP ≤");
            ui.add(
                widgets::ParamSlider::for_param(&params.custom_true_peak, setter).with_width(100.0),
            )
            .on_hover_text("True-peak ceiling");
        });
    }
    ui.horizontal(|ui| {
        ui.label("Level");
        combo(
            ui,
            "level-mode",
            &params.level_mode,
            setter,
            &[
                LevelMode::Vu,
                LevelMode::PpmTypeI,
                LevelMode::PpmTypeII,
                LevelMode::PpmTypeIIBbc,
                LevelMode::PpmIeee,
            ],
            level_mode_name,
        );
        if params.level_mode.value() == LevelMode::Vu {
            ui.label("0 VU =");
            ui.add(widgets::ParamSlider::for_param(&params.vu_reference, setter).with_width(110.0));
        }
    });
}

fn level_mode_name(m: LevelMode) -> &'static str {
    match m {
        LevelMode::Vu => "VU",
        LevelMode::PpmTypeI => "PPM Type I (0 dB = -9 dBFS)",
        LevelMode::PpmTypeII => "PPM EBU (TEST = -18 dBFS)",
        LevelMode::PpmTypeIIBbc => "PPM BBC (4 = -18 dBFS)",
        LevelMode::PpmIeee => "PPM IEEE (TEST -8 = -18 dBFS)",
    }
}

fn target_name(t: LoudnessTarget) -> &'static str {
    match t {
        LoudnessTarget::AribTrB32 => "ARIB TR-B32  -24 LKFS",
        LoudnessTarget::EbuR128 => "EBU R 128  -23 LUFS",
        LoudnessTarget::AtscA85 => "ATSC A/85  -24 LKFS",
        LoudnessTarget::Streaming14 => "Spotify  -14 LUFS",
        LoudnessTarget::ApplePodcasts => "Apple Podcasts  -16 LKFS",
        LoudnessTarget::Custom => "Custom",
    }
}

fn oscillator_controls(
    ui: &mut egui::Ui,
    params: &ShirabeParams,
    setter: &ParamSetter,
    theme: &Theme,
) {
    ui.horizontal(|ui| {
        let mut enabled = params.osc_enabled.value();
        if ui.checkbox(&mut enabled, "Test oscillator").changed() {
            setter.begin_set_parameter(&params.osc_enabled);
            setter.set_parameter(&params.osc_enabled, enabled);
            setter.end_set_parameter(&params.osc_enabled);
        }
        ui.add(widgets::ParamSlider::for_param(&params.osc_level, setter).with_width(110.0));
        ui.add(widgets::ParamSlider::for_param(&params.osc_frequency, setter).with_width(110.0));
        if enabled {
            ui.label(
                RichText::new("Input replaced by test tone")
                    .color(Color32::from_rgb(0xf2, 0xc1, 0x2e)),
            );
        } else {
            let note = if setter.raw_context.plugin_api() == PluginApi::Standalone {
                "Measuring the input device"
            } else {
                "Pass-through, no latency"
            };
            ui.label(RichText::new(note).color(theme.text_dim));
        }
    });
}

/// Formats a number without a trailing `.0` (`-14`, `-14.5`).
fn short_number(v: f32) -> String {
    if (v - v.round()).abs() < 0.05 {
        format!("{:.0}", v)
    } else {
        format!("{:.1}", v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BBC marks: 4 dB apart, 6 dB between 1 and 2, mark 4 = alignment level.
    #[test]
    fn bbc_marks() {
        for (db, mark) in [
            (-14.0, 1.0),
            (-8.0, 2.0),
            (-4.0, 3.0),
            (0.0, 4.0),
            (4.0, 5.0),
            (8.0, 6.0),
            (12.0, 7.0),
        ] {
            assert!((bbc_mark(db) - mark).abs() < 1e-6, "{db}");
        }
    }
}
