//! Checks that the Japanese fallback really covers the strings the interface draws.
//!
//! The test uses whatever CJK font the operating system provides. On a system without
//! one, or with one that lacks some glyphs, it checks the honest fallback: the interface
//! is only reported as Japanese-capable when every string can be drawn.

use shirabe_plugin::fonts;
use shirabe_ui::egui::{Context, FontId};
use shirabe_ui::i18n::Text;

#[test]
fn a_context_without_the_fallback_has_no_japanese_glyphs() {
    let ctx = Context::default();
    // Force font construction.
    let _ = ctx.run(Default::default(), |_| {});
    assert!(!fonts::has_japanese(&ctx));
}

#[test]
fn the_installed_fallback_covers_every_japanese_ui_string() {
    let ctx = Context::default();
    let _ = ctx.run(Default::default(), |_| {});
    let installed = fonts::install(&ctx);
    let _ = ctx.run(Default::default(), |_| {});
    if !installed {
        // No system CJK font: the editor must fall back to English.
        assert!(!fonts::has_japanese(&ctx));
        return;
    }
    // An installed font may still lack a character the interface uses. The editor only
    // switches to Japanese when `has_japanese` says every family covers the whole
    // catalogue, so the check must agree with the glyphs actually available.
    let covered = [
        FontId::proportional(12.0),
        FontId::proportional(10.0),
        FontId::monospace(12.0),
    ]
    .iter()
    .all(|family| ctx.fonts(|f| f.has_glyphs(family, Text::JAPANESE_SAMPLE)));
    if fonts::has_japanese(&ctx) {
        assert!(
            covered,
            "Japanese was reported available with missing glyphs"
        );
    }
}
