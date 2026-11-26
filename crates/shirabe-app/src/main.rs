//! Shirabe Meter Suite standalone application.
//!
//! Measures an audio input device, what the computer plays, or a local WAV / FLAC file
//! played through an output device, with the same meters and the same view as the plug-in.

// Release builds on Windows should not open a console window next to the GUI.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod appearance;
mod audio;
mod context;
mod decode;
mod fonts;
mod output;
mod player;
#[cfg(target_os = "linux")]
mod pulse;
mod resample;
mod ring;
mod settings;
#[cfg(target_os = "macos")]
mod tap;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;
use nih_plug::prelude::ParamSetter;
use shirabe_plugin::engine::MeterEngine;
use shirabe_plugin::shared::SharedMeters;
use shirabe_plugin::{
    editor::{self, ScopeView},
    ShirabeParams,
};
use shirabe_ui::Theme;

use appearance::Appearance;
use audio::{Choice, Source};
use context::StandaloneContext;
use decode::format_time;
use output::DeviceRef;
use player::Playback;
use settings::Settings;

/// Result of the last appearance import or export, shown in its window.
struct AppearanceStatus {
    text: String,
    error: bool,
}

impl AppearanceStatus {
    fn ok(text: String) -> Self {
        Self { text, error: false }
    }

    fn error(text: String) -> Self {
        Self { text, error: true }
    }
}

struct App {
    params: Arc<ShirabeParams>,
    shared: Arc<SharedMeters>,
    context: StandaloneContext,
    theme: Theme,
    source: Source,
    running: Option<audio::Running>,
    start_error: Option<String>,
    hosts: Vec<String>,
    host: String,
    inputs: Vec<Choice>,
    playing: Vec<Choice>,
    outputs: Vec<DeviceRef>,
    /// Output device for file playback; `None` uses the default output.
    output: Option<DeviceRef>,
    /// Path typed or dropped for file playback.
    file: String,
    /// Parameter values at the last save, to save again only after a change.
    saved_params: std::collections::BTreeMap<String, f32>,
    /// Display-only history of the Lissajous view.
    scope: ScopeView,
    last_check: std::time::Instant,
    /// Whether the appearance preset window is open.
    show_appearance: bool,
    /// Path typed for appearance import and export.
    appearance_path: String,
    /// Export target waiting for the user to confirm replacing it.
    appearance_pending_overwrite: Option<PathBuf>,
    /// Outcome of the last appearance action.
    appearance_status: Option<AppearanceStatus>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = Settings::load();
        // An imported theme is saved with the settings; unreadable colours fall back to the
        // built-in dark theme instead of refusing to start.
        let theme = settings
            .theme
            .as_ref()
            .and_then(|spec| spec.to_theme().ok())
            .unwrap_or_default();
        // eframe follows the system theme by default (light on many Windows setups); the
        // meters are designed for the dark theme only.
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        apply_visuals(&cc.egui_ctx, &theme);
        fonts::install(&cc.egui_ctx);

        let params = Arc::new(ShirabeParams::default());
        context::load_params(params.as_ref(), &settings.params);
        let source = settings
            .source
            .clone()
            .unwrap_or_else(|| audio::default_source().unwrap_or_default());
        let file = match &source {
            Source::File { path } => path.clone(),
            _ => String::new(),
        };

        let hosts = audio::hosts();
        let host = match &source {
            Source::Input { host, .. } | Source::Output { host, .. } => host.clone(),
            _ => cpal_default_host(&hosts),
        };
        let mut app = Self {
            params,
            shared: Arc::new(SharedMeters::new()),
            context: StandaloneContext,
            theme,
            source: Source::None,
            running: None,
            start_error: None,
            inputs: audio::inputs(&host),
            playing: audio::playing(&host),
            outputs: output::outputs(&host),
            output: settings.output.clone(),
            file,
            hosts,
            host,
            saved_params: settings.params.clone(),
            scope: ScopeView::new(),
            last_check: std::time::Instant::now(),
            show_appearance: false,
            appearance_path: settings::default_appearance_path()
                .map_or_else(String::new, |p| p.display().to_string()),
            appearance_pending_overwrite: None,
            appearance_status: None,
        };
        // A saved file is not started automatically; the user presses Play.
        if matches!(source, Source::File { .. }) {
            app.source = source;
        } else {
            app.select(source);
        }
        app
    }

    /// Switches to `source`; falls back to no input if it cannot be opened.
    fn select(&mut self, source: Source) {
        // Stop the old stream first: some drivers allow only one stream per device.
        self.running = None;
        if let Source::File { path } = &source {
            if path.trim().is_empty() {
                // The mode is valid without a file; playback starts once one is chosen.
                self.source = source;
                self.start_error = None;
                return;
            }
        }
        let engine = MeterEngine::new(self.params.clone(), self.shared.clone());
        match audio::start(&source, engine, self.output.as_ref()) {
            Ok(r) => {
                self.running = Some(r);
                self.start_error = None;
                self.source = source;
            }
            Err(e) => {
                self.start_error = Some(format!("{e:#}"));
                // Silence keeps the meters falling instead of freezing at the last value.
                let engine = MeterEngine::new(self.params.clone(), self.shared.clone());
                self.running = audio::start(&Source::None, engine, None).ok();
                if !matches!(source, Source::File { .. }) {
                    self.source = Source::None;
                } else {
                    // Keep the file mode so that the path can be corrected.
                    self.source = source;
                }
            }
        }
    }

    fn save_settings(&mut self) {
        self.saved_params = context::save_params(self.params.as_ref());
        Settings {
            source: Some(self.source.clone()),
            params: self.saved_params.clone(),
            output: self.output.clone(),
            theme: Some(appearance::ThemeSpec::from_theme(&self.theme)),
        }
        .save();
    }

    /// Saves the settings about once a second when a parameter has changed, so they survive
    /// an abnormal exit.
    fn save_if_changed(&mut self) {
        if self.last_check.elapsed() < std::time::Duration::from_secs(1) {
            return;
        }
        self.last_check = std::time::Instant::now();
        if context::save_params(self.params.as_ref()) != self.saved_params {
            self.save_settings();
        }
    }

    fn refresh_devices(&mut self) {
        self.inputs = audio::inputs(&self.host);
        self.playing = audio::playing(&self.host);
        self.outputs = output::outputs(&self.host);
        // A device of another audio API cannot be used with this one.
        if self
            .output
            .as_ref()
            .is_some_and(|device| device.host != self.host)
        {
            self.output = None;
        }
    }

    fn playback(&self) -> Option<&Playback> {
        self.running.as_ref().and_then(|r| r.playback.as_ref())
    }

    fn audio_bar(&mut self, ui: &mut egui::Ui) {
        let mut new_source = None;
        ui.horizontal(|ui| {
            ui.label("Audio API");
            let previous_host = self.host.clone();
            egui::ComboBox::from_id_salt("audio-host")
                .selected_text(self.host.as_str())
                .show_ui(ui, |ui| {
                    for h in &self.hosts {
                        ui.selectable_value(&mut self.host, h.clone(), h.as_str());
                    }
                });
            if self.host != previous_host {
                self.refresh_devices();
            }

            ui.label("Source");
            let current = match &self.source {
                Source::File { path } => file_label(path),
                _ => self
                    .inputs
                    .iter()
                    .chain(&self.playing)
                    .find(|c| c.source == self.source)
                    .map_or_else(|| "None".to_owned(), |c| c.label.clone()),
            };
            egui::ComboBox::from_id_salt("audio-source")
                .width(320.0)
                .selected_text(current.as_str())
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.source == Source::None, "None")
                        .clicked()
                    {
                        new_source = Some(Source::None);
                    }
                    let groups = [
                        ("Inputs", &self.inputs),
                        ("Playing on this computer", &self.playing),
                    ];
                    for (title, list) in groups {
                        if list.is_empty() {
                            continue;
                        }
                        ui.separator();
                        ui.label(
                            egui::RichText::new(title)
                                .small()
                                .color(self.theme.text_dim),
                        );
                        for c in list {
                            if ui
                                .selectable_label(c.source == self.source, c.label.as_str())
                                .clicked()
                            {
                                new_source = Some(c.source.clone());
                            }
                        }
                    }
                    ui.separator();
                    ui.label(
                        egui::RichText::new("File playback")
                            .small()
                            .color(self.theme.text_dim),
                    );
                    let selected = matches!(self.source, Source::File { .. });
                    if ui.selectable_label(selected, "Audio file").clicked() {
                        new_source = Some(Source::File {
                            path: self.file.trim().to_owned(),
                        });
                    }
                });
            if ui
                .button("Refresh")
                .on_hover_text("Search for devices again")
                .clicked()
            {
                self.hosts = audio::hosts();
                self.refresh_devices();
            }
        });
        ui.horizontal(|ui| {
            let dim = self.theme.text_dim;
            if let Some(r) = &self.running {
                ui.label(egui::RichText::new(&r.description).color(dim));
                if let Some(e) = r.error.lock().unwrap().as_ref() {
                    ui.label(
                        egui::RichText::new(format!("Stream error: {e}")).color(self.theme.over),
                    );
                }
            }
            if let Some(e) = &self.start_error {
                ui.label(egui::RichText::new(format!("Cannot start: {e}")).color(self.theme.over));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("Appearance")
                    .on_hover_text("Import or export the layout and theme colours")
                    .clicked()
                {
                    self.show_appearance = !self.show_appearance;
                }
            });
        });
        if let Some(s) = new_source {
            if s != self.source {
                self.select(s);
                self.save_settings();
            }
        }
    }

    /// Output device, file and transport; only shown for file playback.
    fn playback_bar(&mut self, ui: &mut egui::Ui) {
        let (paused, ended, position, duration) = match self.playback() {
            Some(p) => (p.paused(), p.ended(), p.position(), p.duration()),
            None => (false, false, 0.0, None),
        };
        let mut new_output: Option<DeviceRef> = None;
        let mut start = false;
        let mut toggle_pause = false;
        ui.horizontal(|ui| {
            ui.label("Output");
            let current = self
                .output
                .as_ref()
                .map_or_else(|| "Default".to_owned(), |d| d.label().to_owned());
            egui::ComboBox::from_id_salt("playback-output")
                .width(210.0)
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for device in &self.outputs {
                        if ui
                            .selectable_label(self.output.as_ref() == Some(device), device.label())
                            .clicked()
                        {
                            new_output = Some(device.clone());
                        }
                    }
                });

            ui.label("File");
            let mut file = std::mem::take(&mut self.file);
            let response = ui.add(
                egui::TextEdit::singleline(&mut file)
                    .desired_width(280.0)
                    .hint_text("WAV or FLAC path; a dropped file works too"),
            );
            self.file = file;
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                start = true;
            }

            if self.playback().is_some() {
                if ui
                    .button(if paused { "Resume" } else { "Pause" })
                    .on_hover_text("Pause keeps the position; the meters fall")
                    .clicked()
                {
                    toggle_pause = true;
                }
                if ui
                    .button("From start")
                    .on_hover_text("Play the file again from the beginning")
                    .clicked()
                {
                    start = true;
                }
            } else if ui
                .button("Play")
                .on_hover_text("Play the file and measure the stored samples")
                .clicked()
            {
                start = true;
            }
            let total = duration.map_or_else(|| "-:--".to_owned(), format_time);
            let mut text = format!("{} / {}", format_time(position), total);
            if ended {
                text.push_str("  (end)");
            }
            ui.label(egui::RichText::new(text).color(self.theme.text_dim));
        });

        if let Some(device) = new_output {
            if self.output.as_ref() != Some(&device) {
                self.output = Some(device);
                if let Source::File { path } = self.source.clone() {
                    if !path.trim().is_empty() {
                        self.select(Source::File { path });
                    }
                }
                self.save_settings();
            }
        }
        if toggle_pause {
            if let Some(p) = self.playback() {
                p.set_paused(!p.paused());
            }
        }
        if start {
            self.select(Source::File {
                path: self.file.trim().to_owned(),
            });
            self.save_settings();
        }
    }

    /// Appearance preset window: a file path and the two explicit actions.
    fn appearance_window(&mut self, ctx: &egui::Context) {
        if !self.show_appearance {
            return;
        }
        let mut open = true;
        egui::Window::new("Appearance preset")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(440.0)
            .show(ctx, |ui| self.appearance_contents(ui));
        self.show_appearance = open;
    }

    fn appearance_contents(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new(
                "Layout and theme colours only. Sources, targets, the oscillator and measured \
                 values stay unchanged.",
            )
            .small()
            .color(ui.visuals().weak_text_color()),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("File");
            let mut path = std::mem::take(&mut self.appearance_path);
            ui.add(
                egui::TextEdit::singleline(&mut path)
                    .desired_width(310.0)
                    .hint_text("Path to an appearance .json file"),
            );
            self.appearance_path = path;
        });
        ui.horizontal(|ui| {
            if ui
                .button("Import")
                .on_hover_text("Apply the layout and colours from this file")
                .clicked()
            {
                self.import_appearance(ui.ctx());
            }
            if ui
                .button("Export")
                .on_hover_text("Write the current layout and colours to this file")
                .clicked()
            {
                self.export_appearance(false);
            }
            if self.appearance_pending_overwrite.is_some() {
                if ui
                    .button("Overwrite")
                    .on_hover_text("Replace the file that already exists")
                    .clicked()
                {
                    self.export_appearance(true);
                }
                if ui.button("Cancel").clicked() {
                    self.appearance_pending_overwrite = None;
                    self.appearance_status = None;
                }
            }
        });
        if let Some(status) = &self.appearance_status {
            let color = if status.error {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().weak_text_color()
            };
            ui.label(egui::RichText::new(&status.text).color(color));
        }
    }

    fn import_appearance(&mut self, ctx: &egui::Context) {
        let path = self.appearance_path.trim().to_owned();
        if path.is_empty() {
            self.appearance_status = Some(AppearanceStatus::error(
                "Choose a file path to import.".to_owned(),
            ));
            return;
        }
        // `import` validates the whole file before it touches the layout parameter.
        let result = {
            let setter = ParamSetter {
                raw_context: &self.context,
            };
            appearance::import(Path::new(&path), &self.params, &setter)
        };
        match result {
            Ok(imported) => {
                self.theme = imported.theme;
                apply_visuals(ctx, &self.theme);
                self.appearance_pending_overwrite = None;
                // The layout is a parameter (saved by the autosave) but the theme is not,
                // so save both now.
                self.save_settings();
                self.appearance_status = Some(AppearanceStatus::ok(format!("Imported {path}.")));
            }
            Err(error) => {
                self.appearance_status =
                    Some(AppearanceStatus::error(format!("Cannot import: {error}")));
            }
        }
    }

    /// Writes the current appearance. `overwrite` is only set after the user confirms the
    /// file that was reported as existing, and that exact file is replaced.
    fn export_appearance(&mut self, overwrite: bool) {
        let typed = self.appearance_path.trim().to_owned();
        let path = if overwrite {
            self.appearance_pending_overwrite
                .clone()
                .unwrap_or_else(|| PathBuf::from(&typed))
        } else {
            PathBuf::from(&typed)
        };
        if path.as_os_str().is_empty() {
            self.appearance_status = Some(AppearanceStatus::error(
                "Choose a file path to export.".to_owned(),
            ));
            return;
        }
        let current = Appearance::current(&self.params, &self.theme);
        match appearance::save(&current, &path, overwrite) {
            Ok(()) => {
                self.appearance_pending_overwrite = None;
                self.appearance_status = Some(AppearanceStatus::ok(format!(
                    "Exported {}.",
                    path.display()
                )));
            }
            Err(appearance::Error::Exists) => {
                self.appearance_pending_overwrite = Some(path.clone());
                self.appearance_status = Some(AppearanceStatus::error(format!(
                    "{} already exists; press Overwrite to replace it.",
                    path.display()
                )));
            }
            Err(error) => {
                self.appearance_pending_overwrite = None;
                self.appearance_status =
                    Some(AppearanceStatus::error(format!("Cannot export: {error}")));
            }
        }
    }

    /// Starts a file dropped on the window.
    fn take_dropped_file(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .find_map(|f| f.path.as_ref().map(|p| p.display().to_string()))
        });
        if let Some(path) = dropped {
            self.file = path.clone();
            self.select(Source::File { path });
            self.save_settings();
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Meters move continuously, so redraw every frame.
        ctx.request_repaint();
        self.take_dropped_file(ctx);
        egui::TopBottomPanel::top("audio").show(ctx, |ui| {
            self.audio_bar(ui);
            if matches!(self.source, Source::File { .. }) {
                ui.separator();
                self.playback_bar(ui);
            }
        });
        self.save_if_changed();
        egui::CentralPanel::default().show(ctx, |ui| {
            let setter = ParamSetter {
                raw_context: &self.context,
            };
            editor::draw(
                ui,
                &self.params,
                &self.shared,
                &setter,
                &self.theme,
                &mut self.scope,
            );
        });
        self.appearance_window(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_settings();
    }
}

/// Applies the colours that egui paints panels and windows with. The meter widgets take their
/// colours from [`Theme`] directly.
fn apply_visuals(ctx: &egui::Context, theme: &Theme) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = theme.background;
    visuals.window_fill = theme.background;
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
}

/// Label of the file entry in the source list and the combo.
fn file_label(path: &str) -> String {
    if path.trim().is_empty() {
        "Audio file".to_owned()
    } else {
        let name = std::path::Path::new(path)
            .file_name()
            .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned());
        format!("Audio file: {name}")
    }
}

/// The audio API cpal uses by default, if it is in `hosts`.
fn cpal_default_host(hosts: &[String]) -> String {
    let name = cpal::default_host().id().name().to_owned();
    if hosts.contains(&name) {
        name
    } else {
        hosts.first().cloned().unwrap_or_default()
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Shirabe Meter Suite")
            .with_inner_size([820.0, 560.0])
            .with_min_inner_size([640.0, 440.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Shirabe Meter Suite",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
