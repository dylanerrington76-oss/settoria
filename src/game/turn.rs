//! The turn cycle: whose turn it is, rolling the dice, production, and ending the turn.

use crate::log_event::LogEvent;
use crate::types::{DevCard, PerPlayer, PlayerId, Res, Resource};
use rand::Rng;
use std::time::Duration;
use super::{Game, Phase};

impl Game {
    /// The player after `p` in turn order.
    fn next_player(&self, p: PlayerId) -> PlayerId {
        let pos = self.order.iter().position(|&x| x == p).unwrap_or(0);
        self.order[(pos + 1) % self.n]
    }

    /// Speed mode: the player who places the robber on the next 7. (Between robber moves this is also `current`.)
    pub(super) fn speed_lead(&self) -> PlayerId {
        self.order[self.rotation % self.n]
    }

    pub(super) fn resume_phase(&self) -> Phase {
        if self.rolled {
            Phase::Main
        } else {
            Phase::Roll
        }
    }

    pub(super) fn do_roll(&mut self) -> Result<(), String> {
        if self.phase != Phase::Roll {
            return Err("You can't roll right now".into());
        }
        self.roll_dice();
        Ok(())
    }

    /// Speed mode: the server rolls for the table (`tick`, when the `AutoRoll` countdown runs out). A roll starts a
    /// new round: development cards bought since the last roll become playable, and each player may play one
    /// development card again and is free of the Monopoly lock from their trades.
    pub(super) fn speed_roll(&mut self) {
        if self.phase != Phase::Main {
            return;
        }
        for pl in self.players.iter_mut() {
            for card in DevCard::ALL {
                let c = pl.dev_new[card];
                pl.dev[card] += c;
                pl.dev_new[card] = 0;
            }
            pl.dev_played = false;
            pl.traded_away = Res::ZERO;
        }
        self.roll_dice();
    }

    /// How many times the dice have been rolled this game (every roll counts, whoever rolled it).
    pub fn rolls(&self) -> u32 {
        self.roll_counts.iter().sum()
    }

    /// The discard limit that applies to the 7 being dealt with (during `Phase::Discard`) or, at any other time,
    /// to the next 7. It only differs from `Settings::discard_limit` with `Settings::variable_discard`.
    pub fn discard_limit_now(&self) -> u8 {
        let rolls = self.rolls();
        // The 7 that is being dealt with is already counted, but the limit it was held to is the one set by the
        // rolls before it.
        let before = if self.phase == Phase::Discard { rolls.saturating_sub(1) } else { rolls };
        self.settings.discard_limit_after(before)
    }

    /// Throw the dice and carry out the result.
    fn roll_dice(&mut self) {
        let mut rng = rand::thread_rng();
        let d = (rng.gen_range(1..=6u8), rng.gen_range(1..=6u8));
        self.roll_dice_as(d);
    }

    /// Carry out a roll that shows `d`: the dice are thrown by `roll_dice`, or named by an Alchemist.
    pub(super) fn roll_dice_as(&mut self, d: (u8, u8)) {
        let total = d.0 + d.1;
        self.dice = Some(d);
        self.lava = None;
        self.roll_counts[usize::from(total) - 2] += 1;
        self.rolled = true;
        if self.settings.speed {
            self.log(LogEvent::DiceRolled { dice: d });
        } else {
            self.log(LogEvent::Rolled { player: self.current, dice: d });
        }
        self.resolve_roll(total);
    }

    /// Carry out the result of a roll: production, or the 7 sequence.
    pub(super) fn resolve_roll(&mut self, total: u8) {
        if total == 7 {
            if self.settings.speed {
                // The robber moves down the line of players: each 7 is placed by the next one in turn order.
                self.current = self.speed_lead();
                self.rotation += 1;
            }
            // This 7 is already counted in `roll_counts`; the limit it is held to comes from the rolls before it.
            let limit = self.settings.discard_limit_after(self.rolls().saturating_sub(1));
            self.discards = PlayerId::range(self.n)
                .filter_map(|i| {
                    let c = self.players[i].hand.total();
                    if c > u32::from(limit) {
                        Some((i, (c / 2) as u8))
                    } else {
                        None
                    }
                })
                .collect();
            if self.discards.is_empty() {
                self.phase = Phase::MoveRobber;
                self.log(LogEvent::RobberMustMove);
            } else {
                self.phase = Phase::Discard;
                self.log(LogEvent::MustDiscard { limit });
            }
        } else {
            self.distribute(total);
            self.phase = Phase::Main;
            self.start_volcanoes(total);
        }
    }

    pub(super) fn distribute(&mut self, roll: u8) {
        let mut gains: PerPlayer<Res> = PerPlayer::from_fn(self.n, |_| Res::ZERO);
        for (hi, hex) in self.board.hexes.iter().enumerate() {
            if hex.number != Some(roll) || hi == self.robber {
                continue;
            }
            let Some(r) = hex.terrain.resource() else { continue };
            for &v in &hex.vertices {
                if let Some(b) = self.buildings[v] {
                    gains[b.owner][r] += if b.city { 2 } else { 1 };
                }
            }
        }
        let mut received: PerPlayer<Res> = PerPlayer::from_fn(self.n, |_| Res::ZERO);
        for r in Resource::ALL {
            let demand: u32 = gains.iter().map(|g| g[r] as u32).sum();
            if demand == 0 {
                continue;
            }
            let takers = gains.iter().filter(|g| g[r] > 0).count();
            if demand <= self.bank[r] as u32 {
                for p in PlayerId::range(self.n) {
                    received[p][r] = gains[p][r];
                }
            } else if takers == 1 {
                // Only one player is owed this resource: they get what's left.
                for p in PlayerId::range(self.n) {
                    if gains[p][r] > 0 {
                        received[p][r] = self.bank[r];
                    }
                }
            } else {
                self.log(LogEvent::BankShort { resource: r });
            }
        }
        for p in PlayerId::range(self.n) {
            for r in Resource::ALL {
                self.players[p].hand[r] += received[p][r];
                self.bank[r] -= received[p][r];
            }
            self.players[p].stats.dice_income += received[p].total();
            if received[p].total() > 0 {
                self.log(LogEvent::Collected { player: p, cards: received[p] });
            }
        }
    }

    pub(super) fn do_end_turn(&mut self) -> Result<(), String> {
        if self.settings.speed {
            return Err("There are no turns in Speed mode".into());
        }
        if self.phase != Phase::Main {
            return Err("You can't end your turn right now".into());
        }
        let p = self.current;
        for card in DevCard::ALL {
            let c = self.players[p].dev_new[card];
            self.players[p].dev[card] += c;
            self.players[p].dev_new[card] = 0;
        }
        self.trades.clear();
        self.dev_played = false;
        self.traded_away = Res::ZERO;
        self.rolled = false;
        self.dice = None;
        self.current = self.next_player(p);
        self.phase = Phase::Roll;
        // A fresh turn: full turn clock, full roll window.
        self.turn_left = Duration::from_secs(self.timer_mode.secs());
        self.roll_left = None;
        Ok(())
    }
}
