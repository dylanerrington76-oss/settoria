//! What clients are sent: the view of the game for one seat (`Game::view`) and the types that make it up.

use crate::board::Board;
use crate::log_event::LogEvent;
use crate::types::{ByResource, DevHand, PerPlayer, PlayerId, Res};
use serde::Serialize;
use std::time::Instant;
use super::{Building, ClockKind, Game, Lava, Phase, PlayerStats, Settings, TimerMode, Trade};

/// How many of the newest log events a view carries. A client that is sent patches keeps the same window
/// (`LOG_WINDOW` in web/src/patch.ts), so the two must agree.
pub const LOG_WINDOW: usize = 25;

/// The countdown as clients see it. `remaining_ms` is relative to the moment the
/// view was built, so clients need no synchronised clock.
#[derive(Serialize)]
pub struct TimerView {
    pub kind: ClockKind,
    pub player: PlayerId,
    pub remaining_ms: u64,
    pub total_ms: u64,
}

/// What a volcano pick is waiting for.
#[derive(Serialize)]
pub struct VolcanoView {
    pub hex: usize,
    /// Whose pick it is.
    pub player: PlayerId,
    /// How many cards that player still picks (more than one for a city, or several buildings).
    pub left: u8,
    /// The pick is the free card for a last starting settlement beside a volcano, not an eruption.
    pub setup: bool,
}

#[derive(Serialize, Default)]
pub struct Legal {
    pub settlements: Vec<usize>,
    pub cities: Vec<usize>,
    pub roads: Vec<usize>,
    /// Where a bridge could go (empty unless the game has bridges on).
    pub bridges: Vec<usize>,
    pub hexes: Vec<usize>,
}

/// Where a player's points came from, already multiplied by the game's point values
/// (so the five parts add up to `Game::vp_total`).
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct VpBreakdown {
    pub settlements: usize,
    pub cities: usize,
    /// Victory Point cards, hidden until the game is over.
    pub dev_cards: usize,
    pub largest_army: usize,
    pub longest_road: usize,
}

impl VpBreakdown {
    /// The points every player can see: everything except the hidden Victory Point cards.
    pub fn public(&self) -> usize {
        self.settlements + self.cities + self.largest_army + self.longest_road
    }

    pub fn total(&self) -> usize {
        self.public() + self.dev_cards
    }
}

#[derive(Serialize)]
pub struct PlayerView {
    pub id: PlayerId,
    pub name: String,
    pub color: String,
    pub connected: bool,
    pub cards: u32,
    pub dev_cards: u32,
    pub knights: u8,
    pub roads_left: u8,
    pub settlements_left: u8,
    pub cities_left: u8,
    /// Visible points (buildings + awards); hidden victory cards excluded.
    pub vp: usize,
    pub longest_road: usize,
    /// Revealed to everyone once the game is over.
    pub vp_total: Option<usize>,
    /// Where `vp_total` came from; revealed together with it.
    pub vp_breakdown: Option<VpBreakdown>,
    /// Dice income so far (`PlayerStats::dice_income`). Public all game, because it feeds the live win% bar:
    /// it is only the sum of the `Collected` lines everyone already sees in the log (plus volcano picks).
    pub dice_income: u32,
    /// Whole-game totals (dice income, dev cards bought, trades); revealed with `vp_total` once the game is over.
    pub stats: Option<PlayerStats>,
}

/// Which harbours a player owns (see `Game::harbours`).
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HarbourView {
    /// A 3:1 harbour for any resource.
    pub any: bool,
    /// A 2:1 harbour, for each resource.
    pub resource: ByResource<bool>,
}

#[derive(Serialize)]
pub struct GameView<'a> {
    pub you: PlayerId,
    pub settings: Settings,
    /// The discard limit a 7 is held to right now (the 7 being dealt with in the discard phase, otherwise the
    /// next one). `settings.discard_limit` is where it starts; with `settings.variable_discard` it rises.
    pub discard_limit_now: u8,
    /// The board never changes during a game, so a client that asked for patches is sent it once, in its first
    /// view, and `view()` always fills it in; the server clears it where it has already been sent
    /// (see `update_seat` in main.rs). Cleared, it is left out of the JSON altogether.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board: Option<&'a Board>,
    pub phase: Phase,
    pub current: PlayerId,
    pub rolled: bool,
    pub dice: Option<(u8, u8)>,
    /// Times each total (2 to 12) has been rolled this game; see `Game::roll_counts`.
    pub roll_counts: [u32; 11],
    pub robber: usize,
    pub buildings: &'a [Option<Building>],
    pub roads: &'a [Option<PlayerId>],
    pub players: Vec<PlayerView>,
    pub hand: Res,
    pub dev: DevHand,
    pub dev_new: DevHand,
    pub vp_you: usize,
    pub bank: Res,
    pub deck_left: usize,
    pub dev_played: bool,
    pub ratios: ByResource<u8>,
    /// The harbours you own, for the trade dialog: players can't be offered bank-rate deals that these give.
    pub harbours: HarbourView,
    /// Resources you can't play Monopoly on until your turn ends, because you gave away
    /// `MONOPOLY_LOCK_CARDS` or more of them to other players this turn. All false when it isn't your turn.
    pub monopoly_locked: ByResource<bool>,
    pub setup_needs: Option<&'static str>,
    pub discards: &'a [(PlayerId, u8)],
    pub steal_candidates: &'a [PlayerId],
    /// Set while players are picking free resources from an erupting volcano.
    pub volcano: Option<VolcanoView>,
    /// Where the last lava flow went (cleared by the next roll).
    pub lava: Option<Lava>,
    /// Corners holding a settlement that is on loan after lava hit a city (see `Game::damaged`).
    pub damaged: &'a [usize],
    pub road_building_left: u8,
    pub trades: &'a [Trade],
    pub legal: Legal,
    pub longest_road_holder: Option<PlayerId>,
    pub largest_army_holder: Option<PlayerId>,
    pub winner: Option<PlayerId>,
    pub log: &'a [LogEvent],
    /// The number of the first event in `log` (events are numbered from 0 for the whole game), so a client can tell
    /// which lines it already has when the window slides.
    pub log_start: usize,
    pub order: &'a [PlayerId],
    pub timer_mode: TimerMode,
    pub timer: Option<TimerView>,
}

impl Game {
    /// The view for seat `you`. `road_lens` is `self.longest_roads()`, passed in because it is the
    /// same for every seat: a broadcast computes it once instead of once per view.
    pub fn view<'a>(&'a self, you: PlayerId, connected: &PerPlayer<bool>, road_lens: &PerPlayer<usize>) -> GameView<'a> {
        let over = self.phase == Phase::GameOver;
        let players = PlayerId::range(self.n)
            .map(|i| {
                let pl = &self.players[i];
                PlayerView {
                    id: i,
                    name: self.names[i].clone(),
                    color: self.colors[i].clone(),
                    connected: connected.get(i).copied().unwrap_or(true),
                    cards: pl.hand.total(),
                    dev_cards: pl.dev.total() + pl.dev_new.total(),
                    knights: pl.knights,
                    roads_left: pl.roads_left,
                    settlements_left: pl.settlements_left,
                    cities_left: pl.cities_left,
                    vp: self.vp_public(i),
                    longest_road: road_lens[i],
                    vp_total: if over { Some(self.vp_total(i)) } else { None },
                    vp_breakdown: if over { Some(self.vp_breakdown(i)) } else { None },
                    dice_income: pl.stats.dice_income,
                    stats: if over { Some(pl.stats) } else { None },
                }
            })
            .collect();
        let start = self.log.len().saturating_sub(LOG_WINDOW);
        let now = Instant::now();
        GameView {
            you,
            settings: self.settings,
            discard_limit_now: self.discard_limit_now(),
            board: Some(&self.board),
            phase: self.phase,
            current: self.current,
            rolled: self.rolled,
            dice: self.dice,
            roll_counts: self.roll_counts,
            robber: self.robber,
            buildings: &self.buildings,
            roads: &self.roads,
            players,
            hand: self.players[you].hand,
            dev: self.players[you].dev,
            dev_new: self.players[you].dev_new,
            vp_you: self.vp_total(you),
            bank: self.bank,
            deck_left: self.deck.len(),
            dev_played: self.dev_played_by(you),
            ratios: ByResource::from_fn(|r| self.ratio(you, r)),
            harbours: self.harbours(you),
            monopoly_locked: ByResource::from_fn(|r| (self.settings.speed || you == self.current) && self.monopoly_locked_for(you, r)),
            setup_needs: if self.phase == Phase::Setup {
                Some(if self.setup_vertex.is_some() { "road" } else { "settlement" })
            } else {
                None
            },
            discards: &self.discards,
            steal_candidates: &self.steal_candidates,
            volcano: match (self.phase, self.volcano_active, self.volcano_picks.first()) {
                (Phase::VolcanoPick, Some(hex), Some(&(player, left))) => Some(VolcanoView { hex, player, left, setup: self.setup_pick }),
                _ => None,
            },
            lava: self.lava,
            damaged: &self.damaged,
            road_building_left: if self.settings.speed { self.players[you].free_roads.min(self.players[you].roads_left) } else { self.road_building_left },
            trades: &self.trades,
            legal: self.legal(you),
            longest_road_holder: self.longest_road_holder,
            largest_army_holder: self.largest_army_holder,
            winner: self.winner,
            log: &self.log[start..],
            log_start: self.log_dropped + start,
            order: &self.order,
            timer_mode: self.timer_mode,
            timer: self.clock.map(|c| TimerView {
                kind: c.kind,
                player: c.player,
                remaining_ms: c.deadline.saturating_duration_since(now).as_millis() as u64,
                total_ms: c.total.as_millis() as u64,
            }),
        }
    }
}
