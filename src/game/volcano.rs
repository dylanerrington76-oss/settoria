//! Volcanoes and gold fields: the free picks when their number is rolled, and the lava that follows a volcano.
//!
//! Gold uses the same queue and the same `Phase::VolcanoPick` as a volcano; the only differences are that no lava
//! flows afterwards (see `Game::advance_volcano`) and that its log events are `GoldPicked` and `GoldBlocked`.

use crate::log_event::{LavaEffect, LogEvent};
use crate::types::{PlayerId, Resource};
use rand::Rng;
use serde::Serialize;
use super::{Building, Game, Phase};

/// One lava flow: the volcano, the die that said where it ran, and the corner it ran down to.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct Lava {
    pub hex: usize,
    /// 1 to 6: the volcano's corners are numbered clockwise from the upper right (corner `i` of
    /// `Hex::vertices` is `i + 1`).
    pub die: u8,
    pub vertex: usize,
}

impl Game {
    /// A non-7 was rolled: every volcano and every gold field carrying that number comes up, one after the
    /// other. For each, the players with a building on it pick their free resources, and then (for a volcano
    /// only) the lava flows. A volcano with no building on it doesn't erupt at all (no lava, nothing in the
    /// log), and a gold field with no building on it is skipped. If no such hex has the number, or none of
    /// them is built on, nothing happens and the phase stays `Main`.
    ///
    /// Gold uses the same queue and the same `Phase::VolcanoPick` as a volcano. The only differences are that no
    /// lava flows afterwards (see `advance_volcano`) and that its log events are `GoldPicked` and `GoldBlocked`.
    pub(super) fn start_volcanoes(&mut self, roll: u8) {
        self.volcano_queue = (0..self.board.hexes.len())
            .filter(|&h| self.board.hexes[h].terrain.gives_free_pick() && self.board.hexes[h].number == Some(roll))
            .collect();
        self.volcano_active = None;
        self.volcano_picks.clear();
        self.advance_volcano();
    }

    /// Whether any player has a settlement or city on a corner of the volcano `hex`.
    fn volcano_occupied(&self, hex: usize) -> bool {
        self.board.hexes[hex].vertices.iter().any(|&v| self.buildings[v].is_some())
    }

    /// Free picks a volcano owes: a settlement on it is one card, a city two, in turn order from the player
    /// whose turn it is. Nothing is owed while the robber sits on it (he stops production, not eruptions).
    fn volcano_owed(&self, hex: usize) -> Vec<(PlayerId, u8)> {
        if hex == self.robber {
            return vec![];
        }
        let start = self.order.iter().position(|&x| x == self.current).unwrap_or(0);
        (0..self.n)
            .map(|i| self.order[(start + i) % self.n])
            .filter_map(|p| {
                let cards: u8 = self.board.hexes[hex]
                    .vertices
                    .iter()
                    .filter_map(|&v| self.buildings[v])
                    .filter(|b| b.owner == p)
                    .map(|b| if b.city { 2 } else { 1 })
                    .sum();
                (cards > 0).then_some((p, cards))
            })
            .collect()
    }

    /// Move the eruption along: wait for the next pick, or let the active volcano's lava flow and start the
    /// next volcano, or go back to `Main` when there are no more. (While `setup_pick` is set it is the free
    /// cards for a last starting settlement instead: no lava, and it goes back to `Setup` at the end.)
    pub(super) fn advance_volcano(&mut self) {
        loop {
            if !self.volcano_picks.is_empty() && self.bank.total() == 0 {
                self.volcano_picks.clear();
                self.log(LogEvent::VolcanoBankEmpty);
            }
            if !self.volcano_picks.is_empty() {
                self.phase = Phase::VolcanoPick;
                return;
            }
            if let Some(hex) = self.volcano_active.take() {
                // (the volcanoes beside a starting settlement only give their card; they don't erupt. Neither
                // does gold: it gives its card and that is all)
                if !self.setup_pick && self.board.hexes[hex].terrain.is_volcano() {
                    self.erupt(hex);
                }
            }
            if self.volcano_queue.is_empty() {
                if self.setup_pick {
                    // Back to setup: the player still has the road for their settlement to place.
                    self.setup_pick = false;
                    self.phase = Phase::Setup;
                } else {
                    // A volcano only erupts straight after a roll, so what comes next is the player's turn.
                    self.phase = Phase::Main;
                }
                return;
            }
            let hex = self.volcano_queue.remove(0);
            // A volcano nobody is on doesn't erupt. (This is checked as each one comes up, not when the dice
            // were rolled: the lava of an earlier volcano may just have wrecked the last building here.)
            if !self.volcano_occupied(hex) {
                continue;
            }
            self.volcano_active = Some(hex);
            if self.setup_pick {
                self.volcano_picks = vec![(self.current, 1)];
            } else {
                self.volcano_picks = self.volcano_owed(hex);
                if hex == self.robber {
                    if self.board.hexes[hex].terrain.is_gold() {
                        self.log(LogEvent::GoldBlocked { hex });
                    } else {
                        self.log(LogEvent::VolcanoBlocked { hex });
                    }
                }
            }
        }
    }

    pub(super) fn do_volcano_pick(&mut self, p: PlayerId, r: Resource) -> Result<(), String> {
        if self.phase != Phase::VolcanoPick {
            return Err("There is no volcano to take a resource from right now".into());
        }
        match self.volcano_picks.first() {
            Some(&(q, _)) if q == p => {}
            Some(_) => return Err("It's not your pick".into()),
            None => return Err("There is nothing to pick".into()),
        }
        if self.bank[r] == 0 {
            return Err(format!("The bank has no {} left", r.name()));
        }
        self.bank[r] -= 1;
        self.players[p].hand[r] += 1;
        if !self.setup_pick {
            // (a starting card is not dice income, like the cards a last starting settlement collects)
            self.players[p].stats.dice_income += 1;
        }
        self.volcano_picks[0].1 -= 1;
        if self.volcano_picks[0].1 == 0 {
            self.volcano_picks.remove(0);
        }
        let from_gold = matches!(self.volcano_active, Some(h) if self.board.hexes[h].terrain.is_gold());
        if from_gold {
            self.log(LogEvent::GoldPicked { player: p, resource: r });
        } else {
            self.log(LogEvent::VolcanoPicked { player: p, resource: r });
        }
        self.advance_volcano();
        Ok(())
    }

    /// Lava flows from `hex`: a die says which of the volcano's six corners it runs down (they are numbered
    /// 1 to 6, clockwise from the upper right). A settlement there is destroyed and goes back to its owner's
    /// stock; a city is reduced to a settlement. If the owner has no settlement piece to put in the city's
    /// place, the settlement is built anyway but is on loan (see `Game::damaged`).
    fn erupt(&mut self, hex: usize) {
        let die: u8 = rand::thread_rng().gen_range(1..=6);
        self.erupt_with(hex, die);
    }

    pub(super) fn erupt_with(&mut self, hex: usize, die: u8) {
        let vertex = self.board.hexes[hex].vertices[usize::from(die) - 1];
        self.lava = Some(Lava { hex, die, vertex });
        let (effect, owner) = match self.buildings[vertex] {
            None => (LavaEffect::Nothing, None),
            Some(Building { owner, city: false }) => {
                self.buildings[vertex] = None;
                if let Some(i) = self.damaged.iter().position(|&d| d == vertex) {
                    // On loan: there was no piece from the stock to give back.
                    self.damaged.remove(i);
                } else if let Some(i) = self.damaged.iter().position(|&d| matches!(self.buildings[d], Some(b) if b.owner == owner)) {
                    // The piece that comes home pays back one of this player's loans instead of the stock.
                    self.damaged.remove(i);
                } else {
                    self.players[owner].settlements_left += 1;
                }
                (LavaEffect::Destroyed, Some(owner))
            }
            Some(Building { owner, city: true }) => {
                self.buildings[vertex] = Some(Building { owner, city: false });
                self.players[owner].cities_left += 1;
                if self.players[owner].settlements_left > 0 {
                    self.players[owner].settlements_left -= 1;
                } else {
                    self.damaged.push(vertex);
                }
                (LavaEffect::Demoted, Some(owner))
            }
        };
        self.log(LogEvent::LavaFlow { hex, die, effect, owner });
        // A destroyed settlement no longer blocks the roads that ran past it.
        self.update_longest_road();
    }
}
