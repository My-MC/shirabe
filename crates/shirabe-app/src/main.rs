//! Shirabe Meter Suite standalone application.
//!
//! Measures an audio input device with the same meters and the same view as the plugin.

// Release builds on Windows should not open a console window next to the GUI.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod audio;
mod context;
mod fonts;
#[cfg(target_os = "linux")]
mod pulse;
mod settings;
#[cfg(target_os = "macos")]
mod tap;

use std::sync::Arc;

use eframe::egui;
use nih_plug::prelude::ParamSetter;
use shirabe_plugin::engine::MeterEngine;
use shirabe_plugin::shared::SharedMeters;
use shirabe_plugin::{editor, ShirabeParams};
use shirabe_ui::Theme;

use audio::{Choice, Source};
use context::StandaloneContext;
use settings::Settings;

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
    /// Parameter values at the last save, to save again only after a change.
    saved_params: std::collections::BTreeMap<String, f32>,
    last_check: std::time::Instant,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let theme = Theme::dark();
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = theme.background;
        visuals.window_fill = theme.background;
        // eframe follows the system theme by default (light on many Windows setups); the
        // meters are designed for the dark theme only.
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        cc.egui_ctx.set_visuals_of(egui::Theme::Dark, visuals);
        fonts::install(&cc.egui_ctx);

        let params = Arc::new(ShirabeParams::default());
        let settings = Settings::load();
        context::load_params(params.as_ref(), &settings.params);
        let source = settings
            .source
            .unwrap_or_else(|| audio::default_source().unwrap_or_default());

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
            hosts,
            host,
            saved_params: settings.params,
            last_check: std::time::Instant::now(),
        };
        app.select(source);
        app
    }

    /// Switches to `source`; falls back to no input if it cannot be opened.
    fn select(&mut self, source: Source) {
        // Stop the old stream first: some drivers allow only one stream per device.
        self.running = None;
        let engine = MeterEngine::new(self.params.clone(), self.shared.clone());
        match audio::start(&source, engine) {
            Ok(r) => {
                self.running = Some(r);
                self.start_error = None;
                self.source = source;
            }
            Err(e) => {
                self.start_error = Some(format!("{e:#}"));
                let engine = MeterEngine::new(self.params.clone(), self.shared.clone());
                self.running = audio::start(&Source::None, engine).ok();
                self.source = Source::None;
            }
        }
    }

    fn save_settings(&mut self) {
        self.saved_params = context::save_params(self.params.as_ref());
        Settings {
            source: Some(self.source.clone()),
            params: self.saved_params.clone(),
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
            let current = self
                .inputs
                .iter()
                .chain(&self.playing)
                .find(|c| c.source == self.source)
                .map_or_else(|| "None".to_owned(), |c| c.label.clone());
            egui::ComboBox::from_id_salt("audio-source")
                .width(300.0)
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
                ui.label(
                    egui::RichText::new(format!("Cannot open the device: {e}"))
                        .color(self.theme.over),
                );
            }
        });
        if let Some(s) = new_source {
            if s != self.source {
                self.select(s);
                self.save_settings();
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Meters move continuously, so redraw every frame.
        ctx.request_repaint();
        egui::TopBottomPanel::top("audio").show(ctx, |ui| self.audio_bar(ui));
        self.save_if_changed();
        egui::CentralPanel::default().show(ctx, |ui| {
            let setter = ParamSetter {
                raw_context: &self.context,
            };
            editor::draw(ui, &self.params, &self.shared, &setter, &self.theme);
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_settings();
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
