//! `Game::tick`: when a countdown has run out, the server plays the move for the player who timed out.

use crate::board::is_hot;
use crate::log_event::LogEvent;
use crate::types::{Res, Resource};
use rand::seq::SliceRandom;
use std::time::Instant;
use super::{Clock, ClockKind, Game, Phase};

impl Game {
    /// Fire the running countdown if it has run out, taking the automatic action for
    /// the player who timed out. Returns true if the game changed (so the caller
    /// should broadcast).
    pub fn tick(&mut self, now: Instant) -> bool {
        let Some(c) = self.clock else { return false };
        if now < c.deadline {
            return false;
        }
        let p = c.player;
        if c.kind != ClockKind::AutoRoll {
            // (the automatic roll is not anybody running out of time)
            self.log(LogEvent::TimedOut { player: p, kind: c.kind });
        }
        let mut rng = rand::thread_rng();
        match c.kind {
            ClockKind::SetupSettlement => {
                if let Some(&v) = self.legal(p).settlements.choose(&mut rng) {
                    let _ = self.do_build_settlement(p, v);
                }
            }
            ClockKind::SetupRoad => {
                if let Some(&e) = self.legal(p).roads.choose(&mut rng) {
                    let _ = self.do_build_road(p, e);
                }
            }
            ClockKind::Roll => {
                let _ = self.do_roll();
            }
            ClockKind::AutoRoll => self.speed_roll(),
            ClockKind::Discard => {
                while self.phase == Phase::Discard {
                    let Some(&(q, need)) = self.discards.first() else { break };
                    let mut pool: Vec<Resource> = Resource::ALL
                        .iter()
                        .flat_map(|&r| std::iter::repeat(r).take(self.players[q].hand[r] as usize))
                        .collect();
                    pool.shuffle(&mut rng);
                    let mut cards = Res::ZERO;
                    for &r in pool.iter().take(need as usize) {
                        cards[r] += 1;
                    }
                    if self.do_discard(q, cards).is_err() {
                        break;
                    }
                }
            }
            ClockKind::Robber => {
                let allowed = self.robber_hexes(p);
                let hot: Vec<usize> = allowed
                    .iter()
                    .copied()
                    .filter(|&h| is_hot(self.board.hexes[h].number))
                    .collect();
                // Normally one of the four 6/8 hexes; any allowed hex if Friendly Robber rules those out.
                let target = hot.choose(&mut rng).copied().or_else(|| allowed.choose(&mut rng).copied());
                if let Some(h) = target {
                    let _ = self.do_move_robber(p, h);
                }
            }
            ClockKind::Volcano => {
                // Every pick this player still owes is made at random from what the bank has.
                while self.phase == Phase::VolcanoPick && matches!(self.volcano_picks.first(), Some(&(q, _)) if q == p) {
                    let open: Vec<Resource> = Resource::ALL.iter().copied().filter(|&r| self.bank[r] > 0).collect();
                    let Some(&r) = open.choose(&mut rng) else { break };
                    if self.do_volcano_pick(p, r).is_err() {
                        break;
                    }
                }
            }
            ClockKind::Steal => {
                if let Some(&victim) = self.steal_candidates.clone().choose(&mut rng) {
                    let _ = self.do_steal(victim);
                }
            }
            ClockKind::Turn => {
                // Free roads from Road Building that were never placed are placed at random.
                while self.phase == Phase::RoadBuilding {
                    let Some(&e) = self.legal_roads(p).choose(&mut rng) else { break };
                    if self.do_build_road(p, e).is_err() {
                        break;
                    }
                }
                self.phase = if self.phase == Phase::RoadBuilding { Phase::Main } else { self.phase };
                self.road_building_left = 0;
                let _ = self.do_end_turn();
            }
        }
        self.check_win();
        self.prune_trades();
        self.sync_clock(now);
        if c.kind == ClockKind::AutoRoll {
            // A roll starts the countdown afresh, and a 7 (or a volcano) that has taken the clock away must not
            // hand back the zero that was left on it.
            self.roll_left = None;
        }
        // Safety net: if the action somehow left the same countdown running, restart
        // it rather than firing again on the next tick.
        if let Some(n) = self.clock {
            if n.player == c.player && n.kind == c.kind && n.deadline <= now {
                self.clock = Some(Clock { deadline: now + n.total, ..n });
            }
        }
        true
    }
}
