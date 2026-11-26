//! A system font with Japanese (CJK) glyphs as a fallback for egui.
//!
//! egui's built-in fonts have no CJK glyphs, so device names such as "スピーカー" would show as
//! boxes. The font is read from the operating system at start-up; nothing is bundled.

use eframe::egui::{FontData, FontDefinitions, FontFamily};

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
        if let Ok(out) = std::process::Command::new("fc-match")
            .args(["-f", "%{file}", "sans-serif:lang=ja"])
            .output()
        {
            if out.status.success() {
                list.push(String::from_utf8_lossy(&out.stdout).into_owned());
            }
        }
        list.push("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc".to_owned());
        list.push("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc".to_owned());
    }
    list
}

/// Adds the first available system CJK font as the last fallback of every family.
pub fn install(ctx: &eframe::egui::Context) {
    let Some(bytes) = candidates()
        .into_iter()
        .filter(|p| !p.is_empty())
        .find_map(|p| std::fs::read(p).ok())
    else {
        return;
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
}
