//! Starting a game (the constructors) and the setup phase: each player's starting settlements and roads.

use crate::board::Board;
use crate::log_event::LogEvent;
use crate::map_def::{MapDef, MapError, MAX_PLAYERS, MIN_PLAYERS, PlayerRange};
use crate::types::{DevCard, DevHand, PerPlayer, PlayerId, Res};
use rand::seq::SliceRandom;
use std::time::{Duration, Instant};
#[cfg(test)]
use super::PALETTE;
use super::{Game, Phase, Player, PlayerStats, Settings, TimerMode};

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
    pub(super) fn build(names: Vec<String>, colors: Vec<String>, mode: TimerMode, settings: Settings, map: &str, shuffle: bool, now: Instant) -> Game {
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
            stats: PlayerStats::default(),
            dev_played: false,
            traded_away: Res::ZERO,
            free_roads: 0,
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
            traded_away: Res::ZERO,
            setup_idx: 0,
            setup_vertex: None,
            discards: vec![],
            steal_candidates: vec![],
            volcano_queue: vec![],
            volcano_active: None,
            volcano_picks: vec![],
            setup_pick: false,
            damaged: vec![],
            lava: None,
            road_building_left: 0,
            trades: vec![],
            next_trade_id: 0,
            longest_road_holder: None,
            largest_army_holder: None,
            winner: None,
            log: vec![],
            log_dropped: 0,
            order,
            rotation: 0,
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
    pub(super) fn advance_setup(&mut self) {
        self.setup_idx += 1;
        let total = self.setup_steps();
        if self.setup_idx < total && !(0..self.buildings.len()).any(|v| self.settlement_free(v)) {
            // Only possible with many starting settlements: nobody can place any more.
            self.log(LogEvent::SetupEndedEarly);
            self.setup_idx = total;
        }
        if self.setup_idx >= total {
            self.current = self.order[0];
            self.turn_left = Duration::from_secs(self.timer_mode.secs());
            self.roll_left = None;
            if self.settings.speed {
                // No turns: the table is open to everybody, and the first automatic roll is one countdown away.
                self.phase = Phase::Main;
                self.rolled = true;
                self.rotation = 0;
            } else {
                self.phase = Phase::Roll;
                self.rolled = false;
            }
            self.log(LogEvent::SetupComplete);
        } else {
            self.current = self.setup_player(self.setup_idx);
        }
    }
}
