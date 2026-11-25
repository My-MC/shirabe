//! The plugin editor (also used by the standalone application).
//!
//! Layout: a header, then three meter sections side by side (sample/true peak, VU, loudness),
//! and the settings and test oscillator controls at the bottom.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, resizable_window::ResizableWindow, widgets};
use shirabe_ui::egui::{
    self, pos2, vec2, Align, Align2, Color32, FontId, Layout, Painter, Rect, RichText, Sense,
};
use shirabe_ui::{
    draw_level_bar, draw_marker, draw_scale_labels, format_db, format_lufs, MeterScale, Theme, Zone,
};

use shirabe_dsp::MINUS_INFINITY_DB;

use crate::params::{LoudnessTarget, ShirabeParams};
use crate::shared::SharedMeters;

const SCALE_WIDTH: f32 = 26.0;
const CAPTION_HEIGHT: f32 = 16.0;
const READOUT_HEIGHT: f32 = 30.0;
const TITLE_HEIGHT: f32 = 16.0;
const MAX_BAR_WIDTH: f32 = 34.0;
const BAR_GAP: f32 = 5.0;
const CONTROLS_HEIGHT: f32 = 92.0;
const SECTION_GAP: f32 = 14.0;
const LOUDNESS_PANEL_WIDTH: f32 = 150.0;

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
                .min_size(vec2(600.0, 360.0))
                .show(ctx, egui_state.as_ref(), |ui| {
                    egui::Frame::default()
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            header(ui, &shared, theme);
                            ui.separator();

                            let height = (ui.available_height() - CONTROLS_HEIGHT).max(120.0);
                            meters(ui, &params, &shared, theme, height);

                            ui.separator();
                            settings(ui, &params, setter, theme);
                            oscillator_controls(ui, &params, setter, theme);
                        });
                });
        },
    )
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
                .on_hover_text("Clear maxima, integrated loudness and loudness range")
                .clicked()
            {
                shared.request_reset();
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
}

impl BarGroup {
    fn new(rect: Rect, count: usize) -> Self {
        let count = count.max(1) as f32;
        let bars_top = rect.top() + TITLE_HEIGHT + READOUT_HEIGHT;
        let bars_bottom = rect.bottom() - CAPTION_HEIGHT;
        let usable = (rect.width() - SCALE_WIDTH).max(0.0);
        let bar_width = ((usable - BAR_GAP * count) / count).clamp(5.0, MAX_BAR_WIDTH);
        let total = count * (bar_width + BAR_GAP);
        let left = rect.left() + SCALE_WIDTH + ((usable - total) * 0.5).max(0.0);
        Self {
            rect,
            bars_top,
            bars_bottom,
            left,
            bar_width,
        }
    }

    fn bar(&self, index: usize) -> Rect {
        let x = self.left + index as f32 * (self.bar_width + BAR_GAP);
        Rect::from_min_max(
            pos2(x, self.bars_top),
            pos2(x + self.bar_width, self.bars_bottom),
        )
    }

    fn draw_frame(&self, painter: &Painter, title: &str, scale: &MeterScale, theme: &Theme) {
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
        );
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
        .on_hover_text("Double-click to clear maxima and the programme loudness")
        .double_clicked()
    {
        shared.request_reset();
    }
    let painter = ui.painter_at(rect);
    let channels = shared
        .active_channels()
        .clamp(1, crate::shared::MAX_CHANNELS);
    let target = params.loudness_target.value();

    // Peak gets the most room, the loudness section has a fixed-width numeric panel.
    let meters_width = (rect.width() - LOUDNESS_PANEL_WIDTH - 3.0 * SECTION_GAP).max(0.0);
    let peak_width = meters_width * 0.42;
    let vu_width = meters_width * 0.33;
    let lufs_width = meters_width - peak_width - vu_width;

    let mut x = rect.left();
    let mut take = |w: f32| {
        let r = Rect::from_min_max(pos2(x, rect.top()), pos2(x + w, rect.bottom()));
        x += w + SECTION_GAP;
        r
    };
    let peak_rect = take(peak_width);
    let vu_rect = take(vu_width);
    let lufs_rect = take(lufs_width);
    let panel_rect = take(LOUDNESS_PANEL_WIDTH);

    peak_section(&painter, peak_rect, shared, theme, channels, target);
    vu_section(&painter, vu_rect, shared, theme, channels);
    loudness_section(&painter, lufs_rect, shared, theme, target);
    loudness_panel(&painter, panel_rect, shared, theme, target, channels);
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
    target: LoudnessTarget,
) {
    let scale = MeterScale::PEAK_DBFS;
    let group = BarGroup::new(rect, channels);
    group.draw_frame(painter, "PEAK dBFS / TP dBTP", &scale, theme);
    let ceiling = target.true_peak_ceiling();
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

fn vu_section(
    painter: &Painter,
    rect: Rect,
    shared: &SharedMeters,
    theme: &Theme,
    channels: usize,
) {
    let scale = MeterScale::VU;
    let group = BarGroup::new(rect, channels);
    group.draw_frame(painter, "VU", &scale, theme);
    for index in 0..channels {
        let bar = group.bar(index);
        let vu = shared.channel(index).vu.load(Ordering::Relaxed);
        draw_level_bar(
            painter,
            bar,
            &scale,
            theme,
            &theme.vu_zones,
            vu,
            MINUS_INFINITY_DB,
        );
        let colour = if vu >= 0.0 { theme.over } else { theme.text };
        group.draw_texts(
            painter,
            bar,
            [(format_db(vu), colour), (String::new(), colour)],
            &caption(shared, index),
            theme,
        );
    }
}

fn loudness_zones(theme: &Theme, target: LoudnessTarget) -> [Zone; 3] {
    let t = target.loudness();
    let tol = target.tolerance();
    [
        Zone {
            from_db: f32::NEG_INFINITY,
            lit: Color32::from_rgb(0x4a, 0x9c, 0xd8),
            unlit: Color32::from_rgb(0x16, 0x26, 0x34),
        },
        Zone {
            from_db: t - tol,
            lit: theme.on_target,
            unlit: Color32::from_rgb(0x14, 0x30, 0x1c),
        },
        Zone {
            from_db: t + tol,
            lit: theme.over,
            unlit: Color32::from_rgb(0x36, 0x16, 0x14),
        },
    ]
}

fn loudness_section(
    painter: &Painter,
    rect: Rect,
    shared: &SharedMeters,
    theme: &Theme,
    target: LoudnessTarget,
) {
    let scale = MeterScale::LOUDNESS;
    let group = BarGroup::new(rect, 2);
    group.draw_frame(painter, "LUFS", &scale, theme);
    let zones = loudness_zones(theme, target);
    let l = &shared.loudness;
    let bars = [
        ("M", l.momentary.load(), l.max_momentary.load()),
        ("S", l.short_term.load(), l.max_short_term.load()),
    ];
    for (index, (name, value, max)) in bars.into_iter().enumerate() {
        let bar = group.bar(index);
        let level = value.filter(|v| v.is_finite()).unwrap_or(MINUS_INFINITY_DB);
        let max_db = max.filter(|v| v.is_finite()).unwrap_or(MINUS_INFINITY_DB);
        draw_level_bar(painter, bar, &scale, theme, &zones, level, max_db);
        draw_marker(painter, bar, &scale, target.loudness(), theme.text);
        group.draw_texts(
            painter,
            bar,
            [
                (format_lufs(value), loudness_colour(theme, target, value)),
                (String::new(), theme.text),
            ],
            name,
            theme,
        );
    }
}

fn loudness_colour(theme: &Theme, target: LoudnessTarget, value: Option<f32>) -> Color32 {
    match value {
        Some(v) if v > target.loudness() + target.tolerance() => theme.over,
        Some(v) if v >= target.loudness() - target.tolerance() => theme.on_target,
        _ => theme.text,
    }
}

fn loudness_panel(
    painter: &Painter,
    rect: Rect,
    shared: &SharedMeters,
    theme: &Theme,
    target: LoudnessTarget,
    channels: usize,
) {
    painter.rect_filled(rect, 4.0, theme.panel);
    let l = &shared.loudness;
    let integrated = l.integrated.load();
    let tp_max = (0..channels)
        .map(|i| shared.channel(i).true_peak_max_db.load(Ordering::Relaxed))
        .fold(MINUS_INFINITY_DB, f32::max);

    let left = rect.left() + 10.0;
    let right = rect.right() - 10.0;
    let mut y = rect.top() + 10.0;

    painter.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        "INTEGRATED",
        FontId::proportional(11.0),
        theme.text_dim,
    );
    y += 16.0;
    painter.text(
        pos2(right, y),
        Align2::RIGHT_TOP,
        format_lufs(integrated),
        FontId::monospace(30.0),
        loudness_colour(theme, target, integrated),
    );
    y += 34.0;
    painter.text(
        pos2(right, y),
        Align2::RIGHT_TOP,
        "LUFS",
        FontId::proportional(11.0),
        theme.text_dim,
    );
    y += 22.0;

    let rows: [(&str, String, Color32); 5] = [
        (
            "LRA",
            format!("{} LU", format_lufs(l.range.load())),
            theme.text,
        ),
        (
            "M max",
            format_lufs(l.max_momentary.load()),
            loudness_colour(theme, target, l.max_momentary.load()),
        ),
        (
            "S max",
            format_lufs(l.max_short_term.load()),
            loudness_colour(theme, target, l.max_short_term.load()),
        ),
        (
            "TP max",
            format_db(tp_max),
            if tp_max > target.true_peak_ceiling() {
                theme.over
            } else {
                theme.text
            },
        ),
        (
            "Target",
            format!("{:.0}", target.loudness()),
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
            FontId::monospace(13.0),
            colour,
        );
        y += 20.0;
    }
}

fn settings(ui: &mut egui::Ui, params: &ShirabeParams, setter: &ParamSetter, _theme: &Theme) {
    ui.horizontal(|ui| {
        ui.label("Target");
        let current = params.loudness_target.value();
        egui::ComboBox::from_id_salt("loudness-target")
            .selected_text(target_name(current))
            .show_ui(ui, |ui| {
                for t in [
                    LoudnessTarget::AribTrB32,
                    LoudnessTarget::EbuR128,
                    LoudnessTarget::AtscA85,
                    LoudnessTarget::Streaming14,
                ] {
                    if ui.selectable_label(t == current, target_name(t)).clicked() {
                        setter.begin_set_parameter(&params.loudness_target);
                        setter.set_parameter(&params.loudness_target, t);
                        setter.end_set_parameter(&params.loudness_target);
                    }
                }
            });
        ui.label("0 VU =");
        ui.add(widgets::ParamSlider::for_param(&params.vu_reference, setter).with_width(110.0));

        let mut auto = params.auto_reset.value();
        if ui
            .checkbox(&mut auto, "Reset on play")
            .on_hover_text("Clear the programme loudness when the host starts playback")
            .changed()
        {
            setter.begin_set_parameter(&params.auto_reset);
            setter.set_parameter(&params.auto_reset, auto);
            setter.end_set_parameter(&params.auto_reset);
        }
    });
}

fn target_name(t: LoudnessTarget) -> &'static str {
    match t {
        LoudnessTarget::AribTrB32 => "ARIB TR-B32  -24 / -1 dBTP",
        LoudnessTarget::EbuR128 => "EBU R128  -23 / -1 dBTP",
        LoudnessTarget::AtscA85 => "ATSC A/85  -24 / -2 dBTP",
        LoudnessTarget::Streaming14 => "Streaming  -14 / -1 dBTP",
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
            ui.label(RichText::new("Pass-through, no latency").color(theme.text_dim));
        }
    });
}
