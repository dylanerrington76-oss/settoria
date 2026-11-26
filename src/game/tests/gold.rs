//! Gold fields: a free pick like a volcano, but nothing ever erupts.

use crate::board::{Board, Terrain};
use crate::game::{Action, Building, Game, PALETTE, Phase, Settings, TimerMode};
use crate::log_event::LogEvent;
use crate::map_def::{MapDef, Slot};
use crate::types::{PlayerId, Resource};
use std::time::Instant;
use super::support::{all_connected, invariants, names, P0, P1, play_random, volcano_def, with};

/// Seven hexes with a gold field numbered 8 in the middle; the robber starts on a rim hex.
fn gold_def() -> MapDef {
    let mut def = crate::builtin::flower();
    def.hexes[0].terrain = Slot::Fixed(Terrain::Gold);
    def.hexes[0].number = Slot::Fixed(8);
    def.robber_start = Some(crate::map_def::HexRef { q: 1, r: 0 });
    def
}

/// A game on `gold_def` with `n` seats, at the start of P0's turn. The gold field is hex 0.
fn gold_game(n: usize) -> Game {
    let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
    let mut g = Game::try_with_def(names(n), colors, TimerMode::Normal, Settings::default(), "gold", &gold_def()).unwrap();
    assert_eq!(g.board.hexes[0].terrain, Terrain::Gold);
    g.phase = Phase::Roll;
    g.current = P0;
    g.rolled = false;
    g.sync_clock(Instant::now());
    g
}

/// A game on `gold_def` still in setup, with `starting` starting settlements each, `n` seats.
fn gold_setup_game(n: usize, starting: u8) -> Game {
    let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
    let settings = with(|s| s.starting_settlements = starting);
    let g = Game::try_with_def(names(n), colors, TimerMode::Normal, settings, "gold", &gold_def()).unwrap();
    assert_eq!(g.phase, Phase::Setup);
    g
}

/// Put a building on corner `i` (0 to 5) of hex 0, taking the piece from the owner's stock.
fn build_on_gold(g: &mut Game, corner: usize, p: PlayerId, city: bool) -> usize {
    let v = g.board.hexes[0].vertices[corner];
    g.buildings[v] = Some(Building { owner: p, city });
    if city {
        g.players[p].cities_left -= 1;
    } else {
        g.players[p].settlements_left -= 1;
    }
    v
}

#[test]
fn gold_is_a_numbered_hex_that_produces_nothing_by_itself() {
    let def = gold_def();
    let report = crate::validate::validate(&def, &Settings::default());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(serde_json::to_string(&Terrain::Gold).unwrap(), "\"gold\"");
    assert_eq!(Terrain::Gold.name(), "gold");
    assert_eq!(Terrain::Gold.resource(), None);
    assert!(Terrain::Gold.is_gold() && !Terrain::Gold.is_volcano() && Terrain::Gold.gives_free_pick());
    // gold dealt from the pool takes an ordinary number token (not necessarily a 6 or 8)
    let mut pooled = crate::builtin::flower();
    pooled.hexes[0].terrain = Slot::Random;
    pooled.terrain_pool = std::collections::BTreeMap::from([(Terrain::Gold, 1), (Terrain::Forest, 3), (Terrain::Hills, 2), (Terrain::Fields, 1)]);
    pooled.robber_start = Some(crate::map_def::HexRef { q: 1, r: 0 });
    pooled.token_pool = std::collections::BTreeMap::from([(3, 1), (4, 1), (5, 1), (9, 1), (10, 1), (11, 1), (2, 1)]);
    let report = crate::validate::validate(&pooled, &Settings::default());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    for _ in 0..20 {
        let b = Board::from_def(&pooled, "pooled", &mut rand::thread_rng()).unwrap();
        let gold = b.hexes.iter().filter(|h| h.terrain == Terrain::Gold).collect::<Vec<_>>();
        assert_eq!(gold.len(), 1, "one gold field is dealt");
        assert!(gold[0].number.is_some());
    }
}

#[test]
fn a_gold_roll_gives_every_building_its_free_picks_and_nothing_erupts() {
    let mut g = gold_game(3);
    build_on_gold(&mut g, 0, P0, false); // 1 card
    build_on_gold(&mut g, 2, P1, true); // 2 cards
    g.resolve_roll(8);
    assert_eq!(g.phase, Phase::VolcanoPick);
    assert_eq!(g.volcano_active, Some(0));
    let view = g.view(P0, &all_connected(3), &g.longest_roads());
    assert!(view.volcano.is_some(), "the pick is shown like a volcano's");
    let other = PlayerId::new(2);
    assert!(g.apply(other, Action::VolcanoPick { resource: Resource::Ore }).is_err(), "only the player whose pick it is");
    while g.phase == Phase::VolcanoPick {
        let (q, _) = g.volcano_picks[0];
        g.apply(q, Action::VolcanoPick { resource: Resource::Grain }).unwrap();
        invariants(&g);
    }
    assert_eq!(g.players[P0].hand.total(), 1);
    assert_eq!(g.players[P1].hand.total(), 2);
    assert_eq!(g.players[P0].hand[Resource::Grain], 1);
    assert_eq!(g.players[P0].stats.dice_income, 1, "a gold pick counts as dice income");
    assert_eq!(g.phase, Phase::Main);
    assert!(g.lava.is_none(), "gold never erupts");
    assert!(!g.log.iter().any(|e| matches!(e, LogEvent::LavaFlow { .. } | LogEvent::VolcanoPicked { .. })));
    assert_eq!(g.log.iter().filter(|e| matches!(e, LogEvent::GoldPicked { .. })).count(), 3);
    // the buildings are untouched
    assert_eq!(g.buildings[g.board.hexes[0].vertices[0]], Some(Building { owner: P0, city: false }));
    assert_eq!(g.buildings[g.board.hexes[0].vertices[2]], Some(Building { owner: P1, city: true }));
    invariants(&g);
}

#[test]
fn buildings_on_gold_are_never_at_risk_whatever_is_rolled() {
    for _ in 0..60 {
        let mut g = gold_game(2);
        let v = build_on_gold(&mut g, 0, P0, true);
        g.resolve_roll(8);
        while g.phase == Phase::VolcanoPick {
            g.apply(P0, Action::VolcanoPick { resource: Resource::Ore }).unwrap();
        }
        assert_eq!(g.buildings[v], Some(Building { owner: P0, city: true }));
        assert!(g.lava.is_none());
        assert!(g.damaged.is_empty());
    }
}

#[test]
fn a_roll_of_another_number_or_gold_with_nobody_on_it_does_nothing() {
    let mut g = gold_game(2);
    build_on_gold(&mut g, 0, P0, false);
    // (the rim hexes are numbered at random from 3, 4, 5, 9, 10 and 11, so one beside the building could pay
    // out on a 9; no hex carries a 6)
    g.resolve_roll(6);
    assert_eq!(g.phase, Phase::Main);
    assert!(g.volcano_picks.is_empty() && g.players[P0].hand.total() == 0);
    let mut g = gold_game(2);
    g.resolve_roll(8);
    assert_eq!(g.phase, Phase::Main, "nobody is on the gold, so nobody picks");
    assert!(g.log.iter().all(|e| !matches!(e, LogEvent::GoldPicked { .. } | LogEvent::GoldBlocked { .. })));
}

#[test]
fn the_robber_on_gold_stops_the_free_resources() {
    let mut g = gold_game(2);
    let v = build_on_gold(&mut g, 0, P0, true);
    g.robber = 0;
    g.resolve_roll(8);
    assert_eq!(g.phase, Phase::Main, "no picks while the robber sits on the gold");
    assert_eq!(g.players[P0].hand.total(), 0);
    assert!(g.log.iter().any(|e| matches!(e, LogEvent::GoldBlocked { hex: 0 })));
    assert!(!g.log.iter().any(|e| matches!(e, LogEvent::VolcanoBlocked { .. })));
    assert!(g.lava.is_none());
    assert_eq!(g.buildings[v], Some(Building { owner: P0, city: true }));
    invariants(&g);
}

#[test]
fn a_last_starting_settlement_beside_gold_picks_one_free_card_then_places_its_road() {
    let mut g = gold_setup_game(2, 1);
    let p = g.current;
    let v = g.board.hexes[0].vertices[0];
    let beside_producing = g.board.vertices[v].hexes.iter().filter(|&&h| g.board.hexes[h].terrain.resource().is_some()).count() as u32;
    g.apply(p, Action::BuildSettlement { vertex: v }).unwrap();
    assert_eq!(g.phase, Phase::VolcanoPick);
    assert_eq!(g.volcano_picks, vec![(p, 1)]);
    assert_eq!(g.players[p].hand.total(), beside_producing, "the producing hexes paid out, the gold waits for a pick");
    g.apply(p, Action::VolcanoPick { resource: Resource::Ore }).unwrap();
    assert_eq!(g.players[p].hand[Resource::Ore], 1);
    assert_eq!(g.players[p].hand.total(), beside_producing + 1);
    assert_eq!(g.phase, Phase::Setup, "back to setup, for the road");
    assert_eq!(g.setup_vertex, Some(v));
    assert!(g.lava.is_none());
    assert!(g.log.iter().any(|e| matches!(e, LogEvent::GoldPicked { .. })));
    let road = g.legal(p).roads[0];
    g.apply(p, Action::BuildRoad { edge: road }).unwrap();
    assert_eq!(g.setup_vertex, None);
    invariants(&g);
}

#[test]
fn a_volcano_and_a_gold_field_with_the_same_number_both_pay_but_only_the_volcano_erupts() {
    let mut def = volcano_def(); // volcano on hex 0, numbered 8
    def.hexes[1].terrain = Slot::Fixed(Terrain::Gold);
    def.hexes[1].number = Slot::Fixed(8);
    def.constraints.spread_tokens = false;
    def.terrain_pool = std::collections::BTreeMap::from([(Terrain::Forest, 2), (Terrain::Hills, 2), (Terrain::Fields, 1)]);
    def.token_pool = std::collections::BTreeMap::from([(3, 1), (4, 1), (5, 1), (9, 1), (10, 1)]);
    let colors = PALETTE.iter().take(2).map(|c| c.to_string()).collect();
    let mut g = Game::try_with_def(names(2), colors, TimerMode::Normal, Settings::default(), "both", &def).unwrap();
    g.phase = Phase::Roll;
    g.current = P0;
    g.robber = 2;
    g.sync_clock(Instant::now());
    let volcano = (0..g.board.hexes.len()).find(|&h| g.board.hexes[h].terrain == Terrain::Volcano).unwrap();
    let gold = (0..g.board.hexes.len()).find(|&h| g.board.hexes[h].terrain == Terrain::Gold).unwrap();
    // a settlement only on the gold (a corner that only it touches) and one only on the volcano
    let a = *g.board.hexes[gold].vertices.iter().find(|&&v| g.board.vertices[v].hexes.len() == 1).unwrap();
    let b = *g.board.hexes[volcano].vertices.iter().find(|&&v| !g.board.hexes[gold].vertices.contains(&v)).unwrap();
    g.buildings[a] = Some(Building { owner: P0, city: false });
    g.buildings[b] = Some(Building { owner: P1, city: false });
    g.players[P0].settlements_left -= 1;
    g.players[P1].settlements_left -= 1;
    g.resolve_roll(8);
    let mut picks = 0;
    while g.phase == Phase::VolcanoPick {
        let (q, _) = g.volcano_picks[0];
        g.apply(q, Action::VolcanoPick { resource: Resource::Wool }).unwrap();
        picks += 1;
        invariants(&g);
    }
    assert_eq!(picks, 2, "one pick at the gold, one at the volcano");
    assert_eq!(g.phase, Phase::Main);
    assert!(matches!(g.lava, Some(l) if l.hex == volcano), "only the volcano erupted");
    assert_eq!(g.log.iter().filter(|e| matches!(e, LogEvent::GoldPicked { .. })).count(), 1);
    assert_eq!(g.log.iter().filter(|e| matches!(e, LogEvent::VolcanoPicked { .. })).count(), 1);
    assert_eq!(g.log.iter().filter(|e| matches!(e, LogEvent::LavaFlow { .. })).count(), 1);
    invariants(&g);
}

#[test]
fn random_games_on_a_gold_map_keep_invariants() {
    let mut rng = rand::thread_rng();
    for game_no in 0..40 {
        let n = 2 + game_no % 2;
        let mut def = gold_def();
        def.hexes[0].number = Slot::Fixed(if game_no % 2 == 0 { 6 } else { 5 });
        def.token_pool = std::collections::BTreeMap::from([(2, 1), (3, 1), (4, 1), (9, 1), (10, 1), (11, 1)]);
        let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
        let s = Settings { starting_settlements: 1, ..Settings::default() };
        let mut g = Game::try_with_def(names(n), colors, TimerMode::VeryFast, s, "gold", &def).unwrap();
        play_random(&mut g, &mut rng);
        invariants(&g);
        assert!(!g.log.iter().any(|e| matches!(e, LogEvent::LavaFlow { .. })), "gold never makes lava");
    }
}
