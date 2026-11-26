//! Tests for the Japanese / English text catalogue.
//!
//! The tests pin the properties the editor relies on: English is the default, the
//! catalogue has no untranslated prose, and the Japanese sample fed to the font check
//! contains every Japanese glyph the interface can draw.

use shirabe_ui::i18n::{Lang, Text};

#[test]
fn english_is_the_default_and_japanese_needs_glyph_support() {
    assert_eq!(Lang::default(), Lang::English);
    assert_eq!(Lang::English.resolve(false), Lang::English);
    assert_eq!(Lang::English.resolve(true), Lang::English);
    assert_eq!(Lang::Japanese.resolve(false), Lang::English);
    assert_eq!(Lang::Japanese.resolve(true), Lang::Japanese);
}

#[test]
fn language_names_and_codes_are_stable() {
    assert_eq!(Lang::English.native_name(), "English");
    assert_eq!(Lang::Japanese.native_name(), "日本語");
    assert_eq!(Lang::English.code(), "EN");
    assert_eq!(Lang::Japanese.code(), "JA");
}

#[test]
fn the_catalogues_have_the_same_entries_and_no_empty_text() {
    assert_eq!(Text::ENGLISH.len(), Text::JAPANESE.len());
    assert!(!Text::ENGLISH.is_empty());
    assert!(Text::ENGLISH.iter().all(|s| !s.trim().is_empty()));
    assert!(Text::JAPANESE.iter().all(|s| !s.trim().is_empty()));
    // Strings are drawn on single lines; a newline would break the read-out layout.
    assert!(Text::ENGLISH.iter().all(|s| !s.contains('\n')));
    assert!(Text::JAPANESE.iter().all(|s| !s.contains('\n')));
}

#[test]
fn the_japanese_sample_contains_every_japanese_string() {
    for text in Text::JAPANESE {
        assert!(
            Text::JAPANESE_SAMPLE.contains(text),
            "{text:?} is missing from the glyph sample"
        );
    }
}

/// Technical designations, units and stable keys that stay identical in both languages.
/// Everything else must be translated.
const TECHNICAL: &[&str] = &[
    "VU",
    "LRA",
    "PLR",
    "TP ≤",
    "0 VU =",
    "L/R",
    "MONO",
    "PPM Type I  dB",
    "PPM EBU  dB",
    "PPM BBC",
    "PPM IEEE  dB",
    "ARIB TR-B32  -24 LKFS",
    "EBU R 128  -23 LUFS",
    "ATSC A/85  -24 LKFS",
    "Spotify  -14 LUFS",
    "Apple Podcasts  -16 LKFS",
    "PPM Type I (0 dB = -9 dBFS)",
    "PPM EBU (TEST = -18 dBFS)",
    "PPM BBC (4 = -18 dBFS)",
    "PPM IEEE (TEST -8 = -18 dBFS)",
];

#[test]
fn untranslated_entries_are_only_technical_designations() {
    for (english, japanese) in Text::ENGLISH.iter().zip(Text::JAPANESE) {
        if english == japanese {
            assert!(TECHNICAL.contains(english), "{english:?} is not translated");
        }
    }
}

#[test]
fn japanese_prose_is_really_japanese() {
    let japanese = Text::new(Lang::Japanese, true);
    for text in [
        japanese.layout(),
        japanese.reset(),
        japanese.integrated(),
        japanese.time(),
        japanese.target_label(),
        japanese.test_oscillator(),
        japanese.audio_api(),
        japanese.file_playback(),
    ] {
        assert!(!text.is_ascii(), "{text:?} contains no Japanese characters");
    }
}

#[test]
fn without_a_japanese_font_the_request_falls_back_to_english() {
    let text = Text::new(Lang::Japanese, false);
    assert_eq!(text.lang(), Lang::English);
    assert!(!text.japanese_available());
    assert_eq!(text.layout(), "Layout");
    // The selector must not offer a label the renderer cannot draw.
    assert_eq!(text.language_option(Lang::Japanese), None);
    assert_eq!(text.language_option(Lang::English), Some("English"));
    // Until the fallback font is loaded, the entry uses its ASCII name so it stays legible.
    assert_eq!(text.language_option_label(Lang::Japanese), "Japanese");
}

#[test]
fn the_language_selector_names_each_language_in_its_own_script() {
    let text = Text::new(Lang::English, true);
    assert_eq!(text.language_option(Lang::English), Some("English"));
    assert_eq!(text.language_option(Lang::Japanese), Some("日本語"));
    assert_eq!(text.language_option_label(Lang::Japanese), "日本語");
}

#[test]
fn messages_with_a_path_or_error_keep_the_value() {
    let english = Text::new(Lang::English, true);
    let japanese = Text::new(Lang::Japanese, true);
    let path = "/tmp/appearance.json";
    let error = "cannot read the file";
    assert!(english.file_exists(path).contains(path));
    assert!(japanese.file_exists(path).contains(path));
    assert!(english.cannot_import(error).contains(error));
    assert!(japanese.cannot_import(error).contains(error));
    assert!(english.imported(path).contains(path));
    assert!(japanese.imported(path).contains(path));
}
