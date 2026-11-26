//! Measurement results as CSV or JSON for the standalone application.
//!
//! The values are copied on the UI thread when the user asks for an export, so a
//! file is not changed by audio that keeps running while it is written. The values
//! are read one by one, so while measuring each may differ from the others by up
//! to one audio block. The recorded loudness history is copied at the same time.
//!
//! Nothing here changes a measurement: the meters, the parameters and the
//! audio path stay untouched.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use nih_plug::prelude::Enum;
use serde::Serialize;
use shirabe_dsp::history::{HISTORY_CAPACITY, HISTORY_INTERVAL_SECONDS};
use shirabe_dsp::MINUS_INFINITY_DB;
use shirabe_plugin::params::LoudnessTarget;
use shirabe_plugin::shared::{SharedMeters, MAX_CHANNELS};
use shirabe_plugin::ShirabeParams;

use crate::audio::Source;
use crate::file_out;

/// Versioned identifier written to every JSON file.
pub const SCHEMA_ID: &str = "shirabe-measurement/1";

/// File format of a measurement export.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Csv,
    Json,
}

impl Format {
    /// File name extension of the format.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }
}

/// How the measured signal reached the application.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceSnapshot {
    /// `none`, `input`, `output`, `monitor`, `system` or `file`.
    pub kind: &'static str,
    /// Audio API name, if the source belongs to one.
    pub host: Option<String>,
    /// Device name or file name, without a directory.
    pub description: String,
    /// Full file path, only when the user chose to include it.
    pub path: Option<String>,
}

/// The loudness target of the current session.
#[derive(Clone, Debug, PartialEq)]
pub struct TargetSnapshot {
    /// Stable preset id (the parameter's saved id).
    pub preset: &'static str,
    /// English name of the preset.
    pub name: &'static str,
    pub loudness_lufs: f64,
    /// Unit name used by the source document.
    pub unit: &'static str,
    pub tolerance_lu: Option<f64>,
    pub true_peak_ceiling_dbtp: f64,
}

/// One recorded loudness point, oldest first.
#[derive(Clone, Debug, PartialEq)]
pub struct HistorySnapshot {
    /// Seconds since the oldest retained point.
    pub time_seconds: f64,
    pub momentary_lufs: Option<f64>,
    pub short_term_lufs: Option<f64>,
}

/// A value of the meter display at the instant of the export.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub app_version: &'static str,
    /// UTC timestamp in ISO 8601 form, ending in `Z`.
    pub exported_at: String,
    pub source: SourceSnapshot,
    pub test_oscillator: bool,
    pub sample_rate_hz: Option<u32>,
    pub channel_layout: Option<&'static str>,
    pub channels: Vec<String>,
    pub programme_seconds: f64,
    pub integrated_lufs: Option<f64>,
    pub loudness_range_lu: Option<f64>,
    pub plr_lu: Option<f64>,
    pub max_momentary_lufs: Option<f64>,
    pub max_short_term_lufs: Option<f64>,
    /// Highest true peak since the last reset, one value per channel.
    pub max_true_peak_dbtp: Vec<Option<f64>>,
    pub target: TargetSnapshot,
    pub history: Vec<HistorySnapshot>,
}

impl Snapshot {
    /// Copies the current read-outs of `shared`, the target and the source.
    ///
    /// Runs on the UI thread; the file worker only serialises the copy.
    pub fn capture(
        shared: &SharedMeters,
        params: &ShirabeParams,
        source: &Source,
        include_path: bool,
        now: SystemTime,
    ) -> Self {
        let channel_count = shared.active_channels().clamp(1, MAX_CHANNELS);
        let layout = shared.layout();
        let mut channels: Vec<String> = match layout {
            Some(layout) => layout
                .roles()
                .iter()
                .map(|role| role.label().to_owned())
                .collect(),
            None => (1..=channel_count).map(|index| index.to_string()).collect(),
        };
        channels.truncate(channel_count);
        while channels.len() < channel_count {
            channels.push((channels.len() + 1).to_string());
        }

        let mut max_true_peak_dbtp = Vec::with_capacity(channel_count);
        for index in 0..channel_count {
            let peak = shared
                .channel(index)
                .true_peak_max_db
                .load(Ordering::Relaxed);
            max_true_peak_dbtp.push(
                (peak.is_finite() && peak > MINUS_INFINITY_DB).then(|| round(f64::from(peak), 2)),
            );
        }

        let target = params.target_spec();
        let preset = params.loudness_target.value();
        let preset_index = preset.to_index();
        let preset_id = LoudnessTarget::ids()
            .and_then(|ids| ids.get(preset_index).copied())
            .unwrap_or("custom");
        let preset_name = LoudnessTarget::variants()
            .get(preset_index)
            .copied()
            .unwrap_or("Custom");

        let sample_rate = shared.sample_rate();
        Self {
            app_version: env!("CARGO_PKG_VERSION"),
            exported_at: format_utc(now),
            source: source_snapshot(source, include_path),
            test_oscillator: params.osc_enabled.value(),
            sample_rate_hz: (sample_rate > 0.0).then(|| sample_rate.round() as u32),
            channel_layout: layout.map(|layout| layout.name()),
            channels,
            programme_seconds: round(
                f64::from(shared.programme_seconds.load(Ordering::Relaxed)),
                3,
            ),
            integrated_lufs: optional(shared.loudness.integrated.load(), 2),
            loudness_range_lu: optional(shared.loudness.range.load(), 2),
            plr_lu: optional(shared.loudness.plr.load(), 2),
            max_momentary_lufs: optional(shared.loudness.max_momentary.load(), 2),
            max_short_term_lufs: optional(shared.loudness.max_short_term.load(), 2),
            max_true_peak_dbtp,
            target: TargetSnapshot {
                preset: preset_id,
                name: preset_name,
                loudness_lufs: round(f64::from(target.loudness), 2),
                unit: target.unit(),
                tolerance_lu: target.tolerance.map(|value| round(f64::from(value), 2)),
                true_peak_ceiling_dbtp: round(f64::from(target.true_peak_ceiling), 2),
            },
            history: history_snapshot(shared),
        }
    }
}

fn optional(value: Option<f32>, decimals: u32) -> Option<f64> {
    value.map(|value| round(f64::from(value), decimals))
}

/// Copies the history, discarding a copy that raced a clear.
fn history_snapshot(shared: &SharedMeters) -> Vec<HistorySnapshot> {
    let mut raw = vec![(f32::NAN, f32::NAN); HISTORY_CAPACITY];
    let mut count = 0;
    for _ in 0..2 {
        let epoch = shared.history.epoch();
        count = shared.history.read_recent(&mut raw);
        if shared.history.epoch() == epoch {
            break;
        }
        count = 0;
    }
    raw[..count]
        .iter()
        .enumerate()
        .map(|(index, &(momentary, short_term))| HistorySnapshot {
            time_seconds: round(index as f64 * HISTORY_INTERVAL_SECONDS, 3),
            momentary_lufs: momentary
                .is_finite()
                .then(|| round(f64::from(momentary), 2)),
            short_term_lufs: short_term
                .is_finite()
                .then(|| round(f64::from(short_term), 2)),
        })
        .collect()
}

/// The source without a directory, unless the user chose to include it.
fn source_snapshot(source: &Source, include_path: bool) -> SourceSnapshot {
    match source {
        Source::None => SourceSnapshot {
            kind: "none",
            host: None,
            description: String::new(),
            path: None,
        },
        Source::Input { host, device } | Source::Output { host, device } => SourceSnapshot {
            kind: if matches!(source, Source::Input { .. }) {
                "input"
            } else {
                "output"
            },
            host: Some(host.clone()),
            description: device.clone(),
            path: None,
        },
        Source::Monitor { sink } => SourceSnapshot {
            kind: "monitor",
            host: None,
            description: sink.clone(),
            path: None,
        },
        Source::SystemAudio => SourceSnapshot {
            kind: "system",
            host: None,
            description: "System audio".to_owned(),
            path: None,
        },
        Source::File { path } => {
            let trimmed = path.trim();
            let name = Path::new(trimmed)
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            SourceSnapshot {
                kind: "file",
                host: None,
                description: name,
                path: include_path.then(|| trimmed.to_owned()),
            }
        }
    }
}

/// UTC timestamp as ISO 8601 (`2026-11-26T03:04:05Z`).
fn format_utc(now: SystemTime) -> String {
    let seconds = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds_of_day / 3_600,
        (seconds_of_day % 3_600) / 60,
        seconds_of_day % 60
    )
}

/// Gregorian date from days since 1970-01-01 (Howard Hinnant's civil calendar algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (year + i64::from(month <= 2), month as u32, day as u32)
}

/// Renders the snapshot as a rectangular CSV table.
pub fn to_csv(snapshot: &Snapshot) -> String {
    let mut out = String::new();
    out.push_str("section,key,channel,time_seconds,value,unit\r\n");
    let mut summary = |key: &str, channel: &str, value: &str, unit: &str| {
        push_row(&mut out, &["summary", key, channel, "", value, unit]);
    };

    summary("schema", "", SCHEMA_ID, "");
    summary("app_version", "", snapshot.app_version, "");
    summary("exported_at", "", &snapshot.exported_at, "");
    summary("source_kind", "", snapshot.source.kind, "");
    summary(
        "source_host",
        "",
        snapshot.source.host.as_deref().unwrap_or(""),
        "",
    );
    summary("source_description", "", &snapshot.source.description, "");
    summary(
        "source_path",
        "",
        snapshot.source.path.as_deref().unwrap_or(""),
        "",
    );
    summary(
        "test_oscillator",
        "",
        if snapshot.test_oscillator {
            "true"
        } else {
            "false"
        },
        "",
    );
    summary(
        "sample_rate_hz",
        "",
        &snapshot
            .sample_rate_hz
            .map_or_else(String::new, |rate| rate.to_string()),
        "Hz",
    );
    summary(
        "channel_layout",
        "",
        snapshot.channel_layout.unwrap_or(""),
        "",
    );
    summary("channels", "", &snapshot.channels.join(" "), "");
    summary(
        "programme_seconds",
        "",
        &number(snapshot.programme_seconds, 3),
        "s",
    );
    summary(
        "integrated_lufs",
        "",
        &optional_number(snapshot.integrated_lufs, 2),
        "LUFS",
    );
    summary(
        "loudness_range_lu",
        "",
        &optional_number(snapshot.loudness_range_lu, 2),
        "LU",
    );
    summary("plr_lu", "", &optional_number(snapshot.plr_lu, 2), "LU");
    summary(
        "max_momentary_lufs",
        "",
        &optional_number(snapshot.max_momentary_lufs, 2),
        "LUFS",
    );
    summary(
        "max_short_term_lufs",
        "",
        &optional_number(snapshot.max_short_term_lufs, 2),
        "LUFS",
    );
    for (channel, peak) in snapshot.channels.iter().zip(&snapshot.max_true_peak_dbtp) {
        summary(
            "max_true_peak_dbtp",
            channel,
            &optional_number(*peak, 2),
            "dBTP",
        );
    }
    summary("target_preset", "", snapshot.target.preset, "");
    summary("target_name", "", snapshot.target.name, "");
    summary(
        "target_loudness_lufs",
        "",
        &number(snapshot.target.loudness_lufs, 2),
        "LUFS",
    );
    summary("target_unit", "", snapshot.target.unit, "");
    summary(
        "target_tolerance_lu",
        "",
        &optional_number(snapshot.target.tolerance_lu, 2),
        "LU",
    );
    summary(
        "target_true_peak_ceiling_dbtp",
        "",
        &number(snapshot.target.true_peak_ceiling_dbtp, 2),
        "dBTP",
    );
    for point in &snapshot.history {
        let time = number(point.time_seconds, 3);
        push_row(
            &mut out,
            &[
                "history",
                "momentary_lufs",
                "",
                &time,
                &optional_number(point.momentary_lufs, 2),
                "LUFS",
            ],
        );
        push_row(
            &mut out,
            &[
                "history",
                "short_term_lufs",
                "",
                &time,
                &optional_number(point.short_term_lufs, 2),
                "LUFS",
            ],
        );
    }
    out
}

fn push_row(out: &mut String, fields: &[&str; 6]) {
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&escape_csv(field));
    }
    out.push_str("\r\n");
}

/// Quotes a field only when it contains a comma, a quote or a line break.
///
/// Text that starts like a spreadsheet formula (`=`, `+`, `-`, `@`, tab, carriage return)
/// gets a leading `'`, so a device or file name cannot run as a formula when the export is
/// opened in a spreadsheet. Numbers keep their sign.
fn escape_csv(field: &str) -> String {
    let formula_like = field.starts_with(['=', '+', '-', '@', '\t', '\r'])
        && !field.parse::<f64>().is_ok_and(f64::is_finite);
    let neutral;
    let field = if formula_like {
        neutral = format!("'{field}");
        neutral.as_str()
    } else {
        field
    };
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

/// Locale-independent fixed-point text.
fn number(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    if text.starts_with("-0.") && text[1..].bytes().all(|b| b == b'0' || b == b'.') {
        text[1..].to_owned()
    } else {
        text
    }
}

fn optional_number(value: Option<f64>, decimals: usize) -> String {
    value.map_or_else(String::new, |value| number(value, decimals))
}

/// Rounds to `decimals` places and removes a negative zero, so the two formats agree.
fn round(value: f64, decimals: u32) -> f64 {
    let factor = 10f64.powi(decimals as i32);
    let rounded = (value * factor).round() / factor;
    if rounded == 0.0 {
        0.0
    } else {
        rounded
    }
}

/// Renders the snapshot as versioned JSON.
pub fn to_json(snapshot: &Snapshot) -> String {
    let file = JsonFile {
        schema: SCHEMA_ID,
        app_version: snapshot.app_version,
        exported_at: &snapshot.exported_at,
        source: JsonSource {
            kind: snapshot.source.kind,
            host: snapshot.source.host.as_deref(),
            description: &snapshot.source.description,
            path: snapshot.source.path.as_deref(),
            test_oscillator: snapshot.test_oscillator,
        },
        measurement: JsonMeasurement {
            sample_rate_hz: snapshot.sample_rate_hz,
            channel_layout: snapshot.channel_layout,
            channels: &snapshot.channels,
            programme_seconds: snapshot.programme_seconds,
            integrated_lufs: snapshot.integrated_lufs,
            loudness_range_lu: snapshot.loudness_range_lu,
            plr_lu: snapshot.plr_lu,
            max_momentary_lufs: snapshot.max_momentary_lufs,
            max_short_term_lufs: snapshot.max_short_term_lufs,
            max_true_peak_dbtp: &snapshot.max_true_peak_dbtp,
        },
        target: JsonTarget {
            preset: snapshot.target.preset,
            name: snapshot.target.name,
            loudness_lufs: snapshot.target.loudness_lufs,
            unit: snapshot.target.unit,
            tolerance_lu: snapshot.target.tolerance_lu,
            true_peak_ceiling_dbtp: snapshot.target.true_peak_ceiling_dbtp,
        },
        history: JsonHistory {
            interval_seconds: HISTORY_INTERVAL_SECONDS,
            points: snapshot
                .history
                .iter()
                .map(|point| JsonPoint {
                    time_seconds: point.time_seconds,
                    momentary_lufs: point.momentary_lufs,
                    short_term_lufs: point.short_term_lufs,
                })
                .collect(),
        },
    };
    let mut json =
        serde_json::to_string_pretty(&file).expect("the export document is serialisable");
    json.push('\n');
    json
}

#[derive(Serialize)]
struct JsonFile<'a> {
    schema: &'static str,
    app_version: &'a str,
    exported_at: &'a str,
    source: JsonSource<'a>,
    measurement: JsonMeasurement<'a>,
    target: JsonTarget,
    history: JsonHistory,
}

#[derive(Serialize)]
struct JsonSource<'a> {
    kind: &'static str,
    host: Option<&'a str>,
    description: &'a str,
    path: Option<&'a str>,
    test_oscillator: bool,
}

#[derive(Serialize)]
struct JsonMeasurement<'a> {
    sample_rate_hz: Option<u32>,
    channel_layout: Option<&'static str>,
    channels: &'a [String],
    programme_seconds: f64,
    integrated_lufs: Option<f64>,
    loudness_range_lu: Option<f64>,
    plr_lu: Option<f64>,
    max_momentary_lufs: Option<f64>,
    max_short_term_lufs: Option<f64>,
    max_true_peak_dbtp: &'a [Option<f64>],
}

#[derive(Serialize)]
struct JsonTarget {
    preset: &'static str,
    name: &'static str,
    loudness_lufs: f64,
    unit: &'static str,
    tolerance_lu: Option<f64>,
    true_peak_ceiling_dbtp: f64,
}

#[derive(Serialize)]
struct JsonHistory {
    interval_seconds: f64,
    points: Vec<JsonPoint>,
}

#[derive(Serialize)]
struct JsonPoint {
    time_seconds: f64,
    momentary_lufs: Option<f64>,
    short_term_lufs: Option<f64>,
}

/// Writes `snapshot` to `path` in `format`.
pub fn save(
    snapshot: &Snapshot,
    format: Format,
    path: &Path,
    overwrite: bool,
) -> Result<(), file_out::WriteError> {
    let text = match format {
        Format::Csv => to_csv(snapshot),
        Format::Json => to_json(snapshot),
    };
    file_out::write(path, text.as_bytes(), overwrite)
}

#[cfg(test)]
mod tests {

    #[test]
    fn csv_text_that_a_spreadsheet_would_run_as_a_formula_is_neutralised() {
        for text in [
            "=cmd|' /C calc'!A0",
            "+SUM(A1)",
            "-2+3",
            "@x",
            "\tname",
            "\rname",
        ] {
            let escaped = escape_csv(text);
            assert!(
                escaped.starts_with('\'') || escaped.starts_with("\"'"),
                "{text:?} -> {escaped:?}"
            );
        }
        // Numbers keep their sign so the value column stays numeric.
        assert_eq!(escape_csv("-23.00"), "-23.00");
        assert_eq!(escape_csv("+1"), "+1");
        assert_eq!(escape_csv("plain"), "plain");
    }

    use super::*;

    fn snapshot() -> Snapshot {
        Snapshot {
            app_version: "0.1.0",
            exported_at: "2026-11-26T03:04:05Z".to_owned(),
            source: SourceSnapshot {
                kind: "file",
                host: None,
                description: "mix, take 2.wav".to_owned(),
                path: None,
            },
            test_oscillator: false,
            sample_rate_hz: Some(48_000),
            channel_layout: Some("Stereo"),
            channels: vec!["L".to_owned(), "R".to_owned()],
            programme_seconds: 12.346,
            integrated_lufs: Some(-23.1),
            loudness_range_lu: None,
            plr_lu: Some(8.0),
            max_momentary_lufs: Some(-18.0),
            max_short_term_lufs: None,
            max_true_peak_dbtp: vec![Some(-1.2), None],
            target: TargetSnapshot {
                preset: "ebu",
                name: "EBU R128 (-23 LUFS)",
                loudness_lufs: -23.0,
                unit: "LUFS",
                tolerance_lu: Some(1.0),
                true_peak_ceiling_dbtp: -1.0,
            },
            history: vec![
                HistorySnapshot {
                    time_seconds: 0.0,
                    momentary_lufs: Some(-23.1),
                    short_term_lufs: None,
                },
                HistorySnapshot {
                    time_seconds: 0.1,
                    momentary_lufs: None,
                    short_term_lufs: Some(-30.2),
                },
            ],
        }
    }

    #[test]
    fn csv_numbers_use_a_dot_decimal_and_two_places() {
        let csv = to_csv(&snapshot());
        assert!(csv.contains("summary,integrated_lufs,,,-23.10,LUFS\r\n"));
    }

    #[test]
    fn csv_leaves_absent_values_empty() {
        let csv = to_csv(&snapshot());
        println!("{csv}");
        assert!(csv.contains("summary,loudness_range_lu,,,,LU\r\n"));
        assert!(csv.contains("summary,max_true_peak_dbtp,R,,,dBTP\r\n"));
        assert!(csv.contains("history,momentary_lufs,,0.100,,LUFS\r\n"));
        assert!(csv.contains("summary,max_short_term_lufs,,,,LUFS\r\n"));
    }

    #[test]
    fn csv_escapes_commas_quotes_and_line_breaks() {
        let mut snapshot = snapshot();
        snapshot.source.description = "mix, \"take\" 2\r\nfinal.wav".to_owned();
        let csv = to_csv(&snapshot);
        assert!(
            csv.contains("summary,source_description,,,\"mix, \"\"take\"\" 2\r\nfinal.wav\",\r\n")
        );
    }

    #[test]
    fn every_csv_row_has_the_same_six_fields() {
        let csv = to_csv(&snapshot());
        assert!(csv.lines().count() > 20);
        for line in csv.lines() {
            let mut quoted = false;
            let mut chars = line.chars().peekable();
            let mut fields = 1;
            while let Some(c) = chars.next() {
                match c {
                    '"' if quoted && chars.peek() == Some(&'"') => {
                        chars.next();
                    }
                    '"' => quoted = !quoted,
                    ',' if !quoted => fields += 1,
                    _ => {}
                }
            }
            assert!(!quoted, "unbalanced quote in {line:?}");
            assert_eq!(fields, 6, "wrong field count in {line:?}");
        }
    }

    #[test]
    fn csv_records_the_history_with_relative_time() {
        let csv = to_csv(&snapshot());
        assert!(csv.contains("history,momentary_lufs,,0.000,-23.10,LUFS\r\n"));
        assert!(csv.contains("history,short_term_lufs,,0.100,-30.20,LUFS\r\n"));
    }

    #[test]
    fn json_carries_the_versioned_schema_id_and_units_in_key_names() {
        let json = to_json(&snapshot());
        assert!(json.contains("\"schema\": \"shirabe-measurement/1\""));
        assert!(json.contains("\"integrated_lufs\": -23.1"));
        assert!(json.contains("\"max_true_peak_dbtp\""));
        assert!(json.contains("\"sample_rate_hz\": 48000"));
    }

    #[test]
    fn json_keeps_absent_values_null() {
        let value: serde_json::Value = serde_json::from_str(&to_json(&snapshot())).unwrap();
        assert!(value["measurement"]["loudness_range_lu"].is_null());
        assert!(value["measurement"]["max_short_term_lufs"].is_null());
        assert_eq!(
            value["measurement"]["max_true_peak_dbtp"][1],
            serde_json::Value::Null
        );
        assert!(value["history"]["points"][0]["short_term_lufs"].is_null());
        assert!(value["source"]["path"].is_null());
    }

    #[test]
    fn json_describes_the_summary_and_the_history() {
        let value: serde_json::Value = serde_json::from_str(&to_json(&snapshot())).unwrap();
        assert_eq!(value["app_version"], "0.1.0");
        assert_eq!(value["exported_at"], "2026-11-26T03:04:05Z");
        assert_eq!(value["source"]["description"], "mix, take 2.wav");
        assert_eq!(value["measurement"]["channels"][0], "L");
        assert_eq!(value["measurement"]["channel_layout"], "Stereo");
        assert_eq!(value["measurement"]["programme_seconds"], 12.346);
        assert_eq!(value["target"]["preset"], "ebu");
        assert_eq!(value["target"]["tolerance_lu"], 1.0);
        assert_eq!(value["history"]["interval_seconds"], 0.1);
        assert_eq!(value["history"]["points"].as_array().unwrap().len(), 2);
        assert_eq!(value["history"]["points"][0]["time_seconds"], 0.0);
        assert_eq!(
            value["history"]["points"][1]["momentary_lufs"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn json_and_csv_show_the_same_rounded_values() {
        let json: serde_json::Value = serde_json::from_str(&to_json(&snapshot())).unwrap();
        let csv = to_csv(&snapshot());
        assert_eq!(json["measurement"]["integrated_lufs"], -23.1);
        assert!(csv.contains("-23.10,LUFS"));
        assert_eq!(json["measurement"]["programme_seconds"], 12.346);
        assert!(csv.contains("summary,programme_seconds,,,12.346,s\r\n"));
    }

    #[test]
    fn capture_copies_the_meters_and_the_target_at_the_click() {
        use std::time::Duration;

        let shared = SharedMeters::new();
        shared.configure(2, 48_000.0, false);
        shared.programme_seconds.store(12.3456, Ordering::Relaxed);
        shared.loudness.integrated.store(Some(-23.456));
        shared.loudness.range.store(Some(5.678));
        shared.loudness.plr.store(Some(8.004));
        shared.loudness.max_momentary.store(Some(-18.004));
        shared.loudness.max_short_term.store(None);
        shared
            .channel(0)
            .true_peak_max_db
            .store(-1.2345, Ordering::Relaxed);
        shared
            .channel(1)
            .true_peak_max_db
            .store(f32::NEG_INFINITY, Ordering::Relaxed);

        let params = ShirabeParams::default();
        let captured = Snapshot::capture(
            &shared,
            &params,
            &Source::None,
            false,
            UNIX_EPOCH + Duration::from_secs(1_795_662_245),
        );

        assert_eq!(captured.app_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(captured.exported_at, "2026-11-26T03:04:05Z");
        assert_eq!(captured.sample_rate_hz, Some(48_000));
        assert_eq!(captured.channel_layout, Some("Stereo"));
        assert_eq!(captured.channels, vec!["L".to_owned(), "R".to_owned()]);
        assert_eq!(captured.programme_seconds, 12.346);
        assert_eq!(captured.integrated_lufs, Some(-23.46));
        assert_eq!(captured.loudness_range_lu, Some(5.68));
        assert_eq!(captured.plr_lu, Some(8.0));
        assert_eq!(captured.max_momentary_lufs, Some(-18.0));
        assert_eq!(captured.max_short_term_lufs, None);
        assert_eq!(captured.max_true_peak_dbtp, vec![Some(-1.23), None]);
        assert_eq!(captured.target.preset, "arib");
        assert_eq!(captured.target.loudness_lufs, -24.0);
        assert_eq!(captured.target.unit, "LKFS");
        assert_eq!(captured.target.tolerance_lu, Some(1.0));
        assert_eq!(captured.target.true_peak_ceiling_dbtp, -1.0);
        assert!(!captured.test_oscillator);
        assert!(captured.history.is_empty());
    }

    #[test]
    fn capture_records_the_history_with_relative_time() {
        let shared = SharedMeters::new();
        shared.configure(2, 48_000.0, false);
        shared.history.record(4_800, Some(-23.0), Some(-30.0));
        shared.history.record(4_800, None, Some(-31.0));

        let captured = Snapshot::capture(
            &shared,
            &ShirabeParams::default(),
            &Source::None,
            false,
            UNIX_EPOCH,
        );

        assert_eq!(
            captured.history,
            vec![
                HistorySnapshot {
                    time_seconds: 0.0,
                    momentary_lufs: Some(-23.0),
                    short_term_lufs: Some(-30.0),
                },
                HistorySnapshot {
                    time_seconds: 0.1,
                    momentary_lufs: None,
                    short_term_lufs: Some(-31.0),
                },
            ]
        );
    }

    #[test]
    fn a_file_source_hides_its_directory_by_default() {
        let source = Source::File {
            path: "/home/someone/private/mix.wav".to_owned(),
        };
        let captured = Snapshot::capture(
            &SharedMeters::new(),
            &ShirabeParams::default(),
            &source,
            false,
            UNIX_EPOCH,
        );
        assert_eq!(captured.source.kind, "file");
        assert_eq!(captured.source.description, "mix.wav");
        assert_eq!(captured.source.path, None);
        let json = to_json(&captured);
        assert!(!json.contains("someone"));
        assert!(!json.contains("/home"));
    }

    #[test]
    fn the_full_file_path_is_written_only_when_chosen() {
        let source = Source::File {
            path: "/home/someone/private/mix.wav".to_owned(),
        };
        let captured = Snapshot::capture(
            &SharedMeters::new(),
            &ShirabeParams::default(),
            &source,
            true,
            UNIX_EPOCH,
        );
        assert_eq!(captured.source.description, "mix.wav");
        assert_eq!(
            captured.source.path.as_deref(),
            Some("/home/someone/private/mix.wav")
        );
        let json = to_json(&captured);
        assert!(json.contains("/home/someone/private/mix.wav"));
    }

    #[test]
    fn a_device_source_names_the_api_and_the_device_without_a_path() {
        let source = Source::Input {
            host: "ALSA".to_owned(),
            device: "Built-in Audio".to_owned(),
        };
        let captured = Snapshot::capture(
            &SharedMeters::new(),
            &ShirabeParams::default(),
            &source,
            true,
            UNIX_EPOCH,
        );
        assert_eq!(captured.source.kind, "input");
        assert_eq!(captured.source.host.as_deref(), Some("ALSA"));
        assert_eq!(captured.source.description, "Built-in Audio");
        assert_eq!(captured.source.path, None);
    }

    #[test]
    fn timestamps_are_utc_iso_8601() {
        use std::time::Duration;

        let at = |seconds| format_utc(UNIX_EPOCH + Duration::from_secs(seconds));
        assert_eq!(at(0), "1970-01-01T00:00:00Z");
        assert_eq!(at(1_709_208_000), "2024-02-29T12:00:00Z");
        assert_eq!(at(946_684_799), "1999-12-31T23:59:59Z");
        assert_eq!(at(1_767_225_600), "2026-01-01T00:00:00Z");
        assert_eq!(at(1_795_662_245), "2026-11-26T03:04:05Z");
    }

    #[test]
    fn a_snapshot_does_not_change_when_the_meters_keep_running() {
        let shared = SharedMeters::new();
        shared.configure(2, 48_000.0, false);
        shared.loudness.integrated.store(Some(-23.0));
        let captured = Snapshot::capture(
            &shared,
            &ShirabeParams::default(),
            &Source::None,
            false,
            UNIX_EPOCH,
        );

        shared.loudness.integrated.store(Some(-10.0));
        shared
            .channel(0)
            .true_peak_max_db
            .store(0.0, Ordering::Relaxed);
        shared.history.record(4_800, Some(0.0), Some(0.0));

        assert_eq!(captured.integrated_lufs, Some(-23.0));
        assert_eq!(captured.max_true_peak_dbtp, vec![None, None]);
        assert!(captured.history.is_empty());
        assert!(to_json(&captured).contains("-23.0"));
    }

    #[test]
    fn a_negative_zero_is_written_as_zero() {
        assert_eq!(number(-0.0, 2), "0.00");
        assert_eq!(number(-0.001, 2), "0.00");
        assert_eq!(round(-0.001, 2), 0.0);
    }

    #[test]
    fn saving_never_replaces_a_file_without_confirmation() {
        let dir = std::env::temp_dir().join(format!(
            "shirabe-measurement-save-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("measurement.json");
        let snapshot = snapshot();

        save(&snapshot, Format::Json, &path, false).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        assert!(first.contains(SCHEMA_ID));

        assert!(matches!(
            save(&snapshot, Format::Csv, &path, false),
            Err(file_out::WriteError::Exists)
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);

        save(&snapshot, Format::Csv, &path, true).unwrap();
        let replaced = std::fs::read_to_string(&path).unwrap();
        assert!(replaced.starts_with("section,key,channel,time_seconds,value,unit\r\n"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_full_history_still_produces_valid_json() {
        let mut snapshot = snapshot();
        snapshot.history = (0..HISTORY_CAPACITY)
            .map(|index| HistorySnapshot {
                time_seconds: round(index as f64 * HISTORY_INTERVAL_SECONDS, 3),
                momentary_lufs: Some(-23.0),
                short_term_lufs: Some(-30.0),
            })
            .collect();
        let value: serde_json::Value = serde_json::from_str(&to_json(&snapshot)).unwrap();
        assert_eq!(
            value["history"]["points"].as_array().unwrap().len(),
            HISTORY_CAPACITY
        );
        assert!(to_csv(&snapshot).lines().count() > HISTORY_CAPACITY);
    }
}
