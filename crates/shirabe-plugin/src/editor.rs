//! The plugin editor (also used by the standalone application).
//!
//! Layout: a header, then the tiles of the selected preset (see [`shirabe_ui::layout`]), then
//! the settings and the test oscillator at the bottom.
//! Scales and read-outs follow the documents listed in `docs/standards.md`; the tile
//! arrangement and the Lissajous view are described in `docs/design.md`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, resizable_window::ResizableWindow, widgets};
use shirabe_dsp::history::{HISTORY_CAPACITY, HISTORY_INTERVAL_SECONDS};
use shirabe_dsp::spectrum::{SpectrumAnalyzer, SpectrumConfig};
use shirabe_dsp::MINUS_INFINITY_DB;
use shirabe_ui::egui::{
    self, pos2, vec2, Align, Align2, Color32, FontId, Layout, Painter, Rect, RichText, Sense,
    Stroke,
};
use shirabe_ui::history::{self as history_graph, Graph as HistoryGraph, Point as HistoryPoint};
use shirabe_ui::i18n::{Lang, Text};
use shirabe_ui::layout::{arrange, TileId};
use shirabe_ui::scope::{
    draw as draw_scope, is_clipped, AutoGain, ScopePlane, ScopeRender, Trail, MAX_POINTS,
    TRAIL_CAPACITY, TRAIL_SECONDS,
};
use shirabe_ui::spectrum::{self as spectrum_tile, SpectrumRender};
use shirabe_ui::view::{MeterView, MeterViewRegistry};
use shirabe_ui::{
    draw_level_bar, draw_marker, draw_scale_labels, format_db, format_delta_lu, format_lufs,
    MeterScale, Theme, Zone,
};

use crate::params::{
    LayoutPreset, LevelMode, LoudnessScale, LoudnessTarget, LoudnessUnit, ShirabeParams,
    SpectrumMode, TargetSpec, UiLanguage,
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
    create_egui_editor(
        params.editor_state.clone(),
        EditorState {
            theme: Theme::dark(),
            scope: ScopeView::new(),
            history: HistoryView::new(),
            spectrum: SpectrumView::new(),
            japanese_font_installed: false,
            japanese: None,
            japanese_check_pending: false,
        },
        |ctx, state| {
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = state.theme.background;
            visuals.window_fill = state.theme.background;
            ctx.set_visuals(visuals);
        },
        move |ctx, setter, state| {
            // Meters move continuously, so redraw every frame while the editor is open.
            ctx.request_repaint();

            state.prepare_japanese(ctx, params.ui_language.value());
            let text = params.text(state.japanese(ctx));
            let japanese_attempted = state.japanese_font_installed;
            plugin_window(ctx, &params, |ui| {
                draw(
                    ui,
                    &params,
                    &shared,
                    setter,
                    &state.theme,
                    &mut state.scope,
                    &mut state.history,
                    &mut state.spectrum,
                    text,
                    japanese_attempted,
                    // The plug-in can offer a host sidechain input; the layouts decide
                    // whether the current instance has one.
                    true,
                );
            });
        },
    )
}

fn plugin_window(ctx: &egui::Context, params: &ShirabeParams, content: impl FnOnce(&mut egui::Ui)) {
    ResizableWindow::new("shirabe-main")
        .min_size(vec2(640.0, 380.0))
        .show(ctx, params.editor_state.as_ref(), |ui| {
            egui::Frame::default()
                .inner_margin(egui::Margin::same(8))
                .show(ui, content);
        });
}

/// State owned by one editor instance: the theme and the display-only histories.
struct EditorState {
    theme: Theme,
    scope: ScopeView,
    history: HistoryView,
    spectrum: SpectrumView,
    /// Whether installing the CJK fallback has been attempted for this context.
    japanese_font_installed: bool,
    /// Whether the current fonts cover the Japanese catalogue (cached per context).
    japanese: Option<bool>,
    /// Fonts installed in this frame are used from the next frame on, so the coverage
    /// check waits one frame instead of caching the old fonts' answer.
    japanese_check_pending: bool,
}

impl EditorState {
    /// Installs the system CJK fallback the first time the Japanese interface is selected.
    ///
    /// An English session never loads the font, so the plug-in does not hold a large CJK
    /// font in memory unless it is used.
    fn prepare_japanese(&mut self, ctx: &egui::Context, requested: UiLanguage) {
        if requested != UiLanguage::Japanese || self.japanese_font_installed {
            return;
        }
        self.japanese_font_installed = true;
        // egui applies `set_fonts` at the next frame, so the check must wait.
        if crate::fonts::install(ctx) {
            self.japanese = None;
            self.japanese_check_pending = true;
        } else {
            self.japanese = Some(false);
        }
    }

    /// Whether this editor can draw Japanese.
    fn japanese(&mut self, ctx: &egui::Context) -> bool {
        if self.japanese_check_pending {
            // This frame still draws with the fonts from before the installation.
            self.japanese_check_pending = false;
            return false;
        }
        *self
            .japanese
            .get_or_insert_with(|| crate::fonts::has_japanese(ctx))
    }
}

/// Draws the whole meter view (header, meters and settings). Used by the plugin editor and by
/// the standalone application.
///
/// `text` carries the interface language; it is view-only and never changes the measurement.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &mut egui::Ui,
    params: &ShirabeParams,
    shared: &SharedMeters,
    setter: &ParamSetter,
    theme: &Theme,
    scope: &mut ScopeView,
    history: &mut HistoryView,
    spectrum: &mut SpectrumView,
    text: Text,
    japanese_attempted: bool,
    sidechain_supported: bool,
) {
    header(
        ui,
        params,
        shared,
        setter,
        theme,
        text,
        japanese_attempted,
        sidechain_supported,
    );
    ui.separator();

    let mut controls = CONTROLS_HEIGHT;
    if params.loudness_target.value() == LoudnessTarget::Custom {
        controls += CUSTOM_ROW_HEIGHT;
    }
    let height = (ui.available_height() - controls).max(120.0);
    meters(
        ui,
        params,
        shared,
        theme,
        height,
        scope,
        history,
        spectrum,
        text,
        sidechain_supported,
    );

    ui.separator();
    settings(ui, params, setter, text);
    oscillator_controls(ui, params, setter, theme, text);
}

fn correlation_text(value: Option<f32>) -> String {
    match value {
        Some(v) if v.abs() < 0.005 => "0.00".into(),
        Some(v) => format!("{v:+.2}"),
        None => "--".into(),
    }
}

fn balance_text(text: Text, value: Option<f32>) -> String {
    match value.map(shirabe_dsp::stereo::balance_to_db) {
        Some(d) if d == f32::NEG_INFINITY => ChannelSide::Left.for_text(text).to_owned(),
        Some(d) if d == f32::INFINITY => ChannelSide::Right.for_text(text).to_owned(),
        Some(d) if d.abs() < 0.05 => "0.0 dB".into(),
        Some(d) => format!("{} {:.1} dB", if d < 0.0 { "L" } else { "R" }, d.abs()),
        None => "--".into(),
    }
}

/// A one-sided stereo signal; the channel letter stays `L` or `R` in both languages.
enum ChannelSide {
    Left,
    Right,
}

impl ChannelSide {
    fn for_text(self, text: Text) -> &'static str {
        match (self, text.lang()) {
            (Self::Left, Lang::English) => "L only",
            (Self::Right, Lang::English) => "R only",
            (Self::Left, Lang::Japanese) => "L のみ",
            (Self::Right, Lang::Japanese) => "R のみ",
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn header(
    ui: &mut egui::Ui,
    params: &ShirabeParams,
    shared: &SharedMeters,
    setter: &ParamSetter,
    theme: &Theme,
    text: Text,
    japanese_attempted: bool,
    sidechain_supported: bool,
) {
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
        ui.separator();
        ui.label(
            RichText::new(format!(
                "{} {}",
                text.corr(),
                correlation_text(shared.stereo.correlation.load())
            ))
            .monospace()
            .color(theme.text),
        )
        .on_hover_text(text.corr_hint());
        ui.label(
            RichText::new(format!(
                "{} {}",
                text.bal(),
                balance_text(text, shared.stereo.balance.load())
            ))
            .monospace()
            .color(theme.text),
        )
        .on_hover_text(text.bal_hint());

        ui.separator();
        ui.label(RichText::new(text.layout()).color(theme.text_dim))
            .on_hover_text(text.layout_hint());
        // The comparison needs a host sidechain input, so the standalone application, which
        // has none, does not offer it.
        let presets = layout_presets(sidechain_supported);
        combo_showing(
            ui,
            "layout-preset",
            &params.layout_preset,
            setter,
            presets,
            |preset| layout_preset_name(text, preset),
            shown_layout(params.layout_preset.value(), sidechain_supported),
        );

        language_menu(ui, params, setter, text, japanese_attempted);

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .button(text.reset())
                .on_hover_text(text.reset_hint())
                .clicked()
            {
                shared.request_reset();
            }
            let running = shared.integrating();
            let label = if running { text.pause() } else { text.resume() };
            if ui.button(label).on_hover_text(text.pause_hint()).clicked() {
                shared.set_integrating(!running);
            }
        });
    });
}

/// The compact language selector.
///
/// The current language code is the button label; the menu names each language in its own
/// script. Japanese can be chosen once; the plug-in loads the system font when it is
/// requested. Only a failed attempt disables the entry, so the interface never switches to
/// text the renderer cannot draw.
fn language_menu(
    ui: &mut egui::Ui,
    params: &ShirabeParams,
    setter: &ParamSetter,
    text: Text,
    japanese_attempted: bool,
) {
    ui.separator();
    let current = params.ui_language.value();
    let menu = ui.menu_button(text.lang().code(), |ui| {
        for option in Lang::ALL {
            let enabled =
                option == Lang::English || text.japanese_available() || !japanese_attempted;
            if enabled {
                let label = text.language_option_label(option);
                if ui
                    .selectable_label(current.lang() == option, label)
                    .clicked()
                {
                    set_language(params, setter, option);
                    ui.close_menu();
                }
            } else {
                ui.add_enabled(false, egui::Button::new(text.language_unavailable()))
                    .on_disabled_hover_text(text.language_unavailable_hint());
            }
        }
    });
    menu.response.on_hover_text(text.language_hint());
}

fn set_language(params: &ShirabeParams, setter: &ParamSetter, language: Lang) {
    setter.begin_set_parameter(&params.ui_language);
    setter.set_parameter(&params.ui_language, UiLanguage::from_lang(language));
    setter.end_set_parameter(&params.ui_language);
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

#[allow(clippy::too_many_arguments)]
fn meters(
    ui: &mut egui::Ui,
    params: &ShirabeParams,
    shared: &SharedMeters,
    theme: &Theme,
    height: f32,
    scope: &mut ScopeView,
    history: &mut HistoryView,
    spectrum: &mut SpectrumView,
    text: Text,
    sidechain_supported: bool,
) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    if response
        .on_hover_text(text.meter_reset_hint())
        .double_clicked()
    {
        shared.request_reset();
    }
    let now = ui.input(|i| i.time);
    let painter = ui.painter_at(rect);
    let channels = shared
        .active_channels()
        .clamp(1, crate::shared::MAX_CHANNELS);
    let target = params.target_spec();

    // The preset decides which tiles exist and where they go; the registered view of each
    // tile draws into its rectangle.
    let geometry = geometry_for(params.layout_preset.value(), sidechain_supported);
    let arrangement = arrange(geometry, rect);
    let loudness = LoudnessView::new(params, text);
    let mut ctx = MeterCtx {
        shared,
        params,
        theme,
        loudness: &loudness,
        scope,
        history,
        spectrum,
        now,
        channels,
        target,
        text,
    };
    let table = editor_views();
    let views = MeterViewRegistry::new(&table).expect("every tile is registered once");
    views.draw(&painter, &arrangement, &mut ctx);
}

/// The state the tile views read while drawing one frame.
struct MeterCtx<'a> {
    shared: &'a SharedMeters,
    params: &'a ShirabeParams,
    theme: &'a Theme,
    /// Loudness settings resolved once per frame and shared by two tiles.
    loudness: &'a LoudnessView,
    scope: &'a mut ScopeView,
    history: &'a mut HistoryView,
    spectrum: &'a mut SpectrumView,
    now: f64,
    channels: usize,
    target: TargetSpec,
    text: Text,
}

/// The view that draws `id`.
///
/// The match is exhaustive over the tile set, so a new tile does not compile until it has a
/// view, and `TileId::ALL` builds the table without repeating an identifier.
fn tile_view(id: TileId) -> fn(&mut MeterCtx<'_>, &Painter, Rect) {
    match id {
        TileId::Peak => tile_peak,
        TileId::Level => tile_level,
        TileId::Loudness => tile_loudness,
        TileId::Readout => tile_readout,
        TileId::Scope => tile_scope,
        TileId::History => tile_history,
        TileId::Spectrum => tile_spectrum,
        TileId::Reference => tile_reference,
    }
}

/// Every tile view in drawing order.
///
/// The order is the one that shipped with the presets: a tile may paint its heading just
/// outside its rectangle, so the later tiles must stay on top.
fn editor_views<'a>() -> [MeterView<MeterCtx<'a>>; TileId::ALL.len()] {
    TileId::ALL.map(|id| MeterView::new(id, tile_view(id)))
}

fn tile_peak(ctx: &mut MeterCtx<'_>, painter: &Painter, rect: Rect) {
    peak_section(
        painter,
        rect,
        ctx.shared,
        ctx.theme,
        ctx.channels,
        ctx.target,
        ctx.text,
    );
}

fn tile_level(ctx: &mut MeterCtx<'_>, painter: &Painter, rect: Rect) {
    level_section(
        painter,
        rect,
        ctx.shared,
        ctx.theme,
        ctx.channels,
        (ctx.params.level_mode.value(), ctx.params.vu_needle.value()),
        ctx.text,
    );
}

fn tile_loudness(ctx: &mut MeterCtx<'_>, painter: &Painter, rect: Rect) {
    loudness_section(painter, rect, ctx.shared, ctx.theme, ctx.loudness);
}

fn tile_readout(ctx: &mut MeterCtx<'_>, painter: &Painter, rect: Rect) {
    loudness_panel(
        painter,
        rect,
        ctx.shared,
        ctx.theme,
        ctx.loudness,
        ctx.channels,
        ctx.text,
    );
}

fn tile_scope(ctx: &mut MeterCtx<'_>, painter: &Painter, rect: Rect) {
    scope_section(
        painter, rect, ctx.shared, ctx.params, ctx.theme, ctx.scope, ctx.now, ctx.text,
    );
}

fn tile_history(ctx: &mut MeterCtx<'_>, painter: &Painter, rect: Rect) {
    let points = ctx.history.update(ctx.shared);
    let graph = HistoryGraph {
        points,
        points_per_second: (1.0 / HISTORY_INTERVAL_SECONDS) as f32,
        span_seconds: HISTORY_CAPACITY as f32 * HISTORY_INTERVAL_SECONDS as f32,
        target: ctx.target.loudness,
        tolerance: ctx.target.tolerance,
        scale: ctx.loudness.scale,
        label_offset: ctx.loudness.label_offset(),
        paused: !ctx.shared.integrating(),
        text: ctx.text,
    };
    history_graph::draw(painter, rect, &graph, ctx.theme);
}

fn tile_spectrum(ctx: &mut MeterCtx<'_>, painter: &Painter, rect: Rect) {
    ctx.spectrum.update(ctx.shared);
    let analyzer = ctx.spectrum.analyzer();
    let render = SpectrumRender {
        mode: ctx.params.spectrum_mode.value().display(),
        bins_db: analyzer.bins_db(),
        sample_rate: analyzer.sample_rate(),
        fft_size: analyzer.fft_size(),
        bands_db: analyzer.bands_db(),
        peak: analyzer.peak(),
        ready: analyzer.is_ready(),
        text: ctx.text,
    };
    spectrum_tile::draw(painter, rect, &render, ctx.theme);
}

/// Display-only state of the loudness history graph.
///
/// The measured path never sees this state: it copies the newest points out of the capture
/// ring so the widget can draw them. The buffers are allocated once, on the UI thread.
pub struct HistoryView {
    raw: Vec<(f32, f32)>,
    points: Vec<HistoryPoint>,
}

impl HistoryView {
    /// Creates an empty view.
    pub fn new() -> Self {
        Self {
            raw: vec![(f32::NAN, f32::NAN); HISTORY_CAPACITY],
            points: vec![HistoryPoint::new(None, None); HISTORY_CAPACITY],
        }
    }

    /// Copies the newest points, oldest first, and returns them for this frame.
    pub fn update(&mut self, shared: &SharedMeters) -> &[HistoryPoint] {
        // A clear can land while the points are copied, which would mix the old run with the
        // new one. Retry once; if the history is cleared again, draw nothing this frame.
        let mut count = 0;
        for _ in 0..2 {
            let epoch = shared.history.epoch();
            count = shared.history.read_recent(&mut self.raw);
            if shared.history.epoch() == epoch {
                break;
            }
            count = 0;
        }
        for (point, &(momentary, short_term)) in self.points[..count].iter_mut().zip(&self.raw) {
            *point = HistoryPoint::new(
                momentary.is_finite().then_some(momentary),
                short_term.is_finite().then_some(short_term),
            );
        }
        &self.points[..count]
    }
}

/// Display-only state of the spectrum tile.
///
/// The analyser allocates its FFT plan and buffers once, when the view is created or when the
/// sample rate changes. The audio callback only writes the capture ring.
pub struct SpectrumView {
    analyzer: SpectrumAnalyzer,
    sample_rate: f32,
}

impl SpectrumView {
    /// Creates an empty view prepared for 48 kHz.
    pub fn new() -> Self {
        let sample_rate = 48_000.0;
        Self {
            analyzer: SpectrumAnalyzer::new(sample_rate, SpectrumConfig::default()),
            sample_rate,
        }
    }

    /// Consumes any new captured audio, ready for [`Self::analyzer`] this frame.
    pub fn update(&mut self, shared: &SharedMeters) {
        let rate = shared.sample_rate();
        if rate > 0.0 && (rate - self.sample_rate).abs() > 0.5 {
            self.analyzer = SpectrumAnalyzer::new(rate, SpectrumConfig::default());
            self.sample_rate = rate;
        }
        self.analyzer.analyze(&shared.spectrum);
    }

    /// The analyser holding the values drawn this frame.
    pub fn analyzer(&self) -> &SpectrumAnalyzer {
        &self.analyzer
    }
}

impl Default for SpectrumView {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for HistoryView {
    fn default() -> Self {
        Self::new()
    }
}

/// Draws the Lissajous view from the captured stereo frames.
#[allow(clippy::too_many_arguments)]
fn scope_section(
    painter: &Painter,
    rect: Rect,
    shared: &SharedMeters,
    params: &ShirabeParams,
    theme: &Theme,
    view: &mut ScopeView,
    now: f64,
    text: Text,
) {
    let trail_on = params.scope_trail.value();
    let auto = params.scope_auto_gain.value();
    let plane = if params.scope_view_lr.value() {
        ScopePlane::LeftRight
    } else {
        ScopePlane::MidSide
    };
    let channels = shared
        .active_channels()
        .clamp(1, crate::shared::MAX_CHANNELS);
    view.update(shared, channels, now, trail_on, auto, plane);
    let render = ScopeRender {
        points: view.points(),
        plane,
        trail: trail_on.then_some(view.trail()),
        now,
        trail_seconds: TRAIL_SECONDS,
        auto,
        gain_db: view.gain_db(),
        holding: view.holding(),
        mono: view.mono(),
        clipped: view.clipped(),
        text,
    };
    draw_scope(painter, rect, &render, theme);
}

/// Display-only state of the scope view: the trail, the automatic gain and the read cursor
/// into the capture ring.
///
/// The measured path never sees this state. Only frames the editor has not read before are
/// appended to the trail, so a paused host does not stack copies of the same trace, and the
/// epoch of the capture clears the history after a source reset or a switch to mono.
pub struct ScopeView {
    cursor: u64,
    epoch: u64,
    last_time: f64,
    last_trail: Option<bool>,
    last_auto: Option<bool>,
    last_plane: Option<ScopePlane>,
    trail: Trail,
    gain: AutoGain,
    /// Raw L/R frames read from the capture ring.
    raw: Vec<(f32, f32)>,
    /// New plane points after the display gain, for this frame.
    fresh: Vec<(f32, f32)>,
    /// Newest trace after display gain, retained while no fresh samples arrive.
    latest: Vec<(f32, f32)>,
    latest_len: usize,
    clipped: bool,
    new_samples: usize,
    gain_db: f32,
    holding: bool,
    mono: bool,
}

impl ScopeView {
    /// Creates an empty view.
    pub fn new() -> Self {
        Self {
            cursor: 0,
            epoch: 0,
            last_time: f64::NAN,
            last_trail: None,
            last_auto: None,
            last_plane: None,
            trail: Trail::new(),
            gain: AutoGain::new(),
            raw: vec![(0.0, 0.0); TRAIL_CAPACITY],
            fresh: vec![(0.0, 0.0); TRAIL_CAPACITY],
            latest: vec![(0.0, 0.0); MAX_POINTS],
            latest_len: 0,
            clipped: false,
            new_samples: 0,
            gain_db: 0.0,
            holding: false,
            mono: false,
        }
    }

    /// Consumes the frames written since the previous update.
    ///
    /// `now` is the editor clock in seconds. `trail_on` and `auto` mirror the two display
    /// settings, and `plane` selects the plotted axes; a change of any of them starts the
    /// history and the gain afresh.
    pub fn update(
        &mut self,
        shared: &SharedMeters,
        channels: usize,
        now: f64,
        trail_on: bool,
        auto: bool,
        plane: ScopePlane,
    ) {
        if !now.is_finite() {
            return;
        }
        let mut restart = false;
        let dt = if !self.last_time.is_finite() {
            self.last_time = now;
            0.0
        } else if now < self.last_time {
            // The editor clock went backwards (a new editor context); old timestamps are
            // meaningless, so drop the history.
            restart = true;
            self.last_time = now;
            0.0
        } else {
            let dt = now - self.last_time;
            self.last_time = now;
            dt as f32
        };

        let epoch = shared.scope.epoch();
        if epoch != self.epoch
            || self.last_trail != Some(trail_on)
            || self.last_auto != Some(auto)
            || self.last_plane != Some(plane)
        {
            restart = true;
        }
        if restart {
            self.trail.clear();
            self.latest_len = 0;
            self.clipped = false;
            self.gain.reset();
            self.epoch = epoch;
            self.last_trail = Some(trail_on);
            self.last_auto = Some(auto);
            self.last_plane = Some(plane);
            // A reset restarts the capture counter at zero, so read from its beginning.
            self.cursor = 0;
        }

        self.new_samples = 0;
        self.gain_db = if auto { self.gain.gain_db() } else { 0.0 };
        self.holding = auto;

        if channels < 2 {
            self.mono = true;
            self.trail.clear();
            self.latest_len = 0;
            self.clipped = false;
            self.gain.reset();
            self.gain_db = 0.0;
            self.holding = auto;
            self.cursor = shared.scope.written();
            self.epoch = epoch;
            return;
        }
        self.mono = false;

        let (count, cursor) = shared.scope.read_since(self.cursor, &mut self.raw);
        if shared.scope.epoch() != epoch {
            return;
        }
        self.cursor = cursor;
        self.new_samples = count;
        for (index, &(left, right)) in self.raw[..count].iter().enumerate() {
            self.fresh[index] = plane.project(left, right);
        }
        if count > 0 && trail_on {
            self.clipped = is_clipped(&self.fresh[..count]);
        }

        // The automatic gain only moves when new audio arrives; a paused source holds the
        // last scale instead of winding up on the frozen samples.
        if auto && count > 0 {
            let dt = if dt > 0.0 { dt } else { 1.0 / 60.0 };
            self.gain.update(&self.fresh[..count], dt);
        }
        let factor = if auto { self.gain.factor() } else { 1.0 };
        for point in &mut self.fresh[..count] {
            point.0 *= factor;
            point.1 *= factor;
        }
        self.gain_db = if auto { self.gain.gain_db() } else { 0.0 };
        self.holding = auto && (self.gain.is_holding() || count == 0);

        if trail_on {
            if count > 0 {
                let start = count.saturating_sub(MAX_POINTS);
                self.latest_len = count - start;
                self.latest[..self.latest_len].copy_from_slice(&self.fresh[start..count]);
            }
            for &(x, y) in &self.fresh[..count] {
                self.trail.push(x, y, now);
            }
            self.trail.prune(now, TRAIL_SECONDS);
        } else {
            let count = shared.scope.read_latest(&mut self.raw[..MAX_POINTS]);
            self.clipped = false;
            for (index, &(left, right)) in self.raw[..count].iter().enumerate() {
                let (x, y) = plane.project(left, right);
                self.clipped |= is_clipped(&[(x, y)]);
                self.latest[index] = (x * factor, y * factor);
            }
            self.latest_len = count;
        }
    }

    /// Most recent trace, not including the fading history.
    pub fn points(&self) -> &[(f32, f32)] {
        &self.latest[..self.latest_len]
    }

    /// Whether unscaled samples exceeded full scale on either axis of the selected plane.
    pub fn clipped(&self) -> bool {
        self.clipped
    }

    /// The fading history (empty while the trail is off).
    pub fn trail(&self) -> &Trail {
        &self.trail
    }

    /// Number of frames consumed by the last update.
    pub fn new_samples(&self) -> usize {
        self.new_samples
    }

    /// Gain applied to the display in the last update, in dB.
    pub fn gain_db(&self) -> f32 {
        self.gain_db
    }

    /// Whether the automatic gain is holding its last value.
    pub fn holding(&self) -> bool {
        self.holding
    }

    /// Whether the last update saw a mono input.
    pub fn mono(&self) -> bool {
        self.mono
    }
}

impl Default for ScopeView {
    fn default() -> Self {
        Self::new()
    }
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
    text: Text,
) {
    let scale = MeterScale::PEAK_DBFS;
    let group = BarGroup::new(rect, channels);
    group.draw_frame(painter, text.peak_title(), &scale, theme, 0.0);
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
    fn new(mode: LevelMode, text: Text) -> Self {
        match mode {
            LevelMode::Vu => Self {
                scale: MeterScale::VU,
                title: text.level_vu_title(),
                offset: 0.0,
                over_from: 0.0,
            },
            LevelMode::PpmTypeI => Self {
                scale: MeterScale::PPM_TYPE_I,
                title: text.level_ppm_i_title(),
                offset: -PERMITTED_MAX_DBFS,
                over_from: 0.0,
            },
            LevelMode::PpmTypeII => Self {
                scale: MeterScale::PPM_EBU,
                title: text.level_ppm_ebu_title(),
                offset: -ALIGNMENT_DBFS,
                over_from: PERMITTED_MAX_DBFS - ALIGNMENT_DBFS,
            },
            LevelMode::PpmTypeIIBbc => Self {
                scale: MeterScale::PPM_BBC,
                title: text.level_ppm_bbc_title(),
                offset: -ALIGNMENT_DBFS,
                // Mark 6 is the permitted maximum level (+8 dB above mark 4).
                over_from: 8.0,
            },
            LevelMode::PpmIeee => Self {
                scale: MeterScale::PPM_IEEE,
                title: text.level_ppm_ieee_title(),
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
    (mode, needle): (LevelMode, bool),
    text: Text,
) {
    let view = LevelView::new(mode, text);
    if mode == LevelMode::Vu && needle {
        painter.text(
            rect.left_top(),
            Align2::LEFT_TOP,
            text.level_vu_needle_title(),
            FontId::proportional(11.0),
            theme.text_dim,
        );
        let top = rect.top() + TITLE_HEIGHT;
        let height = (rect.bottom() - top) / channels as f32;
        for index in 0..channels {
            let y = top + height * index as f32;
            let face =
                Rect::from_min_max(pos2(rect.left(), y), pos2(rect.right(), y + height - 4.0));
            let value = shared.channel(index).vu.load(Ordering::Relaxed);
            shirabe_ui::vu_dial::draw(
                painter,
                face,
                value,
                &caption(shared, index),
                &view.readout(mode, value),
                theme,
            );
        }
        return;
    }
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
    mode_name: &'static str,
}

impl LoudnessView {
    fn new(params: &ShirabeParams, text: Text) -> Self {
        let (scale, scale_name) = match params.loudness_scale.value() {
            LoudnessScale::Ebu9 => (MeterScale::LOUDNESS_EBU_9, "+9"),
            LoudnessScale::Ebu18 => (MeterScale::LOUDNESS_EBU_18, "+18"),
        };
        let target = params.target_spec();
        let mode_name = match target.preset {
            LoudnessTarget::EbuR128 => text.mode_ebu(),
            LoudnessTarget::AribTrB32 => "TR-B32",
            LoudnessTarget::AtscA85 => "A/85",
            LoudnessTarget::Streaming14 => "Spotify",
            LoudnessTarget::ApplePodcasts => "Apple Podcasts",
            LoudnessTarget::Custom => text.mode_custom(),
        };
        Self {
            target,
            scale,
            scale_name,
            relative: params.loudness_unit.value() == LoudnessUnit::Relative,
            mode_name,
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

    /// Added to the scale values on the labels: the target for absolute units, nothing for
    /// relative ones. The bars and the history graph label their axis the same way.
    fn label_offset(&self) -> f32 {
        if self.relative {
            0.0
        } else {
            self.target.loudness
        }
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
}

fn loudness_section(
    painter: &Painter,
    rect: Rect,
    shared: &SharedMeters,
    theme: &Theme,
    view: &LoudnessView,
) {
    let group = BarGroup::new(rect, 2);
    let label_offset = view.label_offset();
    let title = format!("{} {}  {}", view.mode_name, view.scale_name, view.unit());
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
    text: Text,
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

    let heading = if running {
        text.integrated().to_owned()
    } else {
        format!("{}  {}", text.integrated(), text.integrating_paused())
    };
    painter.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        heading,
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
    let rows: [(&str, String, Color32); 7] = [
        (text.lra(), lra_text, lra_colour),
        (
            text.plr(),
            format!("{} LU", format_lufs(l.plr.load())),
            theme.text,
        ),
        (
            text.m_max(),
            format!(
                "{} {unit}",
                format_lufs(view.display(l.max_momentary.load()))
            ),
            view.colour(theme, l.max_momentary.load()),
        ),
        (
            text.s_max(),
            format!(
                "{} {unit}",
                format_lufs(view.display(l.max_short_term.load()))
            ),
            view.colour(theme, l.max_short_term.load()),
        ),
        (
            text.tp_max(),
            format!("{} dBTP", format_db(tp_max)),
            if tp_max > ceiling {
                theme.over
            } else {
                theme.text
            },
        ),
        (
            text.time(),
            format!("{}:{:02}", (seconds / 60.0) as u32, (seconds % 60.0) as u32),
            theme.text_dim,
        ),
        (
            text.target_label(),
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
    const ROW_HEIGHT: f32 = 16.0;
    const ROW_STEP: f32 = 19.0;
    const LRA_NOTE_HEIGHT: f32 = 14.0;
    // At the minimum window height the Custom controls take an extra row. Recover the
    // space for the note from the row gaps, not by shrinking the text or moving tiles.
    let row_step = if lra_unstable {
        ((rect.bottom() - y - LRA_NOTE_HEIGHT) / rows.len() as f32)
            .floor()
            .clamp(ROW_HEIGHT, ROW_STEP)
    } else {
        ROW_STEP
    };
    for (name, value, colour) in rows {
        if y + ROW_HEIGHT > rect.bottom() {
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
        y += row_step;
    }
    if lra_unstable && y + LRA_NOTE_HEIGHT <= rect.bottom() {
        painter.text(
            pos2(left, y),
            Align2::LEFT_TOP,
            text.lra_note(),
            FontId::proportional(10.0),
            theme.text_dim,
        );
    }
}

/// Draws the reference comparison: the deltas of the main measurement against the sidechain
/// input, in LU (main − reference). A missing or silent side is shown as `--`, never as zero.
fn reference_section(
    painter: &Painter,
    rect: Rect,
    shared: &SharedMeters,
    theme: &Theme,
    text: Text,
) {
    painter.rect_filled(rect, 4.0, theme.panel);
    let left = rect.left() + 10.0;
    let right = rect.right() - 10.0;
    let mut y = rect.top() + 10.0;
    painter.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        text.reference_title(),
        FontId::proportional(11.0),
        theme.text_dim,
    );
    y += 16.0;
    let deltas = shared.deltas();
    for (label, delta) in [
        ("M", deltas.momentary),
        ("S", deltas.short_term),
        ("I", deltas.integrated),
    ] {
        painter.text(
            pos2(left, y),
            Align2::LEFT_TOP,
            label,
            FontId::proportional(12.0),
            theme.text_dim,
        );
        painter.text(
            pos2(right, y),
            Align2::RIGHT_TOP,
            format!("{} LU", format_delta_lu(delta)),
            FontId::monospace(12.0),
            match delta {
                Some(_) => theme.text,
                None => theme.text_dim,
            },
        );
        y += 18.0;
    }
    // Without an auxiliary input in the host layout there is nothing to compare against;
    // say so instead of leaving the reader guessing why every value is a dash.
    if !shared.reference_available() && y + 12.0 <= rect.bottom() {
        painter.text(
            pos2(left, y),
            Align2::LEFT_TOP,
            text.reference_missing(),
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
    name: impl Fn(E) -> &'static str,
) {
    combo_showing(ui, id, param, setter, options, name, param.value());
}

/// Like [`combo`], but labelled with `current`, which may differ from the stored value when
/// the stored choice is not available on this target.
fn combo_showing<E: Enum + PartialEq + Copy + 'static>(
    ui: &mut egui::Ui,
    id: &str,
    param: &EnumParam<E>,
    setter: &ParamSetter,
    options: &[E],
    name: impl Fn(E) -> &'static str,
    current: E,
) {
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

fn settings(ui: &mut egui::Ui, params: &ShirabeParams, setter: &ParamSetter, text: Text) {
    ui.horizontal(|ui| {
        ui.label(text.target_label());
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
            |target| target_name(text, target),
        );
        combo(
            ui,
            "loudness-scale",
            &params.loudness_scale,
            setter,
            &[LoudnessScale::Ebu9, LoudnessScale::Ebu18],
            |s| match s {
                LoudnessScale::Ebu9 => text.scale_ebu9(),
                LoudnessScale::Ebu18 => text.scale_ebu18(),
            },
        );
        combo(
            ui,
            "loudness-unit",
            &params.loudness_unit,
            setter,
            &[LoudnessUnit::Absolute, LoudnessUnit::Relative],
            |u| match u {
                LoudnessUnit::Absolute => text.unit_absolute(),
                LoudnessUnit::Relative => text.unit_relative(),
            },
        );

        let mut auto = params.auto_reset.value();
        if ui
            .checkbox(&mut auto, text.reset_on_play())
            .on_hover_text(text.reset_on_play_hint())
            .changed()
        {
            setter.begin_set_parameter(&params.auto_reset);
            setter.set_parameter(&params.auto_reset, auto);
            setter.end_set_parameter(&params.auto_reset);
        }
        // The display choice belongs to the Spectrum layout and shares the loudness row, so
        // no control row (and no meter height) is lost at the 640 px minimum width.
        if params.layout_preset.value() == LayoutPreset::Spectrum {
            ui.separator();
            ui.scope(|ui| {
                combo(
                    ui,
                    "spectrum-mode",
                    &params.spectrum_mode,
                    setter,
                    &[SpectrumMode::Fft, SpectrumMode::Rta],
                    |mode| match mode {
                        SpectrumMode::Fft => text.spectrum_mode_fft(),
                        SpectrumMode::Rta => text.spectrum_mode_rta(),
                    },
                );
            })
            .response
            .on_hover_text(text.spectrum_mode_hint());
        }
    });

    if params.loudness_target.value() == LoudnessTarget::Custom {
        ui.horizontal(|ui| {
            ui.label(text.custom());
            ui.add(
                widgets::ParamSlider::for_param(&params.custom_loudness, setter).with_width(110.0),
            )
            .on_hover_text(text.custom_target_hint());
            ui.label("±");
            ui.add(
                widgets::ParamSlider::for_param(&params.custom_tolerance, setter).with_width(90.0),
            )
            .on_hover_text(text.custom_tolerance_hint());
            ui.label(text.tp_le());
            ui.add(
                widgets::ParamSlider::for_param(&params.custom_true_peak, setter).with_width(100.0),
            )
            .on_hover_text(text.custom_tp_hint());
        });
    }
    ui.horizontal(|ui| {
        ui.label(text.level_label());
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
            |mode| level_mode_name(text, mode),
        );
        if params.level_mode.value() == LevelMode::Vu {
            let mut needle = params.vu_needle.value();
            if ui
                .checkbox(&mut needle, text.needle())
                .on_hover_text(text.needle_hint())
                .changed()
            {
                setter.begin_set_parameter(&params.vu_needle);
                setter.set_parameter(&params.vu_needle, needle);
                setter.end_set_parameter(&params.vu_needle);
            }
            ui.label(text.vu_reference());
            ui.add(widgets::ParamSlider::for_param(&params.vu_reference, setter).with_width(110.0));
        }
        ui.separator();
        // The view switch replaces a "Scope" caption so the group still fits at 640 px.
        let mut left_right = params.scope_view_lr.value();
        if ui
            .checkbox(&mut left_right, text.lr_view())
            .on_hover_text(text.lr_hint())
            .changed()
        {
            setter.begin_set_parameter(&params.scope_view_lr);
            setter.set_parameter(&params.scope_view_lr, left_right);
            setter.end_set_parameter(&params.scope_view_lr);
        }
        let mut trail = params.scope_trail.value();
        if ui
            .checkbox(&mut trail, text.trail())
            .on_hover_text(text.trail_hint())
            .changed()
        {
            setter.begin_set_parameter(&params.scope_trail);
            setter.set_parameter(&params.scope_trail, trail);
            setter.end_set_parameter(&params.scope_trail);
        }
        let mut auto = params.scope_auto_gain.value();
        if ui
            .checkbox(&mut auto, text.auto())
            .on_hover_text(text.auto_hint())
            .changed()
        {
            setter.begin_set_parameter(&params.scope_auto_gain);
            setter.set_parameter(&params.scope_auto_gain, auto);
            setter.end_set_parameter(&params.scope_auto_gain);
        }
    });
}

fn layout_preset_name(text: Text, preset: LayoutPreset) -> &'static str {
    match preset {
        LayoutPreset::Standard => text.layout_standard(),
        LayoutPreset::Stereo => text.layout_stereo(),
        LayoutPreset::History => text.layout_history(),
        LayoutPreset::Spectrum => text.layout_spectrum(),
        LayoutPreset::Compare => text.layout_compare(),
    }
}

/// The layout presets on offer. `Compare` needs a host sidechain input, so a target without
/// one (the standalone application) does not list it.
const LAYOUT_PRESETS: [LayoutPreset; 5] = [
    LayoutPreset::Standard,
    LayoutPreset::Stereo,
    LayoutPreset::History,
    LayoutPreset::Spectrum,
    LayoutPreset::Compare,
];

fn layout_presets(sidechain_supported: bool) -> &'static [LayoutPreset] {
    if sidechain_supported {
        &LAYOUT_PRESETS
    } else {
        &LAYOUT_PRESETS[..4]
    }
}

/// The preset named in the Layout menu: a stored `Compare` that falls back to `Standard`
/// is labelled `Standard`, matching what is drawn.
fn shown_layout(preset: LayoutPreset, sidechain_supported: bool) -> LayoutPreset {
    if preset == LayoutPreset::Compare && !sidechain_supported {
        LayoutPreset::Standard
    } else {
        preset
    }
}

/// The geometry to draw for `preset` on this target. A stored `Compare` falls back to
/// `Standard` where the comparison cannot work, so the meters stay visible.
fn geometry_for(
    preset: LayoutPreset,
    sidechain_supported: bool,
) -> shirabe_ui::layout::LayoutPreset {
    match preset {
        LayoutPreset::Compare if !sidechain_supported => shirabe_ui::layout::LayoutPreset::Standard,
        _ => preset.geometry(),
    }
}

fn tile_reference(ctx: &mut MeterCtx<'_>, painter: &Painter, rect: Rect) {
    reference_section(painter, rect, ctx.shared, ctx.theme, ctx.text);
}

fn level_mode_name(text: Text, m: LevelMode) -> &'static str {
    match m {
        LevelMode::Vu => text.level_mode_vu(),
        LevelMode::PpmTypeI => text.level_mode_ppm_i(),
        LevelMode::PpmTypeII => text.level_mode_ppm_ebu(),
        LevelMode::PpmTypeIIBbc => text.level_mode_ppm_bbc(),
        LevelMode::PpmIeee => text.level_mode_ppm_ieee(),
    }
}

fn target_name(text: Text, t: LoudnessTarget) -> &'static str {
    match t {
        LoudnessTarget::AribTrB32 => text.target_arib(),
        LoudnessTarget::EbuR128 => text.target_ebu(),
        LoudnessTarget::AtscA85 => text.target_atsc(),
        LoudnessTarget::Streaming14 => text.target_spotify(),
        LoudnessTarget::ApplePodcasts => text.target_apple(),
        LoudnessTarget::Custom => text.target_custom(),
    }
}

fn oscillator_controls(
    ui: &mut egui::Ui,
    params: &ShirabeParams,
    setter: &ParamSetter,
    theme: &Theme,
    text: Text,
) {
    ui.horizontal(|ui| {
        let mut enabled = params.osc_enabled.value();
        if ui.checkbox(&mut enabled, text.test_oscillator()).changed() {
            setter.begin_set_parameter(&params.osc_enabled);
            setter.set_parameter(&params.osc_enabled, enabled);
            setter.end_set_parameter(&params.osc_enabled);
        }
        ui.add(widgets::ParamSlider::for_param(&params.osc_level, setter).with_width(110.0));
        ui.add(widgets::ParamSlider::for_param(&params.osc_frequency, setter).with_width(110.0));
        if enabled {
            ui.label(
                RichText::new(text.osc_replaces_input()).color(Color32::from_rgb(0xf2, 0xc1, 0x2e)),
            );
        } else {
            let note = if setter.raw_context.plugin_api() == PluginApi::Standalone {
                text.measuring_source()
            } else {
                text.pass_through()
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

    #[test]
    fn a_stored_compare_layout_is_labelled_as_the_layout_drawn_without_a_sidechain() {
        assert_eq!(
            shown_layout(LayoutPreset::Compare, false),
            LayoutPreset::Standard
        );
        assert_eq!(
            shown_layout(LayoutPreset::Compare, true),
            LayoutPreset::Compare
        );
        assert_eq!(
            shown_layout(LayoutPreset::History, false),
            LayoutPreset::History
        );
    }

    use super::*;

    #[test]
    fn the_editor_registers_every_tile_once() {
        let views = editor_views();
        let registry = MeterViewRegistry::new(&views).expect("the tiles are unique");
        assert_eq!(registry.views().len(), TileId::ALL.len());
        for tile in TileId::ALL {
            assert!(registry.get(tile).is_some(), "{tile} has no view");
        }
        for preset in [
            LayoutPreset::Standard,
            LayoutPreset::Stereo,
            LayoutPreset::History,
            LayoutPreset::Spectrum,
        ] {
            for size in [vec2(820.0, 500.0), vec2(640.0, 260.0)] {
                let rect = Rect::from_min_size(pos2(0.0, 0.0), size);
                let arrangement = arrange(preset.geometry(), rect);
                let missing: Vec<TileId> = registry.missing(&arrangement).collect();
                assert!(missing.is_empty(), "{preset:?} {size:?}: {missing:?}");
            }
        }
    }

    /// Renders the real editor window at `size` with `preset` and a Custom target, seeded with
    /// the values that make every read-out row and the unstable-LRA note visible.
    fn window_frame(preset: LayoutPreset, size: egui::Vec2) -> (egui::FullOutput, Theme) {
        window_frame_with_reference(preset, size, true)
    }

    /// The same, with or without a sidechain reference in the current layout.
    fn window_frame_with_reference(
        preset: LayoutPreset,
        size: egui::Vec2,
        with_reference: bool,
    ) -> (egui::FullOutput, Theme) {
        let params = ShirabeParams::default();
        // These pointers come from the live parameters owned by this test.
        unsafe {
            params
                .layout_preset
                .as_ptr()
                .set_normalized_value(params.layout_preset.preview_normalized(preset));
            params.loudness_target.as_ptr().set_normalized_value(
                params
                    .loudness_target
                    .preview_normalized(LoudnessTarget::Custom),
            );
        }
        assert_eq!(params.layout_preset.value(), preset);
        assert_eq!(params.loudness_target.value(), LoudnessTarget::Custom);
        let shared = SharedMeters::new();
        // The sidechain layouts are the ones that offer a reference.
        shared.configure(2, 48_000.0, with_reference);
        shared.loudness.range.store(Some(0.0));
        shared.loudness.plr.store(Some(16.0));
        shared.loudness.max_momentary.store(Some(-22.0));
        shared.loudness.max_short_term.store(Some(-23.0));
        shared.loudness.integrated.store(Some(-18.0));
        shared.loudness.momentary.store(Some(-19.0));
        shared.loudness.short_term.store(Some(-20.0));
        if with_reference {
            // A sidechain reference 6 / 4 / 5 LU below the main measurement.
            shared.reference.momentary.store(Some(-25.0));
            shared.reference.short_term.store(Some(-24.0));
            shared.reference.integrated.store(Some(-23.0));
        }
        // A few history points, so the graph draws a trace instead of the empty state.
        for index in 0..60 {
            shared.history.record(
                (48_000.0 * 0.1) as usize,
                Some(-23.0 + (index % 4) as f32),
                Some(-24.0),
            );
        }
        let theme = Theme::dark();
        let mut scope = ScopeView::new();
        let mut history = HistoryView::new();
        let mut spectrum = SpectrumView::new();
        let ctx = egui::Context::default();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), size)),
                ..Default::default()
            },
            |ctx| {
                let setter = ParamSetter {
                    raw_context: &TestGuiContext,
                };
                plugin_window(ctx, &params, |ui| {
                    draw(
                        ui,
                        &params,
                        &shared,
                        &setter,
                        &theme,
                        &mut scope,
                        &mut history,
                        &mut spectrum,
                        Text::new(Lang::English, true),
                        true,
                        true,
                    );
                });
            },
        );
        (output, theme)
    }

    /// The read-out panel rectangle, found through its `INTEGRATED` heading.
    fn readout_panel(output: &egui::FullOutput, theme: &Theme) -> Rect {
        let heading = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == "INTEGRATED" => {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
            .expect("read-out heading");
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(panel)
                    if panel.fill == theme.panel && panel.rect.contains_rect(heading) =>
                {
                    Some(panel.rect)
                }
                _ => None,
            })
            .expect("readout panel")
    }

    /// Asserts that every read-out row and the LRA note fit inside the read-out panel without
    /// overlapping.
    fn assert_readout_rows_fit(output: &egui::FullOutput, theme: &Theme, what: &str) {
        let text = |label: &str| {
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => {
                        let bounds = text.visual_bounding_rect();
                        assert!(
                            clipped.clip_rect.contains_rect(bounds),
                            "{what}: {label} clipped"
                        );
                        Some((bounds, text.galley.job.sections[0].format.font_id.size))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{what}: missing {label:?}"))
        };
        let (heading, _) = text("INTEGRATED");
        let panel = readout_panel(output, theme);
        assert!(panel.contains_rect(heading));
        let mut bottom = heading.bottom();
        for (label, value) in [
            ("LRA", "0.0 LU *"),
            ("PLR", "16.0 LU"),
            ("M max", "-22.0 LUFS"),
            ("S max", "-23.0 LUFS"),
            ("TP max", "-inf dBTP"),
            ("Time", "0:00"),
            ("Target", "-14 / -1"),
        ] {
            let (row, size) = text(label);
            let (value_bounds, value_size) = text(value);
            assert_eq!(size, 12.0, "{what}: {label}");
            assert_eq!(value_size, 12.0, "{what}: {value}");
            assert!(
                row.right() <= value_bounds.left(),
                "{what}: {label} overlaps its value"
            );
            let row = row.union(value_bounds);
            assert!(
                panel.contains_rect(row) && row.top() >= bottom,
                "{what}: {label} is clipped or overlaps another row"
            );
            bottom = row.bottom();
        }
        let (note, _) = text("* LRA not yet stable (< 60 s)");
        assert!(
            panel.contains_rect(note) && note.top() >= bottom,
            "{what}: note is clipped or overlaps the rows"
        );
    }

    #[test]
    fn minimum_custom_window_shows_all_readouts_and_unstable_lra_note() {
        for preset in [
            LayoutPreset::Stereo,
            LayoutPreset::Standard,
            LayoutPreset::History,
            LayoutPreset::Spectrum,
        ] {
            let (output, theme) = window_frame(preset, vec2(640.0, 380.0));
            assert_readout_rows_fit(&output, &theme, &format!("{preset:?} at the minimum size"));
        }
    }

    #[test]
    fn the_compare_layout_draws_the_reference_deltas_in_place_of_the_scope() {
        let (output, theme) = window_frame(LayoutPreset::Compare, vec2(900.0, 560.0));
        assert_readout_rows_fit(&output, &theme, "Compare");
        let texts = frame_text(&output);
        assert!(texts.iter().any(|text| text == "REFERENCE Δ"), "{texts:?}");
        // Main − reference for M / S / I, in LU.
        for value in ["+6.0 LU", "+4.0 LU", "+5.0 LU"] {
            assert!(texts.iter().any(|text| text == value), "{value}: {texts:?}");
        }
        // The comparison takes the scope's place, so the figure is not drawn.
        assert!(!texts.iter().any(|text| text.starts_with("LISSAJOUS")));

        // A narrow but tall window still has room for it above the read-out's minimum.
        let (output, theme) = window_frame(LayoutPreset::Compare, vec2(640.0, 560.0));
        assert_readout_rows_fit(&output, &theme, "Compare at 640x560");
        assert!(frame_text(&output).iter().any(|text| text == "REFERENCE Δ"));

        // At the minimum window size the comparison gives way to the read-out, exactly like
        // the scope in the Standard layout, and every row stays.
        let (output, theme) = window_frame(LayoutPreset::Compare, vec2(640.0, 380.0));
        assert_readout_rows_fit(&output, &theme, "Compare at the minimum size");
        assert!(!frame_text(&output).iter().any(|text| text == "REFERENCE Δ"));
    }

    #[test]
    fn the_reference_tile_says_when_there_is_no_sidechain_input() {
        let (output, _) =
            window_frame_with_reference(LayoutPreset::Compare, vec2(900.0, 560.0), false);
        let texts = frame_text(&output);
        assert!(texts.iter().any(|text| text == "REFERENCE Δ"), "{texts:?}");
        assert!(
            texts.iter().any(|text| text == "no sidechain input"),
            "{texts:?}"
        );
        // No reference means dashes, not zeros.
        assert_eq!(texts.iter().filter(|text| *text == "-- LU").count(), 3);
    }

    #[test]
    fn a_target_without_a_sidechain_offers_no_compare_layout() {
        assert_eq!(
            geometry_for(LayoutPreset::Compare, false),
            shirabe_ui::layout::LayoutPreset::Standard
        );
        assert_eq!(
            geometry_for(LayoutPreset::Compare, true),
            shirabe_ui::layout::LayoutPreset::Compare
        );
        assert_eq!(layout_presets(true).len(), 5);
        assert_eq!(layout_presets(false).len(), 4);
        assert!(!layout_presets(false).contains(&LayoutPreset::Compare));
        // Every other preset keeps its meaning on both targets.
        for preset in [
            LayoutPreset::Standard,
            LayoutPreset::Stereo,
            LayoutPreset::History,
            LayoutPreset::Spectrum,
        ] {
            assert_eq!(geometry_for(preset, false), preset.geometry());
        }
    }

    fn frame_text(output: &egui::FullOutput) -> Vec<String> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_spectrum_layout_draws_the_tile_and_its_display_choice() {
        let (output, theme) = window_frame(LayoutPreset::Spectrum, vec2(900.0, 560.0));
        assert_readout_rows_fit(&output, &theme, "Spectrum");
        let texts = frame_text(&output);
        assert!(
            texts.iter().any(|text| text.contains("SPECTRUM")),
            "{texts:?}"
        );
        assert!(texts.iter().any(|text| text == "FFT"), "{texts:?}");
        assert!(
            texts.iter().any(|text| text == "Spectrum"),
            "the display row is missing: {texts:?}"
        );

        // At the minimum size the tile and its row are dropped; the numbers stay readable.
        let (output, theme) = window_frame(LayoutPreset::Spectrum, vec2(640.0, 380.0));
        assert_readout_rows_fit(&output, &theme, "Spectrum at the minimum size");
        let texts = frame_text(&output);
        assert!(!texts.iter().any(|text| text.contains("SPECTRUM")));
        assert!(!texts.iter().any(|text| text == "1/3 octave"));
    }

    #[test]
    fn the_history_layout_keeps_every_row_and_draws_the_graph() {
        let (output, theme) = window_frame(LayoutPreset::History, vec2(820.0, 560.0));
        assert_readout_rows_fit(&output, &theme, "History");

        let mut titles = Vec::new();
        let mut panel = None;
        for clipped in &output.shapes {
            if let egui::Shape::Text(text) = &clipped.shape {
                titles.push(text.galley.text().to_owned());
                if text.galley.text() == "LOUDNESS HISTORY" {
                    panel = Some(text.visual_bounding_rect());
                }
            }
        }
        let title = panel.expect("the graph heading is drawn");
        for expected in ["3 min", "s ago", "M", "S", "-60"] {
            assert!(
                titles.iter().any(|t| t == expected),
                "{expected} missing from {titles:?}"
            );
        }
        // The axis labels and the heading stay inside the meter area of the window.
        assert!(
            title.bottom() < 560.0 && title.left() >= 0.0,
            "{title:?} outside the window"
        );
        // The trace is drawn: at least one line segment per history point pair.
        let segments = output
            .shapes
            .iter()
            .filter(|s| matches!(s.shape, egui::Shape::LineSegment { .. }))
            .count();
        assert!(segments > 30, "the graph drew {segments} segments");

        // At the minimum window height the graph is dropped, and every row still fits.
        let (output, theme) = window_frame(LayoutPreset::History, vec2(640.0, 380.0));
        assert_readout_rows_fit(&output, &theme, "History at the minimum size");
        let drawn = output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => text.galley.text() == "LOUDNESS HISTORY",
            _ => false,
        });
        assert!(!drawn, "the graph must give way to the read-out");
    }

    #[test]
    fn the_editor_installs_the_cjk_fallback_only_when_japanese_is_selected() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |_| {});

        // English never loads a CJK font: the plug-in stays small in memory.
        let mut state = EditorState {
            theme: Theme::dark(),
            scope: ScopeView::new(),
            history: HistoryView::new(),
            spectrum: SpectrumView::new(),
            japanese_font_installed: false,
            japanese: None,
            japanese_check_pending: false,
        };
        let mut available = None;
        let _ = ctx.run(Default::default(), |ctx| {
            state.prepare_japanese(ctx, UiLanguage::English);
            available = Some(state.japanese(ctx));
        });
        assert!(!state.japanese_font_installed);
        assert_eq!(available, Some(false));

        // Selecting Japanese installs the system fallback; egui applies the fonts on the
        // next frame, so the first frame still draws English and the second one reports
        // what the new fonts cover.
        let mut state = EditorState {
            theme: Theme::dark(),
            scope: ScopeView::new(),
            history: HistoryView::new(),
            spectrum: SpectrumView::new(),
            japanese_font_installed: false,
            japanese: None,
            japanese_check_pending: false,
        };
        let mut available = None;
        let _ = ctx.run(Default::default(), |ctx| {
            state.prepare_japanese(ctx, UiLanguage::Japanese);
            available = Some(state.japanese(ctx));
        });
        assert!(state.japanese_font_installed);
        assert_eq!(available, Some(false));
        let mut next_frame = None;
        let mut fresh = None;
        let _ = ctx.run(Default::default(), |ctx| {
            next_frame = Some(state.japanese(ctx));
            fresh = Some(crate::fonts::has_japanese(ctx));
        });
        assert_eq!(
            next_frame, fresh,
            "the second frame must report what the installed fonts cover"
        );
    }

    struct TestGuiContext;

    impl GuiContext for TestGuiContext {
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Vst3
        }
        fn request_resize(&self) -> bool {
            false
        }
        unsafe fn raw_begin_set_parameter(&self, _: ParamPtr) {
            unreachable!()
        }
        unsafe fn raw_set_parameter_normalized(&self, _: ParamPtr, _: f32) {
            unreachable!()
        }
        unsafe fn raw_end_set_parameter(&self, _: ParamPtr) {
            unreachable!()
        }
        fn get_state(&self) -> PluginState {
            unreachable!()
        }
        fn set_state(&self, _: PluginState) {
            unreachable!()
        }
    }

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

    #[test]
    fn stereo_header_labels_distinguish_silence_center_and_one_sided_audio() {
        assert_eq!(correlation_text(None), "--");
        assert_eq!(correlation_text(Some(-1.0)), "-1.00");
        assert_eq!(correlation_text(Some(0.0)), "0.00");
        assert_eq!(correlation_text(Some(1.0)), "+1.00");
        let text = Text::new(Lang::English, true);
        assert_eq!(balance_text(text, None), "--");
        assert_eq!(balance_text(text, Some(0.0)), "0.0 dB");
        assert_eq!(balance_text(text, Some(-0.6)), "L 6.0 dB");
        assert_eq!(balance_text(text, Some(0.6)), "R 6.0 dB");
        assert_eq!(balance_text(text, Some(-1.0)), "L only");
        assert_eq!(balance_text(text, Some(1.0)), "R only");
        let text = Text::new(Lang::Japanese, true);
        assert_eq!(balance_text(text, Some(-1.0)), "L のみ");
        assert_eq!(balance_text(text, Some(1.0)), "R のみ");
    }
}
