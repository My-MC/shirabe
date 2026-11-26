//! Versioned appearance preset files for the standalone application.
//!
//! A preset stores the selected layout and the theme colours. Sources, loudness
//! targets, the test oscillator and every measured value stay out of the file,
//! so importing a look never changes what is being measured.
//!
//! The file is read up to [`MAX_BYTES`] and validated completely before anything
//! is applied. An invalid file leaves the current theme, parameters and settings
//! untouched.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use nih_plug::prelude::{Enum, ParamSetter};
use serde::{Deserialize, Serialize};
use shirabe_plugin::params::{LayoutPreset, ShirabeParams};
use shirabe_ui::egui::Color32;
use shirabe_ui::theme::Zone;
use shirabe_ui::Theme;

/// Marker written to the `format` field.
pub const FORMAT: &str = "shirabe-appearance";
/// Schema version written and read by this build.
pub const VERSION: u32 = 1;
/// Largest file this build reads.
pub const MAX_BYTES: u64 = 64 * 1024;
/// Most colour zones accepted in one list.
pub const MAX_ZONES: usize = 32;

/// UI-only path and one-use overwrite authorization.
pub struct Target {
    typed: String,
    pending: Option<String>,
    revision: u64,
}

impl Target {
    pub fn new(typed: String) -> Self {
        Self {
            typed,
            pending: None,
            revision: 0,
        }
    }

    pub fn path(&self) -> &str {
        &self.typed
    }

    pub fn pending(&self) -> Option<&str> {
        self.pending.as_deref()
    }

    pub fn set_path(&mut self, path: String) {
        self.typed = path;
        self.invalidate();
    }

    pub fn invalidate(&mut self) {
        self.pending = None;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn begin(&mut self, overwrite: bool) -> Option<(String, u64)> {
        let authorized = self.pending() == Some(self.path());
        self.invalidate();
        if self.typed.trim().is_empty() || (overwrite && !authorized) {
            return None;
        }
        Some((self.typed.clone(), self.revision))
    }

    /// Only the still-current request can offer confirmation after a collision.
    pub fn offer_overwrite(&mut self, path: &str, revision: u64) -> bool {
        if self.revision != revision || self.path() != path {
            return false;
        }
        self.pending = Some(path.to_owned());
        true
    }
}

/// A validated appearance: the layout and the theme it applies.
#[derive(Clone, Debug, PartialEq)]
pub struct Appearance {
    /// Selected meter arrangement.
    pub layout: LayoutPreset,
    /// Theme colours.
    pub theme: Theme,
}

impl Appearance {
    /// The appearance currently shown by `params` and `theme`.
    pub fn current(params: &ShirabeParams, theme: &Theme) -> Self {
        Self {
            layout: params.layout_preset.value(),
            theme: theme.clone(),
        }
    }
}

/// Theme colours as stored in a preset file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeSpec {
    /// Window background.
    pub background: ColorSpec,
    /// Background of a meter panel.
    pub panel: ColorSpec,
    /// Primary text.
    pub text: ColorSpec,
    /// Secondary text and tick labels.
    pub text_dim: ColorSpec,
    /// Peak-hold marker.
    pub hold: ColorSpec,
    /// Accent for values within the loudness target tolerance.
    pub on_target: ColorSpec,
    /// Accent for values above the target or true-peak ceiling.
    pub over: ColorSpec,
    /// Peak-meter zones, sorted by ascending `from_db`.
    pub zones: Vec<ZoneSpec>,
    /// Two VU/PPM bar colours; their boundary is set by the meter, not the file.
    pub level_bar: LevelBarSpec,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelBarSpec {
    normal: SegmentSpec,
    over: SegmentSpec,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SegmentSpec {
    lit: ColorSpec,
    unlit: ColorSpec,
}

impl SegmentSpec {
    fn from_zone(zone: &Zone) -> Self {
        Self {
            lit: color_to_hex(zone.lit),
            unlit: color_to_hex(zone.unlit),
        }
    }

    fn to_zone(&self, field: &str, from_db: f32) -> Result<Zone, Error> {
        Ok(Zone {
            from_db,
            lit: parse_color(&self.lit, &format!("level_bar.{field}.lit"))?,
            unlit: parse_color(&self.unlit, &format!("level_bar.{field}.unlit"))?,
        })
    }
}

/// One colour zone as stored in a preset file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZoneSpec {
    /// Lower edge in dB; omitted for the first zone, which reaches down forever.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_db: Option<f32>,
    /// Lit segment colour.
    pub lit: ColorSpec,
    /// Unlit segment colour.
    pub unlit: ColorSpec,
}

/// A colour in a preset file, written as `#rrggbb`.
pub type ColorSpec = String;

impl ThemeSpec {
    /// Stores `theme` as preset data.
    pub fn from_theme(theme: &Theme) -> Self {
        Self {
            background: color_to_hex(theme.background),
            panel: color_to_hex(theme.panel),
            text: color_to_hex(theme.text),
            text_dim: color_to_hex(theme.text_dim),
            hold: color_to_hex(theme.hold),
            on_target: color_to_hex(theme.on_target),
            over: color_to_hex(theme.over),
            zones: theme.zones.iter().map(zone_spec).collect(),
            level_bar: LevelBarSpec {
                normal: SegmentSpec::from_zone(&theme.vu_zones[0]),
                over: SegmentSpec::from_zone(&theme.vu_zones[theme.vu_zones.len() - 1]),
            },
        }
    }

    /// Validates the stored colours and builds a theme.
    ///
    /// The segment height and gap are not part of the file; the built-in shape is
    /// used so an imported theme looks like the one this build draws by default.
    pub fn to_theme(&self) -> Result<Theme, Error> {
        let base = Theme::dark();
        Ok(Theme {
            background: parse_color(&self.background, "background")?,
            panel: parse_color(&self.panel, "panel")?,
            text: parse_color(&self.text, "text")?,
            text_dim: parse_color(&self.text_dim, "text_dim")?,
            hold: parse_color(&self.hold, "hold")?,
            on_target: parse_color(&self.on_target, "on_target")?,
            over: parse_color(&self.over, "over")?,
            zones: parse_zones(&self.zones, "zones")?,
            vu_zones: vec![
                self.level_bar.normal.to_zone("normal", f32::NEG_INFINITY)?,
                self.level_bar.over.to_zone("over", 0.0)?,
            ],
            segment_height: base.segment_height,
            segment_gap: base.segment_gap,
        })
    }
}

/// Parses `#rrggbb`; alpha and named colours are not part of the schema.
fn parse_color(text: &str, field: &str) -> Result<Color32, Error> {
    let Some(hex) = text
        .strip_prefix('#')
        .filter(|hex| hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
    else {
        return Err(Error::InvalidTheme(format!(
            "{field} must be #rrggbb, got {text:?}"
        )));
    };
    let channel = |range: std::ops::Range<usize>| {
        // The range is inside an ASCII hex string checked above.
        u8::from_str_radix(&hex[range], 16).expect("checked hexadecimal")
    };
    Ok(Color32::from_rgb(
        channel(0..2),
        channel(2..4),
        channel(4..6),
    ))
}

/// Validates a zone list: not empty, first zone may reach down forever, and the
/// lower edges strictly ascend so `Theme::zone_at` picks them in order.
fn parse_zones(specs: &[ZoneSpec], field: &str) -> Result<Vec<Zone>, Error> {
    if specs.is_empty() {
        return Err(Error::InvalidTheme(format!(
            "{field} needs at least one zone"
        )));
    }
    if specs.len() > MAX_ZONES {
        return Err(Error::InvalidTheme(format!(
            "{field} has more than {MAX_ZONES} zones"
        )));
    }
    let mut zones = Vec::with_capacity(specs.len());
    let mut previous = f32::NEG_INFINITY;
    for (index, spec) in specs.iter().enumerate() {
        let from_db = match spec.from_db {
            Some(value) if !value.is_finite() => {
                return Err(Error::InvalidTheme(format!(
                    "{field}[{index}].from_db must be finite"
                )));
            }
            Some(value) if value <= previous => {
                return Err(Error::InvalidTheme(format!(
                    "{field}[{index}].from_db must ascend"
                )));
            }
            Some(value) => value,
            None if index == 0 => f32::NEG_INFINITY,
            None => {
                return Err(Error::InvalidTheme(format!(
                    "{field}[{index}].from_db may only be omitted for the first zone"
                )));
            }
        };
        previous = from_db;
        zones.push(Zone {
            from_db,
            lit: parse_color(&spec.lit, &format!("{field}[{index}].lit"))?,
            unlit: parse_color(&spec.unlit, &format!("{field}[{index}].unlit"))?,
        });
    }
    Ok(zones)
}

fn zone_spec(zone: &Zone) -> ZoneSpec {
    ZoneSpec {
        from_db: zone.from_db.is_finite().then_some(zone.from_db),
        lit: color_to_hex(zone.lit),
        unlit: color_to_hex(zone.unlit),
    }
}

fn color_to_hex(color: Color32) -> ColorSpec {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}

/// Header read before the colours, so an unsupported version is reported as
/// such even when the rest of the file uses fields this build does not know.
#[derive(Deserialize)]
struct Header {
    format: String,
    version: u32,
}

/// Raw file layout, validated by [`parse`] and written by [`save`].
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    format: String,
    version: u32,
    layout: String,
    theme: ThemeSpec,
}

/// Writes `appearance` to `path`.
///
/// A new export uses exclusive creation, reporting [`Error::Exists`] on a name
/// collision. A write failure may leave a partial *new* file. A confirmed
/// replacement writes a sibling temporary file, then asks the filesystem to
/// rename it over the existing regular file; it never truncates the old file.
pub fn save(appearance: &Appearance, path: &Path, overwrite: bool) -> Result<(), Error> {
    let file = RawFile {
        format: FORMAT.to_owned(),
        version: VERSION,
        layout: layout_id(appearance.layout).to_owned(),
        theme: ThemeSpec::from_theme(&appearance.theme),
    };
    let json = serde_json::to_vec_pretty(&file).map_err(Error::Json)?;
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;

    if overwrite {
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                require_regular_file(&metadata)?;
                return replace_file(dir, path, &json);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }

    let mut file = match File::create_new(path) {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Err(Error::Exists),
        result => result?,
    };
    file.write_all(&json)?;
    Ok(())
}

fn replace_file(dir: &Path, path: &Path, bytes: &[u8]) -> Result<(), Error> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let (temp, mut file) = loop {
        let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temp = dir.join(format!(
            ".shirabe-appearance-{}-{serial}.tmp",
            std::process::id()
        ));
        match File::create_new(&temp) {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    };

    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    // Rename the directory entry, not its referent. If replacement isn't
    // supported (e.g. a Windows sharing violation), don't fall back to truncation.
    if let Err(error) = written.and_then(|()| fs::rename(&temp, path)) {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    Ok(())
}

fn require_regular_file(metadata: &fs::Metadata) -> io::Result<()> {
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "appearance path must be a regular file",
        ));
    }
    Ok(())
}

/// Reads and validates an appearance file.
pub fn load(path: &Path) -> Result<Appearance, Error> {
    // Reject stable special files and leaf symlinks before opening. Concurrent
    // changes between the check and open can still block the worker.
    require_regular_file(&fs::symlink_metadata(path)?)?;
    let file = File::open(path)?;
    require_regular_file(&file.metadata()?)?;
    let mut reader = file.take(MAX_BYTES + 1);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Error::TooLarge(MAX_BYTES));
    }
    parse(&bytes)
}

/// Validates appearance file bytes.
pub fn parse(bytes: &[u8]) -> Result<Appearance, Error> {
    let header: Header = serde_json::from_slice(bytes).map_err(Error::Json)?;
    if header.format != FORMAT {
        return Err(Error::WrongFormat(header.format));
    }
    if header.version != VERSION {
        return Err(Error::UnsupportedVersion(header.version));
    }
    let raw: RawFile = serde_json::from_slice(bytes).map_err(Error::Json)?;
    let layout = layout_from_id(&raw.layout).ok_or(Error::UnknownLayout(raw.layout))?;
    let theme = raw.theme.to_theme()?;
    Ok(Appearance { layout, theme })
}

/// Applies the layout of a validated appearance on the UI thread.
pub fn apply(appearance: &Appearance, params: &ShirabeParams, setter: &ParamSetter) {
    setter.begin_set_parameter(&params.layout_preset);
    setter.set_parameter(&params.layout_preset, appearance.layout);
    setter.end_set_parameter(&params.layout_preset);
}

/// Stable id of `preset` in a preset file.
pub fn layout_id(preset: LayoutPreset) -> &'static str {
    let ids = LayoutPreset::ids().expect("layout presets have stable ids");
    ids[preset.to_index()]
}

/// Parses a layout id from a preset file.
pub fn layout_from_id(id: &str) -> Option<LayoutPreset> {
    let ids = LayoutPreset::ids()?;
    let index = ids.iter().position(|candidate| *candidate == id)?;
    Some(LayoutPreset::from_index(index))
}

/// Why an appearance file could not be read, written or applied.
#[derive(Debug)]
pub enum Error {
    /// The file could not be read or written.
    Io(io::Error),
    /// The file is larger than [`MAX_BYTES`].
    TooLarge(u64),
    /// The bytes are not a valid appearance file.
    Json(serde_json::Error),
    /// The `format` field names another application or format.
    WrongFormat(String),
    /// The schema version is not the one this build reads.
    UnsupportedVersion(u32),
    /// The layout name is not one of this build's layouts.
    UnknownLayout(String),
    /// A colour, zone or field value is not valid.
    InvalidTheme(String),
    /// Export would replace an existing file without confirmation.
    Exists,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read or write the file: {e}"),
            Self::TooLarge(limit) => write!(f, "the file is larger than {limit} bytes"),
            Self::Json(e) => write!(f, "not a valid appearance file: {e}"),
            Self::WrongFormat(format) => write!(f, "not a Shirabe appearance file ({format:?})"),
            Self::UnsupportedVersion(version) => write!(
                f,
                "unsupported appearance version {version} (this build reads version {VERSION})"
            ),
            Self::UnknownLayout(layout) => write!(f, "unknown layout {layout:?}"),
            Self::InvalidTheme(reason) => write!(f, "invalid theme: {reason}"),
            Self::Exists => f.write_str("the file already exists"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use shirabe_plugin::params::{LevelMode, LoudnessTarget};

    use crate::context::{load_params, save_params, StandaloneContext};

    pub(crate) struct TestDir(PathBuf);

    impl std::ops::Deref for TestDir {
        type Target = Path;

        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    pub(crate) fn temp_dir(tag: &str) -> TestDir {
        let base = std::env::temp_dir();
        for serial in 0.. {
            let dir = base.join(format!(
                "shirabe-appearance-{}-{tag}-{serial}",
                std::process::id()
            ));
            match fs::create_dir(&dir) {
                Ok(()) => return TestDir(dir),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create test directory: {error}"),
            }
        }
        unreachable!("ran out of test directory names");
    }

    fn write_file(dir: &Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, text).unwrap();
        path
    }

    fn stereo_appearance() -> Appearance {
        Appearance {
            layout: LayoutPreset::Stereo,
            theme: Theme::dark(),
        }
    }

    #[test]
    fn a_preset_round_trips_through_a_file() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("appearance.json");
        let appearance = stereo_appearance();

        save(&appearance, &path, false).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("\"format\": \"shirabe-appearance\""),
            "{text}"
        );
        assert!(text.contains("\"version\": 1"), "{text}");
        assert!(text.contains("\"layout\": \"stereo\""), "{text}");

        assert_eq!(load(&path).unwrap(), appearance);
    }

    #[test]
    fn the_file_has_only_the_format_version_layout_and_theme_keys() {
        let dir = temp_dir("keys");
        let path = dir.join("appearance.json");
        save(&stereo_appearance(), &path, false).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["format", "layout", "theme", "version"]);
    }

    #[test]
    fn a_file_with_another_format_is_rejected() {
        let dir = temp_dir("format");
        let path = write_file(
            &dir,
            "other.json",
            r#"{"format":"other-app","version":1,"layout":"standard","theme":{}}"#,
        );
        assert!(matches!(load(&path), Err(Error::WrongFormat(_))));
    }

    #[test]
    fn a_newer_schema_version_is_rejected() {
        let dir = temp_dir("version");
        let path = write_file(
            &dir,
            "v2.json",
            r#"{"format":"shirabe-appearance","version":2,"layout":"standard","theme":{}}"#,
        );
        assert!(matches!(load(&path), Err(Error::UnsupportedVersion(2))));
    }

    #[test]
    fn an_unknown_layout_is_rejected() {
        let dir = temp_dir("layout");
        let appearance = stereo_appearance();
        let mut json = serde_json::to_value(serde_json::json!({
            "format": FORMAT,
            "version": VERSION,
            "layout": "standard",
            "theme": ThemeSpec::from_theme(&appearance.theme),
        }))
        .unwrap();
        json["layout"] = serde_json::json!("arbitrary-tree");
        let path = write_file(&dir, "tree.json", &json.to_string());
        assert!(matches!(
            load(&path),
            Err(Error::UnknownLayout(name)) if name == "arbitrary-tree"
        ));
    }

    #[test]
    fn unknown_fields_and_missing_theme_are_rejected() {
        let dir = temp_dir("schema");
        let appearance = stereo_appearance();
        let theme = serde_json::to_value(ThemeSpec::from_theme(&appearance.theme)).unwrap();
        let with_extra = serde_json::json!({
            "format": FORMAT,
            "version": VERSION,
            "layout": "standard",
            "theme": theme,
            "source": "something else",
        });
        let path = write_file(&dir, "extra.json", &with_extra.to_string());
        assert!(matches!(load(&path), Err(Error::Json(_))));

        let without_theme = serde_json::json!({
            "format": FORMAT,
            "version": VERSION,
            "layout": "standard",
        });
        let path = write_file(&dir, "missing.json", &without_theme.to_string());
        assert!(matches!(load(&path), Err(Error::Json(_))));
    }

    #[test]
    fn a_bad_colour_is_rejected() {
        let dir = temp_dir("colour");
        let mut theme = ThemeSpec::from_theme(&Theme::dark());
        theme.background = "red".to_owned();
        let json = serde_json::json!({
            "format": FORMAT,
            "version": VERSION,
            "layout": "standard",
            "theme": theme,
        });
        let path = write_file(&dir, "bad-colour.json", &json.to_string());
        assert!(matches!(load(&path), Err(Error::InvalidTheme(_))));

        theme.background = "#80121417".to_owned();
        let json = serde_json::json!({
            "format": FORMAT,
            "version": VERSION,
            "layout": "standard",
            "theme": theme,
        });
        let path = write_file(&dir, "alpha.json", &json.to_string());
        assert!(matches!(load(&path), Err(Error::InvalidTheme(_))));
    }

    #[test]
    fn hex_colours_require_the_hash_prefix() {
        assert!(matches!(
            parse_color("121417", "background"),
            Err(Error::InvalidTheme(_))
        ));
        assert_eq!(
            parse_color("#aBcD12", "background").unwrap(),
            Color32::from_rgb(0xab, 0xcd, 0x12)
        );
    }

    #[test]
    fn level_bar_palette_exposes_two_colour_pairs_without_thresholds() {
        let theme = ThemeSpec::from_theme(&Theme::dark());
        let mut theme = serde_json::to_value(theme).unwrap();
        assert!(theme.get("vu_zones").is_none());
        assert_eq!(theme["level_bar"]["normal"]["lit"], "#e8d8a8");
        assert_eq!(theme["level_bar"]["over"]["unlit"], "#361614");
        theme["level_bar"]["normal"]["lit"] = serde_json::json!("#123456");
        theme["level_bar"]["over"]["unlit"] = serde_json::json!("#abcdef");
        let raw = |theme| serde_json::json!({"format": FORMAT, "version": VERSION, "layout": "standard", "theme": theme});
        let back = parse(raw(theme.clone()).to_string().as_bytes())
            .unwrap()
            .theme;
        assert_eq!(back.vu_zones[0].lit, Color32::from_rgb(0x12, 0x34, 0x56));
        assert_eq!(back.vu_zones[1].unlit, Color32::from_rgb(0xab, 0xcd, 0xef));
        assert_eq!(back.vu_zones[1].from_db, 0.0);
        assert_eq!(back.over, Theme::dark().over);
        theme["level_bar"]["over"]["from_db"] = serde_json::json!(-10);
        assert!(matches!(
            parse(raw(theme).to_string().as_bytes()),
            Err(Error::Json(_))
        ));
    }

    #[test]
    fn zone_lists_must_be_non_empty_and_sorted() {
        let dir = temp_dir("zones");

        let mut theme = ThemeSpec::from_theme(&Theme::dark());
        theme.zones.clear();
        let json = serde_json::json!({
            "format": FORMAT, "version": VERSION, "layout": "standard", "theme": theme,
        });
        let path = write_file(&dir, "empty.json", &json.to_string());
        assert!(matches!(load(&path), Err(Error::InvalidTheme(_))));

        let mut theme = ThemeSpec::from_theme(&Theme::dark());
        theme.zones.swap(1, 2);
        let json = serde_json::json!({
            "format": FORMAT, "version": VERSION, "layout": "standard", "theme": theme,
        });
        let path = write_file(&dir, "unsorted.json", &json.to_string());
        assert!(matches!(load(&path), Err(Error::InvalidTheme(_))));

        theme.zones = (0..=MAX_ZONES as i32)
            .map(|db| ZoneSpec {
                from_db: Some(db as f32),
                lit: "#000000".to_owned(),
                unlit: "#000000".to_owned(),
            })
            .collect();
        let json = serde_json::json!({
            "format": FORMAT, "version": VERSION, "layout": "standard", "theme": theme,
        });
        let path = write_file(&dir, "many.json", &json.to_string());
        assert!(matches!(load(&path), Err(Error::InvalidTheme(_))));
    }

    #[test]
    fn an_oversized_file_is_rejected_before_it_is_parsed() {
        let dir = temp_dir("large");
        let path = dir.join("large.json");
        fs::write(&path, vec![b' '; MAX_BYTES as usize + 1]).unwrap();
        assert!(matches!(load(&path), Err(Error::TooLarge(MAX_BYTES))));
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let dir = temp_dir("missing");
        assert!(matches!(load(&dir.join("nothing.json")), Err(Error::Io(_))));
    }

    #[test]
    fn export_never_replaces_a_file_without_confirmation() {
        let dir = temp_dir("overwrite");
        let path = write_file(&dir, "appearance.json", "keep me");
        let error = save(&stereo_appearance(), &path, false).unwrap_err();
        assert!(matches!(error, Error::Exists));
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep me");

        save(&stereo_appearance(), &path, true).unwrap();
        assert_eq!(load(&path).unwrap(), stereo_appearance());
    }

    #[test]
    fn overwrite_requires_fresh_authorization_for_the_exact_visible_path() {
        let mut target = Target::new("first.json".into());
        assert!(target.begin(true).is_none());
        let (first, revision) = target.begin(false).unwrap();
        assert!(target.offer_overwrite(&first, revision));
        target.set_path("first.json ".into());
        target.set_path(first.clone());
        assert!(target.begin(true).is_none());

        let (_, revision) = target.begin(false).unwrap();
        assert!(target.offer_overwrite(&first, revision));
        // Starting another action invalidates confirmation, even if it fails.
        let (invalid_import, _) = target.begin(false).unwrap();
        let dir = temp_dir("invalid-action");
        assert!(load(&dir.join(invalid_import)).is_err());
        assert!(target.begin(true).is_none());

        let (_, revision) = target.begin(false).unwrap();
        assert!(target.offer_overwrite(&first, revision));
        assert_eq!(target.begin(true).unwrap().0, first);
        assert!(target.begin(true).is_none());
    }

    #[test]
    fn a_late_export_collision_cannot_restore_cleared_authorization() {
        let mut target = Target::new("export.json".into());
        let (path, revision) = target.begin(false).unwrap();
        target.invalidate();
        assert!(!target.offer_overwrite(&path, revision));
        assert!(target.begin(true).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn unconfirmed_export_does_not_follow_a_dangling_symlink() {
        let dir = temp_dir("dangling-link");
        let target = dir.join("missing.json");
        let link = dir.join("appearance.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let result = save(&stereo_appearance(), &link, false);
        assert!(matches!(result, Err(Error::Exists)), "{result:?}");
        assert!(!target.exists());
        assert_eq!(fs::read_link(&link).unwrap(), target);
    }

    #[cfg(unix)]
    #[test]
    fn confirmed_export_does_not_modify_a_symlinks_target() {
        let dir = temp_dir("confirmed-link");
        let target = write_file(&dir, "original.json", "keep me");
        let link = dir.join("appearance.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let result = save(&stereo_appearance(), &link, true);
        assert!(matches!(result, Err(Error::Io(_))), "{result:?}");
        assert_eq!(fs::read_link(&link).unwrap(), target);
        assert_eq!(fs::read_to_string(&target).unwrap(), "keep me");
    }

    #[test]
    fn confirmed_export_replaces_the_file_without_modifying_its_other_hard_link() {
        let dir = temp_dir("confirmed-hard-link");
        let path = write_file(&dir, "appearance.json", "keep me");
        let alias = dir.join("original.json");
        fs::hard_link(&path, &alias).unwrap();

        save(&stereo_appearance(), &path, true).unwrap();
        assert_eq!(load(&path).unwrap(), stereo_appearance());
        assert_eq!(fs::read_to_string(&alias).unwrap(), "keep me");
    }

    #[test]
    fn a_failed_replacement_removes_its_temporary_file() {
        let dir = temp_dir("failed-replace");
        let destination = dir.join("directory.json");
        fs::create_dir(&destination).unwrap();
        let unrelated = write_file(&dir, ".shirabe-appearance-unrelated.tmp", "keep me");

        // A directory cannot be replaced by a regular file. Exercise the rename
        // error path directly; save would normally reject this destination earlier.
        assert!(replace_file(&dir, &destination, b"new data").is_err());

        assert!(destination.is_dir());
        assert_eq!(fs::read_to_string(unrelated).unwrap(), "keep me");
        assert_eq!(fs::read_dir(&*dir).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn import_rejects_a_character_device() {
        let result = load(Path::new("/dev/null"));
        assert!(
            matches!(result, Err(Error::Io(ref e)) if e.kind() == io::ErrorKind::InvalidInput),
            "{result:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_named_pipe_is_rejected_without_waiting_for_a_writer_or_reader() {
        let dir = temp_dir("fifo");
        let path = dir.join("fifo.json");
        assert!(std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success());

        let mut worker = crate::file_io::Worker::new(|| {}).unwrap();
        worker
            .start(move || (load(&path), save(&stereo_appearance(), &path, true)))
            .unwrap();
        let (loaded, saved) = worker
            .receive(std::time::Duration::from_secs(5))
            .unwrap()
            .expect("opening the pipe would block without a reader or writer");
        assert!(matches!(loaded, Err(Error::Io(e)) if e.kind() == io::ErrorKind::InvalidInput));
        assert!(matches!(saved, Err(Error::Io(e)) if e.kind() == io::ErrorKind::InvalidInput));
    }

    #[test]
    fn import_changes_only_the_layout_parameter() {
        let dir = temp_dir("import");
        let path = dir.join("appearance.json");
        save(&stereo_appearance(), &path, false).unwrap();

        let params = ShirabeParams::default();
        load_params(
            &params,
            &BTreeMap::from([
                ("ld_c_lvl".to_owned(), -20.0),
                ("ld_c_tol".to_owned(), 1.5),
                ("osc_frq".to_owned(), 440.0),
                ("vu_ref".to_owned(), -20.0),
                ("lvl_mode".to_owned(), LevelMode::PpmTypeI.to_index() as f32),
                ("scp_lr".to_owned(), 1.0),
                ("vu_needle".to_owned(), 1.0),
            ]),
        );
        let before = save_params(&params);

        let setter = ParamSetter {
            raw_context: &StandaloneContext,
        };
        let imported = load(&path).unwrap();
        assert_eq!(save_params(&params), before);
        apply(&imported, &params, &setter);
        let after = save_params(&params);

        for (id, value) in &before {
            if id != "lay_preset" {
                assert_eq!(after.get(id), Some(value), "{id} changed");
            }
        }
        assert_eq!(before.get("lay_preset"), Some(&0.0));
        assert_eq!(after.get("lay_preset"), Some(&1.0));
        assert_eq!(params.layout_preset.value(), LayoutPreset::Stereo);

        // Target, oscillator and display settings are still the user's values.
        assert_eq!(params.custom_loudness.value(), -20.0);
        assert_eq!(params.custom_tolerance.value(), 1.5);
        assert!((params.osc_frequency.value() - 440.0).abs() < 0.01);
        assert_eq!(params.vu_reference.value(), -20.0);
        assert_eq!(params.level_mode.value(), LevelMode::PpmTypeI);
        assert!(params.scope_view_lr.value());
        assert!(params.vu_needle.value());
        assert_eq!(params.loudness_target.value(), LoudnessTarget::AribTrB32);
    }

    #[test]
    fn a_failed_import_leaves_the_layout_parameter_alone() {
        let dir = temp_dir("failed-import");
        let path = write_file(
            &dir,
            "bad.json",
            r#"{"format":"shirabe-appearance","version":1,"layout":"stereo","theme":{}}"#,
        );
        let params = ShirabeParams::default();
        let before: BTreeMap<String, f32> = save_params(&params);
        let setter = ParamSetter {
            raw_context: &StandaloneContext,
        };
        let result = load(&path);
        if let Ok(imported) = &result {
            apply(imported, &params, &setter);
        }
        assert!(result.is_err());
        assert_eq!(save_params(&params), before);
    }

    #[test]
    fn layout_ids_match_the_stored_parameter_ids() {
        assert_eq!(layout_id(LayoutPreset::Standard), "standard");
        assert_eq!(layout_id(LayoutPreset::Stereo), "stereo");
        assert_eq!(layout_from_id("standard"), Some(LayoutPreset::Standard));
        assert_eq!(layout_from_id("stereo"), Some(LayoutPreset::Stereo));
        assert_eq!(layout_from_id("Standard"), None);
        assert_eq!(layout_from_id("tree"), None);
    }

    #[test]
    fn the_default_theme_survives_a_round_trip_exactly() {
        let dir = temp_dir("theme");
        let path = dir.join("appearance.json");
        let theme = Theme::dark();
        save(
            &Appearance {
                layout: LayoutPreset::Standard,
                theme: theme.clone(),
            },
            &path,
            false,
        )
        .unwrap();
        assert_eq!(load(&path).unwrap().theme, theme);
    }
}
