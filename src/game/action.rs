//! What a player can ask for (`Action`), and `Game::apply`, the single place every request is checked.

use crate::types::{PlayerId, Res, Resource};
use serde::Deserialize;
use std::time::{Duration, Instant};
use super::{Game, Phase};

#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Roll,
    BuildRoad { edge: usize },
    /// A road over water (see `Settings::bridges`): the edge must be a bridge site.
    BuildBridge { edge: usize },
    BuildSettlement { vertex: usize },
    BuildCity { vertex: usize },
    BuyDev,
    PlayKnight,
    PlayRoadBuilding,
    PlayYearOfPlenty { a: Resource, b: Resource },
    PlayMonopoly { resource: Resource },
    /// An Alchemist, played at the start of the turn instead of rolling: the player names what the two dice
    /// show (each 1 to 6) and the roll is carried out with them.
    PlayAlchemist { dice: (u8, u8) },
    MoveRobber { hex: usize },
    Steal { victim: PlayerId },
    Discard { cards: Res },
    /// One free resource from an erupting volcano, by the player whose pick it is.
    VolcanoPick { resource: Resource },
    BankTrade { give: Resource, get: Resource },
    OfferTrade { give: Res, want: Res },
    RespondTrade { id: u32, accept: bool },
    ConfirmTrade { id: u32, with: PlayerId },
    /// Withdraw one of your open offers.
    CancelTrade { id: u32 },
    EndTurn,
}

impl Game {
    pub fn apply(&mut self, p: PlayerId, act: Action) -> Result<(), String> {
        self.apply_at(p, act, Instant::now())
    }

    /// `apply` with an explicit clock reading (lets tests drive time).
    pub fn apply_at(&mut self, p: PlayerId, act: Action, now: Instant) -> Result<(), String> {
        self.bonus = Duration::ZERO;
        if !self.has_player(p) {
            return Err("Unknown player".into());
        }
        if self.phase == Phase::GameOver {
            return Err("The game is over".into());
        }
        let res = match act {
            Action::Discard { cards } => self.do_discard(p, cards),
            Action::VolcanoPick { resource } => self.do_volcano_pick(p, resource),
            Action::RespondTrade { id, accept } => self.do_respond_trade(p, id, accept),
            other if self.settings.speed => self.do_speed(p, other),
            other => {
                if p != self.current {
                    return Err("It's not your turn".into());
                }
                self.do_current(p, other)
            }
        };
        if res.is_ok() {
            self.check_win_for(Some(p));
            self.prune_trades();
            self.sync_clock(now);
            self.apply_bonus(now);
        }
        res
    }

    /// Speed mode: who may do what. While the table is open (`Phase::Main`) anybody may build, trade and play
    /// development cards at any moment; the server handles requests one at a time, so when two players ask for the
    /// same spot, the first request processed gets it and the second is refused like any other illegal move.
    /// Every other phase (setup, moving the robber, choosing a victim) belongs to `current` alone. The dice are
    /// the server's (`Game::tick`) and there is no turn to end.
    fn do_speed(&mut self, p: PlayerId, act: Action) -> Result<(), String> {
        if self.phase != Phase::Main && p != self.current {
            return Err("It's not your turn".into());
        }
        match act {
            Action::Roll => Err("In Speed mode the server rolls the dice".into()),
            Action::EndTurn => Err("There are no turns in Speed mode".into()),
            Action::MoveRobber { .. } | Action::Steal { .. } if p != self.current => Err("It's not your turn to place the robber".into()),
            other => self.do_current(p, other),
        }
    }

    fn do_current(&mut self, p: PlayerId, act: Action) -> Result<(), String> {
        match act {
            Action::Roll => self.do_roll(),
            Action::BuildRoad { edge } => self.do_build_road(p, edge),
            Action::BuildBridge { edge } => self.do_build_bridge(p, edge),
            Action::BuildSettlement { vertex } => self.do_build_settlement(p, vertex),
            Action::BuildCity { vertex } => self.do_build_city(p, vertex),
            Action::BuyDev => self.do_buy_dev(p),
            Action::PlayKnight => self.do_knight(p),
            Action::PlayRoadBuilding => self.do_road_building_card(p),
            Action::PlayYearOfPlenty { a, b } => self.do_year_of_plenty(p, a, b),
            Action::PlayMonopoly { resource } => self.do_monopoly(p, resource),
            Action::PlayAlchemist { dice } => self.do_alchemist(p, dice),
            Action::MoveRobber { hex } => self.do_move_robber(p, hex),
            Action::Steal { victim } => self.do_steal(victim),
            Action::BankTrade { give, get } => self.do_bank_trade(p, give, get),
            Action::OfferTrade { give, want } => self.do_offer_trade(p, give, want),
            Action::ConfirmTrade { id, with } => self.do_confirm_trade(p, id, with),
            Action::CancelTrade { id } => self.do_cancel_trade(p, id),
            Action::EndTurn => self.do_end_turn(),
            Action::Discard { .. } | Action::VolcanoPick { .. } | Action::RespondTrade { .. } => unreachable!(),
        }
    }
}
