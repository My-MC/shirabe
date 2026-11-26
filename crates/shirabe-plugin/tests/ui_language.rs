//! Headless checks of the Japanese and English editor views.
//!
//! The view is drawn into an egui frame at the window's real minimum size, so the tests
//! catch clipped labels, overlapping read-out rows and header controls that no longer fit.
//! Values and the programme timer are compared before and after a language change.

use nih_plug::prelude::*;
use shirabe_plugin::editor::{draw, HistoryView, ScopeView};
use shirabe_plugin::params::{LoudnessTarget, UiLanguage};
use shirabe_plugin::shared::SharedMeters;
use shirabe_plugin::ShirabeParams;
use shirabe_ui::egui::{self, pos2, vec2, Rect};
use shirabe_ui::i18n::{Lang, Text};
use shirabe_ui::Theme;

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

/// The standalone's plug-in API, which selects the "measuring the source" note.
struct StandaloneTestContext;

impl GuiContext for StandaloneTestContext {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Standalone
    }
    fn request_resize(&self) -> bool {
        false
    }
    unsafe fn raw_begin_set_parameter(&self, _: ParamPtr) {}
    unsafe fn raw_set_parameter_normalized(&self, _: ParamPtr, _: f32) {}
    unsafe fn raw_end_set_parameter(&self, _: ParamPtr) {}
    fn get_state(&self) -> PluginState {
        unreachable!()
    }
    fn set_state(&self, _: PluginState) {}
}

struct Frame {
    shapes: Vec<egui::epaint::ClippedShape>,
}

impl Frame {
    fn texts(&self) -> Vec<(&str, Rect, Rect)> {
        self.shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some((
                    text.galley.text(),
                    text.visual_bounding_rect(),
                    clipped.clip_rect,
                )),
                _ => None,
            })
            .collect()
    }

    fn find(&self, label: &str) -> (Rect, Rect) {
        self.texts()
            .into_iter()
            .find(|(text, _, _)| *text == label)
            .map(|(_, bounds, clip)| (bounds, clip))
            .unwrap_or_else(|| panic!("missing {label:?}"))
    }

    fn header(&self) -> Vec<(&str, Rect, Rect)> {
        let mut texts: Vec<_> = self
            .texts()
            .into_iter()
            .filter(|(_, bounds, _)| bounds.top() < 40.0)
            .collect();
        texts.sort_by(|a, b| a.1.left().total_cmp(&b.1.left()));
        texts
    }
}

fn render(
    params: &ShirabeParams,
    shared: &SharedMeters,
    text: Text,
    size: egui::Vec2,
    raw_context: &dyn GuiContext,
) -> Frame {
    let theme = Theme::dark();
    let mut scope = ScopeView::new();
    let mut history = HistoryView::new();
    let ctx = egui::Context::default();
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), size)),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let setter = ParamSetter { raw_context };
                draw(
                    ui,
                    params,
                    shared,
                    &setter,
                    &theme,
                    &mut scope,
                    &mut history,
                    text,
                    true,
                );
            });
        },
    );
    Frame {
        shapes: output.shapes,
    }
}

fn set_language(params: &ShirabeParams, language: UiLanguage) {
    unsafe {
        params
            .ui_language
            .as_ptr()
            .set_normalized_value(params.ui_language.preview_normalized(language));
    }
    assert_eq!(params.ui_language.value(), language);
}

fn set_custom_target(params: &ShirabeParams) {
    unsafe {
        params.loudness_target.as_ptr().set_normalized_value(
            params
                .loudness_target
                .preview_normalized(LoudnessTarget::Custom),
        );
    }
    assert_eq!(params.loudness_target.value(), LoudnessTarget::Custom);
}

fn live_meters() -> SharedMeters {
    let shared = SharedMeters::new();
    shared.configure(2, 48_000.0);
    shared.stereo.correlation.store(Some(1.0));
    shared.stereo.balance.store(Some(-0.6));
    shared.loudness.integrated.store(Some(-23.0));
    shared.loudness.range.store(Some(0.0));
    shared.loudness.plr.store(Some(16.0));
    shared.loudness.max_momentary.store(Some(-22.0));
    shared.loudness.max_short_term.store(Some(-23.0));
    shared
        .programme_seconds
        .store(83.0, std::sync::atomic::Ordering::Relaxed);
    shared
}

#[test]
fn english_is_the_default_view() {
    let params = ShirabeParams::default();
    let shared = live_meters();
    let frame = render(
        &params,
        &shared,
        Text::new(Lang::English, true),
        vec2(640.0, 380.0),
        &TestGuiContext,
    );
    for label in [
        "Layout",
        "INTEGRATED",
        "Test oscillator",
        "PEAK dBFS / TP dBTP",
        "Reset",
        "Pause",
    ] {
        frame.find(label);
    }
}

#[test]
fn japanese_translates_the_editor_labels() {
    let params = ShirabeParams::default();
    set_language(&params, UiLanguage::Japanese);
    let shared = live_meters();
    let frame = render(
        &params,
        &shared,
        Text::new(Lang::Japanese, true),
        vec2(640.0, 380.0),
        &TestGuiContext,
    );
    for label in [
        "配置",
        "統合ラウドネス",
        "テストオシレーター",
        "ピーク dBFS / TP dBTP",
        "リセット",
        "一時停止",
        "時間",
        "目標",
    ] {
        frame.find(label);
    }
    // The English prose of these controls must be gone, not just accompanied.
    for label in ["Layout", "INTEGRATED", "Test oscillator", "Reset"] {
        assert!(
            frame.texts().iter().all(|(text, _, _)| *text != label),
            "{label:?} is still drawn in Japanese"
        );
    }
}

#[test]
fn both_languages_fit_the_minimum_window() {
    for (language, text) in [
        (UiLanguage::English, Text::new(Lang::English, true)),
        (UiLanguage::Japanese, Text::new(Lang::Japanese, true)),
    ] {
        let params = ShirabeParams::default();
        set_language(&params, language);
        set_custom_target(&params);
        let shared = live_meters();
        // Worst case for the header: the longest rate, correlation and balance labels.
        shared.configure(2, 192_000.0);
        let frame = render(&params, &shared, text, vec2(640.0, 380.0), &TestGuiContext);

        for (label, bounds, clip) in frame.texts() {
            assert!(
                clip.contains_rect(bounds),
                "{language:?}: {label:?} is clipped ({bounds:?} outside {clip:?})"
            );
            assert!(
                bounds.right() <= 632.0,
                "{language:?}: {label:?} runs past the right margin"
            );
        }

        let header = frame.header();
        for pair in header.windows(2) {
            assert!(
                pair[0].1.right() <= pair[1].1.left() + 0.5,
                "{language:?}: {:?} overlaps {:?}",
                pair[0].0,
                pair[1].0
            );
        }
        assert!(
            header
                .iter()
                .any(|(label, _, _)| *label == language.lang().code()),
            "{language:?}: the language selector is missing"
        );
    }
}

#[test]
fn the_read_out_rows_and_the_lra_note_fit_in_japanese() {
    let params = ShirabeParams::default();
    set_language(&params, UiLanguage::Japanese);
    set_custom_target(&params);
    let shared = live_meters();
    // Still inside the 60 s LRA settling window, so the note is drawn too.
    shared
        .programme_seconds
        .store(30.0, std::sync::atomic::Ordering::Relaxed);
    let frame = render(
        &params,
        &shared,
        Text::new(Lang::Japanese, true),
        vec2(640.0, 380.0),
        &TestGuiContext,
    );

    let (heading, _) = frame.find("統合ラウドネス");
    let theme = Theme::dark();
    let panel = frame
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
        .expect("read-out panel");
    let mut bottom = heading.bottom();
    for (label, value) in [
        ("LRA", "0.0 LU *"),
        ("PLR", "16.0 LU"),
        ("M 最大", "-22.0 LUFS"),
        ("S 最大", "-23.0 LUFS"),
        ("TP 最大", "-inf dBTP"),
        ("時間", "0:30"),
        ("目標", "-14 / -1"),
    ] {
        let (row, _) = frame.find(label);
        let (value_bounds, _) = frame.find(value);
        assert!(
            row.right() <= value_bounds.left(),
            "{label} overlaps its value"
        );
        let row = row.union(value_bounds);
        assert!(
            panel.contains_rect(row) && row.top() >= bottom,
            "{label} is clipped or overlaps another row"
        );
        bottom = row.bottom();
    }
    let (note, _) = frame.find("＊LRA は 60 秒まで不安定");
    assert!(
        panel.contains_rect(note) && note.top() >= bottom,
        "the Japanese LRA note is clipped or overlaps the rows"
    );
}

#[test]
fn changing_the_language_does_not_touch_the_measurement() {
    let params = ShirabeParams::default();
    let shared = live_meters();
    let english = render(
        &params,
        &shared,
        Text::new(Lang::English, true),
        vec2(820.0, 500.0),
        &TestGuiContext,
    );
    let (english_value, _) = english.find("-23.0");

    set_language(&params, UiLanguage::Japanese);
    let japanese = render(
        &params,
        &shared,
        Text::new(Lang::Japanese, true),
        vec2(820.0, 500.0),
        &TestGuiContext,
    );
    let (japanese_value, _) = japanese.find("-23.0");
    assert_eq!(english_value, japanese_value);

    assert_eq!(shared.loudness.integrated.load(), Some(-23.0));
    assert_eq!(shared.loudness.range.load(), Some(0.0));
    assert_eq!(
        shared
            .programme_seconds
            .load(std::sync::atomic::Ordering::Relaxed),
        83.0
    );
    assert_eq!(params.ui_language.value(), UiLanguage::Japanese);
}

#[test]
fn the_note_beside_the_oscillator_fits_at_the_minimum_size() {
    for (language, text) in [
        (UiLanguage::English, Text::new(Lang::English, true)),
        (UiLanguage::Japanese, Text::new(Lang::Japanese, true)),
    ] {
        // Enabled (test tone), disabled in a plug-in (pass-through) and disabled in the
        // standalone (measuring the selected source) are three different notes.
        let cases: [(&dyn GuiContext, bool, &str); 3] = [
            (&TestGuiContext, true, text.osc_replaces_input()),
            (&TestGuiContext, false, text.pass_through()),
            (&StandaloneTestContext, false, text.measuring_source()),
        ];
        for (context, oscillator, note) in cases {
            let params = ShirabeParams::default();
            set_language(&params, language);
            if oscillator {
                // SAFETY: the pointer belongs to `params`, which outlives the frame.
                unsafe {
                    params.osc_enabled.as_ptr().set_normalized_value(1.0);
                }
            }
            assert_eq!(params.osc_enabled.value(), oscillator);
            let shared = live_meters();
            let frame = render(&params, &shared, text, vec2(640.0, 380.0), context);
            frame.find(note);
            for (label, bounds, clip) in frame.texts() {
                assert!(
                    clip.contains_rect(bounds) && bounds.right() <= 632.0,
                    "{language:?} {oscillator}: {label:?} {bounds:?} clip {clip:?}"
                );
            }
        }
    }
}
