//! The plugin editor (also used by the standalone application).

use std::sync::atomic::Ordering;
use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, resizable_window::ResizableWindow, widgets};
use shirabe_ui::egui::{self, vec2, Align, Align2, Color32, FontId, Layout, RichText, Sense};
use shirabe_ui::{draw_level_bar, draw_scale_labels, format_db, MeterScale, Theme};

use crate::params::ShirabeParams;
use crate::shared::SharedMeters;

const SCALE_WIDTH: f32 = 26.0;
const CAPTION_HEIGHT: f32 = 16.0;
const READOUT_HEIGHT: f32 = 18.0;
const MAX_BAR_WIDTH: f32 = 40.0;
const BAR_GAP: f32 = 6.0;
const CONTROLS_HEIGHT: f32 = 64.0;

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
                .min_size(vec2(360.0, 280.0))
                .show(ctx, egui_state.as_ref(), |ui| {
                    egui::Frame::default()
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            header(ui, &shared, theme);
                            ui.separator();

                            let meter_height = (ui.available_height() - CONTROLS_HEIGHT).max(80.0);
                            peak_meters(ui, &shared, theme, meter_height);

                            ui.separator();
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
        ui.label(
            RichText::new(format!("Sample peak  ·  {layout}  ·  {rate}")).color(theme.text_dim),
        );

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .button("Reset max")
                .on_hover_text("Clear the maximum read-outs")
                .clicked()
            {
                shared.request_reset();
            }
        });
    });
}

fn peak_meters(ui: &mut egui::Ui, shared: &SharedMeters, theme: &Theme, height: f32) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    if response
        .on_hover_text("Double-click to clear the maximum read-outs")
        .double_clicked()
    {
        shared.request_reset();
    }
    let painter = ui.painter_at(rect);
    let scale = MeterScale::PEAK_DBFS;

    let channels = shared.active_channels().max(1);
    let roles = shared.layout().map(|l| l.roles());

    let bars_top = rect.top() + READOUT_HEIGHT;
    let bars_bottom = rect.bottom() - CAPTION_HEIGHT;
    let usable = (rect.width() - SCALE_WIDTH).max(0.0);
    let bar_width =
        ((usable - BAR_GAP * channels as f32) / channels as f32).clamp(6.0, MAX_BAR_WIDTH);
    let total = channels as f32 * (bar_width + BAR_GAP);
    let left = rect.left() + SCALE_WIDTH + ((usable - total) * 0.5).max(0.0);

    draw_scale_labels(
        &painter,
        egui::Rect::from_min_max(
            egui::pos2(left - SCALE_WIDTH, bars_top),
            egui::pos2(left - 2.0, bars_bottom),
        ),
        &scale,
        theme,
    );

    let caption_font = FontId::proportional(12.0);
    let readout_font = FontId::monospace(11.0);
    for index in 0..channels {
        let x = left + index as f32 * (bar_width + BAR_GAP);
        let bar = egui::Rect::from_min_max(
            egui::pos2(x, bars_top),
            egui::pos2(x + bar_width, bars_bottom),
        );
        let readout = shared.channel(index);
        let level = readout.level_db.load(Ordering::Relaxed);
        let hold = readout.hold_db.load(Ordering::Relaxed);
        let max = readout.max_db.load(Ordering::Relaxed);

        draw_level_bar(&painter, bar, &scale, theme, level, hold);

        let max_colour = if max >= 0.0 {
            theme.zone_at(0.0).lit
        } else {
            theme.text
        };
        painter.text(
            egui::pos2(bar.center().x, rect.top() + READOUT_HEIGHT * 0.5),
            Align2::CENTER_CENTER,
            format_db(max),
            readout_font.clone(),
            max_colour,
        );

        let caption = roles
            .and_then(|r| r.get(index))
            .map_or_else(|| format!("{}", index + 1), |r| r.label().to_owned());
        painter.text(
            egui::pos2(bar.center().x, rect.bottom() - CAPTION_HEIGHT * 0.5),
            Align2::CENTER_CENTER,
            caption,
            caption_font.clone(),
            theme.text_dim,
        );
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
        if enabled {
            ui.label(
                RichText::new("Input is replaced by the test tone")
                    .color(Color32::from_rgb(0xf2, 0xc1, 0x2e)),
            );
        } else {
            ui.label(RichText::new("Pass-through, no latency").color(theme.text_dim));
        }
    });
    ui.horizontal(|ui| {
        ui.label("Level");
        ui.add(widgets::ParamSlider::for_param(&params.osc_level, setter).with_width(140.0));
        ui.label("Frequency");
        ui.add(widgets::ParamSlider::for_param(&params.osc_frequency, setter).with_width(140.0));
    });
}
