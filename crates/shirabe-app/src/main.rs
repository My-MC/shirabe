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
mod file_io;
mod file_out;
mod measurement;
mod output;
mod player;
#[cfg(target_os = "linux")]
mod pulse;
mod resample;
mod ring;
mod settings;
#[cfg(target_os = "macos")]
mod tap;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;
use nih_plug::prelude::ParamSetter;
use shirabe_plugin::engine::MeterEngine;
use shirabe_plugin::playout::Playout;
use shirabe_plugin::shared::SharedMeters;
use shirabe_plugin::{
    editor::{self, HistoryView, ScopeView, SpectrumView},
    fonts, ShirabeParams,
};
use shirabe_ui::i18n::Text;
use shirabe_ui::Theme;

use appearance::Appearance;
use audio::{Choice, Source};
use context::StandaloneContext;
use decode::format_time;
use file_io::Worker;
use output::DeviceRef;
use player::Playback;
use settings::Settings;

/// Result of the last appearance import or export, shown in its window.
struct FileStatus {
    text: String,
    error: bool,
}

impl FileStatus {
    fn ok(text: String) -> Self {
        Self { text, error: false }
    }

    fn error(text: String) -> Self {
        Self { text, error: true }
    }
}

enum FileResult {
    Import(String, Result<Appearance, appearance::Error>),
    Export {
        path: String,
        revision: u64,
        overwrite: bool,
        result: Result<(), appearance::Error>,
    },
    Measurement {
        path: String,
        revision: u64,
        overwrite: bool,
        result: Result<(), file_out::WriteError>,
    },
    Settings(anyhow::Result<()>),
}

struct App {
    params: Arc<ShirabeParams>,
    shared: Arc<SharedMeters>,
    context: StandaloneContext,
    theme: Theme,
    /// Whether the installed fonts cover the Japanese catalogue; checked in the first frame,
    /// because egui has no fonts before `Context::run()`.
    japanese: Option<bool>,
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
    /// Parameter values in the last requested settings snapshot.
    saved_params: std::collections::BTreeMap<String, f32>,
    /// Display-only history of the Lissajous view.
    scope: ScopeView,
    playout: Playout,
    /// Display-only history of the loudness graph.
    history: HistoryView,
    spectrum: SpectrumView,
    last_check: Instant,
    /// Whether the appearance preset window is open.
    show_appearance: bool,
    appearance_target: appearance::Target,
    /// Outcome of the last appearance action.
    appearance_status: Option<FileStatus>,
    /// Whether the measurement export window is open.
    show_measurement: bool,
    measurement_target: file_out::Target,
    measurement_format: measurement::Format,
    measurement_include_path: bool,
    /// Outcome of the last measurement export.
    measurement_status: Option<FileStatus>,
    file_io: std::io::Result<Worker<FileResult>>,
    /// Only the latest settings snapshot needs saving when the worker is busy.
    pending_settings: Option<Settings>,
    import_awaiting_save: Option<String>,
    import_save_warning: Option<String>,
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
        let repaint = cc.egui_ctx.clone();
        let mut app = Self {
            params,
            shared: Arc::new(SharedMeters::new()),
            context: StandaloneContext,
            theme,
            japanese: None,
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
            playout: Playout::default(),
            history: HistoryView::new(),
            spectrum: SpectrumView::new(),
            last_check: Instant::now(),
            show_appearance: false,
            appearance_target: appearance::Target::new(
                settings::default_appearance_path()
                    .map_or_else(String::new, |p| p.display().to_string()),
            ),
            appearance_status: None,
            show_measurement: false,
            measurement_target: file_out::Target::new(
                settings::default_measurement_path(measurement::Format::Csv.extension())
                    .map_or_else(String::new, |p| p.display().to_string()),
            ),
            measurement_format: measurement::Format::Csv,
            measurement_include_path: false,
            measurement_status: None,
            file_io: Worker::new(move || repaint.request_repaint()),
            pending_settings: None,
            import_awaiting_save: None,
            import_save_warning: None,
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
                self.shared.clear_display_capture();
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
        self.pending_settings = Some(Settings {
            source: Some(self.source.clone()),
            params: self.saved_params.clone(),
            output: self.output.clone(),
            theme: Some(appearance::ThemeSpec::from_theme(&self.theme)),
        });
    }

    /// Saves the settings about once a second when a parameter has changed, so they survive
    /// an abnormal exit.
    fn save_if_changed(&mut self) {
        if self.last_check.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_check = Instant::now();
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

    fn audio_bar(&mut self, ui: &mut egui::Ui, text: Text) {
        let mut new_source = None;
        ui.horizontal(|ui| {
            ui.label(text.audio_api());
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

            ui.label(text.source());
            let current = match &self.source {
                Source::File { path } => file_label(text, path),
                _ => self
                    .inputs
                    .iter()
                    .chain(&self.playing)
                    .find(|c| c.source == self.source)
                    .map_or_else(|| text.none().to_owned(), |c| c.label.clone()),
            };
            egui::ComboBox::from_id_salt("audio-source")
                .width(320.0)
                .selected_text(current.as_str())
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.source == Source::None, text.none())
                        .clicked()
                    {
                        new_source = Some(Source::None);
                    }
                    let groups = [
                        (text.inputs(), &self.inputs),
                        (text.playing_here(), &self.playing),
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
                        egui::RichText::new(text.file_playback())
                            .small()
                            .color(self.theme.text_dim),
                    );
                    let selected = matches!(self.source, Source::File { .. });
                    if ui.selectable_label(selected, text.audio_file()).clicked() {
                        new_source = Some(Source::File {
                            path: self.file.trim().to_owned(),
                        });
                    }
                });
            if ui
                .button(text.refresh())
                .on_hover_text(text.refresh_hint())
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
                    ui.label(egui::RichText::new(text.stream_error(e)).color(self.theme.over));
                }
            }
            if let Some(e) = &self.start_error {
                ui.label(egui::RichText::new(text.cannot_start(e)).color(self.theme.over));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(text.appearance())
                    .on_hover_text(text.appearance_hint())
                    .clicked()
                {
                    self.show_appearance = !self.show_appearance;
                    self.appearance_target.invalidate();
                }
                if ui
                    .button(text.measurement_export())
                    .on_hover_text(text.measurement_export_hint())
                    .clicked()
                {
                    self.show_measurement = !self.show_measurement;
                    self.measurement_target.invalidate();
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
    fn playback_bar(&mut self, ui: &mut egui::Ui, text: Text) {
        let (paused, ended, position, duration) = match self.playback() {
            Some(p) => (p.paused(), p.ended(), p.position(), p.duration()),
            None => (false, false, 0.0, None),
        };
        let mut new_output: Option<DeviceRef> = None;
        let mut start = false;
        let mut toggle_pause = false;
        ui.horizontal(|ui| {
            ui.label(text.output());
            let current = self.output.as_ref().map_or_else(
                || text.default_output().to_owned(),
                |d| d.label().to_owned(),
            );
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

            ui.label(text.file());
            let mut file = std::mem::take(&mut self.file);
            let response = ui.add(
                egui::TextEdit::singleline(&mut file)
                    .desired_width(280.0)
                    .hint_text(text.file_hint()),
            );
            self.file = file;
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                start = true;
            }

            if self.playback().is_some() {
                let pause = if paused {
                    text.resume_playback()
                } else {
                    text.pause_playback()
                };
                if ui
                    .button(pause)
                    .on_hover_text(text.pause_playback_hint())
                    .clicked()
                {
                    toggle_pause = true;
                }
                if ui
                    .button(text.from_start())
                    .on_hover_text(text.from_start_hint())
                    .clicked()
                {
                    start = true;
                }
            } else if ui
                .button(text.play())
                .on_hover_text(text.play_hint())
                .clicked()
            {
                start = true;
            }
            let total = duration.map_or_else(|| "-:--".to_owned(), format_time);
            let mut label = format!("{} / {}", format_time(position), total);
            if ended {
                label.push_str("  ");
                label.push_str(text.end());
            }
            ui.label(egui::RichText::new(label).color(self.theme.text_dim));
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
    fn appearance_window(&mut self, ctx: &egui::Context, text: Text) {
        if !self.show_appearance {
            return;
        }
        let mut open = true;
        let window = egui::Window::new(text.appearance_title())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(440.0)
            .show(ctx, |ui| self.appearance_contents(ui, text));
        if !open
            || window.is_some_and(|window| {
                ctx.input(|input| {
                    input.pointer.any_pressed()
                        && input
                            .pointer
                            .interact_pos()
                            .is_some_and(|p| !window.response.rect.contains(p))
                })
            })
        {
            self.appearance_target.invalidate();
        }
        self.show_appearance = open;
    }

    fn appearance_contents(&mut self, ui: &mut egui::Ui, text: Text) {
        ui.label(
            egui::RichText::new(text.appearance_note())
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(text.file());
            let mut path = self.appearance_target.path().to_owned();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut path)
                        .desired_width(310.0)
                        .hint_text(text.appearance_path_hint()),
                )
                .changed()
            {
                self.appearance_target.set_path(path);
            }
        });
        ui.horizontal(|ui| {
            let enabled = !self.file_io_busy() && self.import_awaiting_save.is_none();
            if ui
                .add_enabled(enabled, egui::Button::new(text.import()))
                .on_hover_text(text.import_hint())
                .clicked()
            {
                self.import_appearance(text);
            }
            if ui
                .add_enabled(enabled, egui::Button::new(text.export()))
                .on_hover_text(text.export_hint())
                .clicked()
            {
                self.export_appearance(false, text);
            }
            if self.appearance_target.pending().is_some() {
                if ui
                    .add_enabled(enabled, egui::Button::new(text.overwrite()))
                    .on_hover_text(text.overwrite_hint())
                    .clicked()
                {
                    self.export_appearance(true, text);
                }
                if ui.button(text.cancel()).clicked() {
                    self.appearance_target.invalidate();
                    self.appearance_status = None;
                }
            }
        });
        if let Some(path) = self.appearance_target.pending() {
            ui.colored_label(ui.visuals().error_fg_color, text.file_exists(path));
        } else if let Some(status) = &self.appearance_status {
            let color = if status.error {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().weak_text_color()
            };
            ui.label(egui::RichText::new(&status.text).color(color));
        }
    }

    fn import_appearance(&mut self, text: Text) {
        let Some((path, _)) = self.appearance_target.begin(false) else {
            self.appearance_status = Some(FileStatus::error(text.choose_import_path().to_owned()));
            return;
        };
        let result = self.start_file_job(move || {
            let result = appearance::load(Path::new(&path));
            FileResult::Import(path, result)
        });
        self.appearance_status = Some(match result {
            Ok(()) => FileStatus::ok(text.reading_appearance().into()),
            Err(error) => FileStatus::error(text.cannot_import(&error.to_string())),
        });
    }

    fn export_appearance(&mut self, overwrite: bool, text: Text) {
        let Some((path, revision)) = self.appearance_target.begin(overwrite) else {
            let message = if overwrite {
                text.overwrite_needs_export()
            } else {
                text.choose_export_path()
            };
            self.appearance_status = Some(FileStatus::error(message.into()));
            return;
        };
        let current = Appearance::current(&self.params, &self.theme);
        let shown = path.clone();
        let result = self.start_file_job(move || {
            let result = appearance::save(&current, Path::new(&path), overwrite);
            FileResult::Export {
                path,
                revision,
                overwrite,
                result,
            }
        });
        self.appearance_status = Some(match result {
            Ok(()) => FileStatus::ok(text.writing_appearance().into()),
            Err(error) => FileStatus::error(text.cannot_export(&shown, &error.to_string())),
        });
    }

    /// Measurement export window: a path, the format and one explicit action.
    fn measurement_window(&mut self, ctx: &egui::Context, text: Text) {
        if !self.show_measurement {
            return;
        }
        let mut open = true;
        let window = egui::Window::new(text.measurement_export())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| self.measurement_contents(ui, text));
        if !open
            || window.is_some_and(|window| {
                ctx.input(|input| {
                    input.pointer.any_pressed()
                        && input
                            .pointer
                            .interact_pos()
                            .is_some_and(|p| !window.response.rect.contains(p))
                })
            })
        {
            self.measurement_target.invalidate();
        }
        self.show_measurement = open;
    }

    fn measurement_contents(&mut self, ui: &mut egui::Ui, text: Text) {
        ui.label(
            egui::RichText::new(text.measurement_export_note())
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(text.file());
            let mut path = self.measurement_target.path().to_owned();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut path)
                        .desired_width(330.0)
                        .hint_text(text.measurement_path_hint()),
                )
                .changed()
            {
                self.measurement_target.set_path(path);
            }
        });
        ui.horizontal(|ui| {
            ui.label(text.measurement_format());
            for format in [measurement::Format::Csv, measurement::Format::Json] {
                let label = match format {
                    measurement::Format::Csv => "CSV",
                    measurement::Format::Json => "JSON",
                };
                if ui
                    .selectable_label(self.measurement_format == format, label)
                    .clicked()
                    && self.measurement_format != format
                {
                    self.change_measurement_format(format);
                }
            }
        });
        let mut include = self.measurement_include_path;
        if ui
            .checkbox(&mut include, text.include_source_path())
            .on_hover_text(text.include_source_path_hint())
            .changed()
        {
            self.measurement_include_path = include;
            self.measurement_target.invalidate();
        }
        ui.horizontal(|ui| {
            let enabled = !self.file_io_busy();
            if ui
                .add_enabled(enabled, egui::Button::new(text.export()))
                .on_hover_text(text.measurement_export_hint())
                .clicked()
            {
                self.export_measurement(false, text);
            }
            if self.measurement_target.pending().is_some() {
                if ui
                    .add_enabled(enabled, egui::Button::new(text.overwrite()))
                    .on_hover_text(text.overwrite_hint())
                    .clicked()
                {
                    self.export_measurement(true, text);
                }
                if ui.button(text.cancel()).clicked() {
                    self.measurement_target.invalidate();
                    self.measurement_status = None;
                }
            }
        });
        if let Some(path) = self.measurement_target.pending() {
            ui.colored_label(ui.visuals().error_fg_color, text.file_exists(path));
        } else if let Some(status) = &self.measurement_status {
            let color = if status.error {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().weak_text_color()
            };
            ui.label(egui::RichText::new(&status.text).color(color));
        }
    }

    /// Switches the file format; the default file name follows it.
    fn change_measurement_format(&mut self, format: measurement::Format) {
        let old_default = settings::default_measurement_path(self.measurement_format.extension())
            .map(|path| path.display().to_string());
        if old_default.as_deref() == Some(self.measurement_target.path()) {
            if let Some(path) = settings::default_measurement_path(format.extension()) {
                self.measurement_target.set_path(path.display().to_string());
            }
        }
        self.measurement_format = format;
        self.measurement_target.invalidate();
    }

    fn export_measurement(&mut self, overwrite: bool, text: Text) {
        let Some((path, revision)) = self.measurement_target.begin(overwrite) else {
            let message = if overwrite {
                text.overwrite_needs_export()
            } else {
                text.choose_export_path()
            };
            self.measurement_status = Some(FileStatus::error(message.into()));
            return;
        };
        // Copy every value now, on the UI thread, so the file describes this instant even
        // though the measurement keeps running while the worker writes it.
        let snapshot = measurement::Snapshot::capture(
            &self.shared,
            &self.params,
            &self.source,
            self.measurement_include_path,
            SystemTime::now(),
        );
        let format = self.measurement_format;
        let shown = path.clone();
        let result = self.start_file_job(move || {
            let result = measurement::save(&snapshot, format, Path::new(&path), overwrite);
            FileResult::Measurement {
                path,
                revision,
                overwrite,
                result,
            }
        });
        self.measurement_status = Some(match result {
            Ok(()) => FileStatus::ok(text.writing_measurement().into()),
            Err(error) => FileStatus::error(text.cannot_export(&shown, &error.to_string())),
        });
    }

    fn file_io_busy(&self) -> bool {
        self.file_io.as_ref().is_ok_and(Worker::busy)
    }

    fn start_file_job(
        &mut self,
        job: impl FnOnce() -> FileResult + Send + 'static,
    ) -> Result<(), String> {
        self.file_io
            .as_mut()
            .map_err(|e| e.to_string())?
            .start(job)
            .map_err(|e| e.to_string())
    }

    fn start_settings_save(&mut self, text: Text) {
        if self.file_io_busy() {
            return;
        }
        if let Some(settings) = self.pending_settings.take() {
            if let Err(error) = self.start_file_job(move || FileResult::Settings(settings.save())) {
                self.finish_settings_save(Err(anyhow::anyhow!(error)), text);
            }
        }
    }

    fn finish_settings_save(&mut self, result: anyhow::Result<()>, text: Text) {
        let imported = self.import_awaiting_save.is_some();
        if let Some(path) = self.import_awaiting_save.take() {
            self.appearance_status = Some(FileStatus::ok(text.imported(&path)));
        }
        match result {
            Ok(()) => self.import_save_warning = None,
            Err(error) if imported => {
                self.import_save_warning = Some(text.settings_not_saved(&format!("{error:#}")));
            }
            Err(_) => {}
        }
    }

    fn poll_file_job(&mut self, ctx: &egui::Context, text: Text) {
        let Ok(worker) = &mut self.file_io else {
            return;
        };
        let result = match worker.poll() {
            Ok(Some(result)) => result,
            Ok(None) => return,
            Err(error) => {
                if self.import_awaiting_save.is_some() {
                    self.finish_settings_save(Err(error.into()), text);
                } else {
                    self.appearance_status = Some(FileStatus::error(error.to_string()));
                }
                return;
            }
        };
        match result {
            FileResult::Import(path, Ok(imported)) => {
                let setter = ParamSetter {
                    raw_context: &self.context,
                };
                appearance::apply(&imported, &self.params, &setter);
                self.theme = imported.theme;
                apply_visuals(ctx, &self.theme);
                self.appearance_status = Some(FileStatus::ok(text.imported_saving(&path)));
                self.import_awaiting_save = Some(path);
                self.save_settings();
            }
            FileResult::Import(_, Err(error)) => {
                self.appearance_status =
                    Some(FileStatus::error(text.cannot_import(&error.to_string())));
            }
            FileResult::Export {
                path,
                revision,
                overwrite,
                result,
            } => {
                self.appearance_status = match result {
                    Ok(()) => Some(FileStatus::ok(text.exported(&path))),
                    Err(appearance::Error::Exists)
                        if !overwrite
                            && self.appearance_target.offer_overwrite(&path, revision) =>
                    {
                        None
                    }
                    Err(error) => Some(FileStatus::error(
                        text.cannot_export(&path, &error.to_string()),
                    )),
                };
            }
            FileResult::Measurement {
                path,
                revision,
                overwrite,
                result,
            } => {
                self.measurement_status = match result {
                    Ok(()) => Some(FileStatus::ok(text.exported(&path))),
                    Err(file_out::WriteError::Exists)
                        if !overwrite
                            && self.measurement_target.offer_overwrite(&path, revision) =>
                    {
                        None
                    }
                    Err(error) => Some(FileStatus::error(
                        text.cannot_export(&path, &error.to_string()),
                    )),
                };
            }
            FileResult::Settings(result) => self.finish_settings_save(result, text),
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
        // The Japanese interface is offered only when the installed fonts can draw it.
        let japanese = *self
            .japanese
            .get_or_insert_with(|| fonts::has_japanese(ctx));
        let text = self.params.text(japanese);
        self.poll_file_job(ctx, text);
        self.take_dropped_file(ctx);
        egui::TopBottomPanel::top("audio").show(ctx, |ui| {
            self.audio_bar(ui, text);
            if let Some(warning) = &self.import_save_warning {
                ui.colored_label(ui.visuals().error_fg_color, warning);
            }
            if matches!(self.source, Source::File { .. }) {
                ui.separator();
                self.playback_bar(ui, text);
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
                &mut self.playout,
                &mut self.scope,
                &mut self.history,
                &mut self.spectrum,
                text,
                true,
                // The standalone application has no sidechain input to compare against.
                false,
            );
        });
        self.appearance_window(ctx, text);
        self.measurement_window(ctx, text);
        self.start_settings_save(text);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Give ordinary settings saves time to finish, but never join a worker
        // that may be blocked indefinitely inside the filesystem.
        let deadline = Instant::now() + Duration::from_millis(250);
        if let Ok(worker) = &mut self.file_io {
            let _ = worker.receive(deadline.saturating_duration_since(Instant::now()));
        }
        self.save_settings();
        let text = self.params.text(self.japanese.unwrap_or(false));
        self.start_settings_save(text);
        if let Ok(worker) = &mut self.file_io {
            let _ = worker.receive(deadline.saturating_duration_since(Instant::now()));
        }
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
fn file_label(text: Text, path: &str) -> String {
    if path.trim().is_empty() {
        text.audio_file().to_owned()
    } else {
        let name = std::path::Path::new(path)
            .file_name()
            .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned());
        text.audio_file_named(&name)
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

#[cfg(test)]
mod tests {
    use super::*;
    use shirabe_ui::i18n::Lang;

    #[test]
    fn the_file_label_follows_the_interface_language() {
        let english = Text::new(Lang::English, true);
        let japanese = Text::new(Lang::Japanese, true);
        assert_eq!(file_label(english, ""), "Audio file");
        assert_eq!(file_label(japanese, ""), "音声ファイル");
        let path = std::path::Path::new("music").join("mix v2.wav");
        let path = path.to_string_lossy();
        assert_eq!(file_label(english, &path), "Audio file: mix v2.wav");
        assert_eq!(file_label(japanese, &path), "音声ファイル: mix v2.wav");
    }
}
