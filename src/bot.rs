//! Computer players ("bots") for a table that is short of people.
//!
//! A bot sees the game as a player at its seat would: its own hand and development cards, and for everyone else
//! only what is public (card counts, points, buildings). Every time it has something to do, it lists the moves
//! that are legal right now, gives each one a single number and takes the highest. Ties are broken at random.
//! Points and production dominate:
//!
//! ```text
//! score = points_gained * 100 + production_gained * 10 + small bonuses
//! ```
//!
//! `production_gained` is counted in pips (a 6 or 8 is five pips, a 2 or 12 one), and a pip is worth `PIP`, so a
//! spot's production is already in the same units as a point. Everything below is in those units; the constants
//! at the top are the knobs.
//!
//! | Move | Score |
//! |---|---|
//! | Settlement | `settlement_points * 100` + the spot (its pips, a bonus for each resource the bot doesn't produce yet, a bonus for a harbour that matches something it produces) |
//! | City | `(city_points - settlement_points) * 100` + the spot's pips (a city doubles production) |
//! | Road, bridge | the best vertex it newly unlocks, discounted by how many more roads that vertex is away; kept low while a settlement can already be built |
//! | Development card | a mid-range baseline, raised when the bot holds spare ore, grain and wool and nothing else is affordable |
//! | Bank trade | only if it leads to a build the bot can afford this turn; a share of what that build is worth |
//! | Robber | the opponent pips on a hex, doubled for cities and weighted toward the public-points leader; a heavy penalty for the bot's own buildings |
//! | Steal | the victim with the most cards, then the most public points |
//! | Discard, volcano or gold pick | keep, or take, what completes the bot's current target (its best build, affordable or not) |
//! | End turn | 0, so the bot ends its turn when nothing scores above it |
//!
//! The spec above is silent about playing development cards and about player-to-player trades, so:
//! knights, Road Building, Year of Plenty and Monopoly are scored in the same units (see `Mind::play_dev_cards`),
//! and a bot never proposes a trade and declines every offer it is sent.
//!
//! A bot is also held to `MAX_ACTIONS_PER_TURN` actions in a turn; after that it ends its turn, so a move that
//! scores badly can never keep it busy for ever.
//!
//! `choose` is the whole brain: it looks at a `Game` and says what one seat would do. `Driver` is what the
//! server runs: it gives the bots turns at a human pace, applies their moves, and counts their actions.

use crate::game::{Action, Game, Phase, Reply, COST_BRIDGE, COST_CITY, COST_DEV, COST_ROAD, COST_SETTLEMENT};
use crate::types::{ByResource, DevCard, PlayerId, Res, Resource};
use rand::seq::SliceRandom;
use rand::Rng;
use std::time::{Duration, Instant};

/// Most actions a bot takes in one turn (the roll counts) before its turn is ended for it.
pub const MAX_ACTIONS_PER_TURN: u32 = 15;

/// A point, and a pip of production, in score units.
const POINT: i32 = 100;
const PIP: i32 = 10;

/// Small bonuses for a settlement spot.
const NEW_RESOURCE_BONUS: i32 = 8;
const HARBOUR_MATCH_BONUS: i32 = 12;
const HARBOUR_ANY_BONUS: i32 = 5;

/// A road shows its value as the best vertex it unlocks: half that spot's score if it can be settled as soon as the
/// road is built, a quarter if one more road is needed first, an eighth if two more are.
const ROAD_LOOKAHEAD: usize = 2;
/// While a settlement can already be built, a road is only worth a quarter of that.
const ROAD_WHEN_SETTLING_DIVISOR: i32 = 4;

/// A development card is worth the baseline; with ore, grain and wool to spare it is worth more, and more again if
/// nothing else the bot wants is affordable.
const DEV_BASE: i32 = 30;
const DEV_SPARE_BONUS: i32 = 30;
const DEV_IDLE_BONUS: i32 = 30;

/// Rolling the dice. A knight played first has to be worth more than this to go before it.
const ROLL_SCORE: i32 = 50;
/// A knight is worth a little even with nothing to rob, because it counts toward Largest Army.
const KNIGHT_BASE: i32 = 15;

/// A bank trade is worth this many tenths of the build it makes affordable, and only for a build worth at least
/// `TRADE_MIN_TARGET`, so cards are never swapped away for a road that goes nowhere.
const TRADE_SHARE_TENTHS: i32 = 6;
const TRADE_MIN_TARGET: i32 = 30;

/// What a Year of Plenty that completes a build is worth, in tenths of that build.
const PLENTY_SHARE_TENTHS: i32 = 9;
/// Cheapest Monopoly (in expected cards, times ten) the bot bothers to play, and Road Building (in road value).
const MONOPOLY_MIN: i32 = 30;
const ROAD_BUILDING_MIN: i32 = 15;

/// Moving the robber onto a hex that holds one of the bot's own buildings costs this much, per building.
const OWN_HEX_PENALTY: i32 = 1000;

/// How long a bot takes over a move, so people can follow what it does.
const THINK_MS_MIN: u64 = 450;
const THINK_MS_MAX: u64 = 900;
/// How long the bots keep still after a move the rules refused (which should never happen).
const REFUSED_PAUSE: Duration = Duration::from_secs(2);

// ----------------------------------------------------------------------
// The board, in numbers
// ----------------------------------------------------------------------

/// Pips of a number token: 5 for a 6 or 8, down to 1 for a 2 or 12, 0 for no token.
fn pips(number: Option<u8>) -> i32 {
    match number {
        Some(n @ 2..=12) if n != 7 => 6 - (7 - i32::from(n)).abs(),
        _ => 0,
    }
}

/// What a hex yields to a building on it, in score units. A volcano gives a free pick of any resource when its
/// number is rolled, but its lava can wreck the building, so it counts for half. Gold gives the same free pick
/// with no lava, so it counts in full.
fn hex_value(g: &Game, hex: usize) -> i32 {
    let h = &g.board.hexes[hex];
    let value = pips(h.number) * PIP;
    if h.terrain.is_volcano() {
        value / 2
    } else if h.terrain.is_gold() || h.terrain.resource().is_some() {
        value
    } else {
        0
    }
}

/// Production at a corner, in score units: what a settlement there yields, and what a city there yields extra.
fn spot_production(g: &Game, v: usize) -> i32 {
    g.board.vertices[v].hexes.iter().map(|&h| hex_value(g, h)).sum()
}

/// Corners where `me` could put a settlement, whoever's turn it is and whatever is in their hand.
fn settlement_sites(g: &Game, me: PlayerId) -> Vec<usize> {
    if g.players[me].settlements_left == 0 {
        return vec![];
    }
    (0..g.buildings.len()).filter(|&v| g.settlement_connected(me, v)).collect()
}

/// Settlements `me` could turn into cities (only the damaged one first, if lava left them one to repair).
fn city_sites(g: &Game, me: PlayerId) -> Vec<usize> {
    if g.players[me].cities_left == 0 {
        return vec![];
    }
    let repairing = g.has_damaged(me);
    (0..g.buildings.len())
        .filter(|&v| matches!(g.buildings[v], Some(b) if b.owner == me && !b.city) && (!repairing || g.damaged.contains(&v)))
        .collect()
}

// ----------------------------------------------------------------------
// What a bot could build
// ----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Settlement(usize),
    City(usize),
    Road(usize),
    Bridge(usize),
    Dev,
}

/// Something the bot could build, wherever it could be, and what completing it is worth. Not necessarily affordable.
#[derive(Clone, Copy, Debug)]
struct Build {
    kind: Kind,
    cost: Res,
    value: i32,
}

impl Build {
    fn action(&self) -> Action {
        match self.kind {
            Kind::Settlement(vertex) => Action::BuildSettlement { vertex },
            Kind::City(vertex) => Action::BuildCity { vertex },
            Kind::Road(edge) => Action::BuildRoad { edge },
            Kind::Bridge(edge) => Action::BuildBridge { edge },
            Kind::Dev => Action::BuyDev,
        }
    }
}

/// One seat's way of looking at the game.
struct Mind<'a> {
    g: &'a Game,
    me: PlayerId,
    /// Resources `me` already collects from a hex next to one of their buildings.
    produced: ByResource<bool>,
    /// How much the robber should want to hurt each seat, in percent: more for a player close to winning, and
    /// more again for the one with the most public points.
    pressure: Vec<i32>,
}

impl<'a> Mind<'a> {
    fn new(g: &'a Game, me: PlayerId) -> Mind<'a> {
        let mut produced = ByResource::splat(false);
        for (v, b) in g.buildings.iter().enumerate() {
            if matches!(b, Some(b) if b.owner == me) {
                for &h in &g.board.vertices[v].hexes {
                    if let Some(r) = g.board.hexes[h].terrain.resource() {
                        produced[r] = true;
                    }
                }
            }
        }
        let points: Vec<i32> = PlayerId::range(g.n).map(|p| g.vp_public(p) as i32).collect();
        let top = PlayerId::range(g.n).filter(|&p| p != me).map(|p| points[p.index()]).max().unwrap_or(0);
        let to_win = i32::from(g.settings.points_to_win).max(1);
        let pressure = points.iter().map(|&vp| 100 + vp * 100 / to_win + if vp == top { 50 } else { 0 }).collect();
        Mind { g, me, produced, pressure }
    }

    fn hand(&self) -> Res {
        self.g.players[self.me].hand
    }

    /// Whether `v` already belongs to `me`'s network: their building, or a road of theirs ends there.
    fn touches_me(&self, v: usize) -> bool {
        let g = self.g;
        matches!(g.buildings[v], Some(b) if b.owner == self.me) || g.board.vertices[v].edges.iter().any(|&e| g.roads[e] == Some(self.me))
    }

    /// What a settlement at `v` is worth beyond its points: production, resources the bot lacks, a harbour it can use.
    fn spot_score(&self, v: usize) -> i32 {
        let g = self.g;
        let mut score = spot_production(g, v);
        let mut here = ByResource::splat(false);
        for &h in &g.board.vertices[v].hexes {
            if let Some(r) = g.board.hexes[h].terrain.resource() {
                if !self.produced[r] && !here[r] {
                    score += NEW_RESOURCE_BONUS;
                }
                here[r] = true;
            }
        }
        for port in g.board.ports.iter().filter(|p| p.a == v || p.b == v) {
            score += match port.resource {
                None => HARBOUR_ANY_BONUS,
                Some(r) if self.produced[r] || here[r] => HARBOUR_MATCH_BONUS,
                Some(_) => 0,
            };
        }
        score
    }

    /// The best vertex a road (or bridge) on `edge` newly unlocks, as a score; `None` if it unlocks nothing worth
    /// having. A vertex is unlocked once the network reaches it. Reaching it takes the new road if the vertex is at
    /// the road's far end and further roads if it is beyond, and each extra road halves what it is worth.
    fn unlock_value(&self, edge: usize) -> Option<i32> {
        let g = self.g;
        let e = &g.board.edges[edge];
        let mut best: Option<i32> = None;
        for (far, near) in [(e.a, e.b), (e.b, e.a)] {
            if self.touches_me(far) {
                continue; // this end is already part of the network, so nothing new starts here
            }
            let mut seen = vec![near, far];
            let mut frontier = vec![far];
            for depth in 0..=ROAD_LOOKAHEAD {
                let mut next = Vec::new();
                for &v in &frontier {
                    if g.settlement_free(v) && !self.touches_me(v) {
                        let s = self.spot_score(v) >> (depth + 1);
                        if s > 0 && best.map_or(true, |b| s > b) {
                            best = Some(s);
                        }
                    }
                    // A road can't run on through a building (an opponent's blocks it), or over a road.
                    if depth < ROAD_LOOKAHEAD && g.buildings[v].is_none() {
                        for &ed in &g.board.vertices[v].edges {
                            let step = &g.board.edges[ed];
                            if ed == edge || g.roads[ed].is_some() || (step.bridge && !g.settings.bridges) {
                                continue;
                            }
                            let w = step.other(v);
                            if !seen.contains(&w) {
                                seen.push(w);
                                next.push(w);
                            }
                        }
                    }
                }
                frontier = next;
            }
        }
        best
    }

    /// What a road on `edge` is worth: what it unlocks, less while the bot has a settlement it could build already.
    /// A road that unlocks nothing scores below ending the turn.
    fn road_value(&self, edge: usize, settling: bool) -> i32 {
        match self.unlock_value(edge) {
            Some(s) if settling => (s / ROAD_WHEN_SETTLING_DIVISOR).max(1),
            Some(s) => s,
            None => -1,
        }
    }

    /// Everything the bot could build, where and for what price, with what each is worth. This is also the bot's
    /// idea of what to save for: see `target_cost`.
    fn builds(&self) -> Vec<Build> {
        let g = self.g;
        let me = self.me;
        let s = &g.settings;
        let mut out = Vec::new();

        let sites = settlement_sites(g, me);
        let settling = !sites.is_empty();
        for v in sites {
            out.push(Build { kind: Kind::Settlement(v), cost: COST_SETTLEMENT, value: i32::from(s.settlement_points) * POINT + self.spot_score(v) });
        }
        for v in city_sites(g, me) {
            let points = i32::from(s.city_points) - i32::from(s.settlement_points);
            out.push(Build { kind: Kind::City(v), cost: COST_CITY, value: points * POINT + spot_production(g, v) });
        }
        for e in g.legal_roads(me) {
            out.push(Build { kind: Kind::Road(e), cost: COST_ROAD, value: self.road_value(e, settling) });
        }
        for e in g.legal_bridges(me) {
            out.push(Build { kind: Kind::Bridge(e), cost: COST_BRIDGE, value: self.road_value(e, settling) });
        }
        if !g.deck.is_empty() {
            let value = self.dev_value(&out);
            out.push(Build { kind: Kind::Dev, cost: COST_DEV, value });
        }
        out
    }

    /// A development card: the baseline, more with ore, grain and wool to spare after saving for the best other build,
    /// and more again if none of the other builds worth having is affordable.
    fn dev_value(&self, others: &[Build]) -> i32 {
        let hand = self.hand();
        let saving = others.iter().filter(|b| b.value > 0).max_by_key(|b| b.value).map_or(Res::ZERO, |b| b.cost);
        let spare = ByResource::from_fn(|r| hand[r].saturating_sub(saving[r]));
        let mut value = DEV_BASE;
        if spare.covers(&COST_DEV) {
            value += DEV_SPARE_BONUS;
            if !others.iter().any(|b| b.value > 0 && hand.covers(&b.cost)) {
                value += DEV_IDLE_BONUS;
            }
        }
        value
    }

    /// The price of the best build there is, affordable or not: what the bot is saving for.
    fn target_cost(&self) -> Res {
        self.builds().iter().filter(|b| b.value > 0).max_by_key(|b| b.value).map_or(Res::ZERO, |b| b.cost)
    }

    /// The first bank trade of the cheapest way to cover what `cost` lacks using only cards the bot doesn't need
    /// for it, if there is one. Each trade brings in one card, at the best rate the bot has for what it gives.
    fn trade_towards(&self, cost: &Res) -> Option<(Resource, Resource)> {
        let g = self.g;
        let hand = self.hand();
        let mut spare = ByResource::from_fn(|r| i32::from(hand[r]) - i32::from(cost[r]));
        let mut bank = g.bank;
        let mut first = None;
        for get in Resource::ALL {
            for _ in 0..cost[get].saturating_sub(hand[get]) {
                if bank[get] == 0 {
                    return None;
                }
                let give = Resource::ALL
                    .into_iter()
                    .filter(|&r| r != get && spare[r] >= i32::from(g.ratio(self.me, r)))
                    .min_by_key(|&r| (g.ratio(self.me, r), -spare[r]))?;
                spare[give] -= i32::from(g.ratio(self.me, give));
                bank[get] -= 1;
                first.get_or_insert((give, get));
            }
        }
        first
    }

    // ------------------------------------------------------------------
    // The robber
    // ------------------------------------------------------------------

    /// What putting the robber on `hex` is worth to `me`: the production of every opponent building on it (doubled for
    /// a city, weighted toward whoever is closest to winning), less a lot for each building of the bot's own.
    fn robber_value(&self, hex: usize) -> i32 {
        let g = self.g;
        let value = hex_value(g, hex);
        let mut score = 0;
        for &v in &g.board.hexes[hex].vertices {
            let Some(b) = g.buildings[v] else { continue };
            let size = if b.city { 2 } else { 1 };
            if b.owner == self.me {
                score -= OWN_HEX_PENALTY + value * size;
            } else {
                score += value * size * self.pressure[b.owner.index()] / 100;
            }
        }
        score
    }

    /// Production of the bot's own that the robber is sitting on.
    fn blocked(&self) -> i32 {
        let g = self.g;
        let value = hex_value(g, g.robber);
        g.board.hexes[g.robber]
            .vertices
            .iter()
            .filter_map(|&v| g.buildings[v])
            .filter(|b| b.owner == self.me)
            .map(|b| value * if b.city { 2 } else { 1 })
            .sum()
    }

    /// What one more knight would give: the points if it takes Largest Army.
    fn army_gain(&self) -> i32 {
        let g = self.g;
        let knights = i32::from(g.players[self.me].knights) + 1;
        let beats = match g.largest_army_holder {
            Some(h) if h == self.me => false,
            Some(h) => knights > i32::from(g.players[h].knights),
            None => true,
        };
        if knights >= i32::from(g.settings.largest_army_min) && beats {
            i32::from(g.settings.largest_army_points) * POINT
        } else {
            0
        }
    }

    // ------------------------------------------------------------------
    // Moves, phase by phase
    // ------------------------------------------------------------------

    fn setup_move<R: Rng>(&self, rng: &mut R) -> Option<Action> {
        let g = self.g;
        let legal = g.legal(self.me);
        let mut moves = Vec::new();
        for &vertex in &legal.settlements {
            moves.push((i32::from(g.settings.settlement_points) * POINT + self.spot_score(vertex), Action::BuildSettlement { vertex }));
        }
        for &edge in &legal.roads {
            moves.push((self.unlock_value(edge).unwrap_or(-1), Action::BuildRoad { edge }));
        }
        pick(moves, rng)
    }

    fn roll_move<R: Rng>(&self, rng: &mut R) -> Option<Action> {
        let mut moves = vec![(ROLL_SCORE, Action::Roll)];
        if self.can_play(DevCard::Knight) {
            // Before the roll a knight only goes first if it frees the bot's own production or wins Largest Army.
            moves.push((self.blocked() + self.army_gain(), Action::PlayKnight));
        }
        pick(moves, rng)
    }

    fn robber_move<R: Rng>(&self, rng: &mut R) -> Option<Action> {
        let moves = self.g.robber_hexes(self.me).into_iter().map(|hex| (self.robber_value(hex), Action::MoveRobber { hex })).collect();
        pick(moves, rng)
    }

    fn steal_move<R: Rng>(&self, rng: &mut R) -> Option<Action> {
        let g = self.g;
        let moves = g
            .steal_candidates
            .iter()
            .map(|&victim| (g.players[victim].hand.total() as i32 * 100 + g.vp_public(victim) as i32, Action::Steal { victim }))
            .collect();
        pick(moves, rng)
    }

    /// A road from a Road Building card is free, so it is worth what it unlocks, whether or not a settlement is waiting.
    fn free_road_move<R: Rng>(&self, rng: &mut R) -> Option<Action> {
        let moves = self.g.legal_roads(self.me).into_iter().map(|edge| (self.unlock_value(edge).unwrap_or(-1), Action::BuildRoad { edge })).collect();
        pick(moves, rng)
    }

    /// `need` cards to throw away: from the biggest pile beyond what the target needs, one card at a time.
    fn discard<R: Rng>(&self, need: u32, rng: &mut R) -> Res {
        let cost = self.target_cost();
        let mut hand = self.hand();
        let mut out = Res::ZERO;
        for _ in 0..need {
            let surplus = |r: Resource| i32::from(hand[r]) - i32::from(cost[r]);
            let Some(top) = Resource::ALL.into_iter().filter(|&r| hand[r] > 0).map(surplus).max() else { break };
            let tied: Vec<Resource> = Resource::ALL.into_iter().filter(|&r| hand[r] > 0 && surplus(r) == top).collect();
            let Some(&r) = tied.choose(rng) else { break };
            hand[r] -= 1;
            out[r] += 1;
        }
        out
    }

    /// The free resource that is furthest from completing the target (any, if it is already covered).
    fn volcano_pick<R: Rng>(&self, rng: &mut R) -> Option<Resource> {
        let cost = self.target_cost();
        let hand = self.hand();
        let lacking = |r: Resource| i32::from(cost[r]) - i32::from(hand[r]);
        let open: Vec<Resource> = Resource::ALL.into_iter().filter(|&r| self.g.bank[r] > 0).collect();
        let top = open.iter().copied().map(lacking).max()?;
        let tied: Vec<Resource> = open.into_iter().filter(|&r| lacking(r) == top).collect();
        tied.choose(rng).copied()
    }

    fn can_play(&self, card: DevCard) -> bool {
        !self.g.dev_played && self.g.players[self.me].dev[card] > 0
    }

    fn main_move<R: Rng>(&self, rng: &mut R) -> Option<Action> {
        let hand = self.hand();
        let builds = self.builds();
        let mut moves = vec![(0, Action::EndTurn)];

        for b in &builds {
            if hand.covers(&b.cost) {
                moves.push((b.value, b.action()));
            } else if b.value >= TRADE_MIN_TARGET {
                // Not affordable yet: trade surplus for what is missing, if that gets it built this turn.
                if let Some((give, get)) = self.trade_towards(&b.cost) {
                    moves.push((b.value * TRADE_SHARE_TENTHS / 10, Action::BankTrade { give, get }));
                }
            }
        }
        self.play_dev_cards(&builds, &mut moves);
        pick(moves, rng)
    }

    /// Playing the development cards the bot holds, in the same units as everything else.
    ///
    /// * Knight: the robber's best move (half), the bot's own production it frees, and Largest Army if it takes it.
    /// * Road Building: twice the best road it can place.
    /// * Year of Plenty: most of what a build it completes is worth.
    /// * Monopoly: what it expects to take (it can't see hands, so a fifth of the cards the others hold) plus a bonus
    ///   if the resource is one it is short of.
    fn play_dev_cards(&self, builds: &[Build], moves: &mut Vec<(i32, Action)>) {
        let g = self.g;
        let me = self.me;
        let hand = self.hand();

        if self.can_play(DevCard::Knight) {
            let best_robbing = g.robber_hexes(me).into_iter().map(|h| self.robber_value(h)).max().unwrap_or(0).max(0);
            moves.push((KNIGHT_BASE + self.blocked() + self.army_gain() + best_robbing / 2, Action::PlayKnight));
        }

        if self.can_play(DevCard::RoadBuilding) {
            let best = g.legal_roads(me).into_iter().filter_map(|e| self.unlock_value(e)).max().unwrap_or(0);
            if best >= ROAD_BUILDING_MIN {
                moves.push((best * 2, Action::PlayRoadBuilding));
            }
        }

        if self.can_play(DevCard::YearOfPlenty) {
            for b in builds.iter().filter(|b| b.value >= TRADE_MIN_TARGET && !hand.covers(&b.cost)) {
                for a in Resource::ALL {
                    for c in Resource::ALL.into_iter().filter(|c| c.index() >= a.index()) {
                        let in_stock = if a == c { g.bank[a] >= 2 } else { g.bank[a] >= 1 && g.bank[c] >= 1 };
                        let mut after = hand;
                        after[a] = after[a].saturating_add(1);
                        after[c] = after[c].saturating_add(1);
                        if in_stock && after.covers(&b.cost) {
                            moves.push((b.value * PLENTY_SHARE_TENTHS / 10, Action::PlayYearOfPlenty { a, b: c }));
                        }
                    }
                }
            }
        }

        if self.can_play(DevCard::Monopoly) {
            let others: i32 = PlayerId::range(g.n).filter(|&p| p != me).map(|p| g.players[p].hand.total() as i32).sum();
            let saving = self.target_cost();
            let lacking = |r: Resource| i32::from(saving[r]) - i32::from(hand[r]);
            let best = Resource::ALL.into_iter().filter(|&r| !g.monopoly_locked(r)).max_by_key(|&r| (lacking(r), !self.produced[r]));
            if let Some(resource) = best {
                // A fifth of the other players' cards, in tenths of a card.
                let value = others * 2 + if lacking(resource) > 0 { 20 } else { 0 };
                if value >= MONOPOLY_MIN {
                    moves.push((value, Action::PlayMonopoly { resource }));
                }
            }
        }
    }
}

/// The highest-scoring move, chosen at random among those that tie for it.
fn pick<R: Rng>(mut moves: Vec<(i32, Action)>, rng: &mut R) -> Option<Action> {
    let top = moves.iter().map(|m| m.0).max()?;
    let tied: Vec<usize> = moves.iter().enumerate().filter(|(_, m)| m.0 == top).map(|(i, _)| i).collect();
    let i = *tied.choose(rng)?;
    Some(moves.swap_remove(i).1)
}

/// What the bot at seat `me` would do right now, or `None` if there is nothing for it to do (it is not its turn, it
/// is waiting on someone else, or the game is over). `acted` is how many actions it has already taken this turn.
pub fn choose<R: Rng>(g: &Game, me: PlayerId, acted: u32, rng: &mut R) -> Option<Action> {
    if g.phase == Phase::GameOver || me.index() >= g.n {
        return None;
    }
    // Some things are asked of seats that are not the one whose turn it is.
    match g.phase {
        Phase::Discard => {
            let &(_, need) = g.discards.iter().find(|d| d.0 == me)?;
            return Some(Action::Discard { cards: Mind::new(g, me).discard(u32::from(need), rng) });
        }
        Phase::VolcanoPick => {
            return match g.volcano_picks.first() {
                Some(&(p, _)) if p == me => Mind::new(g, me).volcano_pick(rng).map(|resource| Action::VolcanoPick { resource }),
                _ => None,
            };
        }
        _ => {}
    }
    if g.current != me {
        // Bots don't trade with people: any offer that is waiting for an answer from this seat is declined.
        return g.trades.iter().find(|t| t.replies.get(me) == Some(&Reply::Pending)).map(|t| Action::RespondTrade { id: t.id, accept: false });
    }
    let mind = Mind::new(g, me);
    match g.phase {
        Phase::Setup => mind.setup_move(rng),
        Phase::Roll => mind.roll_move(rng),
        Phase::MoveRobber => mind.robber_move(rng),
        Phase::Steal => mind.steal_move(rng),
        Phase::RoadBuilding => mind.free_road_move(rng),
        // Out of actions: stop, rather than let a bad score keep the bot busy.
        Phase::Main if acted >= MAX_ACTIONS_PER_TURN => Some(Action::EndTurn),
        Phase::Main => mind.main_move(rng),
        Phase::Discard | Phase::VolcanoPick | Phase::GameOver => None,
    }
}

/// The names bots are given. A table seats at most 8 and the host is always a person, so there are always enough.
pub const NAMES: [&str; 7] = ["Bender", "C-3PO", "Multivac", "Skynet", "AM", "Hal 9000", "Data"];

/// A name for a new bot: drawn at random from `NAMES`, leaving out every name in `taken` (the names at the table
/// already, so nobody is ever named twice, and a name freed by taking a bot away can be drawn again). A person who
/// is called one of these names (in any capitals) keeps it to themselves. If none is left, which takes a table of
/// people who have all picked such names, the bot is "Bot 1", "Bot 2", ... instead.
pub fn bot_name<'a, R: Rng>(taken: impl Iterator<Item = &'a str> + Clone, rng: &mut R) -> String {
    let free = |name: &str| !taken.clone().any(|t| t.trim().eq_ignore_ascii_case(name));
    let unused: Vec<&str> = NAMES.iter().copied().filter(|&name| free(name)).collect();
    match unused.choose(rng) {
        Some(name) => (*name).to_string(),
        None => (1..).map(|i| format!("Bot {i}")).find(|name| free(name)).expect("there are only a few seats"),
    }
}

/// Runs the bots of one table: whichever has something to do moves, one move at a time, at a pace people can follow.
pub struct Driver {
    /// Actions each seat has taken in its current turn (reset when the turn passes).
    acted: Vec<u32>,
    /// Earliest moment of the next move.
    next: Instant,
}

impl Driver {
    /// A driver whose first move comes a moment after `now`, so the game is on screen before a bot acts.
    pub fn new<R: Rng>(now: Instant, rng: &mut R) -> Driver {
        Driver { acted: Vec::new(), next: now + think_time(rng) }
    }

    /// Let one bot make one move if any has something to do and the pause since the last move is over.
    /// Returns whether the game changed (and so should be sent to everyone).
    pub fn step<R: Rng>(&mut self, g: &mut Game, is_bot: impl Fn(PlayerId) -> bool, now: Instant, rng: &mut R) -> bool {
        if now < self.next {
            return false;
        }
        if self.acted.len() != g.n {
            self.acted = vec![0; g.n];
        }
        // A seat's count is for its current turn only.
        for p in PlayerId::range(g.n) {
            if p != g.current {
                self.acted[p.index()] = 0;
            }
        }
        for p in PlayerId::range(g.n).filter(|&p| is_bot(p)) {
            let Some(action) = choose(g, p, self.acted[p.index()], rng) else { continue };
            let on_turn = p == g.current;
            let ends_turn = matches!(action, Action::EndTurn);
            let shown = format!("{action:?}");
            return match g.apply_at(p, action, now) {
                Ok(()) => {
                    self.acted[p.index()] = if ends_turn { 0 } else { self.acted[p.index()] + u32::from(on_turn) };
                    self.next = now + think_time(rng);
                    true
                }
                Err(why) => {
                    // The bot only ever picks moves the rules list as legal, so this is a bug; say so, and let the
                    // game's own clocks play the seat's turn if it keeps happening.
                    eprintln!("bot at seat {} tried {shown}, and the rules refused it: {why}", p.index());
                    self.next = now + REFUSED_PAUSE;
                    false
                }
            };
        }
        false
    }
}

fn think_time<R: Rng>(rng: &mut R) -> Duration {
    Duration::from_millis(rng.gen_range(THINK_MS_MIN..=THINK_MS_MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Port, Terrain};
    use crate::game::{Building, Settings, TimerMode, PALETTE};
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    const P0: PlayerId = PlayerId::new(0);
    const P1: PlayerId = PlayerId::new(1);
    const P2: PlayerId = PlayerId::new(2);

    fn names(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("P{i}")).collect()
    }

    fn rng() -> StdRng {
        StdRng::seed_from_u64(7)
    }

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    /// What `me` would do, asked with a fresh random generator each time (so a test that passes is not just lucky).
    fn all_picks(g: &Game, me: PlayerId, acted: u32) -> Vec<Option<Action>> {
        (0..12).map(|seed| choose(g, me, acted, &mut StdRng::seed_from_u64(seed))).collect()
    }

    /// Every producing hex gets a 2 (one pip) and the harbours go, so a test sets the numbers that matter itself.
    fn flat(g: &mut Game) {
        for h in g.board.hexes.iter_mut() {
            if h.terrain.resource().is_some() {
                h.number = Some(2);
            }
        }
        g.board.ports.clear();
    }

    /// A corner with three producing hexes around it.
    fn solid(g: &Game, v: usize) -> bool {
        let hexes = &g.board.vertices[v].hexes;
        hexes.len() == 3 && hexes.iter().all(|&h| g.board.hexes[h].terrain.resource().is_some())
    }

    fn set_numbers(g: &mut Game, v: usize, numbers: [u8; 3]) {
        let hexes = g.board.vertices[v].hexes.clone();
        for (h, n) in hexes.into_iter().zip(numbers) {
            g.board.hexes[h].number = Some(n);
        }
    }

    fn edge_between(g: &Game, a: usize, b: usize) -> usize {
        *g.board.vertices[a].edges.iter().find(|&&e| g.board.edges[e].other(a) == b).expect("neighbours share an edge")
    }

    /// Two producing hexes that share no corner.
    fn apart(g: &Game) -> (usize, usize) {
        let live: Vec<usize> = (0..g.board.hexes.len()).filter(|&h| g.board.hexes[h].terrain.resource().is_some()).collect();
        for &x in &live {
            for &y in &live {
                if g.board.hexes[x].vertices.iter().all(|v| !g.board.hexes[y].vertices.contains(v)) {
                    return (x, y);
                }
            }
        }
        panic!("no two hexes apart");
    }

    /// Player 0 has a settlement on `v` and, if `roads`, two roads leading to `b` (a corner two steps away, where a
    /// settlement could go). The game is in the main phase of player 0's turn.
    struct Homestead {
        v: usize,
        b: usize,
    }

    fn homestead(g: &mut Game, roads: bool) -> Homestead {
        for v in 0..g.board.vertices.len() {
            if !solid(g, v) {
                continue;
            }
            for &a in &g.board.vertices[v].neighbors {
                for &b in &g.board.vertices[a].neighbors {
                    if b != v && solid(g, b) && !g.board.vertices[v].neighbors.contains(&b) {
                        g.buildings[v] = Some(Building { owner: P0, city: false });
                        if roads {
                            let (e1, e2) = (edge_between(g, v, a), edge_between(g, a, b));
                            g.roads[e1] = Some(P0);
                            g.roads[e2] = Some(P0);
                        }
                        g.phase = Phase::Main;
                        g.current = P0;
                        g.rolled = true;
                        return Homestead { v, b };
                    }
                }
            }
        }
        panic!("no place for a homestead");
    }

    // ------------------------------------------------------------------
    // Numbers
    // ------------------------------------------------------------------

    #[test]
    fn pips_follow_the_dice_odds() {
        assert_eq!([2, 3, 4, 5, 6, 8, 9, 10, 11, 12].map(|n| pips(Some(n))), [1, 2, 3, 4, 5, 5, 4, 3, 2, 1]);
        assert_eq!(pips(None), 0);
        assert_eq!(pips(Some(7)), 0);
    }

    #[test]
    fn bots_are_named_from_the_list_and_never_twice() {
        for seed in 0..20 {
            let mut rng = StdRng::seed_from_u64(seed);
            let mut taken: Vec<String> = vec!["Ann".to_string()]; // a person at the table too
            for _ in 0..NAMES.len() {
                let name = bot_name(taken.iter().map(String::as_str), &mut rng);
                assert!(NAMES.contains(&name.as_str()), "{name}");
                assert!(!taken.contains(&name), "{name} drawn twice");
                taken.push(name);
            }
            let mut drawn: Vec<&str> = taken[1..].iter().map(String::as_str).collect();
            drawn.sort();
            let mut all = NAMES.to_vec();
            all.sort();
            assert_eq!(drawn, all, "seven draws use all seven names");
        }
    }

    #[test]
    fn the_names_are_not_drawn_in_a_fixed_order() {
        let firsts: std::collections::HashSet<String> =
            (0..60).map(|seed| bot_name(std::iter::empty(), &mut StdRng::seed_from_u64(seed))).collect();
        assert!(firsts.len() > 3, "{firsts:?}");
    }

    #[test]
    fn a_name_that_is_taken_is_left_out_whatever_its_capitals_and_a_freed_one_comes_back() {
        for seed in 0..50 {
            let mut rng = StdRng::seed_from_u64(seed);
            let name = bot_name(["data", "  AM ", "Ann"].into_iter(), &mut rng);
            assert!(NAMES.contains(&name.as_str()) && name != "Data" && name != "AM", "{name}");
        }
        // Six of the seven are seated: the seventh is the only one that can come up.
        let six = ["Bender", "C-3PO", "Multivac", "Skynet", "AM", "Hal 9000"];
        assert_eq!(bot_name(six.into_iter(), &mut rng()), "Data");
    }

    #[test]
    fn with_no_name_left_a_numbered_one_is_used() {
        let all = NAMES.to_vec();
        assert_eq!(bot_name(all.iter().copied(), &mut rng()), "Bot 1");
        let more: Vec<&str> = NAMES.iter().copied().chain(["Bot 1", "Bot 2"]).collect();
        assert_eq!(bot_name(more.iter().copied(), &mut rng()), "Bot 3");
    }

    #[test]
    fn a_spot_is_worth_its_pips_new_resources_and_a_harbour_it_can_use() {
        let mut g = Game::new(names(2));
        flat(&mut g);
        let (s1, s2) = {
            let solids: Vec<usize> = (0..g.board.vertices.len()).filter(|&v| solid(&g, v)).collect();
            let pair = solids.iter().flat_map(|&a| solids.iter().map(move |&b| (a, b))).find(|&(a, b)| {
                g.board.vertices[a].hexes.iter().all(|h| !g.board.vertices[b].hexes.contains(h))
            });
            pair.expect("two corners with no hex in common")
        };
        let set = |g: &mut Game, v: usize, terrain: [Terrain; 3]| {
            for (h, t) in g.board.vertices[v].hexes.clone().into_iter().zip(terrain) {
                g.board.hexes[h].terrain = t;
            }
        };
        set(&mut g, s1, [Terrain::Hills, Terrain::Forest, Terrain::Fields]);
        set(&mut g, s2, [Terrain::Hills, Terrain::Hills, Terrain::Hills]);
        let score = |g: &Game, v: usize| Mind::new(g, P0).spot_score(v);

        // Same pips (three hexes of one pip each); the corner with three kinds of resource is worth two more bonuses.
        assert_eq!(spot_production(&g, s1), 3 * PIP);
        assert_eq!(spot_production(&g, s2), 3 * PIP);
        assert_eq!(score(&g, s1), 3 * PIP + 3 * NEW_RESOURCE_BONUS);
        assert_eq!(score(&g, s2), 3 * PIP + NEW_RESOURCE_BONUS);

        // A harbour only counts for much if it matches what the bot gets there.
        let base = score(&g, s2);
        let next = g.board.vertices[s2].neighbors[0];
        for (resource, bonus) in [(Some(Resource::Brick), HARBOUR_MATCH_BONUS), (Some(Resource::Ore), 0), (None, HARBOUR_ANY_BONUS)] {
            g.board.ports = vec![Port { a: s2, b: next, resource }];
            assert_eq!(score(&g, s2), base + bonus, "harbour {resource:?}");
        }
    }

    // ------------------------------------------------------------------
    // Setup, roll, and who is asked
    // ------------------------------------------------------------------

    #[test]
    fn setup_takes_the_strongest_corner_and_then_a_road_beside_it() {
        let mut g = Game::new(names(2));
        flat(&mut g);
        let best = (0..g.board.vertices.len()).find(|&v| solid(&g, v)).unwrap();
        set_numbers(&mut g, best, [6, 8, 5]);
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::BuildSettlement { vertex }) if vertex == best), "{pick:?}");
        }
        g.apply(P0, Action::BuildSettlement { vertex: best }).unwrap();
        for pick in all_picks(&g, P0, 1) {
            let Some(Action::BuildRoad { edge }) = pick else { panic!("expected a road, got {pick:?}") };
            assert!(g.board.edges[edge].a == best || g.board.edges[edge].b == best);
        }
    }

    #[test]
    fn a_bot_waits_for_its_turn() {
        let mut g = Game::new(names(2));
        assert!(all_picks(&g, P1, 0).iter().all(Option::is_none), "setup, player 0's move");
        homestead(&mut g, false);
        assert!(all_picks(&g, P1, 0).iter().all(Option::is_none), "main phase, player 0's turn");
        g.phase = Phase::GameOver;
        assert!(all_picks(&g, P0, 0).iter().all(Option::is_none), "game over");
    }

    #[test]
    fn it_rolls_unless_a_knight_frees_its_own_production() {
        let mut g = Game::new(names(2));
        flat(&mut g);
        let hex = (0..g.board.hexes.len()).find(|&h| g.board.hexes[h].terrain.resource().is_some()).unwrap();
        g.board.hexes[hex].number = Some(6);
        g.phase = Phase::Roll;
        g.current = P0;
        g.rolled = false;
        g.players[P0].dev[DevCard::Knight] = 1;
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::Roll)), "robber is not in the way: {pick:?}");
        }
        g.robber = hex;
        g.buildings[g.board.hexes[hex].vertices[0]] = Some(Building { owner: P0, city: true });
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::PlayKnight)), "the robber sits on its city: {pick:?}");
        }
    }

    // ------------------------------------------------------------------
    // Building
    // ------------------------------------------------------------------

    #[test]
    fn points_dominate_what_it_builds() {
        let both = Res::new(1, 1, 3, 3, 1); // a city and a settlement

        // A city on a strong corner beats a settlement on a weak one, though both are affordable.
        let mut g = Game::new(names(2));
        flat(&mut g);
        let h = homestead(&mut g, true);
        set_numbers(&mut g, h.v, [6, 8, 5]);
        g.players[P0].hand = both;
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::BuildCity { vertex }) if vertex == h.v), "{pick:?}");
        }

        // The other way round the settlement wins.
        let mut g = Game::new(names(2));
        flat(&mut g);
        let h = homestead(&mut g, true);
        set_numbers(&mut g, h.b, [6, 8, 5]);
        g.players[P0].hand = both;
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::BuildSettlement { vertex }) if vertex == h.b), "{pick:?}");
        }

        // A road is never preferred to a settlement it could lead to.
        g.players[P0].hand = Res::new(2, 2, 0, 1, 1); // a settlement and a road
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::BuildSettlement { vertex }) if vertex == h.b), "{pick:?}");
        }
        g.apply(P0, Action::BuildSettlement { vertex: h.b }).expect("the rules allow what the bot chose");

        // Even a settlement on a poor corner (a hundred points' worth) beats a development card it could afford too.
        let mut g = Game::new(names(2));
        flat(&mut g);
        let h = homestead(&mut g, true);
        g.players[P0].hand = Res::new(1, 1, 1, 2, 2); // a settlement, then ore, grain and wool left over for a card
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::BuildSettlement { vertex }) if vertex == h.b), "{pick:?}");
        }
    }

    #[test]
    fn a_road_is_built_only_if_it_leads_somewhere() {
        let mut g = Game::new(names(2));
        flat(&mut g);
        homestead(&mut g, false);
        g.players[P0].hand = COST_ROAD;
        for pick in all_picks(&g, P0, 0) {
            let Some(Action::BuildRoad { edge }) = pick else { panic!("a road to a free corner is worth more than nothing: {pick:?}") };
            assert!(g.legal(P0).roads.contains(&edge));
        }

        // With nothing worth settling anywhere, the same road is worth less than ending the turn.
        for h in g.board.hexes.iter_mut() {
            h.terrain = Terrain::Desert;
            h.number = None;
        }
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::EndTurn)), "{pick:?}");
        }
    }

    #[test]
    fn it_ends_its_turn_after_fifteen_actions() {
        let mut g = Game::new(names(2));
        flat(&mut g);
        let h = homestead(&mut g, false);
        g.players[P0].hand = COST_CITY;
        for pick in all_picks(&g, P0, MAX_ACTIONS_PER_TURN - 1) {
            assert!(matches!(pick, Some(Action::BuildCity { vertex }) if vertex == h.v), "{pick:?}");
        }
        for pick in all_picks(&g, P0, MAX_ACTIONS_PER_TURN) {
            assert!(matches!(pick, Some(Action::EndTurn)), "{pick:?}");
        }
    }

    // ------------------------------------------------------------------
    // Bank trades
    // ------------------------------------------------------------------

    #[test]
    fn it_trades_with_the_bank_only_to_afford_something_now() {
        let mut g = Game::new(names(2));
        flat(&mut g);
        let h = homestead(&mut g, false);

        // One ore short of a city, with four spare bricks: trade bricks for the ore, then build.
        g.players[P0].hand = Res::new(4, 0, 2, 2, 0);
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::BankTrade { give: Resource::Brick, get: Resource::Ore })), "{pick:?}");
        }
        g.apply(P0, Action::BankTrade { give: Resource::Brick, get: Resource::Ore }).unwrap();
        for pick in all_picks(&g, P0, 1) {
            assert!(matches!(pick, Some(Action::BuildCity { vertex }) if vertex == h.v), "{pick:?}");
        }

        // Four spare bricks would pay for a lumber, but a road is not worth four cards: no trade.
        g.players[P0].hand = Res::new(6, 0, 0, 0, 0);
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::EndTurn)), "{pick:?}");
        }

        // Not enough spare cards to pay for the missing one: no trade, the turn ends.
        g.players[P0].hand = Res::new(3, 0, 2, 2, 0);
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::EndTurn)), "{pick:?}");
        }

        // Already affordable: build, don't trade.
        g.players[P0].hand = Res::new(8, 0, 3, 2, 0);
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::BuildCity { vertex }) if vertex == h.v), "{pick:?}");
        }
    }

    // ------------------------------------------------------------------
    // The robber and what comes of it
    // ------------------------------------------------------------------

    #[test]
    fn the_robber_goes_to_a_rivals_best_hex_and_never_onto_the_bots_own() {
        let mut g = Game::new(names(2));
        flat(&mut g);
        let (x, y) = apart(&g);
        g.board.hexes[x].number = Some(6);
        g.board.hexes[y].number = Some(8);
        let vx = g.board.hexes[x].vertices[0];
        let (vy1, vy2) = (g.board.hexes[y].vertices[0], g.board.hexes[y].vertices[2]);
        g.buildings[vx] = Some(Building { owner: P1, city: false });
        g.buildings[vy1] = Some(Building { owner: P1, city: true });
        g.buildings[vy2] = Some(Building { owner: P0, city: false });
        g.phase = Phase::MoveRobber;
        g.current = P0;

        // The city is the better target, but the bot has a settlement on that hex too.
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::MoveRobber { hex }) if hex == x), "{pick:?}");
        }
        // Without it, the rival's city is the target.
        g.buildings[vy2] = None;
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::MoveRobber { hex }) if hex == y), "{pick:?}");
        }
    }

    #[test]
    fn the_robber_leans_on_whoever_leads() {
        let mut g = Game::new(names(3));
        flat(&mut g);
        let (x, y) = apart(&g);
        g.board.hexes[x].number = Some(6);
        g.board.hexes[y].number = Some(6);
        g.buildings[g.board.hexes[x].vertices[0]] = Some(Building { owner: P1, city: false });
        g.buildings[g.board.hexes[y].vertices[0]] = Some(Building { owner: P2, city: false });
        g.phase = Phase::MoveRobber;
        g.current = P0;

        g.longest_road_holder = Some(P1);
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::MoveRobber { hex }) if hex == x), "{pick:?}");
        }
        g.longest_road_holder = Some(P2);
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::MoveRobber { hex }) if hex == y), "{pick:?}");
        }
    }

    #[test]
    fn it_steals_from_the_biggest_hand_then_the_leader() {
        let mut g = Game::new(names(3));
        g.phase = Phase::Steal;
        g.current = P0;
        g.steal_candidates = vec![P1, P2];
        g.players[P1].hand = Res::new(1, 1, 0, 0, 0);
        g.players[P2].hand = Res::new(1, 1, 1, 1, 1);
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::Steal { victim }) if victim == P2), "{pick:?}");
        }
        g.players[P1].hand = Res::new(1, 1, 1, 1, 1);
        g.longest_road_holder = Some(P1);
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::Steal { victim }) if victim == P1), "{pick:?}");
        }
    }

    #[test]
    fn it_discards_what_its_target_does_not_need_even_when_it_is_not_its_turn() {
        let mut g = Game::new(names(2));
        flat(&mut g);
        homestead(&mut g, false); // the best build is a city: 3 ore and 2 grain
        g.players[P0].hand = Res::new(4, 4, 3, 2, 0);
        g.phase = Phase::Discard;
        g.current = P1;
        g.discards = vec![(P0, 6)];
        for pick in all_picks(&g, P0, 0) {
            let Some(Action::Discard { cards }) = pick else { panic!("expected a discard, got {pick:?}") };
            assert_eq!(cards.total(), 6);
            assert_eq!((cards[Resource::Ore], cards[Resource::Grain]), (0, 0), "the city's cards stay in hand");
            assert!(g.players[P0].hand.covers(&cards));
        }
        assert!(all_picks(&g, P1, 0).iter().all(Option::is_none), "player 1 has nothing to discard");
    }

    #[test]
    fn a_volcano_pick_takes_what_the_target_lacks_and_waits_its_turn_in_the_queue() {
        let mut g = Game::new(names(2));
        flat(&mut g);
        homestead(&mut g, false);
        g.players[P0].hand = Res::new(0, 0, 3, 1, 0); // a city, one grain short
        g.phase = Phase::VolcanoPick;
        g.volcano_picks = vec![(P0, 1)];
        for pick in all_picks(&g, P0, 0) {
            assert!(matches!(pick, Some(Action::VolcanoPick { resource: Resource::Grain })), "{pick:?}");
        }
        g.volcano_picks = vec![(P1, 1), (P0, 1)];
        assert!(all_picks(&g, P0, 0).iter().all(Option::is_none), "player 1 picks first");
        assert!(all_picks(&g, P1, 0).iter().all(Option::is_some));
    }

    // ------------------------------------------------------------------
    // People
    // ------------------------------------------------------------------

    #[test]
    fn a_bot_declines_offers_from_people_and_never_makes_any() {
        let mut g = Game::new(names(3));
        g.phase = Phase::Main;
        g.current = P0;
        g.rolled = true;
        g.players[P0].hand = Res::new(2, 0, 0, 0, 0);
        g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 1, 0, 0, 0) }).unwrap();
        let id = g.trades[0].id;
        for seat in [P1, P2] {
            for pick in all_picks(&g, seat, 0) {
                assert!(matches!(pick, Some(Action::RespondTrade { id: i, accept: false }) if i == id), "{pick:?}");
            }
        }
        g.apply(P1, Action::RespondTrade { id, accept: false }).unwrap();
        assert!(all_picks(&g, P1, 0).iter().all(Option::is_none), "answered once is enough");

        // A bot on turn never proposes anything.
        for pick in all_picks(&g, P0, 0) {
            assert!(!matches!(pick, Some(Action::OfferTrade { .. })), "{pick:?}");
        }
    }

    // ------------------------------------------------------------------
    // The driver
    // ------------------------------------------------------------------

    #[test]
    fn the_driver_moves_one_bot_at_a_time_at_a_human_pace_and_leaves_people_alone() {
        let mut g = Game::new(names(2));
        let mut rng = rng();
        let t0 = Instant::now();
        let mut driver = Driver::new(t0, &mut rng);
        let bot = |p: PlayerId| p == P0;

        assert!(!driver.step(&mut g, bot, t0, &mut rng), "the first move waits a moment");
        assert!(g.buildings.iter().all(Option::is_none));
        let t1 = t0 + secs(1);
        assert!(driver.step(&mut g, bot, t1, &mut rng), "a settlement");
        assert_eq!(g.buildings.iter().flatten().count(), 1);
        assert!(!driver.step(&mut g, bot, t1, &mut rng), "then a pause");
        let t2 = t1 + secs(1);
        assert!(driver.step(&mut g, bot, t2, &mut rng), "a road");
        assert_eq!(g.roads.iter().flatten().count(), 1);

        assert_eq!(g.current, P1, "player 1 is up, and is a person");
        assert!(!driver.step(&mut g, bot, t2 + secs(60), &mut rng));
        assert_eq!(g.buildings.iter().flatten().count(), 1);
    }

    // ------------------------------------------------------------------
    // Whole games
    // ------------------------------------------------------------------

    /// Bots play every seat with `choose`, applying each move through the rules. A move the rules refuse, or a moment
    /// when nobody has anything to do, fails the test. Returns the game and the most actions any seat took in a turn.
    fn play_out(mut g: Game, seed: u64, max_actions: usize) -> (Game, u32) {
        let mut rng = StdRng::seed_from_u64(seed);
        let now = Instant::now();
        let mut acted = vec![0u32; g.n];
        let mut most = 0;
        for step in 0..max_actions {
            if g.phase == Phase::GameOver {
                break;
            }
            for p in PlayerId::range(g.n) {
                if p != g.current {
                    acted[p.index()] = 0;
                }
            }
            let mut moved = false;
            for p in PlayerId::range(g.n) {
                let Some(action) = choose(&g, p, acted[p.index()], &mut rng) else { continue };
                let (ends, on_turn, shown) = (matches!(action, Action::EndTurn), p == g.current, format!("{action:?}"));
                g.apply_at(p, action, now).unwrap_or_else(|why| panic!("step {step}: seat {} tried {shown}: {why}", p.index()));
                acted[p.index()] = if ends { 0 } else { acted[p.index()] + u32::from(on_turn) };
                most = most.max(acted[p.index()]);
                moved = true;
                break;
            }
            assert!(moved, "step {step}: nobody can move in phase {:?} (current seat {})", g.phase, g.current.index());
        }
        (g, most)
    }

    fn check_games(label: &str, make: impl Fn() -> Game) {
        for seed in 0..2 {
            let (g, most) = play_out(make(), seed, 2500);
            let rolls: u32 = g.roll_counts.iter().sum();
            assert!(g.phase == Phase::GameOver || rolls > 10, "{label}: the game went nowhere ({rolls} rolls)");
            assert!(most <= MAX_ACTIONS_PER_TURN + 2, "{label}: a seat took {most} actions in one turn");
        }
    }

    fn start(n: usize, map: &str, settings: Settings, edit: impl Fn(&mut crate::map_def::MapDef)) -> Game {
        let mut def = crate::builtin::builtin(map).expect("built-in map").clone();
        edit(&mut def);
        let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
        Game::try_with_def(names(n), colors, TimerMode::Normal, settings, map, &def).expect("the game starts")
    }

    #[test]
    fn bots_play_standard_games_without_a_refused_move() {
        for n in 2..=4 {
            check_games(&format!("standard/{n}"), || start(n, "standard", Settings::default(), |_| {}));
        }
    }

    #[test]
    fn bots_play_on_volcanoes_and_over_bridges() {
        check_games("volcano/3", || {
            start(3, "standard", Settings::default(), |def| {
                def.terrain_pool.insert(Terrain::Fields, 2);
                def.terrain_pool.insert(Terrain::Volcano, 2);
            })
        });
        check_games("earth/4 bridges", || start(4, "earth", Settings { bridges: true, ..Settings::default() }, |_| {}));
    }

    #[test]
    fn bots_play_with_odd_rules() {
        let odd = Settings { points_to_win: 8, friendly_robber: true, starting_settlements: 3, bank_ratio: 2, max_roads: 8, dev_cards: 0, city_points: 1, ..Settings::default() };
        check_games("standard/3 odd rules", || start(3, "standard", odd, |_| {}));
    }
}
