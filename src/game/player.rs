//! The people at the table: the colours they may choose, their hand and pieces, and their running totals.

use crate::types::{DevHand, Res};
use serde::Serialize;

/// Colours a player may choose when joining, as `#RRGGBB` (names live in the client).
pub const PALETTE: [&str; 12] = [
    "#FE5100", // orange
    "#613393", // purple
    "#FFCD00", // gold
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
    /// Robber+: cards this player took from others with the robber (a 7 or a Knight). Each steal is one card.
    pub robber_gained: u32,
    /// Robber-: cards other players took from this player with the robber.
    pub robber_lost: u32,
    /// Dice-: cards this player discarded after a 7 because they held more than the discard limit.
    pub dice_discarded: u32,
    /// Blocked: cards this player would have received from a roll but didn't, because the robber was on the hex
    /// (a settlement counts one card, a city two; a blocked volcano or gold field counts the picks it would have
    /// owed). Bank shortages are not counted here.
    pub blocked: u32,
    /// Luck, in 36ths of a card: the dice income of the rolls that count (see `Game::luck_counts`) minus the
    /// income their buildings were expected to bring in, where a hex that is producing with number `n` is expected
    /// to pay `ways(n) / 36` of a card per settlement (two per city) on any one roll. Every roll adds its
    /// expectation, a 7 included, so a 7 is bad luck for everyone who was due something. Hexes under the robber
    /// are left out of the expectation (their loss is `blocked`). Whole 36ths keep the stats exact and `Eq`;
    /// they are sent to the client as cards, see `luck_in_cards`.
    #[serde(rename = "luck", serialize_with = "luck_in_cards")]
    pub luck_36ths: i32,
}

/// `PlayerStats::luck_36ths` as it goes over the wire: cards, to two decimals.
fn luck_in_cards<S: serde::Serializer>(v: &i32, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64((f64::from(*v) / 36.0 * 100.0).round() / 100.0)
}
