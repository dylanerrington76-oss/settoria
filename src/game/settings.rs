//! The rules of one game that the host can change (`Settings`), and what they must satisfy.

use crate::types::{ByDev, DevCard};
use serde::{Deserialize, Serialize};

/// Most starting settlements (summed over all players) the board can reasonably hold.
/// (Twelve players with the most starting settlements each, 5, is 60.)
pub const MAX_SETUP_SETTLEMENTS_TOTAL: usize = 60;
/// Standard development deck: knights, road building, year of plenty, monopoly, victory points, alchemists.
/// (In a 25-card deck: 14 knights, 2 + 2 + 2, 5 victory points and no alchemist.)
const STANDARD_DECK: ByDev<usize> = ByDev([14, 2, 2, 2, 5, 0]);
/// The mix when `Settings::alchemist` is on: two Alchemists take their place from the knights, which go from
/// 14 to 12 (in a 25-card deck: 12 knights, 2 + 2 + 2, 5 victory points and 2 alchemists).
const ALCHEMIST_DECK: ByDev<usize> = ByDev([12, 2, 2, 2, 5, 2]);

/// The rules of one game. `Settings::default()` is the host's starting point.
/// Every field has a range (see `validate`), and the server refuses anything outside it.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    /// Points needed to win, checked on the player's own turn.
    pub points_to_win: u8,
    /// After a 7, players holding MORE than this many cards discard half. With `variable_discard` this is
    /// the limit the game starts with.
    pub discard_limit: u8,
    /// The discard limit rises as the game goes on: by one after every `discard_step_rolls` rolls of the dice
    /// (every roll of the game counts, whoever rolls it), until it reaches `discard_limit_cap`.
    pub variable_discard: bool,
    /// How many rolls of the dice make the discard limit rise by one (only with `variable_discard`).
    pub discard_step_rolls: u8,
    /// The highest the discard limit can climb to (only with `variable_discard`; never below `discard_limit`).
    pub discard_limit_cap: u8,
    /// The Alchemist development card is in the deck. Played at the start of a turn, before the dice are
    /// rolled, it lets its owner choose what the dice show. See `Game::do_alchemist`. Not played in Speed
    /// mode, which has no turns to start, so the deck is the standard one there whatever this says.
    pub alchemist: bool,
    /// Robber can't go on a hex touching a player with `Settings::friendly_robber_max_vp` or fewer points
    /// (what every player starts the game with, so it follows the starting settlements setting).
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
    /// Settoria Speed: there are no turns. After setup the server rolls the dice every `SPEED_ROLL_SECS`
    /// seconds, and every player can build and trade at any time. See `Game::do_speed`.
    pub speed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            points_to_win: 15,
            discard_limit: 7,
            variable_discard: false,
            discard_step_rolls: 10,
            discard_limit_cap: 10,
            alchemist: false,
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
            speed: false,
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
    /// Friendly Robber protects players with this many visible points or fewer: the points every player has
    /// once setup is over (the starting settlements, at the settlement value), so nobody is fair game for
    /// the robber just because the game started with more than two settlements each.
    pub fn friendly_robber_max_vp(&self) -> usize {
        self.starting_settlements as usize * self.settlement_points as usize
    }

    /// Reject out-of-range values and combinations that cannot be played.
    pub fn validate(&self) -> Result<(), String> {
        in_range("Points to win", self.points_to_win, 3, 99)?;
        in_range("Card discard limit", self.discard_limit, 1, 50)?;
        in_range("Rolls per discard limit increase", self.discard_step_rolls, 1, 99)?;
        in_range("Discard limit cap", self.discard_limit_cap, 1, 50)?;
        if self.variable_discard && self.discard_limit_cap < self.discard_limit {
            return Err("The discard limit cap can't be lower than the starting discard limit".into());
        }
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

    /// The discard limit for a 7 that is rolled after `rolls_before` earlier rolls of the dice. Without
    /// `variable_discard` that is always `discard_limit`; with it, the limit starts there, goes up by one after
    /// every `discard_step_rolls` rolls, and stops at `discard_limit_cap`.
    pub fn discard_limit_after(&self, rolls_before: u32) -> u8 {
        if !self.variable_discard {
            return self.discard_limit;
        }
        let rises = rolls_before / u32::from(self.discard_step_rolls.max(1));
        let cap = u32::from(self.discard_limit_cap.max(self.discard_limit));
        (u32::from(self.discard_limit) + rises).min(cap) as u8
    }

    /// The rules a table of `players` actually plays with. With more than 4 players the shared bank and the
    /// development deck would run dry far too quickly, so each one that the host left at the standard size
    /// grows: 5 or 6 players get 24 cards of each resource and a 34-card deck (the 5-6 player extension of the
    /// board game), 7 or 8 get 30 and 44, 9 or 10 get 36 and 54, and 11 or 12 get 42 and 60 (the largest deck
    /// a table can choose). A value the host changed is never touched.
    pub fn scaled_for(mut self, players: usize) -> Settings {
        let d = Settings::default();
        let (bank, deck) = match players {
            0..=4 => return self,
            5..=6 => (24, 34),
            7..=8 => (30, 44),
            9..=10 => (36, 54),
            _ => (42, 60),
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
        if self.variable_discard {
            v.push(format!(
                "variable discard limit (up 1 every {} rolls, to at most {})",
                self.discard_step_rolls, self.discard_limit_cap
            ));
        }
        if self.alchemist {
            v.push("Alchemist card".to_string());
        }
        if self.friendly_robber {
            v.push("Friendly Robber on".to_string());
        }
        if self.bridges {
            v.push("Bridges on".to_string());
        }
        if self.speed {
            v.push("Speed mode".to_string());
        }
        v
    }

    /// Whether the deck holds Alchemists: the setting is on and the game has turns (Speed mode has none, so a
    /// card played "before the roll" would mean nothing there).
    pub fn alchemist_in_deck(&self) -> bool {
        self.alchemist && !self.speed
    }

    /// Cards of each kind in a deck of `dev_cards`: the standard 14/2/2/2/5 mix (12/2/2/2/5 and two Alchemists with
    /// `alchemist` on) scaled to the chosen size (largest-remainder rounding, so the total is exact).
    pub fn deck_counts(&self) -> ByDev<usize> {
        let mix = if self.alchemist_in_deck() { &ALCHEMIST_DECK } else { &STANDARD_DECK };
        let total: usize = mix.0.iter().sum();
        let want = self.dev_cards as usize;
        let mut counts = ByDev::from_fn(|c| mix[c] * want / total);
        let left = want - counts.0.iter().sum::<usize>();
        let mut order = DevCard::ALL;
        order.sort_by_key(|&c| std::cmp::Reverse(mix[c] * want % total));
        for &c in order.iter().cycle().take(left) {
            counts[c] += 1;
        }
        counts
    }
}
