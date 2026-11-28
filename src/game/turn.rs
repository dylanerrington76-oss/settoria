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
        self.luck_counts = true;
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
        self.expect_income();
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

    /// Luck: before a roll is carried out, add what each player's buildings were expected to bring in on it to
    /// their (negative) running luck; `add_dice_income` adds what they actually got. Every roll is counted, a 7
    /// included, since a 7 pays nobody. A hex pays on a roll of its number `n` with chance `ways(n) / 36`, so
    /// a settlement on it is expected to bring in that many 36ths of a card and a city twice that. Resource hexes
    /// and volcano/gold fields count alike (their picks are dice income too); hexes the robber sits on are left
    /// out, because what they would have paid is counted as `PlayerStats::blocked` instead.
    fn expect_income(&mut self) {
        if !self.luck_counts {
            return;
        }
        for (hi, hex) in self.board.hexes.iter().enumerate() {
            if hi == self.robber || !(hex.terrain.resource().is_some() || hex.terrain.gives_free_pick()) {
                continue;
            }
            let Some(number) = hex.number else { continue };
            let ways = 6 - (7 - i32::from(number)).abs();
            if ways <= 0 {
                continue;
            }
            for &v in &hex.vertices {
                if let Some(b) = self.buildings[v] {
                    self.players[b.owner].stats.luck_36ths -= ways * if b.city { 2 } else { 1 };
                }
            }
        }
    }

    /// Dice income: `cards` received by `p` because of the roll that was just made (production, or free picks at
    /// a volcano or gold field). It counts for the Dice income stat, and for Luck when the roll counts for it.
    pub(super) fn add_dice_income(&mut self, p: PlayerId, cards: u32) {
        let counts = self.luck_counts;
        let stats = &mut self.players[p].stats;
        stats.dice_income += cards;
        if counts {
            stats.luck_36ths += 36 * cards as i32;
        }
    }

    pub(super) fn distribute(&mut self, roll: u8) {
        let mut gains: PerPlayer<Res> = PerPlayer::from_fn(self.n, |_| Res::ZERO);
        let mut blocked: PerPlayer<u32> = PerPlayer::from_fn(self.n, |_| 0);
        for (hi, hex) in self.board.hexes.iter().enumerate() {
            if hex.number != Some(roll) {
                continue;
            }
            let Some(r) = hex.terrain.resource() else { continue };
            for &v in &hex.vertices {
                if let Some(b) = self.buildings[v] {
                    let cards: u32 = if b.city { 2 } else { 1 };
                    if hi == self.robber {
                        // The robber stops the hex paying: those cards are what it cost the owner.
                        blocked[b.owner] += cards;
                    } else {
                        gains[b.owner][r] += cards as u8;
                    }
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
            self.players[p].stats.blocked += blocked[p];
            self.add_dice_income(p, received[p].total());
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
