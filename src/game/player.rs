//! The people at the table: the colours they may choose, their hand and pieces, and their running totals.

use crate::types::{DevHand, Res};
use serde::Serialize;

/// Colours a player may choose when joining, as `#RRGGBB` (names live in the client).
pub const PALETTE: [&str; 12] = [
    "#FE5100", // orange
    "#613393", // purple
    "#FFB81C", // gold
    "#69BE28", // green
    "#0033A0", // blue
    "#CC0000", // red
    "#7BAFD4", // mystic blue
    "#FFFFFF", // white
    "#8A8D8F", // silver
    "#000000", // black
    "#500000", // maroon
    "#C8C372", // khaki
];

/// The palette entry matching `c` (case-insensitive), in its canonical spelling.
pub fn palette_color(c: &str) -> Option<&'static str> {
    PALETTE.iter().copied().find(|p| p.eq_ignore_ascii_case(c))
}

#[derive(Clone, Debug)]
pub struct Player {
    pub hand: Res,
    pub dev: DevHand,
    pub dev_new: DevHand,
    pub knights: u8,
    pub roads_left: u8,
    pub settlements_left: u8,
    pub cities_left: u8,
    /// Running totals for the end-of-game stats tab.
    pub stats: PlayerStats,
    /// Speed mode only (the classic game keeps these on `Game`, because only one player has a turn): whether this
    /// player has played a development card since the last roll, the cards of each resource they gave away in
    /// completed trades since then (two or more lock Monopoly on it, see `MONOPOLY_LOCK_CARDS`), and the free roads
    /// a Road Building card still owes them.
    pub dev_played: bool,
    pub traded_away: Res,
    pub free_roads: u8,
}

/// Per-player totals over the whole game, shown on the game-over screen's Stats tab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct PlayerStats {
    /// Resource cards received because of a dice roll: production for settlements and cities, plus the free
    /// cards picked at an erupting volcano.
    pub dice_income: u32,
    /// Development cards bought from the deck (played or not).
    pub dev_cards_bought: u32,
    /// Completed player-to-player trades (counted once for each of the two players; bank trades don't count).
    pub trades: u32,
}
