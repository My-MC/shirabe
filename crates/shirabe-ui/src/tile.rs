//! Stable identifiers of the meter tiles.
//!
//! The set is closed on purpose: the editor matches over it exhaustively, so adding a tile
//! stops compiling until it has a view. A saved layout file will name tiles by
//! [`TileId::as_str`], so a name must never be renamed or reused.

use std::fmt;
use std::str::FromStr;

/// Identifier of one tile of the meter view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TileId {
    /// Sample peak and true-peak bars.
    Peak,
    /// VU or PPM bars (or needles).
    Level,
    /// Momentary and short-term loudness bars.
    Loudness,
    /// Numeric loudness read-out.
    Readout,
    /// Lissajous or L/R vectorscope view.
    Scope,
}

impl TileId {
    /// Every tile, in the order the editor draws them.
    pub const ALL: [Self; 5] = [
        Self::Peak,
        Self::Level,
        Self::Loudness,
        Self::Readout,
        Self::Scope,
    ];

    /// Name used in a saved layout; stable forever.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Peak => "peak",
            Self::Level => "level",
            Self::Loudness => "loudness",
            Self::Readout => "readout",
            Self::Scope => "scope",
        }
    }
}

impl fmt::Display for TileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A saved tile name that this build does not know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownTile;

impl fmt::Display for UnknownTile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unknown tile id")
    }
}

impl std::error::Error for UnknownTile {}

impl FromStr for TileId {
    type Err = UnknownTile;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|tile| tile.as_str() == name)
            .ok_or(UnknownTile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_are_case_sensitive() {
        for tile in TileId::ALL {
            assert_eq!(tile.as_str().parse(), Ok(tile));
        }
        assert_eq!("Peak".parse::<TileId>(), Err(UnknownTile));
    }
}
