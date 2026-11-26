//! Tile geometry for the meter view.
//!
//! The view is a set of tiles; a preset only decides where each tile goes. Splitting the
//! geometry from the drawing keeps the arrangements testable without a window and lets a
//! later tile tree replace the two built-in presets without touching the meter widgets.
//!
//! The Standard preset is the arrangement used before presets existed, so its numbers are
//! pinned by tests: changing them moves meters under existing users.

use crate::egui::{pos2, Rect};
pub use crate::tile::{TileId, UnknownTile};

/// Horizontal gap between the bar sections.
pub const SECTION_GAP: f32 = 14.0;
/// Vertical gap between the loudness read-out and the scope.
pub const COLUMN_GAP: f32 = 10.0;
/// Width of the right-hand column holding the read-out and the scope.
pub const COLUMN_WIDTH: f32 = 180.0;
/// Smallest readable height of the scope.
pub const MIN_SCOPE_HEIGHT: f32 = 96.0;
/// Smallest readable width of the scope.
pub const MIN_SCOPE_WIDTH: f32 = 80.0;
/// The scope stops growing here so a tall window leaves the read-out room.
pub const MAX_SCOPE_HEIGHT: f32 = COLUMN_WIDTH;
/// Space for the heading, all seven loudness rows and the LRA stability note.
pub const MIN_READOUT_HEIGHT: f32 = 80.0 + 7.0 * 19.0 + 14.0;
/// Space for a loudness bar title, its read-outs, one usable bar and the caption.
pub const MIN_LOUDNESS_HEIGHT: f32 = 110.0;
/// Smallest readable height of the loudness history graph.
pub const MIN_HISTORY_HEIGHT: f32 = 96.0;
/// Smallest readable height of the spectrum tile.
pub const MIN_SPECTRUM_HEIGHT: f32 = 120.0;
/// Smallest readable height of the reference comparison: a heading and three rows.
pub const MIN_REFERENCE_HEIGHT: f32 = 96.0;
/// Share of the meter area the history graph takes when the window has room for it.
const HISTORY_GRAPH_SHARE: f32 = 0.34;
/// Largest height the history graph takes, so a tall window still shows the meters.
const MAX_HISTORY_HEIGHT: f32 = 220.0;
/// Share of the meter area the spectrum tile takes when the window has room for it.
const SPECTRUM_SHARE: f32 = 0.38;
/// Largest height the spectrum tile takes.
const MAX_SPECTRUM_HEIGHT: f32 = 260.0;

/// A built-in arrangement.
///
/// The stored parameter carries its own enum (with nih-plug IDs); this one is the geometry
/// side and has no plug-in dependency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutPreset {
    /// Peak, level and loudness bars side by side, read-out above the scope on the right.
    Standard,
    /// Peak and level stacked on the left, a full-height scope in the middle and the
    /// loudness bars above the read-out on the right.
    Stereo,
    /// The Standard meters with a full-width loudness history graph along the bottom.
    History,
    /// The Standard meters with a full-width FFT / 1/3-octave spectrum along the bottom.
    Spectrum,
    /// The Standard meters with the reference comparison where the scope sits.
    Compare,
}

/// One tile with the rectangle it occupies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The tile.
    pub tile: TileId,
    /// Where it is drawn.
    pub rect: Rect,
}

/// The tiles of one preset, in drawing order.
#[derive(Clone, Debug, Default)]
pub struct Arrangement {
    placements: Vec<Placement>,
}

impl Arrangement {
    fn place(&mut self, tile: TileId, rect: Rect) {
        self.placements.push(Placement { tile, rect });
    }

    /// Drops `tile` from the arrangement, if it is placed.
    fn remove(&mut self, tile: TileId) {
        self.placements.retain(|placement| placement.tile != tile);
    }

    /// All placed tiles.
    pub fn placements(&self) -> &[Placement] {
        &self.placements
    }

    /// Rectangle of `tile`, if this arrangement shows it.
    pub fn tile(&self, tile: TileId) -> Option<Rect> {
        self.placements
            .iter()
            .find(|p| p.tile == tile)
            .map(|p| p.rect)
    }
}

/// Arranges `preset` inside `rect`.
pub fn arrange(preset: LayoutPreset, rect: Rect) -> Arrangement {
    match preset {
        LayoutPreset::Standard => standard(rect),
        LayoutPreset::Stereo => stereo(rect),
        LayoutPreset::History => history(rect),
        LayoutPreset::Spectrum => spectrum(rect),
        LayoutPreset::Compare => compare(rect),
    }
}

/// Peak, level and loudness sections in a row, with the read-out above the scope.
fn standard(rect: Rect) -> Arrangement {
    let meters_width = (rect.width() - COLUMN_WIDTH - 3.0 * SECTION_GAP).max(0.0);
    let peak_width = meters_width * 0.38;
    let level_width = meters_width * 0.37;
    let lufs_width = meters_width - peak_width - level_width;

    let mut x = rect.left();
    let mut take = |w: f32| {
        let r = Rect::from_min_max(pos2(x, rect.top()), pos2(x + w, rect.bottom()));
        x += w + SECTION_GAP;
        r
    };
    let peak = take(peak_width);
    let level = take(level_width);
    let lufs = take(lufs_width);
    let column = take(COLUMN_WIDTH);

    let mut arrangement = Arrangement::default();
    arrangement.place(TileId::Peak, peak);
    arrangement.place(TileId::Level, level);
    arrangement.place(TileId::Loudness, lufs);
    let (readout, scope) = split_column(column);
    arrangement.place(TileId::Readout, readout);
    if let Some(scope) = scope {
        arrangement.place(TileId::Scope, scope);
    }
    arrangement
}

/// Splits the right-hand column into the loudness read-out (top) and the scope (bottom).
///
/// A column too short for both shows only the read-out.
pub fn split_column(rect: Rect) -> (Rect, Option<Rect>) {
    let height = rect.height();
    if height < MIN_READOUT_HEIGHT + MIN_SCOPE_HEIGHT + COLUMN_GAP {
        return (rect, None);
    }
    let scope_height = (height * 0.38)
        .clamp(MIN_SCOPE_HEIGHT, MAX_SCOPE_HEIGHT)
        .min(height - MIN_READOUT_HEIGHT - COLUMN_GAP);
    let scope_top = rect.bottom() - scope_height;
    let readout = Rect::from_min_max(rect.min, pos2(rect.right(), scope_top - COLUMN_GAP));
    let scope = Rect::from_min_max(pos2(rect.left(), scope_top), rect.max);
    (readout, Some(scope))
}

/// Peak and level stacked on the left, a full-height scope in the middle and the loudness
/// bars above the read-out on the right.
///
/// The read-out keeps its minimum height; the loudness bars are dropped first when the
/// window is too short for both, so the integrated value never loses rows.
fn stereo(rect: Rect) -> Arrangement {
    let inner = (rect.width() - COLUMN_WIDTH - 2.0 * SECTION_GAP).max(0.0);
    let left_width = (inner * 0.40).clamp(120.0, 280.0).min(inner);
    let scope_width = (inner - left_width).max(0.0);

    let left = Rect::from_min_max(rect.min, pos2(rect.left() + left_width, rect.bottom()));
    let scope = Rect::from_min_max(
        pos2(left.right() + SECTION_GAP, rect.top()),
        pos2(left.right() + SECTION_GAP + scope_width, rect.bottom()),
    );
    let column = Rect::from_min_max(pos2(scope.right() + SECTION_GAP, rect.top()), rect.max);

    let mut arrangement = Arrangement::default();

    // Split the left column down the middle; the level section keeps the remainder so a
    // fractional height never leaves a one-pixel strip.
    let half = ((left.height() - SECTION_GAP) * 0.5).max(0.0);
    let peak = Rect::from_min_max(left.min, pos2(left.right(), left.top() + half));
    let level = Rect::from_min_max(pos2(left.left(), peak.bottom() + SECTION_GAP), left.max);
    arrangement.place(TileId::Peak, peak);
    arrangement.place(TileId::Level, level);

    if scope_width >= MIN_SCOPE_WIDTH && scope.height() >= MIN_SCOPE_HEIGHT {
        arrangement.place(TileId::Scope, scope);
    }

    if column.height() >= MIN_READOUT_HEIGHT + SECTION_GAP + MIN_LOUDNESS_HEIGHT {
        let loudness_height = column.height() - MIN_READOUT_HEIGHT - SECTION_GAP;
        let loudness = Rect::from_min_max(
            column.min,
            pos2(column.right(), column.top() + loudness_height),
        );
        let readout = Rect::from_min_max(
            pos2(column.left(), loudness.bottom() + SECTION_GAP),
            column.max,
        );
        arrangement.place(TileId::Loudness, loudness);
        arrangement.place(TileId::Readout, readout);
    } else {
        arrangement.place(TileId::Readout, column);
    }
    arrangement
}

/// The Standard meters with the reference comparison in the scope's place.
///
/// The meter columns and the read-out keep the Standard geometry, so the numbers do not move
/// when the view is selected. A column too short for both keeps the read-out and drops the
/// comparison, exactly like the scope in the Standard preset.
fn compare(rect: Rect) -> Arrangement {
    let mut arrangement = standard(rect);
    let Some(scope) = arrangement.tile(TileId::Scope) else {
        return arrangement;
    };
    if scope.height() < MIN_REFERENCE_HEIGHT {
        return arrangement;
    }
    arrangement.remove(TileId::Scope);
    arrangement.place(TileId::Reference, scope);
    arrangement
}

/// The Standard meters with a full-width spectrum along the bottom.
///
/// The meter columns and the read-out keep the Standard geometry, so switching to the view does
/// not move the numbers. The spectrum is dropped before the read-out in a short window.
fn spectrum(rect: Rect) -> Arrangement {
    let needed = MIN_READOUT_HEIGHT + SECTION_GAP + MIN_SPECTRUM_HEIGHT;
    if rect.height() < needed {
        return standard(rect);
    }
    let spectrum_height = (rect.height() * SPECTRUM_SHARE)
        .clamp(MIN_SPECTRUM_HEIGHT, MAX_SPECTRUM_HEIGHT)
        .min(rect.height() - MIN_READOUT_HEIGHT - SECTION_GAP);
    let meters = Rect::from_min_max(
        rect.min,
        pos2(rect.right(), rect.bottom() - spectrum_height - SECTION_GAP),
    );
    let mut arrangement = standard(meters);
    arrangement.place(
        TileId::Spectrum,
        Rect::from_min_max(pos2(rect.left(), meters.bottom() + SECTION_GAP), rect.max),
    );
    arrangement
}

/// The Standard meters with a full-width loudness history graph along the bottom.
/// The meters and the read-out keep the Standard geometry, so the numbers do not move when
/// the view is selected. The graph is dropped before the read-out: a window too short for
/// both shows the Standard meters without the scope, and the numeric loudness keeps every
/// row. A tall window still has room for the scope above the graph.
fn history(rect: Rect) -> Arrangement {
    let needed = MIN_READOUT_HEIGHT + SECTION_GAP + MIN_HISTORY_HEIGHT;
    if rect.height() < needed {
        return standard(rect);
    }
    let graph_height = (rect.height() * HISTORY_GRAPH_SHARE)
        .clamp(MIN_HISTORY_HEIGHT, MAX_HISTORY_HEIGHT)
        .min(rect.height() - MIN_READOUT_HEIGHT - SECTION_GAP);
    let meters = Rect::from_min_max(
        rect.min,
        pos2(rect.right(), rect.bottom() - graph_height - SECTION_GAP),
    );
    let mut arrangement = standard(meters);
    arrangement.place(
        TileId::History,
        Rect::from_min_max(pos2(rect.left(), meters.bottom() + SECTION_GAP), rect.max),
    );
    arrangement
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_without_room_keeps_the_read_out_only() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(COLUMN_WIDTH, 240.0));
        let (readout, scope) = split_column(rect);
        assert!(scope.is_none());
        assert_eq!(readout, rect);
    }

    #[test]
    fn the_scope_never_takes_height_needed_for_the_loudness_rows() {
        for height in 296..400 {
            let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(COLUMN_WIDTH, height as f32));
            let (readout, scope) = split_column(rect);
            if let Some(scope) = scope {
                assert!(
                    readout.height() >= MIN_READOUT_HEIGHT,
                    "read-out clipped at {height}"
                );
                assert!(scope.height() >= MIN_SCOPE_HEIGHT);
            }
        }
        assert!(split_column(Rect::from_min_max(
            pos2(0.0, 0.0),
            pos2(COLUMN_WIDTH, 333.0)
        ))
        .1
        .is_some());
    }
}
