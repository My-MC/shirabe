//! Registration table of the meter views drawn in one editor frame.
//!
//! The layout decides which tiles exist and where they go; a view only draws into the
//! rectangle it is given. Keeping the views in one table puts the drawing order in a single
//! place and lets tests prove that every tile of every preset resolves to exactly one view.
//!
//! The table is built per frame and borrows the editor state, so the context type may hold
//! borrowed data. It is a plain slice: no allocation and no virtual dispatch, only one
//! function pointer per tile.

use std::fmt;

use crate::egui::{Painter, Rect};
use crate::layout::{Arrangement, TileId};

/// One tile and the function that draws it into a rectangle.
pub struct MeterView<C> {
    id: TileId,
    draw: fn(&mut C, &Painter, Rect),
}

impl<C> MeterView<C> {
    /// Creates the view of `id`; `draw` receives the editor state and the tile rectangle.
    pub const fn new(id: TileId, draw: fn(&mut C, &Painter, Rect)) -> Self {
        Self { id, draw }
    }

    /// The tile this view draws.
    pub const fn id(&self) -> TileId {
        self.id
    }

    /// Draws the tile into `rect`.
    pub fn draw(&self, ctx: &mut C, painter: &Painter, rect: Rect) {
        (self.draw)(ctx, painter, rect);
    }
}

/// Why a view table was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryError {
    /// The same tile has more than one view.
    DuplicateTile(TileId),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateTile(tile) => write!(f, "tile {tile} is registered twice"),
        }
    }
}

impl std::error::Error for RegistryError {}

/// The views of one editor draw, in drawing order.
pub struct MeterViewRegistry<'a, C> {
    views: &'a [MeterView<C>],
}

impl<'a, C> MeterViewRegistry<'a, C> {
    /// Registers `views`, rejecting a tile that appears twice.
    pub fn new(views: &'a [MeterView<C>]) -> Result<Self, RegistryError> {
        for (index, view) in views.iter().enumerate() {
            if views[..index].iter().any(|other| other.id == view.id) {
                return Err(RegistryError::DuplicateTile(view.id));
            }
        }
        Ok(Self { views })
    }

    /// The registered views, in drawing order.
    pub fn views(&self) -> &'a [MeterView<C>] {
        self.views
    }

    /// The view of `id`, if this table has one.
    pub fn get(&self, id: TileId) -> Option<&'a MeterView<C>> {
        self.views.iter().find(|view| view.id == id)
    }

    /// Tiles placed by `arrangement` that this table cannot draw.
    pub fn missing<'b>(
        &'b self,
        arrangement: &'b Arrangement,
    ) -> impl Iterator<Item = TileId> + 'b {
        arrangement
            .placements()
            .iter()
            .map(|placement| placement.tile)
            .filter(move |id| self.get(*id).is_none())
    }

    /// Whether every tile placed by `arrangement` has a view.
    pub fn covers(&self, arrangement: &Arrangement) -> bool {
        self.missing(arrangement).next().is_none()
    }

    /// Draws every placed tile once, in table order.
    ///
    /// A tile without a view is skipped so a partially registered table still shows the
    /// rest; [`Self::missing`] reports it for tests and diagnostics.
    pub fn draw(&self, painter: &Painter, arrangement: &Arrangement, ctx: &mut C) {
        for view in self.views {
            if let Some(rect) = arrangement.tile(view.id) {
                view.draw(ctx, painter, rect);
            }
        }
    }
}
