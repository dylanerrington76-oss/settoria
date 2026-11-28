//! The 7: discarding, moving the robber, and stealing (a Knight sends the game down the same path).

use crate::log_event::LogEvent;
use crate::types::{PlayerId, Res, Resource};
use rand::Rng;
use super::{Game, Phase};

impl Game {
    pub(super) fn do_discard(&mut self, p: PlayerId, cards: Res) -> Result<(), String> {
        if self.phase != Phase::Discard {
            return Err("Nobody needs to discard right now".into());
        }
        let idx = self
            .discards
            .iter()
            .position(|d| d.0 == p)
            .ok_or("You don't need to discard")?;
        let need = self.discards[idx].1 as u32;
        if cards.total() != need {
            return Err(format!("Discard exactly {} cards", need));
        }
        if !self.players[p].hand.covers(&cards) {
            return Err("You don't have those cards".into());
        }
        self.pay(p, &cards);
        self.players[p].stats.dice_discarded += need;
        self.discards.remove(idx);
        self.log(LogEvent::Discarded { player: p, count: need });
        if self.discards.is_empty() {
            self.phase = Phase::MoveRobber;
            self.log(LogEvent::RobberMustMove);
        }
        Ok(())
    }

    pub(super) fn do_move_robber(&mut self, p: PlayerId, hex: usize) -> Result<(), String> {
        if self.phase != Phase::MoveRobber {
            return Err("You can't move the robber now".into());
        }
        if hex >= self.board.hexes.len() || hex == self.robber {
            return Err("The robber must move to a different hex".into());
        }
        if !self.robber_hexes(p).contains(&hex) {
            return Err(format!(
                "Friendly Robber: players with {} points or fewer can't be robbed",
                self.settings.friendly_robber_max_vp()
            ));
        }
        self.robber = hex;
        self.log(LogEvent::RobberMoved { player: p });
        let mut cands: Vec<PlayerId> = vec![];
        for &v in &self.board.hexes[hex].vertices {
            if let Some(b) = self.buildings[v] {
                if b.owner != p && self.players[b.owner].hand.total() > 0 && !cands.contains(&b.owner) {
                    cands.push(b.owner);
                }
            }
        }
        match cands.len() {
            0 => self.finish_robber(),
            1 => self.steal_from(p, cands[0]),
            _ => {
                self.steal_candidates = cands;
                self.phase = Phase::Steal;
            }
        }
        Ok(())
    }

    pub(super) fn do_steal(&mut self, victim: PlayerId) -> Result<(), String> {
        if self.phase != Phase::Steal {
            return Err("There is nobody to steal from right now".into());
        }
        if !self.steal_candidates.contains(&victim) {
            return Err("You can't steal from that player".into());
        }
        let thief = self.current;
        self.steal_from(thief, victim);
        Ok(())
    }

    fn steal_from(&mut self, thief: PlayerId, victim: PlayerId) {
        let hand = self.players[victim].hand;
        let total = hand.total();
        if total > 0 {
            let mut k = rand::thread_rng().gen_range(0..total);
            for r in Resource::ALL {
                if k < hand[r] as u32 {
                    self.players[victim].hand[r] -= 1;
                    self.players[thief].hand[r] += 1;
                    self.players[victim].stats.robber_lost += 1;
                    self.players[thief].stats.robber_gained += 1;
                    break;
                }
                k -= hand[r] as u32;
            }
            self.log(LogEvent::Stole { player: thief, victim });
        }
        self.finish_robber();
    }

    fn finish_robber(&mut self) {
        self.steal_candidates.clear();
        if self.settings.speed {
            // Whoever moved it (a Knight's owner, or the 7's turn in the rotation), `current` goes back to being
            // the player who places the next 7.
            self.current = self.speed_lead();
        }
        self.phase = self.resume_phase();
    }
}
