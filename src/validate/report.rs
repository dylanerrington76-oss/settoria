//! What a validation produces: the warnings, the facts about the map, and the `Report` that holds them
//! together with the errors.

use crate::board::Terrain;
use crate::map_def::MapError;
use std::collections::BTreeMap;
use std::fmt;

/// The most errors (and the most warnings) a report holds, so a hopeless file can't make a huge report.
pub const MAX_ISSUES: usize = 100;

/// Worth knowing about, but the map can still be played.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapWarning {
    /// Setup would fit if everyone placed perfectly, but not with room to spare, so people placing
    /// carelessly could run out of corners before everyone has their settlements.
    TightSettlementRoom { needed: usize, available: usize },
    /// No hex of this terrain exists, so the resource is only available by trading with the bank.
    MissingResource { terrain: Terrain },
    /// One resource is far scarcer than another.
    LopsidedResources { scarce: Terrain, scarce_hexes: usize, plentiful: Terrain, plentiful_hexes: usize },
    /// A whole island of nothing but desert, near this hex.
    IslandWithoutResources { near: (i32, i32), hexes: usize },
    NoHarbours,
    /// Two harbours meet at a corner, near this hex (so one settlement there uses both, and their
    /// pictures overlap).
    HarboursShareCorner { near: (i32, i32) },
    /// Some trial deals of the number tokens failed: starting a game on this map sometimes fails and
    /// has to be tried again.
    DealOftenFails { failed: usize, trials: usize },
}

impl fmt::Display for MapWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use MapWarning::*;
        match self {
            TightSettlementRoom { needed, available } => write!(
                f,
                "room for {available} settlements, and setup needs {needed}: it fits if everyone places well, but careless placement could run out of room"
            ),
            MissingResource { terrain } => write!(f, "no {} hexes, so that resource can only be had by trading with the bank", terrain.name()),
            LopsidedResources { scarce, scarce_hexes, plentiful, plentiful_hexes } => write!(
                f,
                "{} is scarce ({scarce_hexes} hexes) next to {} ({plentiful_hexes} hexes)",
                scarce.name(),
                plentiful.name()
            ),
            IslandWithoutResources { near, hexes } => write!(f, "the island near ({}, {}) has {hexes} hexes and all of them are desert", near.0, near.1),
            NoHarbours => write!(f, "the map has no harbours, so players can only trade 4:1 with the bank"),
            HarboursShareCorner { near } => write!(f, "two harbours meet at a corner near hex ({}, {})", near.0, near.1),
            DealOftenFails { failed, trials } => write!(
                f,
                "the number tokens could not be spread out in {failed} of {trials} trial deals, so starting a game here will sometimes fail"
            ),
        }
    }
}

/// Facts about the map, for a summary line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapStats {
    pub hexes: usize,
    pub islands: usize,
    /// Hexes that are not deserts.
    pub producing_hexes: usize,
    pub harbours: usize,
    /// The most settlements that fit under the distance rule.
    pub settlement_room: usize,
    /// How many hexes of each terrain the map ends up with (what is written in, plus the terrain pool).
    pub terrain: BTreeMap<Terrain, usize>,
}

/// Everything `validate` found. The map is playable if `errors` is empty.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Report {
    pub errors: Vec<MapError>,
    pub warnings: Vec<MapWarning>,
    /// Only there when the hex list made sense.
    pub stats: Option<MapStats>,
    /// There were more problems than `MAX_ISSUES`; the rest were dropped.
    pub truncated: bool,
}

impl Report {
    /// Gather what the layers found, keeping at most `MAX_ISSUES` errors and as many warnings.
    pub(super) fn new(mut errors: Vec<MapError>, mut warnings: Vec<MapWarning>, stats: Option<MapStats>) -> Report {
        let truncated = errors.len() > MAX_ISSUES || warnings.len() > MAX_ISSUES;
        errors.truncate(MAX_ISSUES);
        warnings.truncate(MAX_ISSUES);
        Report { errors, warnings, stats, truncated }
    }

    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}
