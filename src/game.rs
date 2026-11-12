//! Rules engine for standard base-game 

use crate::board::*;
use crate::log_event::LogEvent;
use crate::map_def::{MapDef, MapError, PlayerRange, MAX_PLAYERS, MIN_PLAYERS};
use crate::types::{ByDev, ByResource, DevCard, DevHand, PerPlayer, PlayerId, Res, Resource};
use rand::seq::SliceRandom;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub const COST_ROAD: Res = Res::new(1, 1, 0, 0, 0);
pub const COST_SETTLEMENT: Res = Res::new(1, 1, 0, 1, 1);
pub const COST_CITY: Res = Res::new(0, 0, 3, 2, 0);
pub const COST_DEV: Res = Res::new(0, 0, 1, 1, 1);
/// A bridge: 1 brick, 1 lumber and 1 ore.
pub const COST_BRIDGE: Res = Res::new(1, 1, 1, 0, 0);

// Game settings

/// Friendly Robber protects players with this many visible points or fewer.
pub const FRIENDLY_ROBBER_MAX_VP: usize = 2;
/// Most starting settlements (summed over all players) the board can reasonably hold.
/// (Eight players with the most starting settlements each, 5, is 40.)
pub const MAX_SETUP_SETTLEMENTS_TOTAL: usize = 40;
/// Standard development deck: knights, road building, year of plenty, monopoly, victory points.
const STANDARD_DECK: ByDev<usize> = ByDev([14, 2, 2, 2, 5]);

/// The rules of one game. `Settings::default()` is the host's starting point.
/// Every field has a range (see `validate`), and the server refuses anything outside it.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    /// Points needed to win, checked on the player's own turn.
    pub points_to_win: u8,
    /// After a 7, players holding MORE than this many cards discard half.
    pub discard_limit: u8,
    /// Robber can't go on a hex touching a player with `FRIENDLY_ROBBER_MAX_VP` or fewer points.
    pub friendly_robber: bool,
    /// Players may build bridges: a road over a narrow stretch of water between two land corners
    /// (`Edge::bridge`). A bridge costs `COST_BRIDGE`, uses one of the player's road pieces and counts
    /// as one road for Longest Road.
    pub bridges: bool,
    pub max_roads: u8,
    pub max_cities: u8,
    pub max_settlements: u8,
    /// Size of the development card deck (split in the standard proportions).
    pub dev_cards: u8,
    /// Cards of each resource in the bank at the start.
    pub resource_max: u8,
    pub settlement_points: u8,
    pub city_points: u8,
    pub longest_road_points: u8,
    pub largest_army_points: u8,
    /// Points each Victory Point card is worth.
    pub vp_card_points: u8,
    /// Shortest road that can hold Longest Road.
    pub longest_road_min: u8,
    /// Fewest knights that can hold Largest Army.
    pub largest_army_min: u8,
    /// Settlements (each with one road) every player places during setup.
    pub starting_settlements: u8,
    /// Resources given to the bank per card received (harbours can only improve on this).
    pub bank_ratio: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            points_to_win: 15,
            discard_limit: 7,
            friendly_robber: false,
            bridges: false,
            max_roads: 15,
            max_cities: 4,
            max_settlements: 5,
            dev_cards: 25,
            resource_max: 19,
            settlement_points: 1,
            city_points: 2,
            longest_road_points: 2,
            largest_army_points: 2,
            vp_card_points: 1,
            longest_road_min: 5,
            largest_army_min: 3,
            starting_settlements: 2,
            bank_ratio: 4,
        }
    }
}

fn in_range(label: &str, v: u8, lo: u8, hi: u8) -> Result<(), String> {
    if v < lo || v > hi {
        Err(format!("{label} must be between {lo} and {hi}"))
    } else {
        Ok(())
    }
}

impl Settings {
    /// Reject out-of-range values and combinations that cannot be played.
    pub fn validate(&self) -> Result<(), String> {
        in_range("Points to win", self.points_to_win, 3, 99)?;
        in_range("Card discard limit", self.discard_limit, 1, 50)?;
        in_range("Max roads", self.max_roads, 1, 50)?;
        in_range("Max cities", self.max_cities, 0, 20)?;
        in_range("Max settlements", self.max_settlements, 1, 20)?;
        in_range("Development cards", self.dev_cards, 0, 60)?;
        in_range("Max of each resource", self.resource_max, 1, 99)?;
        in_range("Settlement points", self.settlement_points, 0, 10)?;
        in_range("City points", self.city_points, 0, 10)?;
        in_range("Longest Road points", self.longest_road_points, 0, 10)?;
        in_range("Largest Army points", self.largest_army_points, 0, 10)?;
        in_range("Victory Point card points", self.vp_card_points, 0, 10)?;
        in_range("Longest Road minimum", self.longest_road_min, 2, 20)?;
        in_range("Largest Army minimum", self.largest_army_min, 1, 20)?;
        in_range("Starting settlements", self.starting_settlements, 1, 5)?;
        in_range("Bank trade ratio", self.bank_ratio, 1, 8)?;
        if self.starting_settlements > self.max_settlements {
            return Err("Starting settlements can't exceed max settlements".into());
        }
        if self.starting_settlements > self.max_roads {
            return Err("Each starting settlement needs a road, so max roads can't be lower".into());
        }
        Ok(())
    }

    /// The rules a table of `players` actually plays with. With more than 4 players the shared bank and the
    /// development deck would run dry far too quickly, so each one that the host left at the standard size
    /// grows: 5 or 6 players get 24 cards of each resource and a 34-card deck (the 5-6 player extension of the
    /// board game), 7 or 8 get 30 and 44. A value the host changed is never touched.
    pub fn scaled_for(mut self, players: usize) -> Settings {
        let d = Settings::default();
        let (bank, deck) = match players {
            0..=4 => return self,
            5..=6 => (24, 34),
            _ => (30, 44),
        };
        if self.resource_max == d.resource_max {
            self.resource_max = bank;
        }
        if self.dev_cards == d.dev_cards {
            self.dev_cards = deck;
        }
        self
    }

    /// Whether `players` people can all fit their starting settlements on the board.
    pub fn check_players(&self, players: usize) -> Result<(), String> {
        if players * self.starting_settlements as usize > MAX_SETUP_SETTLEMENTS_TOTAL {
            return Err(format!(
                "{} players with {} starting settlements each won't fit on the board (at most {} in total)",
                players, self.starting_settlements, MAX_SETUP_SETTLEMENTS_TOTAL
            ));
        }
        Ok(())
    }

    /// "Label value" for each setting that differs from the standard choice (for the game log).
    pub fn changes(&self) -> Vec<String> {
        let d = Settings::default();
        let mut v = vec![];
        let mut num = |label: &str, now: u8, std: u8| {
            if now != std {
                v.push(format!("{label} {now}"));
            }
        };
        num("points to win", self.points_to_win, d.points_to_win);
        num("discard limit", self.discard_limit, d.discard_limit);
        num("max roads", self.max_roads, d.max_roads);
        num("max cities", self.max_cities, d.max_cities);
        num("max settlements", self.max_settlements, d.max_settlements);
        num("development cards", self.dev_cards, d.dev_cards);
        num("cards per resource", self.resource_max, d.resource_max);
        num("settlement points", self.settlement_points, d.settlement_points);
        num("city points", self.city_points, d.city_points);
        num("Longest Road points", self.longest_road_points, d.longest_road_points);
        num("Largest Army points", self.largest_army_points, d.largest_army_points);
        num("Victory Point card points", self.vp_card_points, d.vp_card_points);
        num("Longest Road minimum", self.longest_road_min, d.longest_road_min);
        num("Largest Army minimum", self.largest_army_min, d.largest_army_min);
        num("starting settlements", self.starting_settlements, d.starting_settlements);
        if self.bank_ratio != d.bank_ratio {
            v.push(format!("bank trade {}:1", self.bank_ratio));
        }
        if self.friendly_robber {
            v.push("Friendly Robber on".to_string());
        }
        if self.bridges {
            v.push("Bridges on".to_string());
        }
        v
    }

    /// Cards of each kind in a deck of `dev_cards`: the standard 14/2/2/2/5 mix scaled
    /// to the chosen size (largest-remainder rounding, so the total is exact).
    pub fn deck_counts(&self) -> ByDev<usize> {
        let total: usize = STANDARD_DECK.0.iter().sum();
        let want = self.dev_cards as usize;
        let mut counts = ByDev::from_fn(|c| STANDARD_DECK[c] * want / total);
        let left = want - counts.0.iter().sum::<usize>();
        let mut order = DevCard::ALL;
        order.sort_by_key(|&c| std::cmp::Reverse(STANDARD_DECK[c] * want % total));
        for &c in order.iter().cycle().take(left) {
            counts[c] += 1;
        }
        counts
    }
}

// ----------------------------------------------------------------------
// Timers
// ----------------------------------------------------------------------

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
    /// The standard per-turn clock (building, trading, playing cards).
    Turn,
}

#[derive(Clone, Copy, Debug)]
struct Clock {
    /// The player whose turn it is (whose action the clock waits for).
    player: PlayerId,
    kind: ClockKind,
    deadline: Instant,
    total: Duration,
}

/// The countdown as clients see it. `remaining_ms` is relative to the moment the
/// view was built, so clients need no synchronised clock.
#[derive(Serialize)]
pub struct TimerView {
    pub kind: ClockKind,
    pub player: PlayerId,
    pub remaining_ms: u64,
    pub total_ms: u64,
}

/// Colours a player may choose when joining, as `#RRGGBB` (names live in the client).
pub const PALETTE: [&str; 10] = [
    "#FE5100", // orange
    "#613393", // purple
    "#FFB81C", // gold
    "#69BE28", // green
    "#0033A0", // blue
    "#CC0000", // red
    "#7BAFD4", // mystic blue
    "#FFFFFF", // white
    "#8A8D8F", // silver
    "#000000", // black
];

/// The palette entry matching `c` (case-insensitive), in its canonical spelling.
pub fn palette_color(c: &str) -> Option<&'static str> {
    PALETTE.iter().copied().find(|p| p.eq_ignore_ascii_case(c))
}

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

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    Proposer,
    Pending,
    Accepted,
    Declined,
}

/// The most player offers one proposer may have open at once.
pub const MAX_OPEN_TRADES: usize = 6;

#[derive(Clone, Debug, Serialize)]
pub struct Trade {
    /// Unique for the whole game; replies, confirmations and withdrawals name an offer by it.
    pub id: u32,
    pub from: PlayerId,
    /// What the proposer hands over.
    pub give: Res,
    /// What the proposer wants in return.
    pub want: Res,
    pub replies: PerPlayer<Reply>,
}

#[derive(Clone, Debug)]
pub struct Player {
    pub hand: Res,
    pub dev: DevHand,
    pub dev_new: DevHand,
    pub knights: u8,
    pub roads_left: u8,
    pub settlements_left: u8,
    pub cities_left: u8,
}

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
    MoveRobber { hex: usize },
    Steal { victim: PlayerId },
    Discard { cards: Res },
    BankTrade { give: Resource, get: Resource },
    OfferTrade { give: Res, want: Res },
    RespondTrade { id: u32, accept: bool },
    ConfirmTrade { id: u32, with: PlayerId },
    /// Withdraw one of your open offers.
    CancelTrade { id: u32 },
    EndTurn,
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
    pub dev_played: bool,
    pub setup_idx: usize,
    pub setup_vertex: Option<usize>,
    pub discards: Vec<(PlayerId, u8)>,
    pub steal_candidates: Vec<PlayerId>,
    pub road_building_left: u8,
    /// Player offers currently open. Only the player whose turn it is can have any.
    pub trades: Vec<Trade>,
    next_trade_id: u32,
    pub longest_road_holder: Option<PlayerId>,
    pub largest_army_holder: Option<PlayerId>,
    pub winner: Option<PlayerId>,
    pub log: Vec<LogEvent>,
    /// Seat ids in turn order (randomised when the game starts). Drives the setup
    /// draft (forward, then backward) and every turn after it.
    pub order: Vec<PlayerId>,
    pub timer_mode: TimerMode,
    clock: Option<Clock>,
    /// Turn-clock time left while the clock is paused (robber sequence) or not yet started.
    turn_left: Duration,
    /// Roll-clock time left when a Knight is played before rolling.
    roll_left: Option<Duration>,
    /// Bonus earned by the action being applied; consumed by `apply_at`.
    bonus: Duration,
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
}

#[derive(Serialize)]
pub struct GameView<'a> {
    pub you: PlayerId,
    pub settings: Settings,
    pub board: &'a Board,
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
    pub setup_needs: Option<&'static str>,
    pub discards: &'a [(PlayerId, u8)],
    pub steal_candidates: &'a [PlayerId],
    pub road_building_left: u8,
    pub trades: &'a [Trade],
    pub legal: Legal,
    pub longest_road_holder: Option<PlayerId>,
    pub largest_army_holder: Option<PlayerId>,
    pub winner: Option<PlayerId>,
    pub log: &'a [LogEvent],
    pub order: &'a [PlayerId],
    pub timer_mode: TimerMode,
    pub timer: Option<TimerView>,
}

impl Game {
    /// A game where seat `i` simply gets the `i`th palette color.
    #[cfg(test)]
    pub fn new(names: Vec<String>) -> Game {
        let colors = PALETTE.iter().take(names.len()).map(|c| c.to_string()).collect();
        Game::with_colors(names, colors)
    }

    /// Seats play in seat order and the timer mode is `Normal`. The server uses
    /// `try_with_options`, which shuffles the order; this keeps tests deterministic.
    #[cfg(test)]
    pub fn with_colors(names: Vec<String>, colors: Vec<String>) -> Game {
        Game::build(names, colors, TimerMode::Normal, Settings::default(), crate::builtin::DEFAULT_MAP, false, Instant::now())
    }

    /// Like `new`, with custom rules (tests).
    #[cfg(test)]
    pub fn with_settings(names: Vec<String>, settings: Settings) -> Game {
        let colors = PALETTE.iter().take(names.len()).map(|c| c.to_string()).collect();
        Game::build(names, colors, TimerMode::Normal, settings, crate::builtin::DEFAULT_MAP, false, Instant::now())
    }

    /// Like `new`, on the given map (tests).
    #[cfg(test)]
    pub fn with_map(names: Vec<String>, map: &str) -> Game {
        let colors = PALETTE.iter().take(names.len()).map(|c| c.to_string()).collect();
        Game::build(names, colors, TimerMode::Normal, Settings::default(), map, false, Instant::now())
    }

    /// A real game on a built-in map: random draft/turn order, the host's timer mode and rules.
    /// Fails (and creates nothing) if the map can't be built or isn't meant for this many players.
    /// (The server goes through the map registry and `try_with_def`; this is for tests.)
    #[cfg(test)]
    pub fn try_with_options(names: Vec<String>, colors: Vec<String>, mode: TimerMode, settings: Settings, map: &str) -> Result<Game, MapError> {
        let def = crate::builtin::builtin(map).unwrap_or_else(|| panic!("`{map}` is not a built-in map"));
        Game::try_with_def(names, colors, mode, settings, map, def)
    }

    /// Like `try_with_options`, on any map definition. `map` is the id the map has in the registry; the
    /// board and the game log are labelled with it.
    pub fn try_with_def(names: Vec<String>, colors: Vec<String>, mode: TimerMode, settings: Settings, map: &str, def: &MapDef) -> Result<Game, MapError> {
        Game::try_build(names, colors, mode, settings, map, def, true, Instant::now())
    }

    /// Like `try_with_options` for the built-in maps, which can't fail (tests).
    #[cfg(test)]
    pub fn with_options(names: Vec<String>, colors: Vec<String>, mode: TimerMode, settings: Settings, map: &str) -> Game {
        Game::build(names, colors, mode, settings, map, true, Instant::now())
    }

    /// `try_build` for a built-in map, which can't fail (tests).
    #[cfg(test)]
    fn build(names: Vec<String>, colors: Vec<String>, mode: TimerMode, settings: Settings, map: &str, shuffle: bool, now: Instant) -> Game {
        let def = crate::builtin::builtin(map).unwrap_or_else(|| panic!("`{map}` is not a built-in map"));
        Game::try_build(names, colors, mode, settings, map, def, shuffle, now).expect("the built-in maps are valid")
    }

    #[allow(clippy::too_many_arguments)]
    fn try_build(
        names: Vec<String>,
        colors: Vec<String>,
        mode: TimerMode,
        settings: Settings,
        map: &str,
        def: &MapDef,
        shuffle: bool,
        now: Instant,
    ) -> Result<Game, MapError> {
        // The lobby only ever passes settings it has already validated, so this is a programming error.
        settings.validate().expect("invalid game settings");
        assert_eq!(names.len(), colors.len(), "one color per player");
        let n = names.len();
        // What the host chose is what the game log reports as custom rules; the game itself plays with the
        // bank and development deck sized for the table (see `scaled_for`).
        let chosen_settings = settings;
        let settings = settings.scaled_for(n);
        // The map's own say comes first, so the host gets "this map is for 3 to 4 players".
        let PlayerRange { min, max } = def.players;
        if n < min || n > max {
            return Err(MapError::PlayerCount { players: n, min, max });
        }
        let mut rng = rand::thread_rng();
        let board = Board::from_def(def, map, &mut rng)?;
        // `from_def` has checked that the map's player range lies within the engine's limits.
        assert!((MIN_PLAYERS..=MAX_PLAYERS).contains(&n), "Settoria needs {MIN_PLAYERS}-{MAX_PLAYERS} players");

        let mut deck: Vec<DevCard> = Vec::new();
        for (card, &count) in settings.deck_counts().iter() {
            deck.extend(std::iter::repeat(card).take(count));
        }
        deck.shuffle(&mut rng);

        let players = PerPlayer::from_fn(n, |_| Player {
            hand: Res::ZERO,
            dev: DevHand::ZERO,
            dev_new: DevHand::ZERO,
            knights: 0,
            roads_left: settings.max_roads,
            settlements_left: settings.max_settlements,
            cities_left: settings.max_cities,
        });

        let mut order: Vec<PlayerId> = PlayerId::range(n).collect();
        if shuffle {
            order.shuffle(&mut rng);
        }

        let mut g = Game {
            settings,
            n,
            names: names.into(),
            colors: colors.into(),
            players,
            buildings: vec![None; board.vertices.len()],
            roads: vec![None; board.edges.len()],
            robber: board.desert,
            board,
            bank: Res::splat(settings.resource_max),
            deck,
            phase: Phase::Setup,
            current: order[0],
            rolled: false,
            dice: None,
            roll_counts: [0; 11],
            dev_played: false,
            setup_idx: 0,
            setup_vertex: None,
            discards: vec![],
            steal_candidates: vec![],
            road_building_left: 0,
            trades: vec![],
            next_trade_id: 0,
            longest_road_holder: None,
            largest_army_holder: None,
            winner: None,
            log: vec![],
            order,
            timer_mode: mode,
            clock: None,
            turn_left: Duration::from_secs(mode.secs()),
            roll_left: None,
            bonus: Duration::ZERO,
        };
        g.log(LogEvent::GameStarted);
        if map != crate::builtin::DEFAULT_MAP {
            g.log(LogEvent::MapChosen { map: map.to_string(), name: def.name.clone() });
        }
        let changes = chosen_settings.changes();
        if !changes.is_empty() {
            g.log(LogEvent::CustomRules { changes });
        }
        let order = g.order.clone();
        g.log(LogEvent::TurnOrder { order });
        g.sync_clock(now);
        Ok(g)
    }

    fn log(&mut self, event: LogEvent) {
        self.log.push(event);
        if self.log.len() > 200 {
            self.log.remove(0);
        }
    }

    /// Who places during setup step `idx`: turn order forward, then backward, then
    /// forward again, and so on for each round of starting settlements.
    fn setup_player(&self, idx: usize) -> PlayerId {
        let (round, i) = (idx / self.n, idx % self.n);
        let pos = if round % 2 == 0 { i } else { self.n - 1 - i };
        self.order[pos]
    }

    /// Total setup placements (one settlement + road each).
    fn setup_steps(&self) -> usize {
        self.n * self.settings.starting_settlements as usize
    }

    /// Move on to the next setup placement, or into the game when setup is over.
    fn advance_setup(&mut self) {
        self.setup_idx += 1;
        let total = self.setup_steps();
        if self.setup_idx < total && !(0..self.buildings.len()).any(|v| self.settlement_free(v)) {
            // Only possible with many starting settlements: nobody can place any more.
            self.log(LogEvent::SetupEndedEarly);
            self.setup_idx = total;
        }
        if self.setup_idx >= total {
            self.phase = Phase::Roll;
            self.current = self.order[0];
            self.rolled = false;
            self.turn_left = Duration::from_secs(self.timer_mode.secs());
            self.roll_left = None;
            self.log(LogEvent::SetupComplete);
        } else {
            self.current = self.setup_player(self.setup_idx);
        }
    }

    /// The player after `p` in turn order.
    fn next_player(&self, p: PlayerId) -> PlayerId {
        let pos = self.order.iter().position(|&x| x == p).unwrap_or(0);
        self.order[(pos + 1) % self.n]
    }

    fn resume_phase(&self) -> Phase {
        if self.rolled {
            Phase::Main
        } else {
            Phase::Roll
        }
    }

    // ------------------------------------------------------------------
    // Queries
    // ------------------------------------------------------------------

    /// Empty vertex with no building on any adjacent vertex (distance rule).
    pub fn settlement_free(&self, v: usize) -> bool {
        self.buildings[v].is_none()
            && self.board.vertices[v]
                .neighbors
                .iter()
                .all(|&n| self.buildings[n].is_none())
    }

    pub fn settlement_connected(&self, p: PlayerId, v: usize) -> bool {
        self.settlement_free(v)
            && self.board.vertices[v]
                .edges
                .iter()
                .any(|&e| self.roads[e] == Some(p))
    }

    /// Edge is empty and touches one of `p`'s buildings or roads
    /// (an opponent's building blocks continuing through its vertex).
    pub fn road_connected(&self, p: PlayerId, e: usize) -> bool {
        if self.roads[e].is_some() {
            return false;
        }
        let edge = &self.board.edges[e];
        for v in [edge.a, edge.b] {
            match self.buildings[v] {
                Some(b) if b.owner == p => return true,
                Some(_) => continue,
                None => {
                    if self.board.vertices[v]
                        .edges
                        .iter()
                        .any(|&o| o != e && self.roads[o] == Some(p))
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Where `p` could build a road: bridge sites are not roads (see `legal_bridges`).
    pub fn legal_roads(&self, p: PlayerId) -> Vec<usize> {
        if self.players[p].roads_left == 0 {
            return vec![];
        }
        (0..self.roads.len())
            .filter(|&e| !self.board.edges[e].bridge && self.road_connected(p, e))
            .collect()
    }

    /// Where `p` could build a bridge: a bridge site that touches their roads or buildings, the way a road
    /// must. Empty when the game has bridges off or `p` has no road pieces left.
    pub fn legal_bridges(&self, p: PlayerId) -> Vec<usize> {
        if !self.settings.bridges || self.players[p].roads_left == 0 {
            return vec![];
        }
        (0..self.roads.len())
            .filter(|&e| self.board.edges[e].bridge && self.road_connected(p, e))
            .collect()
    }

    pub fn legal(&self, p: PlayerId) -> Legal {
        let mut l = Legal::default();
        if p != self.current {
            return l;
        }
        match self.phase {
            Phase::Setup => {
                if let Some(v) = self.setup_vertex {
                    l.roads = self.board.vertices[v]
                        .edges
                        .iter()
                        .copied()
                        .filter(|&e| self.roads[e].is_none() && !self.board.edges[e].bridge)
                        .collect();
                } else {
                    l.settlements = (0..self.buildings.len())
                        .filter(|&v| self.settlement_free(v))
                        .collect();
                }
            }
            Phase::Main => {
                let pl = &self.players[p];
                l.roads = self.legal_roads(p);
                l.bridges = self.legal_bridges(p);
                if pl.settlements_left > 0 {
                    l.settlements = (0..self.buildings.len())
                        .filter(|&v| self.settlement_connected(p, v))
                        .collect();
                }
                if pl.cities_left > 0 {
                    l.cities = (0..self.buildings.len())
                        .filter(|&v| {
                            matches!(self.buildings[v], Some(b) if b.owner == p && !b.city)
                        })
                        .collect();
                }
            }
            Phase::RoadBuilding => l.roads = self.legal_roads(p),
            Phase::MoveRobber => {
                l.hexes = self.robber_hexes(p)
            }
            _ => {}
        }
        l
    }

    /// Hexes where `p` may put the robber. With Friendly Robber on, a hex is off limits
    /// if any other player with `FRIENDLY_ROBBER_MAX_VP` or fewer visible points has a
    /// building on it (unless that would leave nowhere to go).
    pub fn robber_hexes(&self, p: PlayerId) -> Vec<usize> {
        let all: Vec<usize> = (0..self.board.hexes.len()).filter(|&h| h != self.robber).collect();
        if !self.settings.friendly_robber {
            return all;
        }
        let protected = PerPlayer::from_fn(self.n, |q| q != p && self.vp_public(q) <= FRIENDLY_ROBBER_MAX_VP);
        let open: Vec<usize> = all
            .iter()
            .copied()
            .filter(|&h| {
                !self.board.hexes[h]
                    .vertices
                    .iter()
                    .any(|&v| matches!(self.buildings[v], Some(b) if protected[b.owner]))
            })
            .collect();
        if open.is_empty() {
            all
        } else {
            open
        }
    }

    /// Best trade ratio `p` has for giving `res` to the bank: the base ratio from the
    /// settings (4 by default), improved by harbours to 3 or 2 where that is better.
    pub fn ratio(&self, p: PlayerId, res: Resource) -> u8 {
        let mut r = self.settings.bank_ratio;
        for port in &self.board.ports {
            let owned = [port.a, port.b]
                .iter()
                .any(|&v| matches!(self.buildings[v], Some(b) if b.owner == p));
            if !owned {
                continue;
            }
            match port.resource {
                None => r = r.min(3),
                Some(x) if x == res => r = r.min(2),
                _ => {}
            }
        }
        r
    }

    pub fn longest_road(&self, p: PlayerId) -> usize {
        let mut used = vec![false; self.roads.len()];
        let mut best = 0;
        for v in 0..self.board.vertices.len() {
            if self.board.vertices[v]
                .edges
                .iter()
                .any(|&e| self.roads[e] == Some(p))
            {
                best = best.max(self.road_dfs(p, v, &mut used, 0, false));
            }
        }
        best
    }

    /// `longest_road` for every player. The search enumerates every trail in a road network, so it
    /// is the costly part of a view; callers that need all the lengths (the award check, and a
    /// broadcast that builds one view per seat) compute them once with this and share the result.
    pub fn longest_roads(&self) -> PerPlayer<usize> {
        PerPlayer::from_fn(self.n, |p| self.longest_road(p))
    }

    fn road_dfs(&self, p: PlayerId, v: usize, used: &mut Vec<bool>, len: usize, arrived: bool) -> usize {
        // An opponent's settlement/city interrupts a road that arrives at it.
        if arrived {
            if let Some(b) = self.buildings[v] {
                if b.owner != p {
                    return len;
                }
            }
        }
        let mut best = len;
        for &e in &self.board.vertices[v].edges {
            if self.roads[e] == Some(p) && !used[e] {
                used[e] = true;
                let next = self.board.edges[e].other(v);
                best = best.max(self.road_dfs(p, next, used, len + 1, true));
                used[e] = false;
            }
        }
        best
    }

    pub fn vp_public(&self, p: PlayerId) -> usize {
        let mut vp = 0;
        for b in self.buildings.iter().flatten() {
            if b.owner == p {
                vp += if b.city { self.settings.city_points } else { self.settings.settlement_points } as usize;
            }
        }
        if self.longest_road_holder == Some(p) {
            vp += self.settings.longest_road_points as usize;
        }
        if self.largest_army_holder == Some(p) {
            vp += self.settings.largest_army_points as usize;
        }
        vp
    }

    /// The parts of `vp_total(p)`: points from settlements, cities, Victory Point cards and the two awards.
    pub fn vp_breakdown(&self, p: PlayerId) -> VpBreakdown {
        let (mut settlements, mut cities) = (0, 0);
        for b in self.buildings.iter().flatten() {
            if b.owner == p {
                if b.city {
                    cities += self.settings.city_points as usize;
                } else {
                    settlements += self.settings.settlement_points as usize;
                }
            }
        }
        let pl = &self.players[p];
        let cards = pl.dev[DevCard::VictoryPoint] as usize + pl.dev_new[DevCard::VictoryPoint] as usize;
        VpBreakdown {
            settlements,
            cities,
            dev_cards: cards * self.settings.vp_card_points as usize,
            largest_army: if self.largest_army_holder == Some(p) { self.settings.largest_army_points as usize } else { 0 },
            longest_road: if self.longest_road_holder == Some(p) { self.settings.longest_road_points as usize } else { 0 },
        }
    }

    pub fn vp_total(&self, p: PlayerId) -> usize {
        let pl = &self.players[p];
        let cards = pl.dev[DevCard::VictoryPoint] as usize + pl.dev_new[DevCard::VictoryPoint] as usize;
        self.vp_public(p) + cards * self.settings.vp_card_points as usize
    }

    // ------------------------------------------------------------------
    // Awards
    // ------------------------------------------------------------------

    fn update_longest_road(&mut self) {
        let lens = self.longest_roads();
        let max = *lens.iter().max().unwrap();
        let top: Vec<PlayerId> = PlayerId::range(self.n).filter(|&p| lens[p] == max).collect();
        let old = self.longest_road_holder;
        let new = if max < self.settings.longest_road_min as usize {
            None
        } else if old.map_or(false, |h| top.contains(&h)) {
            old
        } else if top.len() == 1 {
            Some(top[0])
        } else {
            None
        };
        if new != old {
            self.longest_road_holder = new;
            match new {
                Some(p) => self.log(LogEvent::LongestRoadTaken { player: p, roads: lens[p] }),
                None => self.log(LogEvent::LongestRoadLost),
            }
        }
    }

    fn update_largest_army(&mut self, p: PlayerId) {
        let k = self.players[p].knights;
        if k < self.settings.largest_army_min || self.largest_army_holder == Some(p) {
            return;
        }
        let beats = match self.largest_army_holder {
            None => true,
            Some(h) => k > self.players[h].knights,
        };
        if beats {
            self.largest_army_holder = Some(p);
            self.log(LogEvent::LargestArmyTaken { player: p, knights: k });
        }
    }

    fn check_win(&mut self) {
        if self.phase == Phase::Setup || self.phase == Phase::GameOver {
            return;
        }
        let p = self.current;
        if self.vp_total(p) >= self.settings.points_to_win as usize {
            self.winner = Some(p);
            self.phase = Phase::GameOver;
            self.trades.clear();
            let points = self.vp_total(p);
            self.log(LogEvent::Won { player: p, points });
        }
    }

    // ------------------------------------------------------------------
    // Actions
    // ------------------------------------------------------------------

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
            Action::RespondTrade { id, accept } => self.do_respond_trade(p, id, accept),
            other => {
                if p != self.current {
                    return Err("It's not your turn".into());
                }
                self.do_current(p, other)
            }
        };
        if res.is_ok() {
            self.check_win();
            self.prune_trades();
            self.sync_clock(now);
            self.apply_bonus(now);
        }
        res
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
            Action::MoveRobber { hex } => self.do_move_robber(p, hex),
            Action::Steal { victim } => self.do_steal(victim),
            Action::BankTrade { give, get } => self.do_bank_trade(p, give, get),
            Action::OfferTrade { give, want } => self.do_offer_trade(p, give, want),
            Action::ConfirmTrade { id, with } => self.do_confirm_trade(p, id, with),
            Action::CancelTrade { id } => self.do_cancel_trade(id),
            Action::EndTurn => self.do_end_turn(),
            Action::Discard { .. } | Action::RespondTrade { .. } => unreachable!(),
        }
    }

    /// Whether `p` is one of the seats at this table. (A `PlayerId` can come straight off the wire, so it is
    /// checked here once rather than trusted.)
    fn has_player(&self, p: PlayerId) -> bool {
        p.index() < self.n
    }

    fn pay(&mut self, p: PlayerId, cost: &Res) {
        for r in Resource::ALL {
            self.players[p].hand[r] -= cost[r];
            self.bank[r] += cost[r];
        }
    }

    fn do_roll(&mut self) -> Result<(), String> {
        if self.phase != Phase::Roll {
            return Err("You can't roll right now".into());
        }
        let mut rng = rand::thread_rng();
        let d = (rng.gen_range(1..=6u8), rng.gen_range(1..=6u8));
        let total = d.0 + d.1;
        self.dice = Some(d);
        self.roll_counts[usize::from(total) - 2] += 1;
        self.rolled = true;
        self.log(LogEvent::Rolled { player: self.current, dice: d });
        self.resolve_roll(total);
        Ok(())
    }

    /// Carry out the result of a roll: production, or the 7 sequence.
    fn resolve_roll(&mut self, total: u8) {
        if total == 7 {
            self.discards = PlayerId::range(self.n)
                .filter_map(|i| {
                    let c = self.players[i].hand.total();
                    if c > self.settings.discard_limit as u32 {
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
                self.log(LogEvent::MustDiscard { limit: self.settings.discard_limit });
            }
        } else {
            self.distribute(total);
            self.phase = Phase::Main;
        }
    }

    fn distribute(&mut self, roll: u8) {
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
            if received[p].total() > 0 {
                self.log(LogEvent::Collected { player: p, cards: received[p] });
            }
        }
    }

    fn do_discard(&mut self, p: PlayerId, cards: Res) -> Result<(), String> {
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
        self.discards.remove(idx);
        self.log(LogEvent::Discarded { player: p, count: need });
        if self.discards.is_empty() {
            self.phase = Phase::MoveRobber;
            self.log(LogEvent::RobberMustMove);
        }
        Ok(())
    }

    fn do_move_robber(&mut self, p: PlayerId, hex: usize) -> Result<(), String> {
        if self.phase != Phase::MoveRobber {
            return Err("You can't move the robber now".into());
        }
        if hex >= self.board.hexes.len() || hex == self.robber {
            return Err("The robber must move to a different hex".into());
        }
        if !self.robber_hexes(p).contains(&hex) {
            return Err(format!(
                "Friendly Robber: players with {} points or fewer can't be robbed",
                FRIENDLY_ROBBER_MAX_VP
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

    fn do_steal(&mut self, victim: PlayerId) -> Result<(), String> {
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
        self.phase = self.resume_phase();
    }

    fn do_build_road(&mut self, p: PlayerId, edge: usize) -> Result<(), String> {
        if edge >= self.roads.len() {
            return Err("Unknown road location".into());
        }
        match self.phase {
            Phase::Setup => {
                let v = self.setup_vertex.ok_or("Place a settlement first")?;
                let e = &self.board.edges[edge];
                if self.roads[edge].is_some() || e.bridge || (e.a != v && e.b != v) {
                    return Err("The road must touch your new settlement".into());
                }
                self.roads[edge] = Some(p);
                self.players[p].roads_left -= 1;
                self.setup_vertex = None;
                self.advance_setup();
                Ok(())
            }
            Phase::RoadBuilding => {
                if self.players[p].roads_left == 0 || self.board.edges[edge].bridge || !self.road_connected(p, edge) {
                    return Err("You can't build a road there".into());
                }
                self.roads[edge] = Some(p);
                self.players[p].roads_left -= 1;
                self.road_building_left -= 1;
                self.bonus += Duration::from_secs(BONUS_SECS);
                self.update_longest_road();
                if self.road_building_left == 0 || self.legal_roads(p).is_empty() {
                    self.road_building_left = 0;
                    self.phase = self.resume_phase();
                }
                Ok(())
            }
            Phase::Main => {
                if self.players[p].roads_left == 0 {
                    return Err("You have no roads left".into());
                }
                if !self.players[p].hand.covers(&COST_ROAD) {
                    return Err("A road costs 1 brick and 1 lumber".into());
                }
                if self.board.edges[edge].bridge {
                    return Err("That is water: a bridge goes there, not a road".into());
                }
                if !self.road_connected(p, edge) {
                    return Err("You can't build a road there".into());
                }
                self.pay(p, &COST_ROAD);
                self.roads[edge] = Some(p);
                self.players[p].roads_left -= 1;
                self.log(LogEvent::BuiltRoad { player: p });
                self.bonus += Duration::from_secs(BONUS_SECS);
                self.update_longest_road();
                Ok(())
            }
            _ => Err("You can't build a road right now".into()),
        }
    }

    /// Build a bridge: a road over water, paid for with `COST_BRIDGE`. Main phase only (so not in setup and
    /// not with a Road Building card). It takes one of the player's road pieces, joins their road network
    /// like any road and counts as one road for Longest Road.
    fn do_build_bridge(&mut self, p: PlayerId, edge: usize) -> Result<(), String> {
        if edge >= self.roads.len() {
            return Err("Unknown bridge location".into());
        }
        if self.phase != Phase::Main {
            return Err("You can't build a bridge right now".into());
        }
        if !self.settings.bridges {
            return Err("Bridges are off in this game".into());
        }
        if self.players[p].roads_left == 0 {
            return Err("You have no roads left".into());
        }
        if !self.board.edges[edge].bridge {
            return Err("A bridge can only be built over water".into());
        }
        if !self.players[p].hand.covers(&COST_BRIDGE) {
            return Err("A bridge costs 1 brick, 1 lumber and 1 ore".into());
        }
        if !self.road_connected(p, edge) {
            return Err("You can't build a bridge there".into());
        }
        self.pay(p, &COST_BRIDGE);
        self.roads[edge] = Some(p);
        self.players[p].roads_left -= 1;
        self.log(LogEvent::BuiltBridge { player: p });
        self.bonus += Duration::from_secs(BONUS_SECS);
        self.update_longest_road();
        Ok(())
    }

    fn do_build_settlement(&mut self, p: PlayerId, v: usize) -> Result<(), String> {
        if v >= self.buildings.len() {
            return Err("Unknown settlement location".into());
        }
        match self.phase {
            Phase::Setup => {
                if self.setup_vertex.is_some() {
                    return Err("Place a road next to your new settlement".into());
                }
                if !self.settlement_free(v) {
                    return Err("You can't build a settlement there".into());
                }
                self.buildings[v] = Some(Building { owner: p, city: false });
                self.players[p].settlements_left -= 1;
                self.setup_vertex = Some(v);
                if self.setup_idx / self.n == self.settings.starting_settlements as usize - 1 {
                    // Each player's last starting settlement: collect one card per adjacent hex.
                    let hexes = self.board.vertices[v].hexes.clone();
                    for h in hexes {
                        if let Some(r) = self.board.hexes[h].terrain.resource() {
                            if self.bank[r] > 0 {
                                self.bank[r] -= 1;
                                self.players[p].hand[r] += 1;
                            }
                        }
                    }
                }
                self.log(LogEvent::PlacedSettlement { player: p });
                Ok(())
            }
            Phase::Main => {
                if self.players[p].settlements_left == 0 {
                    return Err("You have no settlements left".into());
                }
                if !self.players[p].hand.covers(&COST_SETTLEMENT) {
                    return Err("A settlement costs 1 brick, 1 lumber, 1 grain and 1 wool".into());
                }
                if !self.settlement_connected(p, v) {
                    return Err("A settlement must connect to your road and be 2+ steps from any other settlement".into());
                }
                self.pay(p, &COST_SETTLEMENT);
                self.buildings[v] = Some(Building { owner: p, city: false });
                self.players[p].settlements_left -= 1;
                self.log(LogEvent::BuiltSettlement { player: p });
                self.bonus += Duration::from_secs(BONUS_SECS);
                self.update_longest_road();
                Ok(())
            }
            _ => Err("You can't build a settlement right now".into()),
        }
    }

    fn do_build_city(&mut self, p: PlayerId, v: usize) -> Result<(), String> {
        if self.phase != Phase::Main {
            return Err("You can't build a city right now".into());
        }
        if v >= self.buildings.len() {
            return Err("Unknown city location".into());
        }
        if self.players[p].cities_left == 0 {
            return Err("You have no cities left".into());
        }
        if !self.players[p].hand.covers(&COST_CITY) {
            return Err("A city costs 3 ore and 2 grain".into());
        }
        match self.buildings[v] {
            Some(b) if b.owner == p && !b.city => {}
            _ => return Err("A city must replace one of your settlements".into()),
        }
        self.pay(p, &COST_CITY);
        self.buildings[v] = Some(Building { owner: p, city: true });
        self.players[p].cities_left -= 1;
        self.players[p].settlements_left += 1;
        self.log(LogEvent::BuiltCity { player: p });
        self.bonus += Duration::from_secs(BONUS_SECS);
        Ok(())
    }

    fn do_buy_dev(&mut self, p: PlayerId) -> Result<(), String> {
        if self.phase != Phase::Main {
            return Err("You can't buy a development card right now".into());
        }
        if self.deck.is_empty() {
            return Err("The development deck is empty".into());
        }
        if !self.players[p].hand.covers(&COST_DEV) {
            return Err("A development card costs 1 ore, 1 grain and 1 wool".into());
        }
        self.pay(p, &COST_DEV);
        let card = self.deck.pop().unwrap();
        self.players[p].dev_new[card] += 1;
        self.log(LogEvent::BoughtDevCard { player: p });
        self.bonus += Duration::from_secs(BONUS_SECS);
        Ok(())
    }

    fn check_dev_play(&self, p: PlayerId, kind: DevCard) -> Result<(), String> {
        if !matches!(self.phase, Phase::Roll | Phase::Main) {
            return Err("You can't play a development card right now".into());
        }
        if self.dev_played {
            return Err("You can only play one development card per turn".into());
        }
        if self.players[p].dev[kind] == 0 {
            return Err("You don't have that card, or you bought it this turn".into());
        }
        Ok(())
    }

    fn use_dev(&mut self, p: PlayerId, kind: DevCard) {
        self.players[p].dev[kind] -= 1;
        self.dev_played = true;
        self.trades.clear();
    }

    fn do_knight(&mut self, p: PlayerId) -> Result<(), String> {
        self.check_dev_play(p, DevCard::Knight)?;
        self.use_dev(p, DevCard::Knight);
        self.players[p].knights += 1;
        self.log(LogEvent::PlayedKnight { player: p });
        self.update_largest_army(p);
        self.phase = Phase::MoveRobber;
        Ok(())
    }

    fn do_road_building_card(&mut self, p: PlayerId) -> Result<(), String> {
        self.check_dev_play(p, DevCard::RoadBuilding)?;
        if self.legal_roads(p).is_empty() {
            return Err("You have nowhere to build a road".into());
        }
        self.use_dev(p, DevCard::RoadBuilding);
        self.road_building_left = self.players[p].roads_left.min(2);
        self.phase = Phase::RoadBuilding;
        self.log(LogEvent::PlayedRoadBuilding { player: p });
        Ok(())
    }

    fn do_year_of_plenty(&mut self, p: PlayerId, a: Resource, b: Resource) -> Result<(), String> {
        self.check_dev_play(p, DevCard::YearOfPlenty)?;
        let ok = if a == b {
            self.bank[a] >= 2
        } else {
            self.bank[a] >= 1 && self.bank[b] >= 1
        };
        if !ok {
            return Err("The bank doesn't have those resources".into());
        }
        self.use_dev(p, DevCard::YearOfPlenty);
        self.bank[a] -= 1;
        self.bank[b] -= 1;
        self.players[p].hand[a] += 1;
        self.players[p].hand[b] += 1;
        self.log(LogEvent::PlayedYearOfPlenty { player: p, first: a, second: b });
        Ok(())
    }

    fn do_monopoly(&mut self, p: PlayerId, r: Resource) -> Result<(), String> {
        self.check_dev_play(p, DevCard::Monopoly)?;
        self.use_dev(p, DevCard::Monopoly);
        let mut taken = 0u32;
        for o in PlayerId::range(self.n) {
            if o != p {
                let c = self.players[o].hand[r];
                self.players[o].hand[r] = 0;
                self.players[p].hand[r] += c;
                taken += c as u32;
            }
        }
        self.log(LogEvent::PlayedMonopoly { player: p, resource: r, taken });
        Ok(())
    }

    fn do_bank_trade(&mut self, p: PlayerId, give: Resource, get: Resource) -> Result<(), String> {
        if self.phase != Phase::Main {
            return Err("You can only trade after rolling".into());
        }
        if give == get {
            return Err("Pick two different resources".into());
        }
        let ratio = self.ratio(p, give);
        if self.players[p].hand[give] < ratio {
            return Err(format!("You need {} {} for that trade", ratio, give.name()));
        }
        if self.bank[get] == 0 {
            return Err(format!("The bank has no {} left", get.name()));
        }
        self.players[p].hand[give] -= ratio;
        self.bank[give] += ratio;
        self.bank[get] -= 1;
        self.players[p].hand[get] += 1;
        self.log(LogEvent::BankTrade { player: p, give, get, ratio });
        self.bonus += Duration::from_secs(BONUS_SECS);
        Ok(())
    }

    /// Open another offer to every other player. A proposer may have several open at
    /// once (up to `MAX_OPEN_TRADES`), each answered separately.
    fn do_offer_trade(&mut self, p: PlayerId, give: Res, want: Res) -> Result<(), String> {
        if self.phase != Phase::Main {
            return Err("You can only trade after rolling".into());
        }
        if give.total() == 0 || want.total() == 0 {
            return Err("A trade needs cards on both sides".into());
        }
        if Resource::ALL.iter().any(|&r| give[r] > 0 && want[r] > 0) {
            return Err("You can't offer and ask for the same resource".into());
        }
        if !self.players[p].hand.covers(&give) {
            return Err("You don't have those cards".into());
        }
        if self.trades.iter().any(|t| t.give == give && t.want == want) {
            return Err("You already have that offer open".into());
        }
        if self.trades.len() >= MAX_OPEN_TRADES {
            return Err(format!("You can have at most {MAX_OPEN_TRADES} offers open at once"));
        }
        let mut replies = PerPlayer::from_fn(self.n, |_| Reply::Pending);
        replies[p] = Reply::Proposer;
        let id = self.next_trade_id;
        self.next_trade_id += 1;
        self.trades.push(Trade {
            id,
            from: p,
            give,
            want,
            replies,
        });
        self.log(LogEvent::TradeProposed { player: p });
        Ok(())
    }

    fn do_respond_trade(&mut self, p: PlayerId, id: u32, accept: bool) -> Result<(), String> {
        let hand = self.players[p].hand;
        let t = self.trades.iter_mut().find(|t| t.id == id).ok_or("That offer is no longer open")?;
        if t.from == p {
            return Err("You proposed this trade".into());
        }
        if accept && !hand.covers(&t.want) {
            return Err("You don't have the cards to accept".into());
        }
        t.replies[p] = if accept { Reply::Accepted } else { Reply::Declined };
        Ok(())
    }

    /// Complete offer `id` with a player who accepted it. The proposer's other offers stay
    /// open; `prune_trades` then drops any the proposer can no longer pay for.
    fn do_confirm_trade(&mut self, p: PlayerId, id: u32, with: PlayerId) -> Result<(), String> {
        let t = self.trades.iter().find(|t| t.id == id).cloned().ok_or("That offer is no longer open")?;
        if t.from != p {
            return Err("Only the proposer can confirm".into());
        }
        if !self.has_player(with) || t.replies[with] != Reply::Accepted {
            return Err("That player hasn't accepted".into());
        }
        if !self.players[p].hand.covers(&t.give) || !self.players[with].hand.covers(&t.want) {
            return Err("One of you no longer has the cards for this trade".into());
        }
        for r in Resource::ALL {
            self.players[p].hand[r] = self.players[p].hand[r] - t.give[r] + t.want[r];
            self.players[with].hand[r] = self.players[with].hand[r] + t.give[r] - t.want[r];
        }
        self.trades.retain(|o| o.id != id);
        self.log(LogEvent::TradeCompleted { player: p, with });
        self.bonus += Duration::from_secs(BONUS_SECS);
        Ok(())
    }

    fn do_cancel_trade(&mut self, id: u32) -> Result<(), String> {
        if !self.trades.iter().any(|t| t.id == id) {
            return Err("That offer is no longer open".into());
        }
        self.trades.retain(|t| t.id != id);
        Ok(())
    }

    /// Keep the open offers honest after any change to the hands (a completed trade, a bank
    /// trade, building, buying a card): an offer the proposer can no longer pay for is
    /// withdrawn, and a player whose acceptance they can no longer cover is asked again.
    /// Offers only exist during the proposer's own main phase.
    fn prune_trades(&mut self) {
        if self.trades.is_empty() {
            return;
        }
        if self.phase != Phase::Main {
            self.trades.clear();
            return;
        }
        let players = &self.players;
        self.trades.retain(|t| players[t.from].hand.covers(&t.give));
        for t in &mut self.trades {
            for q in PlayerId::range(players.len()) {
                if t.replies[q] == Reply::Accepted && !players[q].hand.covers(&t.want) {
                    t.replies[q] = Reply::Pending;
                }
            }
        }
    }

    fn do_end_turn(&mut self) -> Result<(), String> {
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
        self.rolled = false;
        self.dice = None;
        self.current = self.next_player(p);
        self.phase = Phase::Roll;
        // A fresh turn: full turn clock, full roll window.
        self.turn_left = Duration::from_secs(self.timer_mode.secs());
        self.roll_left = None;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Timers
    // ------------------------------------------------------------------

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
            Phase::Main | Phase::RoadBuilding => Some(ClockKind::Turn),
            Phase::GameOver => None,
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
            ClockKind::Turn => self.timer_mode.secs(),
        })
    }

    /// Bring the running countdown in line with the game state. Called after every
    /// state change. Only the turn clock and the roll clock are paused and resumed
    /// (around a Knight); every other countdown starts fresh when its phase begins.
    fn sync_clock(&mut self, now: Instant) {
        let want = self.clock_kind().map(|k| (self.current, k));
        let have = self.clock.map(|c| (c.player, c.kind));
        if want == have {
            return;
        }
        if let Some(c) = self.clock.take() {
            // Save what is left, but only within the same turn (a new turn resets both).
            if c.player == self.current {
                let left = c.deadline.saturating_duration_since(now);
                match c.kind {
                    ClockKind::Turn => self.turn_left = left,
                    ClockKind::Roll => self.roll_left = Some(left),
                    _ => {}
                }
            }
        }
        if let Some((player, kind)) = want {
            let total = self.full_budget(kind);
            let left = match kind {
                ClockKind::Turn => self.turn_left,
                ClockKind::Roll => self.roll_left.take().unwrap_or(total),
                _ => total,
            };
            self.clock = Some(Clock { player, kind, deadline: now + left, total });
        }
    }

    /// Add the bonus earned by the action just applied to the turn clock, never
    /// letting it exceed the mode's starting time.
    fn apply_bonus(&mut self, now: Instant) {
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

    /// Fire the running countdown if it has run out, taking the automatic action for
    /// the player who timed out. Returns true if the game changed (so the caller
    /// should broadcast).
    pub fn tick(&mut self, now: Instant) -> bool {
        let Some(c) = self.clock else { return false };
        if now < c.deadline {
            return false;
        }
        let p = c.player;
        self.log(LogEvent::TimedOut { player: p, kind: c.kind });
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
                    .filter(|&h| matches!(self.board.hexes[h].number, Some(6) | Some(8)))
                    .collect();
                // Normally one of the four 6/8 hexes; any allowed hex if Friendly Robber rules those out.
                let target = hot.choose(&mut rng).copied().or_else(|| allowed.choose(&mut rng).copied());
                if let Some(h) = target {
                    let _ = self.do_move_robber(p, h);
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
        // Safety net: if the action somehow left the same countdown running, restart
        // it rather than firing again on the next tick.
        if let Some(n) = self.clock {
            if n.player == c.player && n.kind == c.kind && n.deadline <= now {
                self.clock = Some(Clock { deadline: now + n.total, ..n });
            }
        }
        true
    }

    // ------------------------------------------------------------------
    // Views
    // ------------------------------------------------------------------

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
                }
            })
            .collect();
        let start = self.log.len().saturating_sub(50);
        let now = Instant::now();
        GameView {
            you,
            settings: self.settings,
            board: &self.board,
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
            dev_played: self.dev_played,
            ratios: ByResource::from_fn(|r| self.ratio(you, r)),
            setup_needs: if self.phase == Phase::Setup {
                Some(if self.setup_vertex.is_some() { "road" } else { "settlement" })
            } else {
                None
            },
            discards: &self.discards,
            steal_candidates: &self.steal_candidates,
            road_building_left: self.road_building_left,
            trades: &self.trades,
            legal: self.legal(you),
            longest_road_holder: self.longest_road_holder,
            largest_army_holder: self.largest_army_holder,
            winner: self.winner,
            log: &self.log[start..],
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

#[cfg(test)]
mod tests {
    use super::*;

    const P0: PlayerId = PlayerId::new(0);
    const P1: PlayerId = PlayerId::new(1);
    const P2: PlayerId = PlayerId::new(2);

    fn all_connected(n: usize) -> PerPlayer<bool> {
        PerPlayer::from_fn(n, |_| true)
    }

    fn random_resource(rng: &mut impl Rng) -> Resource {
        *Resource::ALL.choose(rng).unwrap()
    }

    fn names(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("P{i}")).collect()
    }

    fn pick<T: Copy>(v: &[T], rng: &mut impl Rng) -> Option<T> {
        v.choose(rng).copied()
    }

    /// Finish the setup phase by placing at random legal spots.
    fn run_setup(g: &mut Game, rng: &mut impl Rng) {
        while g.phase == Phase::Setup {
            let p = g.current;
            let l = g.legal(p);
            if g.setup_vertex.is_some() {
                g.apply(p, Action::BuildRoad { edge: pick(&l.roads, rng).unwrap() }).unwrap();
            } else {
                g.apply(p, Action::BuildSettlement { vertex: pick(&l.settlements, rng).unwrap() })
                    .unwrap();
            }
        }
    }

    #[test]
    fn setup_order_and_resources() {
        let mut rng = rand::thread_rng();
        let mut g = Game::new(names(3));
        let mut order = vec![];
        while g.phase == Phase::Setup {
            let p = g.current;
            if g.setup_vertex.is_none() {
                order.push(p);
            }
            let l = g.legal(p);
            if g.setup_vertex.is_some() {
                g.apply(p, Action::BuildRoad { edge: pick(&l.roads, &mut rng).unwrap() }).unwrap();
            } else {
                g.apply(p, Action::BuildSettlement { vertex: pick(&l.settlements, &mut rng).unwrap() })
                    .unwrap();
            }
        }
        assert_eq!(order, vec![P0, P1, P2, P2, P1, P0]);
        assert_eq!(g.phase, Phase::Roll);
        assert_eq!(g.current, P0);
        for p in PlayerId::range(3) {
            assert_eq!(g.players[p].settlements_left, 3);
            assert_eq!(g.players[p].roads_left, 13);
        }
    }

    #[test]
    fn not_your_turn_is_rejected() {
        let mut g = Game::new(names(2));
        assert!(g.apply(P1, Action::BuildSettlement { vertex: 0 }).is_err());
        assert!(g.apply(P0, Action::Roll).is_err());
    }

    #[test]
    fn distance_rule_enforced() {
        let mut g = Game::new(names(2));
        g.apply(P0, Action::BuildSettlement { vertex: 0 }).unwrap();
        let e = g.board.vertices[0].edges[0];
        g.apply(P0, Action::BuildRoad { edge: e }).unwrap();
        let nb = g.board.vertices[0].neighbors[0];
        assert!(g.apply(P1, Action::BuildSettlement { vertex: nb }).is_err());
    }

    fn line_of_roads(g: &mut Game, p: PlayerId, count: usize) -> Vec<usize> {
        // Find a simple path of `count` edges by DFS and give it to player p.
        fn dfs(g: &Game, v: usize, used: &mut Vec<usize>, count: usize) -> bool {
            if used.len() == count {
                return true;
            }
            for &e in &g.board.vertices[v].edges {
                if used.contains(&e) {
                    continue;
                }
                let next = g.board.edges[e].other(v);
                // keep path simple: don't revisit vertices
                let visited = used.iter().any(|&u| g.board.edges[u].a == next || g.board.edges[u].b == next);
                if visited {
                    continue;
                }
                used.push(e);
                if dfs(g, next, used, count) {
                    return true;
                }
                used.pop();
            }
            false
        }
        for start in 0..g.board.vertices.len() {
            let mut used = vec![];
            if dfs(g, start, &mut used, count) {
                for &e in &used {
                    g.roads[e] = Some(p);
                }
                return used;
            }
        }
        panic!("no path");
    }

    #[test]
    fn longest_road_award_and_break() {
        let mut g = Game::new(names(3));
        g.phase = Phase::Main;
        let path = line_of_roads(&mut g, P0, 5);
        assert_eq!(g.longest_road(P0), 5);
        g.update_longest_road();
        assert_eq!(g.longest_road_holder, Some(P0));
        assert_eq!(g.vp_public(P0), 2);

        // An opponent settlement in the middle of the path breaks it.
        let mid = {
            let (e1, e2) = (&g.board.edges[path[2]], &g.board.edges[path[3]]);
            [e1.a, e1.b].into_iter().find(|v| *v == e2.a || *v == e2.b).unwrap()
        };
        g.buildings[mid] = Some(Building { owner: P1, city: false });
        assert!(g.longest_road(P0) < 5);
        g.update_longest_road();
        assert_eq!(g.longest_road_holder, None);
    }

    #[test]
    fn longest_roads_match_the_per_player_search_and_feed_every_view() {
        let mut g = Game::new(names(3));
        g.phase = Phase::Main;
        let path = line_of_roads(&mut g, P0, 5);
        // One road for P1 on the last edge, which the search for P0's path (from vertex 0) never reaches.
        let far = g.roads.len() - 1;
        assert!(!path.contains(&far));
        g.roads[far] = Some(P1);

        let lens = g.longest_roads();
        for p in PlayerId::range(3) {
            assert_eq!(lens[p], g.longest_road(p));
        }
        assert_eq!((lens[P0], lens[P1], lens[P2]), (5, 1, 0));

        // Every seat's view reports the shared lengths, even though no award update has run.
        for you in PlayerId::range(3) {
            let v = g.view(you, &all_connected(3), &lens);
            for p in PlayerId::range(3) {
                assert_eq!(v.players[p.index()].longest_road, lens[p]);
            }
        }
    }

    #[test]
    fn largest_army_requires_three_and_must_be_beaten() {
        let mut g = Game::new(names(3));
        g.players[P0].knights = 3;
        g.update_largest_army(P0);
        assert_eq!(g.largest_army_holder, Some(P0));
        g.players[P1].knights = 3;
        g.update_largest_army(P1);
        assert_eq!(g.largest_army_holder, Some(P0));
        g.players[P1].knights = 4;
        g.update_largest_army(P1);
        assert_eq!(g.largest_army_holder, Some(P1));
    }

    #[test]
    fn bank_trade_ratios_with_ports() {
        let mut g = Game::new(names(2));
        assert_eq!(g.ratio(P0, Resource::Brick), 4);
        let port = g.board.ports[0].clone();
        g.buildings[port.a] = Some(Building { owner: P0, city: false });
        match port.resource {
            None => assert_eq!(g.ratio(P0, Resource::Brick), 3),
            Some(r) => {
                assert_eq!(g.ratio(P0, r), 2);
                let other = Resource::from_index((r.index() + 1) % 5).unwrap();
                assert_eq!(g.ratio(P0, other), 4);
            }
        }
    }

    #[test]
    fn robber_blocks_production() {
        for _ in 0..30 {
            let mut g = Game::new(names(2));
            // Find a hex + corner where no other hex touching that corner shares the number,
            // so the robber on that hex must stop all production for the corner.
            let (h, v) = (0..19)
                .filter(|&h| g.board.hexes[h].number.is_some())
                .flat_map(|h| g.board.hexes[h].vertices.map(|v| (h, v)))
                .find(|&(h, v)| {
                    g.board.vertices[v]
                        .hexes
                        .iter()
                        .all(|&o| o == h || g.board.hexes[o].number != g.board.hexes[h].number)
                })
                .expect("some corner always qualifies");
            g.buildings[v] = Some(Building { owner: P0, city: true });
            let roll = g.board.hexes[h].number.unwrap();
            g.robber = h;
            g.distribute(roll);
            assert_eq!(g.players[P0].hand.total(), 0, "robber must block the hex");
            g.robber = g.board.desert;
            g.distribute(roll);
            assert_eq!(g.players[P0].hand.total(), 2, "a city on one producing hex pays 2");
        }
    }

    fn invariants(g: &Game) {
        for r in Resource::ALL {
            let held: u32 = g.players.iter().map(|p| p.hand[r] as u32).sum();
            assert_eq!(held + g.bank[r] as u32, g.settings.resource_max as u32, "{} not conserved", r.name());
        }
        for p in PlayerId::range(g.n) {
            let roads = g.roads.iter().filter(|r| **r == Some(p)).count() as u8;
            assert_eq!(roads + g.players[p].roads_left, g.settings.max_roads);
            let s = g.buildings.iter().flatten().filter(|b| b.owner == p && !b.city).count() as u8;
            let c = g.buildings.iter().flatten().filter(|b| b.owner == p && b.city).count() as u8;
            assert_eq!(s + g.players[p].settlements_left, g.settings.max_settlements);
            assert_eq!(c + g.players[p].cities_left, g.settings.max_cities);
        }
        // Open offers: only during the current player's main phase, always payable by the
        // proposer, never more than the cap, ids unique, accepters always able to pay.
        if !g.trades.is_empty() {
            assert_eq!(g.phase, Phase::Main, "offers outlive the main phase");
            assert!(g.trades.len() <= MAX_OPEN_TRADES);
            let mut ids: Vec<u32> = g.trades.iter().map(|t| t.id).collect();
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), g.trades.len(), "trade ids are unique");
            for t in &g.trades {
                assert_eq!(t.from, g.current);
                assert!(g.players[t.from].hand.covers(&t.give), "proposer cannot pay for an open offer");
                for q in PlayerId::range(g.n) {
                    if t.replies[q] == Reply::Accepted {
                        assert!(g.players[q].hand.covers(&t.want), "an accepter cannot pay");
                    }
                }
            }
        }
    }

    /// Play one game to the end (or 6000 steps) with random legal moves, checking the
    /// invariants all the way. The harness occasionally moves cards from the bank to a
    /// player (conserving totals) so that games progress quickly.
    fn play_random(g: &mut Game, rng: &mut impl Rng) {
        // Timeouts are simulated by jumping a fake clock far into the future.
        let base = Instant::now();
        let mut fake = 0u64;
        run_setup(g, rng);
        for _step in 0..6000 {
            invariants(g);
            if rng.gen_bool(0.03) {
                fake += 1000;
                g.tick(base + secs(fake));
                invariants(g);
            }
            let p = g.current;
            match g.phase {
                Phase::GameOver => break,
                Phase::Setup => unreachable!(),
                Phase::Roll => {
                    // cheat: top up the current hand from the bank
                    for _ in 0..3 {
                        let r = random_resource(rng);
                        if g.bank[r] > 0 {
                            g.bank[r] -= 1;
                            g.players[p].hand[r] += 1;
                        }
                    }
                    if rng.gen_bool(0.2) {
                        let _ = g.apply(p, Action::PlayKnight);
                    }
                    if g.phase == Phase::Roll {
                        g.apply(p, Action::Roll).unwrap();
                    }
                }
                Phase::Discard => {
                    let pending = g.discards.clone();
                    for (q, k) in pending {
                        let mut cards = Res::ZERO;
                        let mut left = k;
                        while left > 0 {
                            let r = random_resource(rng);
                            if g.players[q].hand[r] > cards[r] {
                                cards[r] += 1;
                                left -= 1;
                            }
                        }
                        g.apply(q, Action::Discard { cards }).unwrap();
                    }
                }
                Phase::MoveRobber => {
                    let l = g.legal(p);
                    g.apply(p, Action::MoveRobber { hex: pick(&l.hexes, rng).unwrap() })
                        .unwrap();
                }
                Phase::Steal => {
                    let v = pick(&g.steal_candidates.clone(), rng).unwrap();
                    g.apply(p, Action::Steal { victim: v }).unwrap();
                }
                Phase::RoadBuilding => {
                    let l = g.legal(p);
                    g.apply(p, Action::BuildRoad { edge: pick(&l.roads, rng).unwrap() })
                        .unwrap();
                }
                Phase::Main => {
                    let l = g.legal(p);
                    match rng.gen_range(0..14) {
                        0..=2 => {
                            if let Some(v) = pick(&l.cities, rng) {
                                let _ = g.apply(p, Action::BuildCity { vertex: v });
                            }
                        }
                        3..=5 => {
                            if let Some(v) = pick(&l.settlements, rng) {
                                let _ = g.apply(p, Action::BuildSettlement { vertex: v });
                            }
                        }
                        6..=7 => {
                            if let Some(e) = pick(&l.bridges, rng).filter(|_| rng.gen_bool(0.5)) {
                                let _ = g.apply(p, Action::BuildBridge { edge: e });
                            } else if let Some(e) = pick(&l.roads, rng) {
                                let _ = g.apply(p, Action::BuildRoad { edge: e });
                            }
                        }
                        8 => {
                            let _ = g.apply(p, Action::BuyDev);
                        }
                        9 => {
                            let a = match rng.gen_range(0..4) {
                                0 => Action::PlayKnight,
                                1 => Action::PlayRoadBuilding,
                                2 => Action::PlayYearOfPlenty { a: random_resource(rng), b: random_resource(rng) },
                                _ => Action::PlayMonopoly { resource: random_resource(rng) },
                            };
                            let _ = g.apply(p, a);
                        }
                        10 => {
                            let _ = g.apply(p, Action::BankTrade { give: random_resource(rng), get: random_resource(rng) });
                        }
                        11 => {
                            // Several offers at once, answered and confirmed in random order.
                            for _ in 0..rng.gen_range(1..=3) {
                                let mut give = Res::ZERO;
                                let mut want = Res::ZERO;
                                give[random_resource(rng)] = rng.gen_range(1..3);
                                want[random_resource(rng)] = rng.gen_range(1..3);
                                let _ = g.apply(p, Action::OfferTrade { give, want });
                            }
                            let ids: Vec<u32> = g.trades.iter().map(|t| t.id).collect();
                            for id in ids {
                                for q in PlayerId::range(g.n) {
                                    if q != p {
                                        let _ = g.apply(q, Action::RespondTrade { id, accept: rng.gen_bool(0.5) });
                                    }
                                }
                                invariants(g);
                                let _ = g.apply(p, Action::ConfirmTrade { id, with: PlayerId::new(rng.gen_range(0..g.n)) });
                                invariants(g);
                                if rng.gen_bool(0.5) {
                                    let _ = g.apply(p, Action::CancelTrade { id });
                                }
                            }
                        }
                        _ => {
                            if rng.gen_bool(0.5) {
                                g.apply(p, Action::EndTurn).unwrap();
                            }
                        }
                    }
                }
            }
        }
    }

    /// Random-play fuzzing. The harness occasionally moves cards from the bank
    /// to a player (conserving totals) so that games progress quickly.
    #[test]
    fn random_games_keep_invariants() {
        let mut rng = rand::thread_rng();
        let mut finished = 0;
        for game_no in 0..60 {
            let n = 2 + game_no % 3;
            // Half the games use a shuffled order and the real constructor.
            let mut g = if game_no % 2 == 0 {
                let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
                Game::with_options(names(n), colors, TimerMode::VeryFast, Settings::default(), "standard")
            } else {
                Game::new(names(n))
            };
            play_random(&mut g, &mut rng);
            invariants(&g);
            if g.phase == Phase::GameOver {
                finished += 1;
                let w = g.winner.unwrap();
                assert!(g.vp_total(w) >= g.settings.points_to_win as usize);
            }
        }
        assert!(finished > 10, "only {finished} of 60 random games finished");
    }

    // ------------------------------------------------------------------
    // Maps
    // ------------------------------------------------------------------

    #[test]
    fn the_map_decides_the_board_and_where_the_robber_starts() {
        let std = Game::new(names(2));
        assert_eq!(std.board.map, "standard");
        assert_eq!(std.board.hexes.len(), 19);
        assert!(!std.log.iter().any(|l| matches!(l, LogEvent::MapChosen { .. })), "the standard map isn't announced");

        for _ in 0..30 {
            let g = Game::with_map(names(3), "earth");
            assert_eq!(g.board.map, "earth");
            assert_eq!(g.board.hexes.len(), 81);
            assert_eq!(g.board.ports.len(), 27);
            assert_eq!(g.buildings.len(), g.board.vertices.len());
            assert_eq!(g.roads.len(), g.board.edges.len());
            assert_eq!(g.board.hexes[g.robber].terrain, Terrain::Desert);
            assert!(g.log.contains(&LogEvent::MapChosen { map: "earth".into(), name: "Earth".into() }));
        }
    }

    #[test]
    fn earth_games_can_be_played_to_the_end() {
        let mut rng = rand::thread_rng();
        let mut finished = 0;
        for game_no in 0..40 {
            let n = 2 + game_no % 3;
            let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
            let mut g = Game::with_options(names(n), colors, TimerMode::VeryFast, Settings::default(), "earth");
            play_random(&mut g, &mut rng);
            invariants(&g);
            if g.phase == Phase::GameOver {
                finished += 1;
                let w = g.winner.unwrap();
                assert!(g.vp_total(w) >= g.settings.points_to_win as usize);
            }
        }
        assert!(finished > 5, "only {finished} of 40 random Earth games finished");
    }

    fn archipelago() -> MapDef {
        crate::builtin::archipelago()
    }

    fn start_on(def: &MapDef, n: usize) -> Result<Game, MapError> {
        let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
        Game::try_with_def(names(n), colors, TimerMode::VeryFast, Settings::default(), "archipelago", def)
    }

    #[test]
    fn a_map_written_as_json_can_be_played_to_the_end() {
        let def = archipelago();
        let mut rng = rand::thread_rng();
        let mut finished = 0;
        for game_no in 0..40 {
            let n = 2 + game_no % 3;
            let mut g = start_on(&def, n).expect("the map can be played with 2, 3 or 4 players");
            assert_eq!(g.board.hexes.len(), 14);
            assert_eq!(g.board.ports.len(), 7);
            // the written-in parts are where the file put them
            let hex = |q, r| g.board.hexes.iter().find(|h| (h.q, h.r) == (q, r)).unwrap();
            assert_eq!((hex(1, 1).terrain, hex(1, 1).number), (Terrain::Mountains, Some(8)));
            assert_eq!(hex(6, 0).terrain, Terrain::Desert);
            play_random(&mut g, &mut rng);
            invariants(&g);
            if g.phase == Phase::GameOver {
                finished += 1;
                let w = g.winner.unwrap();
                assert!(g.vp_total(w) >= g.settings.points_to_win as usize);
            }
        }
        assert!(finished > 5, "only {finished} of 40 random games on the custom map finished");
    }

    #[test]
    fn the_robber_starts_where_the_map_says_or_on_a_desert() {
        let mut def = archipelago();
        for _ in 0..30 {
            let g = start_on(&def, 3).unwrap();
            assert_eq!(g.board.hexes[g.robber].terrain, Terrain::Desert);
        }
        def.robber_start = Some(crate::map_def::HexRef { q: 2, r: 1 });
        let g = start_on(&def, 3).unwrap();
        assert_eq!((g.board.hexes[g.robber].q, g.board.hexes[g.robber].r), (2, 1));
    }

    #[test]
    fn a_map_is_only_started_for_the_players_it_is_meant_for() {
        let mut def = archipelago();
        def.players = crate::map_def::PlayerRange { min: 3, max: 4 };
        assert_eq!(start_on(&def, 2).err(), Some(MapError::PlayerCount { players: 2, min: 3, max: 4 }));
        assert!(start_on(&def, 3).is_ok());
        assert!(start_on(&def, 4).is_ok());
        def.players = crate::map_def::PlayerRange { min: 2, max: 2 };
        assert_eq!(start_on(&def, 3).err(), Some(MapError::PlayerCount { players: 3, min: 2, max: 2 }));
        // a range the engine can't play is the map's mistake, whatever the lobby holds
        def.players = crate::map_def::PlayerRange { min: 0, max: 9 };
        assert_eq!(start_on(&def, 3).err(), Some(MapError::BadPlayerRange { min: 0, max: 9 }));
    }

    #[test]
    fn a_map_that_cannot_be_built_is_an_error_not_a_panic() {
        let mut def = archipelago();
        def.token_pool.clear(); // 11 hexes want a token
        let e = start_on(&def, 3).err().expect("refused");
        assert_eq!(e, MapError::TokenPoolMismatch { needed: 11, pool: 0 });
        assert!(e.to_string().contains("11"), "{e}");

        let mut def = archipelago();
        def.ports = crate::map_def::PortsDef::Fixed { spots: vec![crate::map_def::PortSpot::at(1, 1, 0)], pool: [(crate::map_def::PortKind::Any, 1)].into() };
        assert!(matches!(start_on(&def, 3).err(), Some(MapError::PortNotOnCoast { q: 1, r: 1, side: 0 })));
    }

    #[test]
    fn the_built_in_maps_start_through_the_fallible_path_too() {
        for map in ["standard", "earth"] {
            let colors = PALETTE.iter().take(3).map(|c| c.to_string()).collect();
            let g = Game::try_with_options(names(3), colors, TimerMode::Normal, Settings::default(), map).unwrap();
            assert_eq!(g.board.map, map);
        }
    }

    #[test]
    fn players_keep_their_chosen_colors() {
        let g = Game::with_colors(names(2), vec!["#000000".into(), "#FFFFFF".into()]);
        let v = g.view(P0, &all_connected(2), &g.longest_roads());
        assert_eq!(v.players[0].color, "#000000");
        assert_eq!(v.players[1].color, "#FFFFFF");
        // `Game::new` hands out palette colors in seat order
        assert_eq!(Game::new(names(3)).colors, PerPlayer::from(vec![PALETTE[0].to_string(), PALETTE[1].to_string(), PALETTE[2].to_string()]));
    }

    #[test]
    fn palette_lookup_is_exact_but_ignores_case() {
        assert_eq!(palette_color("#7bafd4"), Some("#7BAFD4"));
        assert_eq!(palette_color("69BE28"), None); // the leading # is required
        assert_eq!(palette_color("#123456"), None);
        let mut all = PALETTE.to_vec();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), PALETTE.len(), "palette has no duplicates");
    }

    // ------------------------------------------------------------------
    // Turn order and timers
    // ------------------------------------------------------------------

    fn secs(d: u64) -> Duration {
        Duration::from_secs(d)
    }

    /// Seconds left on the running clock at `now`.
    fn left(g: &Game, now: Instant) -> f64 {
        g.clock.unwrap().deadline.saturating_duration_since(now).as_secs_f64()
    }

    /// A started game of `n` players in the given mode with a fixed clock reading.
    fn timed(n: usize, mode: TimerMode, t0: Instant) -> Game {
        let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
        Game::build(names(n), colors, mode, Settings::default(), "standard", false, t0)
    }

    /// Put the game straight into the main phase of player `p`'s turn at `t`.
    fn into_main(g: &mut Game, p: PlayerId, t: Instant) {
        g.phase = Phase::Main;
        g.current = p;
        g.rolled = true;
        g.sync_clock(t);
    }

    #[test]
    fn draft_order_is_random_and_drives_setup_and_turns() {
        let mut rng = rand::thread_rng();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..60 {
            let colors: Vec<String> = PALETTE.iter().take(3).map(|c| c.to_string()).collect();
            let mut g = Game::with_options(names(3), colors, TimerMode::Fast, Settings::default(), "standard");
            let mut sorted = g.order.clone();
            sorted.sort();
            assert_eq!(sorted, vec![P0, P1, P2], "order is a permutation of the seats");
            seen.insert(g.order.clone());

            let o = g.order.clone();
            assert_eq!(g.current, o[0]);
            let mut placed = vec![];
            while g.phase == Phase::Setup {
                if g.setup_vertex.is_none() {
                    placed.push(g.current);
                }
                let p = g.current;
                let l = g.legal(p);
                if g.setup_vertex.is_some() {
                    g.apply(p, Action::BuildRoad { edge: pick(&l.roads, &mut rng).unwrap() }).unwrap();
                } else {
                    g.apply(p, Action::BuildSettlement { vertex: pick(&l.settlements, &mut rng).unwrap() }).unwrap();
                }
            }
            assert_eq!(placed, vec![o[0], o[1], o[2], o[2], o[1], o[0]]);
            assert_eq!(g.current, o[0], "the first placer also takes the first turn");
            // turns follow the same order
            g.apply(o[0], Action::Roll).unwrap();
            if g.phase == Phase::Main {
                g.apply(o[0], Action::EndTurn).unwrap();
                assert_eq!(g.current, o[1]);
            }
        }
        assert!(seen.len() > 1, "60 shuffled games all had the same order");
    }

    #[test]
    fn timer_modes_have_the_specified_lengths() {
        assert_eq!(TimerMode::VeryFast.secs(), 30);
        assert_eq!(TimerMode::Fast.secs(), 60);
        assert_eq!(TimerMode::Normal.secs(), 120);
        assert_eq!(TimerMode::Slow.secs(), 240);
        let m: TimerMode = serde_json::from_str("\"very_fast\"").unwrap();
        assert_eq!(m, TimerMode::VeryFast);
    }

    #[test]
    fn setup_gives_120s_for_a_settlement_then_30s_for_a_road_in_every_mode() {
        for mode in [TimerMode::VeryFast, TimerMode::Fast, TimerMode::Normal, TimerMode::Slow] {
            let t0 = Instant::now();
            let mut g = timed(3, mode, t0);
            let first = g.current;
            assert_eq!(g.clock.unwrap().kind, ClockKind::SetupSettlement);
            assert_eq!(g.clock.unwrap().total, secs(120));
            assert!(!g.tick(t0 + secs(119)), "nothing happens before the deadline");
            assert!(g.setup_vertex.is_none());

            // time out on the settlement: one is placed, and the road clock is 30s
            assert!(g.tick(t0 + secs(121)));
            assert!(g.setup_vertex.is_some(), "a settlement was auto-placed");
            assert_eq!(g.current, first);
            let c = g.clock.unwrap();
            assert_eq!(c.kind, ClockKind::SetupRoad);
            assert_eq!(c.total, secs(30));
            assert!((left(&g, t0 + secs(121)) - 30.0).abs() < 0.01);

            // time out on the road: it is placed and the next player starts at 120s
            assert!(!g.tick(t0 + secs(150)));
            assert!(g.tick(t0 + secs(152)));
            assert!(g.setup_vertex.is_none());
            assert_ne!(g.current, first);
            assert_eq!(g.clock.unwrap().kind, ClockKind::SetupSettlement);
            assert_eq!(g.clock.unwrap().total, secs(120));
            assert_eq!(g.roads.iter().flatten().count(), 1);
        }
    }

    #[test]
    fn dice_roll_automatically_after_10s_and_are_not_on_the_turn_clock() {
        let t0 = Instant::now();
        let mut g = timed(2, TimerMode::VeryFast, t0);
        g.phase = Phase::Roll;
        g.rolled = false;
        g.sync_clock(t0);
        let c = g.clock.unwrap();
        assert_eq!((c.kind, c.total), (ClockKind::Roll, secs(10)));
        assert!(!g.tick(t0 + secs(9)));
        assert!(g.dice.is_none());
        assert!(g.tick(t0 + secs(11)), "auto-roll");
        assert!(g.dice.is_some() && g.rolled);
        // Unless that roll was a 7, the turn clock now starts from its full value,
        // because the 10 seconds spent rolling were not charged to it.
        if let Some(c) = g.clock {
            if c.kind == ClockKind::Turn {
                assert!((left(&g, t0 + secs(11)) - 30.0).abs() < 0.01);
            }
        }
    }

    #[test]
    fn a_player_who_rolls_in_time_keeps_the_roll_and_full_turn_clock() {
        let t0 = Instant::now();
        let mut g = timed(2, TimerMode::Fast, t0);
        g.phase = Phase::Roll;
        g.sync_clock(t0);
        g.apply_at(g.current, Action::Roll, t0 + secs(4)).unwrap();
        if g.phase == Phase::Main {
            assert!((left(&g, t0 + secs(4)) - 60.0).abs() < 0.01);
        }
    }

    #[test]
    fn every_roll_is_counted_by_its_total() {
        let t0 = Instant::now();
        let mut g = timed(2, TimerMode::Normal, t0);
        assert_eq!(g.roll_counts, [0; 11]);
        let mut rolls = 0u32;
        for _ in 0..20 {
            g.phase = Phase::Roll;
            g.rolled = false;
            g.sync_clock(t0);
            g.apply_at(g.current, Action::Roll, t0).unwrap();
            let (a, b) = g.dice.unwrap();
            rolls += 1;
            assert!(g.roll_counts[usize::from(a + b) - 2] >= 1);
            assert_eq!(g.roll_counts.iter().sum::<u32>(), rolls);
        }
    }

    #[test]
    fn seven_sequence_has_its_own_clocks_and_leaves_the_turn_clock_untouched() {
        let t0 = Instant::now();
        let mut g = timed(3, TimerMode::Normal, t0);
        g.phase = Phase::Roll;
        g.current = P0;
        g.sync_clock(t0);
        // players 0 and 1 are over the limit; player 2 is not
        g.players[P0].hand = Res::new(3, 3, 3, 0, 0); // 9 -> discard 4
        g.players[P1].hand = Res::new(2, 2, 2, 2, 0); // 8 -> discard 4
        g.players[P2].hand = Res::new(1, 1, 0, 0, 0);
        g.dice = Some((3, 4));
        g.rolled = true;
        g.resolve_roll(7);
        g.sync_clock(t0 + secs(5));
        let c = g.clock.unwrap();
        assert_eq!((g.phase, c.kind, c.total), (Phase::Discard, ClockKind::Discard, secs(20)));

        // one player discards in time, the other does not
        g.apply_at(P1, Action::Discard { cards: Res::new(1, 1, 1, 1, 0) }, t0 + secs(12)).unwrap();
        assert_eq!(g.phase, Phase::Discard);
        assert!(!g.tick(t0 + secs(24)), "20s from the 7, not from the first discard");
        assert!(g.tick(t0 + secs(26)), "20s after the 7 the rest are discarded at random");
        assert_eq!(g.players[P0].hand.total(), 5);
        assert_eq!(g.players[P1].hand.total(), 4);
        assert_eq!(g.phase, Phase::MoveRobber);
        let c = g.clock.unwrap();
        assert_eq!((c.kind, c.total), (ClockKind::Robber, secs(20)));

        // robber: 20s, then it goes on a 6 or an 8
        let before = g.robber;
        assert!(!g.tick(t0 + secs(45)));
        assert!(g.tick(t0 + secs(47)));
        assert_ne!(g.robber, before);
        assert!(matches!(g.board.hexes[g.robber].number, Some(6) | Some(8)));
        // the turn clock was never started, so it is still the full 120s whichever
        // phase (steal or main) comes next
        if g.phase == Phase::Steal {
            let c = g.clock.unwrap();
            assert_eq!((c.kind, c.total), (ClockKind::Steal, secs(15)));
            assert!(g.tick(t0 + secs(63)));
        }
        assert_eq!(g.phase, Phase::Main);
        let now = t0 + secs(100);
        g.sync_clock(now);
        assert_eq!(g.turn_left, secs(120));
    }

    #[test]
    fn steal_victim_is_chosen_at_random_after_15s() {
        let t0 = Instant::now();
        let mut g = timed(3, TimerMode::Normal, t0);
        into_main(&mut g, P0, t0);
        g.phase = Phase::Steal;
        g.steal_candidates = vec![P1, P2];
        g.players[P1].hand = Res::new(1, 0, 0, 0, 0);
        g.players[P2].hand = Res::new(0, 1, 0, 0, 0);
        g.sync_clock(t0);
        assert_eq!(g.clock.unwrap().kind, ClockKind::Steal);
        assert!(!g.tick(t0 + secs(14)));
        assert!(g.tick(t0 + secs(16)));
        assert_eq!(g.players[P0].hand.total(), 1, "one card was stolen");
        assert_eq!(g.phase, Phase::Main);
    }

    #[test]
    fn turn_clock_runs_out_and_the_turn_passes() {
        let t0 = Instant::now();
        let mut g = timed(3, TimerMode::VeryFast, t0);
        into_main(&mut g, P1, t0);
        assert_eq!(g.clock.unwrap().kind, ClockKind::Turn);
        assert!(!g.tick(t0 + secs(29)));
        assert!(g.tick(t0 + secs(31)));
        assert_eq!(g.current, P2);
        assert_eq!(g.phase, Phase::Roll);
        let c = g.clock.unwrap();
        assert_eq!((c.player, c.kind, c.total), (P2, ClockKind::Roll, secs(10)));
        assert_eq!(g.turn_left, secs(30), "the next player starts with a full turn clock");
    }

    #[test]
    fn unplaced_road_building_roads_are_placed_when_time_runs_out() {
        let t0 = Instant::now();
        let mut g = timed(2, TimerMode::VeryFast, t0);
        let mut rng = rand::thread_rng();
        run_setup(&mut g, &mut rng);
        let p = g.current;
        g.sync_clock(t0);
        into_main(&mut g, p, t0);
        g.players[p].dev[DevCard::RoadBuilding] = 1;
        g.apply_at(p, Action::PlayRoadBuilding, t0 + secs(1)).unwrap();
        assert_eq!(g.phase, Phase::RoadBuilding);
        let roads_before = g.roads.iter().flatten().count();
        assert!(g.tick(t0 + secs(40)));
        assert_eq!(g.roads.iter().flatten().count(), roads_before + 2);
        assert_ne!(g.current, p);
    }

    #[test]
    fn bonuses_add_10s_but_never_exceed_the_mode_start() {
        let t0 = Instant::now();
        let mut rng = rand::thread_rng();
        let mut g = timed(2, TimerMode::VeryFast, t0);
        run_setup(&mut g, &mut rng);
        let p = g.current;
        into_main(&mut g, p, t0);
        g.players[p].hand = Res::new(9, 9, 9, 9, 9);

        // 8s used (22s left): +10 would be 32, so it is capped at the 30s start
        let road = g.legal(p).roads[0];
        g.apply_at(p, Action::BuildRoad { edge: road }, t0 + secs(8)).unwrap();
        assert!((left(&g, t0 + secs(8)) - 30.0).abs() < 0.01, "capped at the mode start");

        // Each later action: the clock gains 10s, or less if that would pass 30s.
        // 17s later 13s are left, so a development card gives exactly +10.
        let t = t0 + secs(25);
        assert!((left(&g, t) - 13.0).abs() < 0.01);
        g.apply_at(p, Action::BuyDev, t).unwrap();
        assert!((left(&g, t) - 23.0).abs() < 0.01, "13s + 10s bonus");

        // a bank trade gives +10s as well
        let t = t0 + secs(27);
        assert!((left(&g, t) - 21.0).abs() < 0.01);
        g.apply_at(p, Action::BankTrade { give: Resource::Brick, get: Resource::Ore }, t).unwrap();
        assert!((left(&g, t) - 30.0).abs() < 0.01, "21s + 10s, capped at the 30s start");

        // a settlement (when one is legal) gives +10 up to the cap
        let t = t0 + secs(30);
        if let Some(&v) = g.legal(p).settlements.first() {
            let before = left(&g, t);
            g.apply_at(p, Action::BuildSettlement { vertex: v }, t).unwrap();
            assert!((left(&g, t) - (before + 10.0).min(30.0)).abs() < 0.01);
        }

        // a city likewise
        let t = t0 + secs(31);
        let before = left(&g, t);
        let city = g.legal(p).cities[0];
        g.apply_at(p, Action::BuildCity { vertex: city }, t).unwrap();
        assert!((left(&g, t) - (before + 10.0).min(30.0)).abs() < 0.01);
        assert!(left(&g, t) <= 30.0 + 0.01);
    }

    #[test]
    fn completed_trades_earn_a_bonus_but_replies_alone_do_not() {
        let t0 = Instant::now();
        let mut g = timed(2, TimerMode::Slow, t0);
        into_main(&mut g, P0, t0);
        g.players[P0].hand = Res::new(2, 0, 0, 0, 0);
        g.players[P1].hand = Res::new(0, 2, 0, 0, 0);
        g.apply_at(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 1, 0, 0, 0) }, t0 + secs(100)).unwrap();
        let id = g.trades[0].id;
        g.apply_at(P1, Action::RespondTrade { id, accept: true }, t0 + secs(101)).unwrap();
        assert!((left(&g, t0 + secs(101)) - 139.0).abs() < 0.01, "no bonus yet");
        g.apply_at(P0, Action::ConfirmTrade { id, with: P1 }, t0 + secs(102)).unwrap();
        assert!((left(&g, t0 + secs(102)) - 148.0).abs() < 0.01, "138s + 10s");
    }

    #[test]
    fn a_bank_trade_gives_a_10s_bonus_capped_at_the_mode_start() {
        let t0 = Instant::now();
        let mut g = timed(2, TimerMode::Slow, t0);
        into_main(&mut g, P0, t0);
        g.players[P0].hand = Res::new(8, 0, 0, 0, 0);
        // 5s used: 235s left, +10 would be 245, so it is capped at the 240s start
        g.apply_at(P0, Action::BankTrade { give: Resource::Brick, get: Resource::Ore }, t0 + secs(5)).unwrap();
        assert!((left(&g, t0 + secs(5)) - 240.0).abs() < 0.01, "capped at the mode start");
        // 100s later 140s are left, so the bonus is the full 10s
        g.apply_at(P0, Action::BankTrade { give: Resource::Brick, get: Resource::Grain }, t0 + secs(105)).unwrap();
        assert!((left(&g, t0 + secs(105)) - 150.0).abs() < 0.01, "140s + 10s");
        // a refused bank trade (no cards to give) earns nothing
        let before = left(&g, t0 + secs(106));
        assert!(g.apply_at(P0, Action::BankTrade { give: Resource::Lumber, get: Resource::Ore }, t0 + secs(106)).is_err());
        assert!((left(&g, t0 + secs(106)) - before).abs() < 0.01);
    }

    /// A game in the main phase of player 0's turn with 3 players and known hands.
    fn trading_game() -> Game {
        let mut g = Game::new(names(3));
        g.phase = Phase::Main;
        g.current = P0;
        g.rolled = true;
        g.players[P0].hand = Res::new(3, 0, 0, 0, 0);
        g.players[P1].hand = Res::new(0, 2, 2, 0, 0);
        g.players[P2].hand = Res::new(0, 0, 2, 2, 0);
        g
    }

    #[test]
    fn several_offers_can_be_open_at_once_and_are_answered_separately() {
        let mut g = trading_game();
        g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 1, 0, 0, 0) }).unwrap();
        g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 0, 1, 0, 0) }).unwrap();
        g.apply(P0, Action::OfferTrade { give: Res::new(2, 0, 0, 0, 0), want: Res::new(0, 0, 0, 1, 0) }).unwrap();
        assert_eq!(g.trades.len(), 3);
        let ids: Vec<u32> = g.trades.iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![0, 1, 2]);

        // player 1 declines the first and accepts the second; player 2 accepts the third
        g.apply(P1, Action::RespondTrade { id: 0, accept: false }).unwrap();
        g.apply(P1, Action::RespondTrade { id: 1, accept: true }).unwrap();
        g.apply(P2, Action::RespondTrade { id: 2, accept: true }).unwrap();
        assert_eq!(g.trades[0].replies[P1], Reply::Declined);
        assert_eq!(g.trades[0].replies[P2], Reply::Pending, "replies are per offer");
        assert_eq!(g.trades[1].replies[P1], Reply::Accepted);
        assert_eq!(g.trades[1].replies[P0], Reply::Proposer);

        // the proposer withdraws one; the others are untouched
        g.apply(P0, Action::CancelTrade { id: 0 }).unwrap();
        assert_eq!(g.trades.iter().map(|t| t.id).collect::<Vec<_>>(), vec![1, 2]);
        assert!(g.apply(P0, Action::CancelTrade { id: 0 }).is_err(), "already gone");
        assert!(g.apply(P1, Action::RespondTrade { id: 0, accept: true }).is_err(), "can't answer a gone offer");
    }

    #[test]
    fn identical_offers_and_too_many_offers_are_refused() {
        let mut g = trading_game();
        g.players[P0].hand = Res::new(9, 0, 0, 0, 0);
        let offer = Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 1, 0, 0, 0) };
        g.apply(P0, offer.clone()).unwrap();
        assert!(g.apply(P0, offer).is_err(), "same give and want twice");
        for k in 2..=MAX_OPEN_TRADES as u8 {
            g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, k, 0, 0, 0) }).unwrap();
        }
        assert_eq!(g.trades.len(), MAX_OPEN_TRADES);
        assert!(g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 0, 0, 0, 1) }).is_err());
    }

    #[test]
    fn completing_one_trade_keeps_the_others_open_and_drops_the_unaffordable() {
        let mut g = trading_game();
        // Offers A and B each need 2 of player 0's 3 brick, so only one can ever complete.
        // C needs 1 brick and stays payable after A completes.
        g.apply(P0, Action::OfferTrade { give: Res::new(2, 0, 0, 0, 0), want: Res::new(0, 1, 0, 0, 0) }).unwrap(); // A = 0
        g.apply(P0, Action::OfferTrade { give: Res::new(2, 0, 0, 0, 0), want: Res::new(0, 0, 1, 0, 0) }).unwrap(); // B = 1
        g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 0, 0, 1, 0) }).unwrap(); // C = 2
        g.apply(P1, Action::RespondTrade { id: 0, accept: true }).unwrap();
        g.apply(P1, Action::RespondTrade { id: 1, accept: true }).unwrap();
        g.apply(P2, Action::RespondTrade { id: 2, accept: true }).unwrap();

        g.apply(P0, Action::ConfirmTrade { id: 0, with: P1 }).unwrap();
        assert_eq!(g.players[P0].hand, Res::new(1, 1, 0, 0, 0));
        assert_eq!(g.players[P1].hand, Res::new(2, 1, 2, 0, 0));
        // B needs 2 brick and only 1 is left: withdrawn. C survives.
        assert_eq!(g.trades.iter().map(|t| t.id).collect::<Vec<_>>(), vec![2]);
        g.apply(P0, Action::ConfirmTrade { id: 2, with: P2 }).unwrap();
        assert_eq!(g.players[P0].hand, Res::new(0, 1, 0, 1, 0));
        assert!(g.trades.is_empty());
    }

    #[test]
    fn an_accepter_who_can_no_longer_pay_is_asked_again() {
        let mut g = trading_game();
        // Player 1 accepts two offers that both ask for 2 ore; completing one uses it up.
        g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 0, 2, 0, 0) }).unwrap(); // 0
        g.apply(P0, Action::OfferTrade { give: Res::new(2, 0, 0, 0, 0), want: Res::new(0, 0, 2, 0, 0) }).unwrap(); // 1
        g.apply(P1, Action::RespondTrade { id: 0, accept: true }).unwrap();
        g.apply(P1, Action::RespondTrade { id: 1, accept: true }).unwrap();
        g.apply(P0, Action::ConfirmTrade { id: 0, with: P1 }).unwrap();
        assert_eq!(g.trades.len(), 1);
        assert_eq!(g.trades[0].replies[P1], Reply::Pending, "player 1 spent the ore, so must answer again");
        assert!(g.apply(P0, Action::ConfirmTrade { id: 1, with: P1 }).is_err());
    }

    #[test]
    fn a_bank_trade_withdraws_offers_the_proposer_can_no_longer_pay() {
        let mut g = trading_game();
        g.players[P0].hand = Res::new(4, 1, 1, 1, 1);
        g.apply(P0, Action::OfferTrade { give: Res::new(4, 0, 0, 0, 0), want: Res::new(0, 0, 1, 0, 0) }).unwrap();
        g.apply(P0, Action::OfferTrade { give: Res::new(0, 1, 0, 0, 0), want: Res::new(0, 0, 1, 0, 0) }).unwrap();
        // 4:1 with the bank spends all the brick
        g.apply(P0, Action::BankTrade { give: Resource::Brick, get: Resource::Ore }).unwrap();
        assert_eq!(g.trades.len(), 1);
        assert_eq!(g.trades[0].give, Res::new(0, 1, 0, 0, 0));
    }

    #[test]
    fn declined_offers_stay_visible_to_the_proposer_as_declined() {
        let mut g = trading_game();
        g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 1, 0, 0, 0) }).unwrap();
        g.apply(P1, Action::RespondTrade { id: 0, accept: false }).unwrap();
        let v = g.view(P0, &all_connected(3), &g.longest_roads());
        assert_eq!(v.trades.len(), 1);
        assert_eq!(v.trades[0].replies[P1], Reply::Declined);
        let json = serde_json::to_string(&v.trades).unwrap();
        assert!(json.contains("\"id\":0") && json.contains("\"declined\""), "{json}");
    }

    #[test]
    fn offers_end_with_the_turn_and_with_playing_a_development_card() {
        let mut g = trading_game();
        g.players[P0].dev[DevCard::Monopoly] = 1;
        g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 1, 0, 0, 0) }).unwrap();
        g.apply(P0, Action::PlayMonopoly { resource: Resource::Lumber }).unwrap();
        assert!(g.trades.is_empty());
        g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 0, 1, 0, 0) }).unwrap();
        g.apply(P0, Action::EndTurn).unwrap();
        assert!(g.trades.is_empty());
    }

    #[test]
    fn a_knight_pauses_the_turn_clock_for_the_whole_robber_sequence() {
        let t0 = Instant::now();
        let mut g = timed(2, TimerMode::Normal, t0);
        into_main(&mut g, P0, t0);
        g.players[P0].dev[DevCard::Knight] = 1;
        g.apply_at(P0, Action::PlayKnight, t0 + secs(40)).unwrap();
        assert_eq!(g.clock.unwrap().kind, ClockKind::Robber);
        let hex = g.legal(P0).hexes[0];
        // 15s spent choosing where to put the robber, then maybe 5s choosing a victim
        let mut last = t0 + secs(55);
        g.apply_at(P0, Action::MoveRobber { hex }, last).unwrap();
        if g.phase == Phase::Steal {
            let victim = g.steal_candidates[0];
            last = t0 + secs(60);
            g.apply_at(P0, Action::Steal { victim }, last).unwrap();
        }
        assert_eq!(g.phase, Phase::Main);
        assert_eq!(g.clock.unwrap().kind, ClockKind::Turn);
        assert!((left(&g, last) - 80.0).abs() < 0.01, "got {}", left(&g, last));
    }

    #[test]
    fn a_knight_before_rolling_keeps_the_remaining_roll_time() {
        let t0 = Instant::now();
        let mut g = timed(2, TimerMode::Normal, t0);
        g.phase = Phase::Roll;
        g.current = P0;
        g.rolled = false;
        g.sync_clock(t0);
        g.players[P0].dev[DevCard::Knight] = 1;
        g.apply_at(P0, Action::PlayKnight, t0 + secs(4)).unwrap(); // 6s of roll time left
        assert_eq!(g.clock.unwrap().kind, ClockKind::Robber);
        let hex = g.legal(P0).hexes[0];
        let mut last = t0 + secs(10);
        g.apply_at(P0, Action::MoveRobber { hex }, last).unwrap();
        if g.phase == Phase::Steal {
            let victim = g.steal_candidates[0];
            last = t0 + secs(12);
            g.apply_at(P0, Action::Steal { victim }, last).unwrap();
        }
        assert_eq!(g.phase, Phase::Roll);
        assert_eq!(g.clock.unwrap().kind, ClockKind::Roll);
        assert!((left(&g, last) - 6.0).abs() < 0.01, "got {}", left(&g, last));
    }

    #[test]
    fn view_carries_the_countdown() {
        let t0 = Instant::now();
        let g = timed(2, TimerMode::Fast, t0);
        let v = g.view(P0, &all_connected(2), &g.longest_roads());
        let t = v.timer.unwrap();
        assert_eq!(t.kind, ClockKind::SetupSettlement);
        assert_eq!(t.total_ms, 120_000);
        assert!(t.remaining_ms <= 120_000);
        assert_eq!(v.timer_mode, TimerMode::Fast);
        let json = serde_json::to_string(&g.view(P0, &all_connected(2), &g.longest_roads())).unwrap();
        assert!(json.contains("\"timer_mode\":\"fast\""));
        assert!(json.contains("\"kind\":\"setup_settlement\""));
    }

    #[test]
    fn no_clock_after_the_game_ends() {
        let t0 = Instant::now();
        let mut g = timed(2, TimerMode::Fast, t0);
        into_main(&mut g, P0, t0);
        g.phase = Phase::GameOver;
        g.sync_clock(t0);
        assert!(g.clock.is_none());
        assert!(!g.tick(t0 + secs(1000)));
    }

    // ------------------------------------------------------------------
    // Game settings
    // ------------------------------------------------------------------

    fn with(f: impl FnOnce(&mut Settings)) -> Settings {
        let mut s = Settings::default();
        f(&mut s);
        s
    }

    /// The first vertex of some hex, plus that hex, for placing test buildings.
    fn some_hex_vertex(g: &Game) -> (usize, usize) {
        let h = (0..g.board.hexes.len()).find(|&h| h != g.robber).unwrap();
        (h, g.board.hexes[h].vertices[0])
    }

    #[test]
    fn defaults_are_the_standard_rules_with_15_points() {
        let d = Settings::default();
        assert_eq!(d.validate(), Ok(()));
        assert_eq!(
            (d.points_to_win, d.discard_limit, d.friendly_robber, d.max_roads, d.max_cities, d.max_settlements),
            (15, 7, false, 15, 4, 5)
        );
        assert_eq!((d.dev_cards, d.resource_max, d.settlement_points, d.city_points), (25, 19, 1, 2));
        assert_eq!((d.longest_road_points, d.largest_army_points, d.vp_card_points), (2, 2, 1));
        assert_eq!((d.longest_road_min, d.largest_army_min, d.starting_settlements, d.bank_ratio), (5, 3, 2, 4));
        assert!(d.changes().is_empty(), "defaults log nothing");
        assert_eq!(d.deck_counts(), ByDev([14, 2, 2, 2, 5]));
        // A partial JSON object fills in the rest from the defaults.
        let s: Settings = serde_json::from_str("{\"points_to_win\":12}").unwrap();
        assert_eq!((s.points_to_win, s.max_roads), (12, 15));
    }

    #[test]
    fn invalid_settings_are_rejected() {
        assert!(with(|s| s.points_to_win = 2).validate().is_err());
        assert!(with(|s| s.discard_limit = 0).validate().is_err());
        assert!(with(|s| s.bank_ratio = 0).validate().is_err());
        assert!(with(|s| s.longest_road_min = 1).validate().is_err());
        assert!(with(|s| s.largest_army_min = 0).validate().is_err());
        assert!(with(|s| s.starting_settlements = 0).validate().is_err());
        assert!(with(|s| s.starting_settlements = 6).validate().is_err());
        assert!(with(|s| s.max_roads = 0).validate().is_err());
        assert!(with(|s| s.resource_max = 0).validate().is_err());
        // every starting settlement needs a road and a settlement piece
        assert!(with(|s| { s.starting_settlements = 3; s.max_roads = 2 }).validate().is_err());
        assert!(with(|s| { s.starting_settlements = 3; s.max_settlements = 2 }).validate().is_err());
        assert!(with(|s| { s.starting_settlements = 3; s.max_roads = 3; s.max_settlements = 3 }).validate().is_ok());
        // too many starting settlements for the board
        let five = with(|s| s.starting_settlements = 5);
        assert!(five.check_players(4).is_ok());
        assert!(with(|s| { s.starting_settlements = 5; s.max_settlements = 6 }).check_players(9).is_err());
        assert!(Settings::default().check_players(4).is_ok());
        // eight players with the most starting settlements each is what the limit is sized for
        assert!(five.check_players(8).is_ok());
        assert!(Settings::default().check_players(8).is_ok());
    }

    #[test]
    fn the_bank_and_deck_grow_with_the_table_unless_the_host_chose_otherwise() {
        let d = Settings::default();
        for n in 2..=4 {
            assert_eq!(d.scaled_for(n), d, "{n} players play the standard rules");
        }
        for (n, bank, deck) in [(5, 24, 34), (6, 24, 34), (7, 30, 44), (8, 30, 44)] {
            let s = d.scaled_for(n);
            assert_eq!((s.resource_max, s.dev_cards), (bank, deck), "{n} players");
            assert_eq!(Settings { resource_max: d.resource_max, dev_cards: d.dev_cards, ..s }, d, "nothing else changes");
            assert_eq!(s.validate(), Ok(()));
            assert_eq!(s.deck_counts().0.iter().sum::<usize>(), deck as usize);
        }
        // a value the host changed is left alone, the other one still grows
        let s = with(|s| { s.resource_max = 12; s.dev_cards = 10 }).scaled_for(8);
        assert_eq!((s.resource_max, s.dev_cards), (12, 10));
        let s = with(|s| s.dev_cards = 40).scaled_for(6);
        assert_eq!((s.resource_max, s.dev_cards), (24, 40));
    }

    #[test]
    fn up_to_eight_players_can_play_on_the_big_maps() {
        let mut rng = rand::thread_rng();
        for n in 5..=8usize {
            for map in ["earth", "usa"] {
                let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
                let mut g = Game::with_options(names(n), colors, TimerMode::VeryFast, Settings::default(), map);
                let scaled = Settings::default().scaled_for(n);
                assert_eq!((g.n, g.settings), (n, scaled), "{map} with {n}");
                assert_eq!(g.deck.len(), scaled.dev_cards as usize);
                assert_eq!(g.bank, Res::splat(scaled.resource_max));
                assert!(!g.log.iter().any(|e| matches!(e, LogEvent::CustomRules { .. })), "scaling isn't a custom rule");
                play_random(&mut g, &mut rng);
                invariants(&g);
            }
        }
    }

    #[test]
    fn dev_deck_has_the_chosen_size_in_standard_proportions() {
        for total in 0..=60u8 {
            let s = with(|s| s.dev_cards = total);
            let c = s.deck_counts();
            assert_eq!(c.0.iter().sum::<usize>(), total as usize, "deck of {total}");
            let g = Game::with_settings(names(2), s);
            assert_eq!(g.deck.len(), total as usize);
        }
        assert_eq!(with(|s| s.dev_cards = 50).deck_counts(), ByDev([28, 4, 4, 4, 10]));
        assert_eq!(with(|s| s.dev_cards = 10).deck_counts(), ByDev([5, 1, 1, 1, 2]));
        // an empty deck can't be bought from
        let mut g = Game::with_settings(names(2), with(|s| s.dev_cards = 0));
        g.phase = Phase::Main;
        g.players[P0].hand = Res::new(0, 0, 1, 1, 1);
        assert!(g.apply(P0, Action::BuyDev).is_err());
    }

    #[test]
    fn piece_limits_and_bank_size_come_from_the_settings() {
        let s = with(|s| { s.max_roads = 9; s.max_settlements = 3; s.max_cities = 2; s.resource_max = 7 });
        let g = Game::with_settings(names(3), s);
        for p in &g.players {
            assert_eq!((p.roads_left, p.settlements_left, p.cities_left), (9, 3, 2));
        }
        assert_eq!(g.bank, Res::splat(7));
        invariants(&g);
    }

    #[test]
    fn points_to_win_and_point_values_are_configurable() {
        let s = with(|s| {
            s.points_to_win = 12;
            s.settlement_points = 2;
            s.city_points = 3;
            s.longest_road_points = 1;
            s.largest_army_points = 4;
            s.vp_card_points = 2;
        });
        let mut g = Game::with_settings(names(3), s);
        g.phase = Phase::Main;
        let vs: Vec<usize> = (0..54).step_by(3).take(3).collect();
        g.buildings[vs[0]] = Some(Building { owner: P0, city: false });
        g.buildings[vs[1]] = Some(Building { owner: P0, city: true });
        assert_eq!(g.vp_public(P0), 2 + 3);
        g.longest_road_holder = Some(P0);
        assert_eq!(g.vp_public(P0), 2 + 3 + 1);
        g.largest_army_holder = Some(P0);
        assert_eq!(g.vp_public(P0), 2 + 3 + 1 + 4);
        g.players[P0].dev[DevCard::VictoryPoint] = 1;
        g.players[P0].dev_new[DevCard::VictoryPoint] = 1;
        assert_eq!(g.vp_public(P0), 10, "victory point cards stay hidden");
        assert_eq!(g.vp_total(P0), 10 + 2 * 2);
        assert_eq!(
            g.vp_breakdown(P0),
            VpBreakdown { settlements: 2, cities: 3, dev_cards: 2 * 2, largest_army: 4, longest_road: 1 }
        );
        let b = g.vp_breakdown(P0);
        assert_eq!(b.settlements + b.cities + b.dev_cards + b.largest_army + b.longest_road, g.vp_total(P0));
        // 14 points is over 12: the current player wins on the spot
        g.current = P0;
        g.check_win();
        assert_eq!(g.winner, Some(P0));
        assert_eq!(g.phase, Phase::GameOver);

        // one point short of the target does not win
        let mut g = Game::with_settings(names(2), with(|s| s.points_to_win = 4));
        g.phase = Phase::Main;
        g.buildings[0] = Some(Building { owner: P0, city: true });
        g.buildings[10] = Some(Building { owner: P0, city: false });
        g.check_win();
        assert_eq!(g.winner, None, "3 points, 4 needed");
        g.buildings[20] = Some(Building { owner: P0, city: false });
        g.check_win();
        assert_eq!(g.winner, Some(P0));
    }

    #[test]
    fn default_target_is_15_not_10() {
        let mut g = Game::new(names(2));
        g.phase = Phase::Main;
        g.players[P0].dev[DevCard::VictoryPoint] = 12;
        g.check_win();
        assert_eq!(g.winner, None, "12 points is not enough at the default 15");
        g.players[P0].dev[DevCard::VictoryPoint] = 15;
        g.check_win();
        assert_eq!(g.winner, Some(P0));
    }

    #[test]
    fn discard_limit_decides_who_discards() {
        let mut g = Game::with_settings(names(3), with(|s| s.discard_limit = 4));
        g.phase = Phase::Roll;
        g.players[P0].hand = Res::new(2, 2, 1, 0, 0); // 5 > 4: discards 2
        g.players[P1].hand = Res::new(2, 2, 0, 0, 0); // exactly 4: keeps all
        g.players[P2].hand = Res::new(3, 3, 3, 3, 0); // 12: discards 6
        g.resolve_roll(7);
        assert_eq!(g.phase, Phase::Discard);
        let mut d = g.discards.clone();
        d.sort();
        assert_eq!(d, vec![(P0, 2), (P2, 6)]);

        // With a high limit nobody discards and the robber moves straight away.
        let mut g = Game::with_settings(names(2), with(|s| s.discard_limit = 12));
        g.phase = Phase::Roll;
        g.players[P0].hand = Res::new(3, 3, 3, 3, 0);
        g.resolve_roll(7);
        assert_eq!(g.phase, Phase::MoveRobber);
    }

    #[test]
    fn award_minimums_are_configurable() {
        // Longest Road from 3
        let mut g = Game::with_settings(names(3), with(|s| s.longest_road_min = 3));
        g.phase = Phase::Main;
        line_of_roads(&mut g, P0, 3);
        g.update_longest_road();
        assert_eq!(g.longest_road_holder, Some(P0));
        // ...and a 5-road line does not qualify when 7 are needed
        let mut g = Game::with_settings(names(3), with(|s| s.longest_road_min = 7));
        g.phase = Phase::Main;
        line_of_roads(&mut g, P0, 5);
        g.update_longest_road();
        assert_eq!(g.longest_road_holder, None);

        // Largest Army from 2, and from 5
        let mut g = Game::with_settings(names(3), with(|s| s.largest_army_min = 2));
        g.players[P0].knights = 2;
        g.update_largest_army(P0);
        assert_eq!(g.largest_army_holder, Some(P0));
        let mut g = Game::with_settings(names(3), with(|s| s.largest_army_min = 5));
        g.players[P0].knights = 4;
        g.update_largest_army(P0);
        assert_eq!(g.largest_army_holder, None);
        g.players[P0].knights = 5;
        g.update_largest_army(P0);
        assert_eq!(g.largest_army_holder, Some(P0));
    }

    #[test]
    fn bank_ratio_is_the_base_and_harbours_only_improve_it() {
        let mut g = Game::with_settings(names(2), with(|s| s.bank_ratio = 5));
        assert_eq!(g.ratio(P0, Resource::Brick), 5);
        let port = g.board.ports.iter().find(|p| p.resource.is_none()).unwrap().clone();
        g.buildings[port.a] = Some(Building { owner: P0, city: false });
        assert_eq!(g.ratio(P0, Resource::Brick), 3, "a 3:1 harbour beats a 5:1 base");

        // a 2:1 base is already better than a 3:1 harbour
        let mut g = Game::with_settings(names(2), with(|s| s.bank_ratio = 2));
        g.buildings[port.a] = Some(Building { owner: P0, city: false });
        assert_eq!(g.ratio(P0, Resource::Brick), 2, "a 3:1 harbour never makes a 2:1 base worse");
        // and a 1:1 base is better than any harbour
        let mut g = Game::with_settings(names(2), with(|s| s.bank_ratio = 1));
        g.buildings[port.a] = Some(Building { owner: P0, city: false });
        assert_eq!(g.ratio(P0, Resource::Brick), 1);

        // the trade itself uses the ratio
        let mut g = Game::with_settings(names(2), with(|s| s.bank_ratio = 3));
        g.phase = Phase::Main;
        g.players[P0].hand = Res::new(3, 0, 0, 0, 0);
        g.bank = Res::new(16, 19, 19, 19, 19);
        g.apply(P0, Action::BankTrade { give: Resource::Brick, get: Resource::Ore }).unwrap();
        assert_eq!(g.players[P0].hand, Res::new(0, 0, 1, 0, 0));
        assert_eq!(g.bank[Resource::Brick], 19);
        assert_eq!(g.view(P0, &all_connected(2), &g.longest_roads()).ratios, Res::splat(3));
    }

    /// Run the whole setup phase with random legal moves; returns (player, vertex, step) per settlement.
    fn setup_log(g: &mut Game, rng: &mut impl Rng) -> Vec<(PlayerId, usize, usize)> {
        let mut placed = vec![];
        while g.phase == Phase::Setup {
            let p = g.current;
            let l = g.legal(p);
            if let Some(_) = g.setup_vertex {
                g.apply(p, Action::BuildRoad { edge: pick(&l.roads, rng).unwrap() }).unwrap();
            } else {
                let v = pick(&l.settlements, rng).unwrap();
                placed.push((p, v, g.setup_idx));
                g.apply(p, Action::BuildSettlement { vertex: v }).unwrap();
            }
        }
        placed
    }

    #[test]
    fn starting_settlements_snake_draft_one_road_each() {
        let mut rng = rand::thread_rng();
        for start in 1..=4u8 {
            for n in 2..=4usize {
                let mut g = Game::with_settings(names(n), with(|s| s.starting_settlements = start));
                let placed = setup_log(&mut g, &mut rng);
                // snake order: forward, backward, forward...
                let expect: Vec<PlayerId> = (0..start as usize)
                    .flat_map(|r| if r % 2 == 0 { PlayerId::range(n).collect::<Vec<_>>() } else { PlayerId::range(n).collect::<Vec<_>>().into_iter().rev().collect() })
                    .collect();
                assert_eq!(placed.iter().map(|x| x.0).collect::<Vec<_>>(), expect, "start={start} n={n}");
                assert_eq!(g.phase, Phase::Roll);
                assert_eq!(g.current, P0, "the first placer takes the first turn");
                for p in PlayerId::range(n) {
                    let s = g.buildings.iter().flatten().filter(|b| b.owner == p).count() as u8;
                    let r = g.roads.iter().filter(|r| **r == Some(p)).count() as u8;
                    assert_eq!((s, r), (start, start), "one road per starting settlement");
                    assert_eq!(g.players[p].settlements_left, 5 - start);
                    assert_eq!(g.players[p].roads_left, 15 - start);
                }
                // only each player's LAST settlement pays out
                for p in PlayerId::range(n) {
                    let last = placed.iter().filter(|x| x.0 == p).last().unwrap().1;
                    let mut want = Res::ZERO;
                    for &h in &g.board.vertices[last].hexes {
                        if let Some(r) = g.board.hexes[h].terrain.resource() {
                            want[r] += 1;
                        }
                    }
                    assert_eq!(g.players[p].hand, want, "start={start} n={n} player {}", p.index());
                }
                invariants(&g);
            }
        }
    }

    #[test]
    fn friendly_robber_protects_low_scoring_players() {
        let mut g = Game::with_settings(names(3), with(|s| s.friendly_robber = true));
        g.phase = Phase::MoveRobber;
        g.current = P0;
        let (_, v) = some_hex_vertex(&g);
        g.buildings[v] = Some(Building { owner: P1, city: false });
        let touching: Vec<usize> = g.board.vertices[v].hexes.clone();
        let allowed = g.robber_hexes(P0);
        for &h in &touching {
            assert!(!allowed.contains(&h), "hex {h} touches a 1-point player");
        }
        assert!(allowed.len() >= 15 && !allowed.contains(&g.robber));
        assert_eq!(g.legal(P0).hexes, allowed, "the client is shown only legal hexes");
        let blocked = touching.iter().copied().find(|&h| h != g.robber).unwrap();
        let err = g.apply(P0, Action::MoveRobber { hex: blocked }).unwrap_err();
        assert!(err.contains("Friendly Robber"), "{err}");
        assert_eq!(g.phase, Phase::MoveRobber, "a refused move changes nothing");

        // your own buildings never protect a hex from you
        let mut mine = Game::with_settings(names(2), with(|s| s.friendly_robber = true));
        mine.buildings[v] = Some(Building { owner: P0, city: false });
        assert!(touching.iter().all(|&h| h == mine.robber || mine.robber_hexes(P0).contains(&h)));

        // at 3 visible points a player is fair game
        g.longest_road_holder = Some(P1); // +2
        assert_eq!(g.vp_public(P1), 3);
        assert!(g.robber_hexes(P0).contains(&blocked));
        g.apply(P0, Action::MoveRobber { hex: blocked }).unwrap();
        assert_eq!(g.robber, blocked);

        // switched off: everything but the current hex is allowed
        let mut off = Game::with_settings(names(3), Settings::default());
        off.buildings[v] = Some(Building { owner: P1, city: false });
        assert_eq!(off.robber_hexes(P0).len(), 18);
    }

    #[test]
    fn friendly_robber_never_leaves_the_game_stuck() {
        let mut g = Game::with_settings(names(2), with(|s| s.friendly_robber = true));
        g.phase = Phase::MoveRobber;
        // player 1 (1 point each building, but low) sits on every vertex
        for v in 0..g.buildings.len() {
            g.buildings[v] = Some(Building { owner: P1, city: false });
        }
        g.longest_road_holder = None;
        assert!(g.vp_public(P1) > FRIENDLY_ROBBER_MAX_VP);
        g.settings.settlement_points = 0; // now player 1 has 0 points and protects every hex
        assert_eq!(g.vp_public(P1), 0);
        assert_eq!(g.robber_hexes(P0).len(), 18, "if every hex is protected, any hex is allowed");
    }

    #[test]
    fn timed_out_robber_respects_friendly_robber() {
        for _ in 0..40 {
            let t0 = Instant::now();
            let mut g = Game::with_settings(names(2), with(|s| s.friendly_robber = true));
            let (_, v) = some_hex_vertex(&g);
            g.buildings[v] = Some(Building { owner: P1, city: false });
            g.phase = Phase::MoveRobber;
            g.current = P0;
            g.sync_clock(t0);
            assert!(g.tick(t0 + secs(100)));
            let touching = g.board.vertices[v].hexes.clone();
            assert!(!touching.contains(&g.robber), "auto-placed robber on a protected hex");
        }
    }

    #[test]
    fn custom_rules_are_announced_in_the_log() {
        let g = Game::with_settings(names(2), with(|s| { s.points_to_win = 12; s.friendly_robber = true; s.bank_ratio = 3 }));
        let line = g
            .log
            .iter()
            .find_map(|l| match l {
                LogEvent::CustomRules { changes } => Some(changes.join(", ")),
                _ => None,
            })
            .expect("a custom-rules event");
        assert!(line.contains("points to win 12") && line.contains("Friendly Robber on") && line.contains("bank trade 3:1"), "{line}");
        assert!(Game::new(names(2)).log.iter().all(|l| !matches!(l, LogEvent::CustomRules { .. })));
        let v = g.view(P0, &all_connected(2), &g.longest_roads());
        assert_eq!(v.settings.points_to_win, 12);
        let json = serde_json::to_string(&v).unwrap();
        assert!(json.contains("\"settings\":{\"points_to_win\":12"));
    }

    // ------------------------------------------------------------------
    // Structured log
    // ------------------------------------------------------------------

    #[test]
    fn the_log_records_events_not_sentences() {
        let g = Game::new(names(2));
        assert_eq!(g.log.first(), Some(&LogEvent::GameStarted));
        assert!(g.log.contains(&LogEvent::TurnOrder { order: g.order.clone() }));
        let json = serde_json::to_value(g.view(P0, &all_connected(2), &g.longest_roads())).unwrap();
        assert_eq!(json["log"][0], serde_json::json!({ "type": "game_started" }));
    }

    #[test]
    fn a_roll_is_logged_with_its_dice() {
        let mut rng = rand::thread_rng();
        let mut g = Game::new(names(2));
        run_setup(&mut g, &mut rng);
        assert!(g.log.contains(&LogEvent::SetupComplete));
        let p = g.current;
        g.apply(p, Action::Roll).unwrap();
        let dice = g.dice.unwrap();
        assert!(g.log.contains(&LogEvent::Rolled { player: p, dice }));
    }

    #[test]
    fn production_is_logged_as_the_cards_received() {
        let mut g = Game::new(names(2));
        let h = (0..g.board.hexes.len())
            .find(|&h| g.board.hexes[h].number.is_some())
            .expect("a producing hex");
        let roll = g.board.hexes[h].number.unwrap();
        let v = g.board.hexes[h].vertices[0];
        g.buildings[v] = Some(Building { owner: P0, city: false });
        g.distribute(roll);
        let cards = g.players[P0].hand;
        assert!(cards.total() >= 1);
        assert_eq!(g.log.last(), Some(&LogEvent::Collected { player: P0, cards }));
        assert_eq!(g.players[P1].hand.total(), 0, "nothing is logged for players who got nothing");
    }

    #[test]
    fn building_and_trading_are_logged_with_who_did_it() {
        let mut rng = rand::thread_rng();
        let mut g = Game::new(names(2));
        run_setup(&mut g, &mut rng);
        let p = g.current;
        g.phase = Phase::Main;
        g.rolled = true;
        g.players[p].hand = Res::new(5, 1, 0, 0, 0);
        let e = pick(&g.legal(p).roads, &mut rng).unwrap();
        g.apply(p, Action::BuildRoad { edge: e }).unwrap();
        assert_eq!(g.log.last(), Some(&LogEvent::BuiltRoad { player: p }));
        // A harbour from the random setup may improve on 4:1, so ask the game for the real ratio.
        let ratio = g.ratio(p, Resource::Brick);
        g.apply(p, Action::BankTrade { give: Resource::Brick, get: Resource::Ore }).unwrap();
        assert_eq!(g.log.last(), Some(&LogEvent::BankTrade { player: p, give: Resource::Brick, get: Resource::Ore, ratio }));
    }

    #[test]
    fn a_timeout_is_logged_with_who_and_what_ran_out() {
        let t0 = Instant::now();
        let mut g = timed(3, TimerMode::VeryFast, t0);
        into_main(&mut g, P1, t0);
        assert!(g.tick(t0 + secs(31)));
        assert!(g.log.contains(&LogEvent::TimedOut { player: P1, kind: ClockKind::Turn }));
    }

    // ------------------------------------------------------------------
    // Bridges
    // ------------------------------------------------------------------

    /// An Earth game with bridges on, in the main phase of P0's turn, plus the ends of one bridge site
    /// (`a`, the one P0 holds a settlement on, and `b`) and the site's edge id.
    fn bridge_game(on: bool) -> (Game, usize, usize, usize) {
        let colors = PALETTE.iter().take(2).map(|c| c.to_string()).collect();
        let mut g = Game::build(names(2), colors, TimerMode::Normal, with(|s| s.bridges = on), "earth", false, Instant::now());
        let e = g.board.edges.iter().position(|e| e.bridge).expect("Earth has bridge sites");
        let (a, b) = (g.board.edges[e].a, g.board.edges[e].b);
        g.buildings[a] = Some(Building { owner: P0, city: false });
        g.players[P0].settlements_left -= 1;
        into_main(&mut g, P0, Instant::now());
        (g, a, b, e)
    }

    /// Hand `p` exactly `cards` from the bank.
    fn deal(g: &mut Game, p: PlayerId, cards: Res) {
        for r in Resource::ALL {
            g.players[p].hand[r] = cards[r];
            g.bank[r] -= cards[r];
        }
    }

    #[test]
    fn a_bridge_costs_brick_lumber_and_ore_and_joins_the_road_network() {
        let (mut g, _, b, e) = bridge_game(true);
        assert_eq!(COST_BRIDGE, Res::new(1, 1, 1, 0, 0));
        deal(&mut g, P0, COST_BRIDGE);
        let l = g.legal(P0);
        assert!(l.bridges.contains(&e), "a bridge can start at a settlement");
        assert!(!l.roads.contains(&e), "and the site is not offered as a road");
        assert!(g.apply(P0, Action::BuildRoad { edge: e }).is_err(), "a road can't be built over water");
        assert_eq!(g.players[P0].hand, COST_BRIDGE);

        let bank = g.bank;
        g.apply(P0, Action::BuildBridge { edge: e }).unwrap();
        assert_eq!(g.roads[e], Some(P0));
        assert_eq!(g.players[P0].hand, Res::ZERO);
        assert_eq!(g.players[P0].roads_left, g.settings.max_roads - 1, "a bridge is one of the road pieces");
        for r in Resource::ALL {
            assert_eq!(g.bank[r], bank[r] + COST_BRIDGE[r]);
        }
        assert_eq!(g.log.last(), Some(&LogEvent::BuiltBridge { player: P0 }));
        assert_eq!(g.longest_road(P0), 1, "and one segment of the longest road");
        assert!(g.legal(P0).settlements.contains(&b), "the far shore is now connected to P0's network");
        assert!(g.legal(P0).bridges.is_empty() || !g.legal(P0).bridges.contains(&e), "built bridges are not offered again");
        invariants(&g);
    }

    #[test]
    fn a_bridge_needs_the_full_price_a_connection_and_a_free_road_piece() {
        let (mut g, _, _, e) = bridge_game(true);
        // one card short of each cost in turn
        for short in [Res::new(0, 1, 1, 0, 0), Res::new(1, 0, 1, 0, 0), Res::new(1, 1, 0, 0, 0)] {
            deal(&mut g, P0, short);
            assert!(g.apply(P0, Action::BuildBridge { edge: e }).is_err());
            assert_eq!(g.roads[e], None);
            for r in Resource::ALL {
                g.bank[r] += g.players[P0].hand[r];
                g.players[P0].hand[r] = 0;
            }
        }
        // a road piece is needed
        deal(&mut g, P0, COST_BRIDGE);
        g.players[P0].roads_left = 0;
        assert!(g.legal(P0).bridges.is_empty());
        assert!(g.apply(P0, Action::BuildBridge { edge: e }).is_err());
        g.players[P0].roads_left = 1;
        // ... and a connection: P1 owns nothing near the site
        assert!(g.apply(P1, Action::BuildBridge { edge: e }).is_err(), "not P1's turn");
        g.current = P1;
        g.players[P1].hand = COST_BRIDGE;
        g.players[P1].roads_left = 1;
        assert!(g.apply(P1, Action::BuildBridge { edge: e }).is_err(), "P1 has no road or settlement there");
        // only a bridge site can take a bridge
        g.current = P0;
        let land = g.board.edges.iter().position(|e| !e.bridge).unwrap();
        assert!(g.apply(P0, Action::BuildBridge { edge: land }).is_err());
        assert!(g.apply(P0, Action::BuildBridge { edge: g.roads.len() }).is_err());
        g.apply(P0, Action::BuildBridge { edge: e }).unwrap();
    }

    #[test]
    fn bridges_are_off_unless_the_host_turns_them_on() {
        assert!(!Settings::default().bridges);
        let (mut g, _, _, e) = bridge_game(false);
        deal(&mut g, P0, COST_BRIDGE);
        assert!(g.legal(P0).bridges.is_empty());
        assert!(g.apply(P0, Action::BuildBridge { edge: e }).is_err());
        assert_eq!(g.roads[e], None);
        assert!(with(|s| s.bridges = true).changes().contains(&"Bridges on".to_string()));
        assert!(Settings::default().changes().is_empty());
    }

    #[test]
    fn bridges_are_for_the_main_phase_not_for_setup_or_road_building() {
        let colors = PALETTE.iter().take(2).map(|c| c.to_string()).collect();
        let mut g = Game::build(names(2), colors, TimerMode::Normal, with(|s| s.bridges = true), "earth", false, Instant::now());
        let e = g.board.edges.iter().position(|e| e.bridge).unwrap();
        let a = g.board.edges[e].a;
        // setup: the road next to a settlement on a bridge site's corner is a land road
        g.apply(P0, Action::BuildSettlement { vertex: a }).unwrap();
        let l = g.legal(P0);
        assert!(!l.roads.is_empty() && l.roads.iter().all(|&r| !g.board.edges[r].bridge));
        assert!(l.bridges.is_empty());
        assert!(g.apply(P0, Action::BuildRoad { edge: e }).is_err());
        assert!(g.apply(P0, Action::BuildBridge { edge: e }).is_err());

        // Road Building card: free roads only, never a bridge
        let (mut g, _, _, e) = bridge_game(true);
        g.phase = Phase::RoadBuilding;
        g.road_building_left = 2;
        assert!(g.legal(P0).roads.iter().all(|&r| !g.board.edges[r].bridge));
        assert!(g.apply(P0, Action::BuildRoad { edge: e }).is_err());
        assert!(g.apply(P0, Action::BuildBridge { edge: e }).is_err());
        assert_eq!(g.roads[e], None);
    }

    #[test]
    fn a_bridge_extends_longest_road() {
        let (mut g, _, b, e) = bridge_game(true);
        deal(&mut g, P0, COST_BRIDGE);
        // a land road leaving the far corner, then the bridge to it: 2 long
        let far = g.board.vertices[b].edges.iter().copied().find(|&x| !g.board.edges[x].bridge).unwrap();
        g.roads[far] = Some(P0);
        g.players[P0].roads_left -= 1;
        assert_eq!(g.longest_road(P0), 1);
        g.apply(P0, Action::BuildBridge { edge: e }).unwrap();
        assert_eq!(g.longest_road(P0), 2);
    }

    #[test]
    fn bridges_have_a_wire_format() {
        let a: Action = serde_json::from_str(r#"{"type":"build_bridge","edge":3}"#).unwrap();
        assert!(matches!(a, Action::BuildBridge { edge: 3 }));
        let s: Settings = serde_json::from_str(r#"{"bridges":true}"#).unwrap();
        assert!(s.bridges && s.points_to_win == 15);
        let (g, _, _, e) = bridge_game(true);
        let v = serde_json::to_value(&g.board).unwrap();
        assert_eq!(v["edges"][e]["bridge"], true);
        let land = g.board.edges.iter().position(|e| !e.bridge).unwrap();
        assert!(v["edges"][land].get("bridge").is_none(), "land edges are sent as before");
    }

    fn random_settings(rng: &mut impl Rng) -> Settings {
        let start = rng.gen_range(1..=3u8);
        let s = Settings {
            points_to_win: rng.gen_range(5..=15),
            discard_limit: rng.gen_range(2..=10),
            friendly_robber: rng.gen_bool(0.5),
            bridges: rng.gen_bool(0.5),
            max_roads: rng.gen_range(start.max(3)..=20),
            max_cities: rng.gen_range(0..=6),
            max_settlements: rng.gen_range(start..=8),
            dev_cards: rng.gen_range(0..=40),
            resource_max: rng.gen_range(4..=30),
            settlement_points: rng.gen_range(0..=3),
            city_points: rng.gen_range(0..=4),
            longest_road_points: rng.gen_range(0..=3),
            largest_army_points: rng.gen_range(0..=3),
            vp_card_points: rng.gen_range(0..=3),
            longest_road_min: rng.gen_range(2..=7),
            largest_army_min: rng.gen_range(1..=5),
            starting_settlements: start,
            bank_ratio: rng.gen_range(1..=5),
        };
        s.validate().unwrap();
        s
    }

    #[test]
    fn random_games_with_random_settings_keep_invariants() {
        let mut rng = rand::thread_rng();
        let mut finished = 0;
        for game_no in 0..80 {
            let n = 2 + game_no % 3;
            let s = random_settings(&mut rng);
            s.check_players(n).unwrap();
            let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
            let map = if game_no % 2 == 0 { "standard" } else { "earth" };
            let mut g = Game::with_options(names(n), colors, TimerMode::VeryFast, s, map);
            play_random(&mut g, &mut rng);
            invariants(&g);
            if g.phase == Phase::GameOver {
                finished += 1;
                let w = g.winner.unwrap();
                assert!(g.vp_total(w) >= s.points_to_win as usize);
            }
        }
        assert!(finished > 10, "only {finished} of 80 random-rule games finished");
    }
}