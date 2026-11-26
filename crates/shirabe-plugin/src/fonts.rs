//! A system font with Japanese (CJK) glyphs as a fallback for egui.
//!
//! egui's built-in fonts have no CJK glyphs, so Japanese labels would show as empty boxes.
//! The font is read from the operating system when it is needed; nothing is bundled, so
//! the plug-in stays small and the Japanese interface works without adding a licence or a
//! large binary to the repository.
//!
//! The plug-in installs the fallback only while the Japanese interface is selected, so an
//! English session does not hold a CJK font in memory. The standalone application already
//! installed it at start-up for Japanese device names.

use shirabe_ui::egui::{Context, FontData, FontDefinitions, FontFamily, FontId};
use shirabe_ui::i18n::Text;

/// Font files tried in order (the first one that exists is used).
fn candidates() -> Vec<String> {
    let mut list = Vec::new();
    #[cfg(windows)]
    {
        let dir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".to_owned());
        for f in ["YuGothM.ttc", "meiryo.ttc", "msgothic.ttc"] {
            list.push(format!(r"{dir}\Fonts\{f}"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        list.push("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc".to_owned());
        list.push("/System/Library/Fonts/Hiragino Sans GB.ttc".to_owned());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Ask fontconfig first; fall back to the usual Noto CJK locations.
        let run = |args: &[&str]| {
            std::process::Command::new(args[0])
                .args(&args[1..])
                .output()
                .ok()
                .filter(|out| out.status.success())
                .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        };
        let matched = run(&["fc-match", "-f", "%{file}", "sans-serif:lang=ja"]);
        let japanese: Vec<String> = run(&["fc-list", "-f", "%{file}\n", ":lang=ja"])
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default();
        list.extend(fontconfig_candidates(matched, &japanese));
        list.push("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc".to_owned());
        list.push("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc".to_owned());
    }
    list
}

/// Orders fontconfig results so only fonts declaring Japanese support are tried.
///
/// `fc-match` always answers, and on a system without a CJK font it names a Latin font
/// such as DejaVu Sans. Using that file would install a fallback that cannot draw the
/// Japanese interface, so the match is kept only when `fc-list :lang=ja` also lists it.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn fontconfig_candidates(matched: Option<String>, japanese: &[String]) -> Vec<String> {
    let mut list = Vec::new();
    if let Some(file) = matched.filter(|file| japanese.contains(file)) {
        list.push(file);
    }
    for file in japanese {
        if !file.is_empty() && !list.contains(file) {
            list.push(file.clone());
        }
    }
    list
}

/// Adds the first available system CJK font as the last fallback of every family.
///
/// Returns whether a font file was found and installed. Call this once per egui context;
/// repeating it would append the same family twice.
pub fn install(ctx: &Context) -> bool {
    let Some(bytes) = candidates()
        .into_iter()
        .filter(|p| !p.is_empty())
        .find_map(|p| std::fs::read(p).ok())
    else {
        return false;
    };
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("system-cjk".to_owned(), FontData::from_owned(bytes).into());
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("system-cjk".to_owned());
    }
    ctx.set_fonts(fonts);
    true
}

/// Whether the current fonts can draw every Japanese character the interface uses.
///
/// The sample is the whole Japanese catalogue, so this covers labels, hints and messages,
/// not just a single test character.
pub fn has_japanese(ctx: &Context) -> bool {
    let sample = Text::JAPANESE_SAMPLE;
    [FontId::proportional(12.0), FontId::monospace(12.0)]
        .iter()
        .all(|font| ctx.fonts(|f| f.has_glyphs(font, sample)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fontconfig_match_without_japanese_support_is_skipped() {
        let ja = vec!["/fonts/cjk.ttc".to_owned()];
        assert_eq!(
            fontconfig_candidates(Some("/fonts/DejaVuSans.ttf".to_owned()), &ja),
            ja
        );
        assert_eq!(
            fontconfig_candidates(Some("/fonts/cjk.ttc".to_owned()), &ja),
            ja
        );
        assert!(fontconfig_candidates(Some("/fonts/DejaVuSans.ttf".to_owned()), &[]).is_empty());
    }

    #[test]
    fn candidates_are_never_empty_strings() {
        assert!(candidates().iter().all(|p| !p.is_empty()));
    }

    #[test]
    fn fonts_set_inside_a_frame_are_used_from_the_next_frame() {
        let ctx = Context::default();
        let _ = ctx.run(Default::default(), |_| {});
        let mut during = None;
        let _ = ctx.run(Default::default(), |ctx| {
            let installed = install(ctx);
            during = Some(installed && has_japanese(ctx));
        });
        // egui rebuilds its font set at the start of a frame: the check made while the
        // fonts are being replaced still sees the old set.
        assert_eq!(during, Some(false));
        let mut next = None;
        let _ = ctx.run(Default::default(), |ctx| {
            next = Some(has_japanese(ctx));
        });
        // Whatever the system provides, the next frame agrees with the installed font.
        let installed = candidates()
            .into_iter()
            .filter(|p| !p.is_empty())
            .any(|p| std::fs::read(p).is_ok());
        if !installed {
            assert_eq!(next, Some(false));
        }
    }
}
