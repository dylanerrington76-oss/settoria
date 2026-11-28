//! Helpers shared by more than one group of tests.

use crate::board::Terrain;
use crate::game::trade::MAX_OPEN_TRADES;
use crate::game::{Action, Building, Game, PALETTE, Phase, Reply, Settings, TimerMode};
use crate::map_def::{MapDef, Slot};
use crate::types::{PerPlayer, PlayerId, Res, Resource};
use rand::Rng;
use rand::seq::SliceRandom;
use std::time::{Duration, Instant};

pub(super) const P0: PlayerId = PlayerId::new(0);

pub(super) const P1: PlayerId = PlayerId::new(1);

pub(super) const P2: PlayerId = PlayerId::new(2);

pub(super) fn all_connected(n: usize) -> PerPlayer<bool> {
    PerPlayer::from_fn(n, |_| true)
}

pub(super) fn random_resource(rng: &mut impl Rng) -> Resource {
    *Resource::ALL.choose(rng).unwrap()
}

pub(super) fn names(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("P{i}")).collect()
}

pub(super) fn pick<T: Copy>(v: &[T], rng: &mut impl Rng) -> Option<T> {
    v.choose(rng).copied()
}

/// Finish the setup phase by placing at random legal spots.
pub(super) fn run_setup(g: &mut Game, rng: &mut impl Rng) {
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

pub(super) fn line_of_roads(g: &mut Game, p: PlayerId, count: usize) -> Vec<usize> {
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

pub(super) fn invariants(g: &Game) {
    // The kept longest roads never go stale: they are what a fresh search finds, whatever just happened.
    assert_eq!(g.longest_roads(), PerPlayer::from_fn(g.n, |p| g.longest_road(p)), "stale longest roads");
    for r in Resource::ALL {
        let held: u32 = g.players.iter().map(|p| p.hand[r] as u32).sum();
        assert_eq!(held + g.bank[r] as u32, g.settings.resource_max as u32, "{} not conserved", r.name());
    }
    for p in PlayerId::range(g.n) {
        let roads = g.roads.iter().filter(|r| **r == Some(p)).count() as u8;
        assert_eq!(roads + g.players[p].roads_left, g.settings.max_roads);
        let s = g.buildings.iter().flatten().filter(|b| b.owner == p && !b.city).count() as u8;
        let c = g.buildings.iter().flatten().filter(|b| b.owner == p && b.city).count() as u8;
        // A settlement on loan (lava hit a city and no piece was left) holds no piece from the stock.
        let loans = g.damaged.iter().filter(|&&v| matches!(g.buildings[v], Some(b) if b.owner == p)).count() as u8;
        assert_eq!(s + g.players[p].settlements_left, g.settings.max_settlements + loans);
        assert_eq!(c + g.players[p].cities_left, g.settings.max_cities);
    }
    for &v in &g.damaged {
        assert!(matches!(g.buildings[v], Some(b) if !b.city), "a loan must be a settlement");
    }
    // A volcano only waits while its phase lasts.
    if g.phase == Phase::VolcanoPick {
        assert!(g.volcano_active.is_some() && !g.volcano_picks.is_empty());
    } else if g.phase != Phase::GameOver {
        assert!(g.volcano_queue.is_empty() && g.volcano_active.is_none() && g.volcano_picks.is_empty());
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
            assert!(!g.bank_rate_deal(t.from, &t.give, &t.want), "an open offer is a bank deal for its proposer");
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
pub(super) fn play_random(g: &mut Game, rng: &mut impl Rng) {
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
            // A free pick beside a last starting settlement (volcano or gold) sends the game back to setup
            // for that settlement's road, so setup can come round again after the first call to `run_setup`.
            Phase::Setup => run_setup(g, rng),
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
                if g.phase == Phase::Roll && rng.gen_bool(0.3) {
                    // An Alchemist, if the player has one: it is the roll, with the dice they name.
                    let _ = g.apply(p, Action::PlayAlchemist { dice: (rng.gen_range(1..=6), rng.gen_range(1..=6)) });
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
            Phase::VolcanoPick => {
                let (q, _) = g.volcano_picks[0];
                let open: Vec<Resource> = Resource::ALL.iter().copied().filter(|&r| g.bank[r] > 0).collect();
                g.apply(q, Action::VolcanoPick { resource: pick(&open, rng).unwrap() }).unwrap();
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

pub(super) fn secs(d: u64) -> Duration {
    Duration::from_secs(d)
}

/// A started game of `n` players in the given mode with a fixed clock reading.
pub(super) fn timed(n: usize, mode: TimerMode, t0: Instant) -> Game {
    let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
    Game::build(names(n), colors, mode, Settings::default(), "standard", false, t0)
}

/// Put the game straight into the main phase of player `p`'s turn at `t`.
pub(super) fn into_main(g: &mut Game, p: PlayerId, t: Instant) {
    g.phase = Phase::Main;
    g.current = p;
    g.rolled = true;
    g.sync_clock(t);
}

/// A game in the main phase of player 0's turn with 3 players and known hands.
pub(super) fn trading_game() -> Game {
    let mut g = Game::new(names(3));
    g.phase = Phase::Main;
    g.current = P0;
    g.rolled = true;
    g.players[P0].hand = Res::new(3, 0, 0, 0, 0);
    g.players[P1].hand = Res::new(0, 2, 2, 0, 0);
    g.players[P2].hand = Res::new(0, 0, 2, 2, 0);
    g
}

pub(super) fn with(f: impl FnOnce(&mut Settings)) -> Settings {
    let mut s = Settings::default();
    f(&mut s);
    s
}

/// Hand `p` exactly `cards` from the bank.
pub(super) fn deal(g: &mut Game, p: PlayerId, cards: Res) {
    for r in Resource::ALL {
        g.players[p].hand[r] = cards[r];
        g.bank[r] -= cards[r];
    }
}

/// Seven hexes with a volcano numbered 8 in the middle; the robber starts on a rim hex.
pub(super) fn volcano_def() -> MapDef {
    let mut def = crate::builtin::flower();
    def.hexes[0].terrain = Slot::Fixed(Terrain::Volcano);
    def.hexes[0].number = Slot::Fixed(8);
    def.robber_start = Some(crate::map_def::HexRef { q: 1, r: 0 });
    def
}

/// A game on `volcano_def` with `n` seats, at the start of P0's turn, and the volcano's hex (always 0).
pub(super) fn volcano_game(n: usize) -> Game {
    let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
    let mut g = Game::try_with_def(names(n), colors, TimerMode::Normal, Settings::default(), "volcano", &volcano_def()).unwrap();
    assert_eq!(g.board.hexes[0].terrain, Terrain::Volcano);
    // skip setup: P0 is to roll
    g.phase = Phase::Roll;
    g.current = P0;
    g.rolled = false;
    g.sync_clock(Instant::now());
    g
}

/// Put a building on corner `i` (0 to 5) of hex 0, taking the piece from the owner's stock.
pub(super) fn build_on_volcano(g: &mut Game, corner: usize, p: PlayerId, city: bool) -> usize {
    let v = g.board.hexes[0].vertices[corner];
    g.buildings[v] = Some(Building { owner: p, city });
    if city {
        g.players[p].cities_left -= 1;
    } else {
        g.players[p].settlements_left -= 1;
    }
    v
}
