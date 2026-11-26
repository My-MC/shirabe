//! Japanese and English text for the shared editor and the standalone window.
//!
//! English is the default. The strings here are view-only: technical units (`dBFS`,
//! `dBTP`, `LUFS`, `LU`, `dB`), standard designations (`EBU`, `ARIB TR-B32`, `ATSC A/85`,
//! `PPM`), the channel labels and every parameter ID stay unchanged in both languages.
//!
//! Japanese needs a font with CJK glyphs. [`Lang::resolve`] falls back to English when
//! none is available, so the interface never shows empty boxes. The caller reports that
//! availability (see `shirabe_plugin::fonts`).
//!
//! Untranslated on purpose:
//! - Operating-system device names, host names and file paths.
//! - Detailed errors raised by the appearance-file parser (the message text is English;
//!   the surrounding sentence is translated).
//! - `CLIP`, `FIXED`, `AUTO` and `HOLD` on the scope, which are short technical states.

/// A display language.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    /// English (the default).
    #[default]
    English,
    /// Japanese.
    Japanese,
}

impl Lang {
    /// Every language the interface offers, in selector order.
    pub const ALL: [Lang; 2] = [Lang::English, Lang::Japanese];

    /// The language's own name, so a reader can find it in either interface language.
    pub fn native_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Japanese => "日本語",
        }
    }

    /// Short code shown by the compact selector (`EN`, `JA`).
    pub fn code(self) -> &'static str {
        match self {
            Self::English => "EN",
            Self::Japanese => "JA",
        }
    }

    /// The language that can actually be drawn: Japanese only with CJK glyph support.
    pub fn resolve(self, japanese_available: bool) -> Self {
        match self {
            Self::Japanese if !japanese_available => Self::English,
            other => other,
        }
    }
}

/// Text in the requested language, resolved against the available glyphs.
#[derive(Clone, Copy, Debug)]
pub struct Text {
    lang: Lang,
    japanese: bool,
}

impl Text {
    /// Text for `requested`, falling back to English without CJK glyph support.
    pub fn new(requested: Lang, japanese_available: bool) -> Self {
        Self {
            lang: requested.resolve(japanese_available),
            japanese: japanese_available,
        }
    }

    /// The language that will be drawn.
    pub fn lang(self) -> Lang {
        self.lang
    }

    /// Whether Japanese glyphs are available on this system.
    pub fn japanese_available(self) -> bool {
        self.japanese
    }

    /// Label of `option` in the language selector, or `None` when it cannot be drawn.
    pub fn language_option(self, option: Lang) -> Option<&'static str> {
        match (option, self.japanese) {
            (Lang::Japanese, false) => None,
            (option, _) => Some(option.native_name()),
        }
    }

    /// Label for the selector before it is known whether Japanese can be drawn.
    ///
    /// The plug-in loads the system CJK font when Japanese is first requested, so the
    /// entry must exist before that. It uses the ASCII name until the font is available,
    /// because "日本語" itself would otherwise be drawn with missing glyphs.
    pub fn language_option_label(self, option: Lang) -> &'static str {
        match (option, self.japanese) {
            (Lang::Japanese, false) => "Japanese",
            (option, _) => option.native_name(),
        }
    }
}

macro_rules! texts {
    ($( $name:ident: $english:literal, $japanese:literal; )*) => {
        impl Text {
            $(
                #[doc = concat!("`", $english, "` / `", $japanese, "`")]
                pub fn $name(self) -> &'static str {
                    match self.lang {
                        Lang::English => $english,
                        Lang::Japanese => $japanese,
                    }
                }
            )*

            /// Every English string, in catalogue order.
            pub const ENGLISH: &'static [&'static str] = &[$($english),*];
            /// Every Japanese string, in catalogue order.
            pub const JAPANESE: &'static [&'static str] = &[$($japanese),*];
            /// Every Japanese string concatenated, for glyph-coverage checks.
            pub const JAPANESE_SAMPLE: &'static str = concat!($($japanese),*);
        }
    };
}

texts! {
    // Header.
    layout: "Layout", "配置";
    layout_hint: "Meter arrangement; the measurement and its values do not change",
        "メーターの配置です。測定と測定値は変わりません";
    corr: "Corr", "相関";
    corr_hint: "Stereo correlation over 400 ms: −1 opposite phase, +1 in phase",
        "400 ms のステレオ相関です。−1 で逆相、+1 で同相";
    bal: "Bal", "バランス";
    bal_hint: "Left/right energy difference over 400 ms; shows the louder side",
        "400 ms の左右のエネルギー差です。大きい側を表示します";
    reset: "Reset", "リセット";
    reset_hint: "Reset integrated loudness, loudness range and maxima together",
        "統合ラウドネス、ラウドネスレンジ、最大値をまとめてリセットします";
    pause: "Pause", "一時停止";
    resume: "Continue", "再開";
    pause_hint: "Pause or continue integrated loudness and loudness range together",
        "統合ラウドネスとラウドネスレンジをまとめて一時停止・再開します";
    language: "Language", "表示言語";
    language_hint: "Interface language; the measurement does not change",
        "画面の表示言語です。測定は変わりません";
    language_unavailable: "Japanese (CJK font not found)",
        "日本語（CJK フォントがありません）";
    language_unavailable_hint:
        "Install a Japanese font such as Noto Sans CJK to use the Japanese interface",
        "日本語の表示には Noto Sans CJK などの日本語フォントが必要です";
    layout_standard: "Standard", "標準";
    layout_stereo: "Stereo", "ステレオ";
    layout_history: "History", "履歴";

    // Meter tiles.
    meter_reset_hint: "Double-click to reset integrated loudness, loudness range and maxima",
        "ダブルクリックで統合ラウドネス、ラウドネスレンジ、最大値をリセットします";
    peak_title: "PEAK dBFS / TP dBTP", "ピーク dBFS / TP dBTP";
    level_vu_title: "VU", "VU";
    level_vu_needle_title: "VU  ·  Needle", "VU  ·  針";
    level_ppm_i_title: "PPM Type I  dB", "PPM Type I  dB";
    level_ppm_ebu_title: "PPM EBU  dB", "PPM EBU  dB";
    level_ppm_bbc_title: "PPM BBC", "PPM BBC";
    level_ppm_ieee_title: "PPM IEEE  dB", "PPM IEEE  dB";
    mode_ebu: "EBU Mode", "EBUモード";
    mode_custom: "Custom", "カスタム";
    mono_label: "MONO", "MONO";
    no_stereo_content: "no stereo content", "ステレオ成分がありません";
    history_title: "LOUDNESS HISTORY", "ラウドネス履歴";
    history_span_label: "3 min", "3 分";
    history_time_axis: "s ago", "秒前";
    history_waiting: "waiting for audio", "音声を待っています";

    // Loudness read-out.
    integrated: "INTEGRATED", "統合ラウドネス";
    integrating_paused: "PAUSED", "一時停止";
    lra: "LRA", "LRA";
    plr: "PLR", "PLR";
    m_max: "M max", "M 最大";
    s_max: "S max", "S 最大";
    tp_max: "TP max", "TP 最大";
    time: "Time", "時間";
    target_label: "Target", "目標";
    lra_note: "* LRA not yet stable (< 60 s)", "＊LRA は 60 秒まで不安定";

    // Settings.
    scale_ebu9: "+9 scale", "+9 目盛り";
    scale_ebu18: "+18 scale", "+18 目盛り";
    unit_absolute: "Absolute", "絶対値";
    unit_relative: "Relative (LU)", "目標との差 (LU)";
    reset_on_play: "Reset on play", "再生時にリセット";
    reset_on_play_hint: "Reset the programme measurement when the host starts playback",
        "ホストの再生開始に合わせて番組の測定をリセットします";
    custom: "Custom", "カスタム";
    custom_target_hint: "Target integrated loudness", "統合ラウドネスの目標値";
    custom_tolerance_hint: "Tolerance around the target (none = no tolerance)",
        "目標値の許容範囲（なし = 許容範囲なし）";
    custom_tp_hint: "True-peak ceiling", "トゥルーピークの上限";
    tp_le: "TP ≤", "TP ≤";
    level_label: "Level", "レベル";
    needle: "Needle", "針";
    needle_hint: "Switch the VU display between bars and moving needles",
        "VU の表示をバーと針で切り替えます";
    vu_reference: "0 VU =", "0 VU =";
    lr_view: "L/R", "L/R";
    lr_hint: "Plot left on the horizontal and right on the vertical axis",
        "横軸を L、縦軸を R にして表示します";
    trail: "Trail", "残光";
    trail_hint: "Fade the recent Lissajous figures over about 0.3 seconds",
        "直近の図を約 0.3 秒かけて薄く表示します";
    auto: "Auto", "自動";
    auto_hint: "Fit the figure to the signal (up to +20 dB) instead of the fixed ±1 scale",
        "固定の ±1 目盛りの代わりに、信号に合わせて最大 +20 dB 拡大します";
    test_oscillator: "Test oscillator", "テストオシレーター";
    osc_replaces_input: "Test tone input", "テスト音を入力中";
    pass_through: "Pass-through, no latency", "素通し（遅延なし）";
    measuring_source: "Measuring input", "入力を測定中";
    target_arib: "ARIB TR-B32  -24 LKFS", "ARIB TR-B32  -24 LKFS";
    target_ebu: "EBU R 128  -23 LUFS", "EBU R 128  -23 LUFS";
    target_atsc: "ATSC A/85  -24 LKFS", "ATSC A/85  -24 LKFS";
    target_spotify: "Spotify  -14 LUFS", "Spotify  -14 LUFS";
    target_apple: "Apple Podcasts  -16 LKFS", "Apple Podcasts  -16 LKFS";
    target_custom: "Custom", "カスタム";
    level_mode_vu: "VU", "VU";
    level_mode_ppm_i: "PPM Type I (0 dB = -9 dBFS)", "PPM Type I (0 dB = -9 dBFS)";
    level_mode_ppm_ebu: "PPM EBU (TEST = -18 dBFS)", "PPM EBU (TEST = -18 dBFS)";
    level_mode_ppm_bbc: "PPM BBC (4 = -18 dBFS)", "PPM BBC (4 = -18 dBFS)";
    level_mode_ppm_ieee: "PPM IEEE (TEST -8 = -18 dBFS)", "PPM IEEE (TEST -8 = -18 dBFS)";

    // Standalone window.
    audio_api: "Audio API", "オーディオ API";
    source: "Source", "入力";
    none: "None", "なし";
    inputs: "Inputs", "入力";
    playing_here: "Playing on this computer", "このコンピューターの再生音";
    file_playback: "File playback", "ファイル再生";
    audio_file: "Audio file", "音声ファイル";
    refresh: "Refresh", "再読み込み";
    refresh_hint: "Search for devices again", "デバイスを再検索します";
    appearance: "Appearance", "外観";
    appearance_hint: "Import or export the layout and theme colours",
        "配置とテーマ色を読み込み・書き出しします";
    output: "Output", "出力";
    default_output: "Default", "既定";
    file: "File", "ファイル";
    file_hint: "WAV or FLAC path; a dropped file works too",
        "WAV または FLAC のパス（ファイルのドロップも可）";
    play: "Play", "再生";
    play_hint: "Play the file and measure the stored samples",
        "ファイルを再生して、記録されたサンプルを測定します";
    pause_playback: "Pause", "一時停止";
    resume_playback: "Resume", "再開";
    pause_playback_hint: "Pause keeps the position; the meters fall",
        "一時停止すると位置を保ったまま、メーターは下がります";
    from_start: "From start", "最初から";
    from_start_hint: "Play the file again from the beginning",
        "ファイルを先頭から再生し直します";
    end: "(end)", "（終了）";
    appearance_title: "Appearance preset", "外観プリセット";
    appearance_note: "Layout and theme colours only. Sources, targets, the oscillator and measured values stay unchanged.",
        "配置とテーマ色だけを扱います。入力、目標値、オシレーター、測定値は変わりません。";
    appearance_path_hint: "Path to an appearance .json file", "外観 .json ファイルのパス";
    import: "Import", "読み込み";
    export: "Export", "書き出し";
    overwrite: "Overwrite", "上書き";
    cancel: "Cancel", "キャンセル";
    import_hint: "Apply the layout and colours from this file",
        "このファイルの配置と色を適用します";
    export_hint: "Write the current layout and colours to this file",
        "現在の配置と色をこのファイルに書き出します";
    overwrite_hint: "Replace only the file named below", "下に表示されたファイルだけを置き換えます";
    reading_appearance: "Reading appearance file…", "外観ファイルを読み込み中…";
    writing_appearance: "Writing appearance file…", "外観ファイルを書き出し中…";
    measurement_export: "Export measurement", "測定結果の書き出し";
    measurement_export_hint: "Write the values and the recorded history shown now to a file",
        "現在の測定値と記録した履歴をファイルに書き出します";
    measurement_export_note: "Writes the summary and the recorded loudness history at the moment you press Export. The measurement keeps running.",
        "「書き出し」を押した時点の概要と、記録したラウドネスの履歴を書きます。測定は止まりません。";
    measurement_format: "Format", "形式";
    measurement_path_hint: "Path to a measurement .csv or .json file",
        "測定結果の .csv / .json ファイルのパス";
    include_source_path: "Include the full file path", "ファイルのフルパスを含める";
    include_source_path_hint: "Off writes only the name of the measured file",
        "オフでは測定したファイルの名前だけを書きます";
    writing_measurement: "Writing measurement export…", "測定結果を書き出し中…";
}

impl Text {
    /// Label of a file chosen for playback, including its name.
    pub fn audio_file_named(self, name: &str) -> String {
        match self.lang {
            Lang::English => format!("Audio file: {name}"),
            Lang::Japanese => format!("音声ファイル: {name}"),
        }
    }

    /// Confirmation shown before replacing an existing appearance file.
    pub fn file_exists(self, path: &str) -> String {
        match self.lang {
            Lang::English => format!("File already exists. Replace ‘{path}’?"),
            Lang::Japanese => format!("ファイルが存在します。「{path}」を置き換えますか？"),
        }
    }

    /// Stream error reported by the audio device or the file feed.
    pub fn stream_error(self, error: &str) -> String {
        match self.lang {
            Lang::English => format!("Stream error: {error}"),
            Lang::Japanese => format!("ストリームエラー: {error}"),
        }
    }

    /// A source that could not be opened.
    pub fn cannot_start(self, error: &str) -> String {
        match self.lang {
            Lang::English => format!("Cannot start: {error}"),
            Lang::Japanese => format!("開始できません: {error}"),
        }
    }

    /// An appearance file that failed validation. The parser's detail stays as it is.
    pub fn cannot_import(self, error: &str) -> String {
        match self.lang {
            Lang::English => format!("Cannot import: {error}"),
            Lang::Japanese => format!("読み込めません: {error}"),
        }
    }

    /// A failed export, including the destination path.
    pub fn cannot_export(self, path: &str, error: &str) -> String {
        match self.lang {
            Lang::English => format!("Cannot export {path:?}: {error}"),
            Lang::Japanese => format!("{path} を書き出せません: {error}"),
        }
    }

    /// A completed import.
    pub fn imported(self, path: &str) -> String {
        match self.lang {
            Lang::English => format!("Imported {path}."),
            Lang::Japanese => format!("{path} を読み込みました。"),
        }
    }

    /// A completed import whose settings are still being saved.
    pub fn imported_saving(self, path: &str) -> String {
        match self.lang {
            Lang::English => format!("Imported {path}; saving settings…"),
            Lang::Japanese => format!("{path} を読み込みました。設定を保存中…"),
        }
    }

    /// A completed export.
    pub fn exported(self, path: &str) -> String {
        match self.lang {
            Lang::English => format!("Exported {path}."),
            Lang::Japanese => format!("{path} を書き出しました。"),
        }
    }

    /// Import requested without a path.
    pub fn choose_import_path(self) -> &'static str {
        match self.lang {
            Lang::English => "Choose a file path to import.",
            Lang::Japanese => "読み込むファイルのパスを指定してください。",
        }
    }

    /// Export requested without a path.
    pub fn choose_export_path(self) -> &'static str {
        match self.lang {
            Lang::English => "Choose a file path to export.",
            Lang::Japanese => "書き出すファイルのパスを指定してください。",
        }
    }

    /// Overwrite requested before the file to replace was confirmed.
    pub fn overwrite_needs_export(self) -> &'static str {
        match self.lang {
            Lang::English => "Use Export first to confirm the file to replace.",
            Lang::Japanese => "置き換えるファイルを確認するため、先に書き出しを実行してください。",
        }
    }

    /// The appearance was applied but the settings file could not be written.
    pub fn settings_not_saved(self, error: &str) -> String {
        match self.lang {
            Lang::English => format!(
                "Appearance applied, but settings could not be saved and may not survive \
                 restart: {error}"
            ),
            Lang::Japanese => format!(
                "外観は適用しましたが、設定を保存できませんでした。再起動後に維持されない\
                 可能性があります: {error}"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_is_the_default_language() {
        assert_eq!(Lang::default(), Lang::English);
        assert_eq!(Text::new(Lang::English, false).lang(), Lang::English);
    }

    #[test]
    fn japanese_without_glyphs_resolves_to_english() {
        let text = Text::new(Lang::Japanese, false);
        assert_eq!(text.lang(), Lang::English);
        assert_eq!(text.reset(), "Reset");
    }

    #[test]
    fn the_measurement_export_window_is_translated() {
        let english = Text::new(Lang::English, true);
        let japanese = Text::new(Lang::Japanese, true);
        for (en, ja) in [
            (english.measurement_export(), japanese.measurement_export()),
            (
                english.measurement_export_hint(),
                japanese.measurement_export_hint(),
            ),
            (
                english.measurement_export_note(),
                japanese.measurement_export_note(),
            ),
            (english.measurement_format(), japanese.measurement_format()),
            (
                english.measurement_path_hint(),
                japanese.measurement_path_hint(),
            ),
            (
                english.include_source_path(),
                japanese.include_source_path(),
            ),
            (
                english.include_source_path_hint(),
                japanese.include_source_path_hint(),
            ),
            (
                english.writing_measurement(),
                japanese.writing_measurement(),
            ),
        ] {
            assert_ne!(en, ja);
            assert!(!en.is_empty() && !ja.is_empty());
            assert!(!ja.is_ascii(), "not Japanese: {ja}");
        }
        // Units and format names stay unchanged in both languages.
        assert_eq!(english.measurement_format(), "Format");
        assert_eq!(japanese.measurement_format(), "形式");
    }
}
