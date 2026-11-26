//! The countdowns. Exactly one runs at a time (`Game::clock`), kept in step with the phase by `sync_clock`.
//! What happens when one runs out is in `timeout.rs`.

use crate::types::PlayerId;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use super::{Game, Phase};

/// Setup placement budgets, the same in every timer mode.
pub const SETUP_SETTLEMENT_SECS: u64 = 120;
pub const SETUP_ROAD_SECS: u64 = 30;
/// Time to roll before the dice are rolled automatically (not part of the turn clock).
pub const ROLL_SECS: u64 = 10;
/// After a 7: discarding, moving the robber and choosing a victim. None of these
/// count against the turn clock.
pub const DISCARD_SECS: u64 = 20;
pub const ROBBER_SECS: u64 = 20;
pub const STEAL_SECS: u64 = 15;
/// Time a player has to pick a free resource from an erupting volcano (not counted against the turn clock).
pub const VOLCANO_SECS: u64 = 15;
/// Speed mode: seconds between two automatic dice rolls. The countdown is always visible and starts again
/// after every roll (but not while a 7 is being dealt with, see `Game::sync_clock`).
pub const SPEED_ROLL_SECS: u64 = 20;
/// Added to the turn clock (never above the mode's starting time) for each of:
/// road, settlement, city, bank trade, completed player trade, development card bought.
pub const BONUS_SECS: u64 = 10;

/// The host picks one of these when the game is set up.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimerMode {
    VeryFast,
    Fast,
    #[default]
    Normal,
    Slow,
}

impl TimerMode {
    /// Starting (and maximum) time on the turn clock.
    pub fn secs(self) -> u64 {
        match self {
            TimerMode::VeryFast => 30,
            TimerMode::Fast => 60,
            TimerMode::Normal => 120,
            TimerMode::Slow => 240,
        }
    }
}

/// What the running countdown is for.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClockKind {
    SetupSettlement,
    SetupRoad,
    Roll,
    Discard,
    Robber,
    Steal,
    /// Picking free resources from an erupting volcano (see `Phase::VolcanoPick`).
    Volcano,
    /// The standard per-turn clock (building, trading, playing cards).
    Turn,
    /// Speed mode: the countdown to the next automatic roll (see `SPEED_ROLL_SECS`).
    AutoRoll,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Clock {
    /// The player whose turn it is (whose action the clock waits for).
    pub(super) player: PlayerId,
    pub(super) kind: ClockKind,
    pub(super) deadline: Instant,
    pub(super) total: Duration,
}

impl Game {
    /// Which countdown the current phase calls for, if any.
    fn clock_kind(&self) -> Option<ClockKind> {
        match self.phase {
            Phase::Setup => Some(if self.setup_vertex.is_some() {
                ClockKind::SetupRoad
            } else {
                ClockKind::SetupSettlement
            }),
            Phase::Roll => Some(ClockKind::Roll),
            Phase::Discard => Some(ClockKind::Discard),
            Phase::MoveRobber => Some(ClockKind::Robber),
            Phase::Steal => Some(ClockKind::Steal),
            Phase::VolcanoPick => Some(ClockKind::Volcano),
            Phase::Main if self.settings.speed => Some(ClockKind::AutoRoll),
            Phase::Main | Phase::RoadBuilding => Some(ClockKind::Turn),
            Phase::GameOver => None,
        }
    }

    /// Whose action the clock waits for: the player whose turn it is, except while a volcano is waiting
    /// for one particular player's pick.
    fn clock_player(&self) -> PlayerId {
        match (self.phase, self.volcano_picks.first()) {
            (Phase::VolcanoPick, Some(&(p, _))) => p,
            _ => self.current,
        }
    }

    fn full_budget(&self, kind: ClockKind) -> Duration {
        Duration::from_secs(match kind {
            ClockKind::SetupSettlement => SETUP_SETTLEMENT_SECS,
            ClockKind::SetupRoad => SETUP_ROAD_SECS,
            ClockKind::Roll => ROLL_SECS,
            ClockKind::Discard => DISCARD_SECS,
            ClockKind::Robber => ROBBER_SECS,
            ClockKind::Steal => STEAL_SECS,
            ClockKind::Volcano => VOLCANO_SECS,
            ClockKind::Turn => self.timer_mode.secs(),
            ClockKind::AutoRoll => SPEED_ROLL_SECS,
        })
    }

    /// Bring the running countdown in line with the game state. Called after every
    /// state change. Only the turn clock and the roll clock are paused and resumed
    /// (around a Knight); every other countdown starts fresh when its phase begins.
    pub(super) fn sync_clock(&mut self, now: Instant) {
        let want = self.clock_kind().map(|k| (self.clock_player(), k));
        let have = self.clock.map(|c| (c.player, c.kind));
        if want == have {
            return;
        }
        if let Some(c) = self.clock.take() {
            // Save what is left, but only within the same turn (a new turn resets both).
            // (The Speed countdown belongs to nobody: what is left of it is kept whoever `current` is, so that a Knight's
            // robber sequence pauses it rather than restarting it.)
            if c.player == self.current || c.kind == ClockKind::AutoRoll {
                let left = c.deadline.saturating_duration_since(now);
                match c.kind {
                    ClockKind::Turn => self.turn_left = left,
                    ClockKind::Roll | ClockKind::AutoRoll => self.roll_left = Some(left),
                    _ => {}
                }
            }
        }
        if let Some((player, kind)) = want {
            let total = self.full_budget(kind);
            let left = match kind {
                ClockKind::Turn => self.turn_left,
                ClockKind::Roll | ClockKind::AutoRoll => self.roll_left.take().unwrap_or(total),
                _ => total,
            };
            self.clock = Some(Clock { player, kind, deadline: now + left, total });
        }
    }

    /// Earn `BONUS_SECS` of extra clock time for the action being applied; `apply_bonus` banks it.
    pub(super) fn add_bonus(&mut self) {
        self.bonus += Duration::from_secs(BONUS_SECS);
    }

    /// Add the bonus earned by the action just applied to the turn clock, never
    /// letting it exceed the mode's starting time.
    pub(super) fn apply_bonus(&mut self, now: Instant) {
        let bonus = std::mem::take(&mut self.bonus);
        if bonus.is_zero() {
            return;
        }
        let current = self.current;
        if let Some(c) = self.clock.as_mut() {
            if c.kind == ClockKind::Turn && c.player == current {
                let left = c.deadline.saturating_duration_since(now);
                c.deadline = now + (left + bonus).min(c.total);
            }
        }
    }
}
