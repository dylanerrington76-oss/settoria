//! The rules engine: everything about how a game of Settoria is played, and the clocks that keep it moving.
//!
//! `Game` (below) holds the state of one game; what it can do is spread over the files in `game/`, one for each
//! part of the rules, each adding its own `impl Game` block:
//!
//! | File | What it holds |
//! |---|---|
//! | `settings.rs` | `Settings`: the rules the host can change, and their ranges |
//! | `setup.rs` | the constructors, and the setup phase (starting settlements and roads) |
//! | `action.rs` | `Action` and `Game::apply`, the one place every request is checked |
//! | `turn.rs` | whose turn it is, the dice, production, ending the turn |
//! | `legal.rs` | where things may go (free corners and sides, the robber's hexes) |
//! | `build.rs` | roads, bridges, settlements, cities, and their costs |
//! | `dev_cards.rs` | buying and playing development cards, the Monopoly lock |
//! | `robber.rs` | the 7: discarding, moving the robber, stealing |
//! | `volcano.rs` | volcanoes and gold: the free picks, the lava |
//! | `trade.rs` | harbours, bank trades, offers between players |
//! | `scoring.rs` | points, Longest Road, Largest Army, winning |
//! | `clock.rs`, `timeout.rs` | the countdowns, and the move played for a player who ran out of time |
//! | `player.rs` | colours, a player's hand and pieces, their stats |
//! | `view.rs` | `Game::view` and the types a client is sent |
//!
//! Everything a caller needs is re-exported here, so the rest of the program says `game::Action`, not
//! `game::action::Action`.

mod action;
mod build;
mod clock;
mod dev_cards;
mod legal;
mod player;
mod robber;
mod scoring;
mod settings;
mod setup;
mod timeout;
mod trade;
mod turn;
mod view;
mod volcano;
#[cfg(test)]
mod tests;

pub use action::Action;
pub use build::{COST_BRIDGE, COST_CITY, COST_ROAD, COST_SETTLEMENT};
pub use clock::{ClockKind, TimerMode};
pub use dev_cards::COST_DEV;
pub use player::{PALETTE, palette_color};
pub use settings::Settings;
pub use trade::Reply;
pub use view::{GameView, LOG_WINDOW};
use clock::Clock;
use player::{Player, PlayerStats};
use trade::Trade;
use view::{HarbourView, Legal, VpBreakdown};
use volcano::Lava;

use crate::board::Board;
use crate::log_event::LogEvent;
use crate::types::{DevCard, PerPlayer, PlayerId, Res, Resource};
use serde::Serialize;
use std::cell::RefCell;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Initial placement (settlement, then road; forward then backward).
    Setup,
    /// Start of a turn: roll the dice (a knight may be played first).
    Roll,
    /// A 7 was rolled: players with more than 7 cards must discard.
    Discard,
    MoveRobber,
    Steal,
    /// A volcano's number was rolled: the players with a building on it pick their free resources, one
    /// player at a time (see `Game::volcano_picks`), before the lava flows.
    VolcanoPick,
    /// After rolling: build, trade, play cards, end turn.
    Main,
    /// Placing the free roads from a Road Building card.
    RoadBuilding,
    GameOver,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct Building {
    pub owner: PlayerId,
    pub city: bool,
}

pub struct Game {
    pub board: Board,
    pub settings: Settings,
    pub n: usize,
    pub names: PerPlayer<String>,
    pub colors: PerPlayer<String>,
    pub players: PerPlayer<Player>,
    pub buildings: Vec<Option<Building>>,
    pub roads: Vec<Option<PlayerId>>,
    pub robber: usize,
    pub bank: Res,
    pub deck: Vec<DevCard>,
    pub phase: Phase,
    pub current: PlayerId,
    pub rolled: bool,
    pub dice: Option<(u8, u8)>,
    /// How many times each dice total has been rolled this game: index `i` counts totals of `i + 2`
    /// (so 11 entries, for 2 through 12). Kept here rather than read back from the log, which is capped.
    pub roll_counts: [u32; 11],
    /// Whether the latest roll counts toward `PlayerStats::luck_36ths`: true for every roll of the dice, false for a
    /// roll named with the Alchemist (its owner chose it, so nobody was lucky or unlucky). It stays as the roll left
    /// it until the next one, because the volcano and gold picks that follow a roll are part of that roll's income.
    pub luck_counts: bool,
    pub dev_played: bool,
    /// Cards of each resource the current player has handed to other players in completed trades this turn.
    /// Two or more of a resource lock it out of Monopoly until the turn ends (`MONOPOLY_LOCK_CARDS`).
    /// Bank trades don't count. Cleared when the turn ends.
    pub traded_away: Res,
    pub setup_idx: usize,
    pub setup_vertex: Option<usize>,
    pub discards: Vec<(PlayerId, u8)>,
    pub steal_candidates: Vec<PlayerId>,
    /// Volcano hexes whose number was rolled and that have not erupted yet, in the order they erupt.
    pub volcano_queue: Vec<usize>,
    /// The volcano that is erupting now: its players are picking, then its lava flows.
    pub volcano_active: Option<usize>,
    /// Free picks still owed at the active volcano: `(player, cards)`. Only the first player picks; the
    /// bank can never be asked for a card it doesn't have, because picks are made one at a time.
    pub volcano_picks: Vec<(PlayerId, u8)>,
    /// Set while the player placing their last starting settlement picks free cards for the volcanoes it touches
    /// (one card each, no eruption). The picks use the volcano phase; when they are done the game goes back to
    /// `Phase::Setup`, where that player still has a road to place.
    pub setup_pick: bool,
    /// Settlements that were cities until lava hit them while their owner had no settlement piece left to
    /// put in their place. The settlement stands anyway but is on loan: it holds no piece from the stock,
    /// and its owner may build no other city until it is upgraded again (repaired).
    pub damaged: Vec<usize>,
    /// Where the last lava flow went, shown on the board until the next roll.
    pub lava: Option<Lava>,
    pub road_building_left: u8,
    /// Player offers currently open. Only the player whose turn it is can have any.
    pub trades: Vec<Trade>,
    next_trade_id: u32,
    pub longest_road_holder: Option<PlayerId>,
    /// Every player's longest road, kept from the last time `longest_roads` computed it (the search is the
    /// costly part of a view). `None` means "not computed since the roads or buildings last changed": see
    /// `Game::invalidate_road_lens` for who must clear it.
    road_lens: RefCell<Option<PerPlayer<usize>>>,
    pub largest_army_holder: Option<PlayerId>,
    pub winner: Option<PlayerId>,
    pub log: Vec<LogEvent>,
    /// Events dropped off the front of `log` so far, so every event keeps a number that never changes.
    log_dropped: usize,
    /// Seat ids in turn order (randomised when the game starts). Drives the setup
    /// draft (forward, then backward) and every turn after it.
    pub order: Vec<PlayerId>,
    /// Speed mode: how many 7s have handed the robber out so far. The next 7 is placed by
    /// `order[rotation % n]` (see `Game::speed_lead`): the robber moves down the line of players.
    pub rotation: usize,
    pub timer_mode: TimerMode,
    clock: Option<Clock>,
    /// Turn-clock time left while the clock is paused (robber sequence) or not yet started.
    turn_left: Duration,
    /// Roll-clock time left when a Knight is played before rolling.
    roll_left: Option<Duration>,
    /// Bonus earned by the action being applied; consumed by `apply_at`.
    bonus: Duration,
}

impl Game {
    pub(super) fn log(&mut self, event: LogEvent) {
        self.log.push(event);
        if self.log.len() > 200 {
            self.log.remove(0);
            self.log_dropped += 1;
        }
    }

    /// Add an event to the log as the rules do, for tests of what clients are sent, which can't make the rules
    /// log hundreds of events on demand.
    #[cfg(test)]
    pub fn push_log_for_test(&mut self, event: LogEvent) {
        self.log(event);
    }

    /// Whether `p` is one of the seats at this table. (A `PlayerId` can come straight off the wire, so it is
    /// checked here once rather than trusted.)
    pub(super) fn has_player(&self, p: PlayerId) -> bool {
        p.index() < self.n
    }

    pub(super) fn pay(&mut self, p: PlayerId, cost: &Res) {
        for r in Resource::ALL {
            self.players[p].hand[r] -= cost[r];
            self.bank[r] += cost[r];
        }
    }
}
