//! Geometry of the built-in layout presets.
//!
//! The Standard preset must reproduce the arrangement that shipped before presets existed, so
//! these tests pin its section widths, its right-hand column and the point at which a short
//! window drops the scope. The Stereo preset trades the loudness bars for a larger scope and
//! must still keep the numeric loudness read-out intact in a compact window.

use shirabe_ui::egui::{pos2, vec2, Rect};
use shirabe_ui::layout::{
    arrange, LayoutPreset, TileId, COLUMN_WIDTH, MIN_READOUT_HEIGHT, SECTION_GAP,
};

fn area(width: f32, height: f32) -> Rect {
    Rect::from_min_size(pos2(0.0, 0.0), vec2(width, height))
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

/// Every tile lies inside `rect` and no two tiles overlap.
fn assert_tiles_fit(preset: LayoutPreset, rect: Rect) {
    let arrangement = arrange(preset, rect);
    for placement in arrangement.placements() {
        let r = placement.rect;
        assert!(
            r.left() >= rect.left() - 0.01
                && r.top() >= rect.top() - 0.01
                && r.right() <= rect.right() + 0.01
                && r.bottom() <= rect.bottom() + 0.01,
            "{:?} {:?} outside {:?}",
            preset,
            placement.tile,
            rect
        );
    }
    let placements = arrangement.placements();
    for (i, a) in placements.iter().enumerate() {
        for b in &placements[i + 1..] {
            let overlap = a.rect.intersect(b.rect);
            assert!(
                overlap.width() <= 0.01 || overlap.height() <= 0.01,
                "{:?} overlaps {:?} in {:?}",
                a.tile,
                b.tile,
                rect
            );
        }
    }
}

#[test]
fn standard_reproduces_the_existing_arrangement() {
    let rect = area(900.0, 400.0);
    let arrangement = arrange(LayoutPreset::Standard, rect);
    let meters = rect.width() - COLUMN_WIDTH - 3.0 * SECTION_GAP;

    let peak = arrangement.tile(TileId::Peak).expect("peak section");
    assert!(close(peak.width(), meters * 0.38));
    assert!(close(peak.left(), 0.0));
    assert!(close(peak.height(), rect.height()));

    let level = arrangement.tile(TileId::Level).expect("level section");
    assert!(close(level.width(), meters * 0.37));
    assert!(close(level.left(), peak.right() + SECTION_GAP));

    let loudness = arrangement.tile(TileId::Loudness).expect("loudness bars");
    assert!(close(loudness.left(), level.right() + SECTION_GAP));
    assert!(close(
        loudness.right(),
        rect.right() - COLUMN_WIDTH - SECTION_GAP
    ));

    let readout = arrangement.tile(TileId::Readout).expect("read-out");
    assert!(close(readout.left(), rect.right() - COLUMN_WIDTH));
    assert!(close(readout.width(), COLUMN_WIDTH));
    assert!(close(readout.height(), 238.0));

    let scope = arrangement.tile(TileId::Scope).expect("scope");
    assert!(close(scope.top(), 248.0));
    assert!(close(scope.bottom(), 400.0));
    assert!(close(scope.width(), COLUMN_WIDTH));
}

#[test]
fn standard_hides_only_the_scope_in_a_short_window() {
    let rect = area(820.0, 240.0);
    let arrangement = arrange(LayoutPreset::Standard, rect);
    assert!(arrangement.tile(TileId::Scope).is_none());
    let readout = arrangement.tile(TileId::Readout).expect("read-out");
    // The read-out keeps the full height of the right-hand column.
    assert!(close(readout.left(), rect.right() - COLUMN_WIDTH));
    assert!(close(readout.right(), rect.right()));
    assert!(close(readout.top(), rect.top()));
    assert!(close(readout.bottom(), rect.bottom()));
    assert!(arrangement.tile(TileId::Peak).is_some());
    assert!(arrangement.tile(TileId::Level).is_some());
    assert!(arrangement.tile(TileId::Loudness).is_some());
    assert_tiles_fit(LayoutPreset::Standard, rect);
}

#[test]
fn stereo_gives_the_scope_the_largest_area() {
    let rect = area(900.0, 400.0);
    let arrangement = arrange(LayoutPreset::Stereo, rect);
    let scope = arrangement.tile(TileId::Scope).expect("stereo scope");
    let peak = arrangement.tile(TileId::Peak).expect("peak");
    let level = arrangement.tile(TileId::Level).expect("level");
    let loudness = arrangement.tile(TileId::Loudness).expect("loudness");
    let readout = arrangement.tile(TileId::Readout).expect("read-out");

    // Full-height scope in the middle, bars stacked on the left, loudness on the right.
    assert!(close(scope.height(), rect.height()));
    assert!(close(scope.top(), 0.0));
    assert!(close(peak.left(), 0.0));
    assert!(close(level.left(), 0.0));
    assert!(close(level.top(), peak.bottom() + SECTION_GAP));
    assert!(close(scope.left(), peak.right() + SECTION_GAP));
    assert!(close(readout.right(), rect.right()));
    assert!(close(readout.width(), COLUMN_WIDTH));
    assert!(close(loudness.left(), readout.left()));
    assert!(close(loudness.bottom(), readout.top() - SECTION_GAP));

    let scope_area = scope.width() * scope.height();
    assert!(scope_area > peak.width() * peak.height());
    assert!(scope_area > loudness.width() * loudness.height());

    let standard = arrange(LayoutPreset::Standard, rect)
        .tile(TileId::Scope)
        .expect("standard scope");
    assert!(scope.width() > standard.width() * 1.5);
    assert_tiles_fit(LayoutPreset::Stereo, rect);
}

#[test]
fn stereo_keeps_the_read_out_when_the_window_is_short() {
    // 640x260 is below the height that fits both the loudness bars and the read-out.
    let rect = area(640.0, 260.0);
    let arrangement = arrange(LayoutPreset::Stereo, rect);
    assert!(arrangement.tile(TileId::Loudness).is_none());
    let readout = arrangement.tile(TileId::Readout).expect("read-out");
    assert!(readout.height() >= MIN_READOUT_HEIGHT);
    assert_tiles_fit(LayoutPreset::Stereo, rect);

    // An extremely short viewport still places the read-out and drops the scope.
    let tiny = area(640.0, 80.0);
    let arrangement = arrange(LayoutPreset::Stereo, tiny);
    assert!(arrangement.tile(TileId::Scope).is_none());
    assert!(arrangement.tile(TileId::Readout).is_some());
    assert_tiles_fit(LayoutPreset::Stereo, tiny);
}

#[test]
fn both_presets_place_every_tile_at_the_minimum_window_size() {
    for preset in [LayoutPreset::Standard, LayoutPreset::Stereo] {
        for (w, h) in [(640.0, 300.0), (640.0, 380.0), (820.0, 500.0)] {
            assert_tiles_fit(preset, area(w, h));
        }
    }
}
