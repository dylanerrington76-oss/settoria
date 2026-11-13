//! Structured entries for the game log.

use crate::game::ClockKind;
use crate::types::{PlayerId, Res, Resource};
use serde::Serialize;

/// What a lava flow did to the corner it ran down to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LavaEffect {
    /// Nobody had a building there.
    Nothing,
    /// A settlement was destroyed and went back to its owner's stock.
    Destroyed,
    /// A city was reduced to a settlement.
    Demoted,
}

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
    TurnOrder { order: Vec<PlayerId> },
    /// With many starting settlements the board can fill up before everyone has placed theirs.
    SetupEndedEarly,
    SetupComplete,

    // ---- dice, production, the robber ------------------------------------------
    Rolled { player: PlayerId, dice: (u8, u8) },
    /// What `player` received from the bank for a roll.
    Collected { player: PlayerId, cards: Res },
    /// The bank could not cover `resource` and more than one player was owed it.
    BankShort { resource: Resource },
    /// A 7 was rolled and at least one player holds more than `limit` cards.
    MustDiscard { limit: u8 },
    Discarded { player: PlayerId, count: u32 },
    /// A 7 was rolled (or a Knight played) and the robber has to move.
    RobberMustMove,
    RobberMoved { player: PlayerId },
    Stole { player: PlayerId, victim: PlayerId },

    // ---- volcanoes ---------------------------------------------------------------------
    /// `player` took a free card from an erupting volcano.
    VolcanoPicked { player: PlayerId, resource: Resource },
    /// A volcano's number came up while the robber sat on it: nobody picks, but the lava still flows.
    VolcanoBlocked { hex: usize },
    /// The bank ran out of cards, so the picks still owed at a volcano were dropped.
    VolcanoBankEmpty,
    /// The lava from the volcano on `hex` ran down corner `die` (1 to 6). `owner` is whose building was hit.
    LavaFlow { hex: usize, die: u8, effect: LavaEffect, owner: Option<PlayerId> },

    // ---- building and development cards -------------------------------------------
    /// A free setup placement (as opposed to `BuiltSettlement`, which is paid for).
    PlacedSettlement { player: PlayerId },
    BuiltSettlement { player: PlayerId },
    BuiltRoad { player: PlayerId },
    /// A road over water (see `Settings::bridges`).
    BuiltBridge { player: PlayerId },
    BuiltCity { player: PlayerId },
    BoughtDevCard { player: PlayerId },
    PlayedKnight { player: PlayerId },
    PlayedRoadBuilding { player: PlayerId },
    PlayedYearOfPlenty { player: PlayerId, first: Resource, second: Resource },
    /// `taken` is the number of cards the player collected from the others.
    PlayedMonopoly { player: PlayerId, resource: Resource, taken: u32 },

    // ---- trading --------------------------------------------------------------------
    /// `player` handed the bank `ratio` of `give` for one `get`.
    BankTrade { player: PlayerId, give: Resource, get: Resource, ratio: u8 },
    TradeProposed { player: PlayerId },
    TradeCompleted { player: PlayerId, with: PlayerId },

    // ---- awards and the end ---------------------------------------------------------------
    LongestRoadTaken { player: PlayerId, roads: usize },
    LongestRoadLost,
    LargestArmyTaken { player: PlayerId, knights: u8 },
    Won { player: PlayerId, points: usize },

    // ---- timers -----------------------------------------------------------------------------
    /// A countdown ran out and the server acted for the player. `player` owns the clock;
    /// for `ClockKind::Discard` that is whoever's turn it is, not necessarily who had to discard.
    TimedOut { player: PlayerId, kind: ClockKind },
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
            wire(&LogEvent::Rolled { player: PlayerId::new(2), dice: (3, 4) }),
            json!({ "type": "rolled", "player": 2, "dice": [3, 4] })
        );
        assert_eq!(
            wire(&LogEvent::Collected { player: PlayerId::new(0), cards: Res::new(1, 0, 2, 0, 0) }),
            json!({ "type": "collected", "player": 0, "cards": [1, 0, 2, 0, 0] })
        );
        assert_eq!(
            wire(&LogEvent::MapChosen { map: "earth".into(), name: "Earth".into() }),
            json!({ "type": "map_chosen", "map": "earth", "name": "Earth" })
        );
        assert_eq!(
            wire(&LogEvent::TimedOut { player: PlayerId::new(1), kind: ClockKind::SetupRoad }),
            json!({ "type": "timed_out", "player": 1, "kind": "setup_road" })
        );
        assert_eq!(
            wire(&LogEvent::LavaFlow { hex: 4, die: 6, effect: LavaEffect::Demoted, owner: Some(PlayerId::new(1)) }),
            json!({ "type": "lava_flow", "hex": 4, "die": 6, "effect": "demoted", "owner": 1 })
        );
        assert_eq!(
            wire(&LogEvent::VolcanoPicked { player: PlayerId::new(0), resource: Resource::Grain }),
            json!({ "type": "volcano_picked", "player": 0, "resource": 3 })
        );
        assert_eq!(
            wire(&LogEvent::BankTrade { player: PlayerId::new(1), give: Resource::Brick, get: Resource::Ore, ratio: 3 }),
            json!({ "type": "bank_trade", "player": 1, "give": 0, "get": 2, "ratio": 3 })
        );
    }
}