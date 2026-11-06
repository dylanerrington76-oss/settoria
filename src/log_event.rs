//! Structured entries for the game log.

use crate::board::Res;
use crate::game::ClockKind;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LogEvent {
    // ---- game and setup ----------------------------------------------------
    GameStarted,
    /// Only logged for maps other than the standard one.
    MapChosen { map: String, name: String },
    /// Human-readable labels for every rule that differs from the standard (see `Settings::changes`).
    CustomRules { changes: Vec<String> },
    /// Seat ids in turn order.
    TurnOrder { order: Vec<usize> },
    /// With many starting settlements the board can fill up before everyone has placed theirs.
    SetupEndedEarly,
    SetupComplete,

    // ---- dice, production, the robber ------------------------------------------
    Rolled { player: usize, dice: (u8, u8) },
    /// What `player` received from the bank for a roll.
    Collected { player: usize, cards: Res },
    /// The bank could not cover `resource` and more than one player was owed it.
    BankShort { resource: usize },
    /// A 7 was rolled and at least one player holds more than `limit` cards.
    MustDiscard { limit: u8 },
    Discarded { player: usize, count: u32 },
    /// A 7 was rolled (or a Knight played) and the robber has to move.
    RobberMustMove,
    RobberMoved { player: usize },
    Stole { player: usize, victim: usize },

    // ---- building and development cards -------------------------------------------
    /// A free setup placement (as opposed to `BuiltSettlement`, which is paid for).
    PlacedSettlement { player: usize },
    BuiltSettlement { player: usize },
    BuiltRoad { player: usize },
    BuiltCity { player: usize },
    BoughtDevCard { player: usize },
    PlayedKnight { player: usize },
    PlayedRoadBuilding { player: usize },
    PlayedYearOfPlenty { player: usize, first: usize, second: usize },
    /// `taken` is the number of cards the player collected from the others.
    PlayedMonopoly { player: usize, resource: usize, taken: u32 },

    // ---- trading --------------------------------------------------------------------
    /// `player` handed the bank `ratio` of `give` for one `get`.
    BankTrade { player: usize, give: usize, get: usize, ratio: u8 },
    TradeProposed { player: usize },
    TradeCompleted { player: usize, with: usize },

    // ---- awards and the end ---------------------------------------------------------------
    LongestRoadTaken { player: usize, roads: usize },
    LongestRoadLost,
    LargestArmyTaken { player: usize, knights: u8 },
    Won { player: usize, points: usize },

    // ---- timers -----------------------------------------------------------------------------
    /// A countdown ran out and the server acted for the player. `player` owns the clock;
    /// for `ClockKind::Discard` that is whoever's turn it is, not necessarily who had to discard.
    TimedOut { player: usize, kind: ClockKind },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn wire(e: &LogEvent) -> serde_json::Value {
        serde_json::to_value(e).unwrap()
    }

    #[test]
    fn events_are_tagged_objects() {
        assert_eq!(wire(&LogEvent::GameStarted), json!({ "type": "game_started" }));
        assert_eq!(
            wire(&LogEvent::Rolled { player: 2, dice: (3, 4) }),
            json!({ "type": "rolled", "player": 2, "dice": [3, 4] })
        );
        assert_eq!(
            wire(&LogEvent::Collected { player: 0, cards: [1, 0, 2, 0, 0] }),
            json!({ "type": "collected", "player": 0, "cards": [1, 0, 2, 0, 0] })
        );
        assert_eq!(
            wire(&LogEvent::MapChosen { map: "earth".into(), name: "Earth".into() }),
            json!({ "type": "map_chosen", "map": "earth", "name": "Earth" })
        );
        assert_eq!(
            wire(&LogEvent::TimedOut { player: 1, kind: ClockKind::SetupRoad }),
            json!({ "type": "timed_out", "player": 1, "kind": "setup_road" })
        );
        assert_eq!(
            wire(&LogEvent::BankTrade { player: 1, give: 0, get: 2, ratio: 3 }),
            json!({ "type": "bank_trade", "player": 1, "give": 0, "get": 2, "ratio": 3 })
        );
    }
}