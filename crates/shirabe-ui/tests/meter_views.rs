//! Registration table of the editor's meter views.
//!
//! Every tile a preset places must resolve to exactly one view, and the views must draw in
//! the order that shipped with the preset, because a tile may paint its heading slightly
//! outside its rectangle. The identifier strings are the future layout-file keys, so the
//! test pins them.

use shirabe_ui::egui::{self, pos2, vec2, LayerId, Painter, Rect};
use shirabe_ui::layout::{arrange, LayoutPreset, TileId, UnknownTile};
use shirabe_ui::view::{MeterView, MeterViewRegistry, RegistryError};

fn area(width: f32, height: f32) -> Rect {
    Rect::from_min_size(pos2(0.0, 0.0), vec2(width, height))
}

/// Records the tiles the registry draws, in call order, with their rectangles.
#[derive(Default)]
struct Recorder {
    calls: Vec<(TileId, Rect)>,
}

fn peak(ctx: &mut Recorder, _painter: &Painter, rect: Rect) {
    ctx.calls.push((TileId::Peak, rect));
}

fn level(ctx: &mut Recorder, _painter: &Painter, rect: Rect) {
    ctx.calls.push((TileId::Level, rect));
}

fn loudness(ctx: &mut Recorder, _painter: &Painter, rect: Rect) {
    ctx.calls.push((TileId::Loudness, rect));
}

fn readout(ctx: &mut Recorder, _painter: &Painter, rect: Rect) {
    ctx.calls.push((TileId::Readout, rect));
}

fn scope(ctx: &mut Recorder, _painter: &Painter, rect: Rect) {
    ctx.calls.push((TileId::Scope, rect));
}

fn history(ctx: &mut Recorder, _painter: &Painter, rect: Rect) {
    ctx.calls.push((TileId::History, rect));
}

fn spectrum(ctx: &mut Recorder, _painter: &Painter, rect: Rect) {
    ctx.calls.push((TileId::Spectrum, rect));
}

fn reference(ctx: &mut Recorder, _painter: &Painter, rect: Rect) {
    ctx.calls.push((TileId::Reference, rect));
}

fn registry() -> [MeterView<Recorder>; 8] {
    [
        MeterView::new(TileId::Peak, peak),
        MeterView::new(TileId::Level, level),
        MeterView::new(TileId::Loudness, loudness),
        MeterView::new(TileId::Readout, readout),
        MeterView::new(TileId::Scope, scope),
        MeterView::new(TileId::History, history),
        MeterView::new(TileId::Spectrum, spectrum),
        MeterView::new(TileId::Reference, reference),
    ]
}

/// Runs `draw` with a painter that needs no window.
fn draw_calls(
    preset: LayoutPreset,
    rect: Rect,
    views: &[MeterView<Recorder>],
) -> Vec<(TileId, Rect)> {
    let arrangement = arrange(preset, rect);
    let registry = MeterViewRegistry::new(views).expect("unique views");
    let mut recorder = Recorder::default();
    let ctx = egui::Context::default();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        },
        |ctx| {
            let painter = ctx.layer_painter(LayerId::background());
            registry.draw(&painter, &arrangement, &mut recorder);
        },
    );
    recorder.calls
}

#[test]
fn tile_ids_are_unique_and_stable() {
    let names: Vec<&str> = TileId::ALL.iter().map(|t| t.as_str()).collect();
    // Saved layout files will refer to these names: renaming one breaks them.
    assert_eq!(
        names,
        [
            "peak",
            "level",
            "loudness",
            "readout",
            "scope",
            "history",
            "spectrum",
            "reference",
        ]
    );
    for (index, tile) in TileId::ALL.iter().enumerate() {
        assert_eq!(tile.as_str().parse(), Ok(*tile));
        assert!(!names[index + 1..].contains(&tile.as_str()), "{tile:?}");
    }
}

#[test]
fn unknown_tile_names_are_rejected() {
    assert_eq!("Peak".parse::<TileId>(), Err(UnknownTile));
    assert_eq!("".parse::<TileId>(), Err(UnknownTile));
    assert_eq!("peak ".parse::<TileId>(), Err(UnknownTile));
}

#[test]
fn a_duplicate_view_is_rejected() {
    let views = [
        MeterView::new(TileId::Peak, peak),
        MeterView::new(TileId::Peak, level),
    ];
    assert_eq!(
        MeterViewRegistry::new(&views).err(),
        Some(RegistryError::DuplicateTile(TileId::Peak))
    );
}

#[test]
fn lookup_finds_each_registered_tile_once() {
    let views = registry();
    let registry = MeterViewRegistry::new(&views).expect("unique views");
    for tile in TileId::ALL {
        let view = registry.get(tile).expect("registered");
        assert_eq!(view.id(), tile);
    }
    let partial = [MeterView::new(TileId::Peak, peak)];
    let partial = MeterViewRegistry::new(&partial).expect("unique views");
    assert!(partial.get(TileId::Scope).is_none());
}

#[test]
fn every_tile_of_every_preset_has_a_view() {
    for preset in [
        LayoutPreset::Standard,
        LayoutPreset::Stereo,
        LayoutPreset::History,
        LayoutPreset::Spectrum,
        LayoutPreset::Compare,
    ] {
        for (w, h) in [
            (900.0, 400.0),
            (820.0, 500.0),
            (820.0, 240.0),
            (640.0, 380.0),
            (640.0, 260.0),
            (640.0, 80.0),
        ] {
            let rect = area(w, h);
            let arrangement = arrange(preset, rect);
            let views = registry();
            let registry = MeterViewRegistry::new(&views).expect("unique views");
            let missing: Vec<TileId> = registry.missing(&arrangement).collect();
            assert!(missing.is_empty(), "{preset:?} {w}x{h}: {missing:?}");
            let calls = draw_calls(preset, rect, &views);
            assert_eq!(
                calls.len(),
                arrangement.placements().len(),
                "{preset:?} {w}x{h}"
            );
        }
    }
}

#[test]
fn an_unregistered_tile_is_reported_and_skipped() {
    let rect = area(900.0, 400.0);
    let arrangement = arrange(LayoutPreset::Standard, rect);
    let views = [
        MeterView::new(TileId::Peak, peak),
        MeterView::new(TileId::Level, level),
        MeterView::new(TileId::Loudness, loudness),
        MeterView::new(TileId::Readout, readout),
    ];
    let registry = MeterViewRegistry::new(&views).expect("unique views");
    assert_eq!(
        registry.missing(&arrangement).collect::<Vec<_>>(),
        [TileId::Scope]
    );
    let calls = draw_calls(LayoutPreset::Standard, rect, &views);
    assert_eq!(calls.len(), 4);
    assert!(calls.iter().all(|(tile, _)| *tile != TileId::Scope));
}

#[test]
fn views_draw_in_registration_order_with_the_arrangement_rectangles() {
    let rect = area(900.0, 400.0);
    let arrangement = arrange(LayoutPreset::Stereo, rect);
    let calls = draw_calls(LayoutPreset::Stereo, rect, &registry());
    // `Stereo` places the scope third, but the views keep the shipped drawing order.
    let order: Vec<TileId> = calls.iter().map(|(tile, _)| *tile).collect();
    assert_eq!(
        order,
        [
            TileId::Peak,
            TileId::Level,
            TileId::Loudness,
            TileId::Readout,
            TileId::Scope
        ]
    );
    for (tile, drawn) in calls {
        assert_eq!(Some(drawn), arrangement.tile(tile), "{tile:?}");
    }
}

#[test]
fn a_compact_window_draws_only_the_placed_tiles() {
    // Below the breakpoint the preset drops the loudness bars; the registry must not
    // resurrect them just because a view exists.
    let calls = draw_calls(LayoutPreset::Stereo, area(640.0, 260.0), &registry());
    let order: Vec<TileId> = calls.iter().map(|(tile, _)| *tile).collect();
    assert_eq!(
        order,
        [TileId::Peak, TileId::Level, TileId::Readout, TileId::Scope]
    );
}

#[test]
fn presets_keep_their_tile_order() {
    let rect = area(900.0, 400.0);
    let order = |preset| -> Vec<TileId> {
        arrange(preset, rect)
            .placements()
            .iter()
            .map(|p| p.tile)
            .collect()
    };
    assert_eq!(
        order(LayoutPreset::Standard),
        [
            TileId::Peak,
            TileId::Level,
            TileId::Loudness,
            TileId::Readout,
            TileId::Scope
        ]
    );
    assert_eq!(
        order(LayoutPreset::Stereo),
        [
            TileId::Peak,
            TileId::Level,
            TileId::Scope,
            TileId::Loudness,
            TileId::Readout
        ]
    );

    let compact = area(820.0, 240.0);
    let ids: Vec<TileId> = arrange(LayoutPreset::Standard, compact)
        .placements()
        .iter()
        .map(|p| p.tile)
        .collect();
    assert_eq!(
        ids,
        [
            TileId::Peak,
            TileId::Level,
            TileId::Loudness,
            TileId::Readout
        ]
    );
}

#[test]
fn the_history_preset_draws_its_graph_after_the_meters() {
    let rect = area(900.0, 400.0);
    let calls = draw_calls(LayoutPreset::History, rect, &registry());
    let order: Vec<TileId> = calls.iter().map(|(tile, _)| *tile).collect();
    // The graph is the last tile of the set, so it paints over nothing else.
    assert_eq!(
        order,
        [
            TileId::Peak,
            TileId::Level,
            TileId::Loudness,
            TileId::Readout,
            TileId::History
        ]
    );
    let arrangement = arrange(LayoutPreset::History, rect);
    for (tile, drawn) in calls {
        assert_eq!(Some(drawn), arrangement.tile(tile), "{tile:?}");
    }

    // A short window drops the graph with the preset, not just with the view.
    let calls = draw_calls(LayoutPreset::History, area(640.0, 300.0), &registry());
    let order: Vec<TileId> = calls.iter().map(|(tile, _)| *tile).collect();
    assert_eq!(
        order,
        [
            TileId::Peak,
            TileId::Level,
            TileId::Loudness,
            TileId::Readout
        ]
    );
}

#[test]
fn the_spectrum_preset_draws_its_tile_after_the_meters() {
    let rect = area(900.0, 500.0);
    let calls = draw_calls(LayoutPreset::Spectrum, rect, &registry());
    let order: Vec<TileId> = calls.iter().map(|(tile, _)| *tile).collect();
    assert_eq!(
        order,
        [
            TileId::Peak,
            TileId::Level,
            TileId::Loudness,
            TileId::Readout,
            TileId::Spectrum
        ]
    );
    let arrangement = arrange(LayoutPreset::Spectrum, rect);
    for (tile, drawn) in calls {
        assert_eq!(Some(drawn), arrangement.tile(tile), "{tile:?}");
    }

    // A short window drops the spectrum with the preset, not just with the view.
    let calls = draw_calls(LayoutPreset::Spectrum, area(640.0, 300.0), &registry());
    let order: Vec<TileId> = calls.iter().map(|(tile, _)| *tile).collect();
    assert_eq!(
        order,
        [
            TileId::Peak,
            TileId::Level,
            TileId::Loudness,
            TileId::Readout
        ]
    );
}
