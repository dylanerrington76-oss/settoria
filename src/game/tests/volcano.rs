//! Volcanoes: the free picks, the lava, and the clocks they run on.

use crate::board::{Board, Terrain};
use crate::game::clock::VOLCANO_SECS;
use crate::game::{Action, Building, ClockKind, Game, PALETTE, Phase, Settings, TimerMode};
use crate::log_event::{LavaEffect, LogEvent};
use crate::map_def::Slot;
use crate::types::{PlayerId, Res, Resource};
use std::time::{Duration, Instant};
use super::support::{all_connected, build_on_volcano, deal, into_main, invariants, names, P0, P1, P2, play_random, secs, volcano_def, volcano_game, with};

/// A game on `volcano_def` still in setup, with `starting` starting settlements each, `n` seats.
fn volcano_setup_game(n: usize, starting: u8) -> Game {
    let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
    let settings = with(|s| s.starting_settlements = starting);
    let g = Game::try_with_def(names(n), colors, TimerMode::Normal, settings, "volcano", &volcano_def()).unwrap();
    assert_eq!(g.phase, Phase::Setup);
    g
}

#[test]
fn a_last_starting_settlement_beside_a_volcano_picks_one_free_card_then_places_its_road() {
    let mut g = volcano_setup_game(2, 1); // the one starting settlement is the last one
    let p = g.current;
    let v = g.board.hexes[0].vertices[0];
    let beside_volcano = g.board.vertices[v].hexes.iter().filter(|&&h| g.board.hexes[h].terrain.resource().is_some()).count() as u32;
    g.apply(p, Action::BuildSettlement { vertex: v }).unwrap();
    assert_eq!(g.phase, Phase::VolcanoPick);
    assert_eq!(g.volcano_picks, vec![(p, 1)]);
    assert_eq!(g.players[p].hand.total(), beside_volcano, "the producing hexes paid out, the volcano waits for a pick");
    assert!(g.legal(p).roads.is_empty(), "the road comes after the pick");
    let view = g.view(p, &all_connected(2), &g.longest_roads());
    let vol = view.volcano.expect("the pick is shown");
    assert_eq!((vol.hex, vol.player, vol.left, vol.setup), (0, p, 1, true));
    let other = if p == P0 { P1 } else { P0 };
    assert!(g.apply(other, Action::VolcanoPick { resource: Resource::Ore }).is_err(), "only the settler picks");

    let before = g.players[p].hand[Resource::Ore];
    let bank = g.bank[Resource::Ore];
    g.apply(p, Action::VolcanoPick { resource: Resource::Ore }).unwrap();
    assert_eq!(g.players[p].hand[Resource::Ore], before + 1);
    assert_eq!(g.bank[Resource::Ore], bank - 1);
    assert_eq!(g.players[p].hand.total(), beside_volcano + 1);
    assert_eq!(g.players[p].stats.dice_income, 0, "a starting card is not dice income");
    assert_eq!(g.phase, Phase::Setup, "back to setup, for the road");
    assert_eq!(g.setup_vertex, Some(v));
    assert_eq!(g.current, p);
    assert!(!g.setup_pick && g.lava.is_none(), "no eruption");
    assert!(g.volcano_active.is_none() && g.volcano_picks.is_empty() && g.volcano_queue.is_empty());
    let road = g.legal(p).roads[0];
    g.apply(p, Action::BuildRoad { edge: road }).unwrap();
    assert_eq!(g.setup_vertex, None);
    assert_eq!(g.phase, Phase::Setup, "the other player is still to place");
    assert_ne!(g.current, p);
    invariants(&g);
}

#[test]
fn a_starting_settlement_that_is_not_the_last_gets_nothing_from_a_volcano() {
    let mut g = volcano_setup_game(2, 2);
    let p = g.current;
    let v = g.board.hexes[0].vertices[0];
    g.apply(p, Action::BuildSettlement { vertex: v }).unwrap();
    assert_eq!(g.phase, Phase::Setup, "only the last starting settlement pays out");
    assert!(g.volcano_picks.is_empty() && g.players[p].hand.total() == 0);
}

#[test]
fn a_last_starting_settlement_picks_once_for_every_volcano_it_touches() {
    let mut def = volcano_def();
    // a second volcano beside the first (hexes 0 and 1 touch); both corners shared by hexes 0, 1 and one more
    def.hexes[1].terrain = Slot::Fixed(Terrain::Volcano);
    def.hexes[1].number = Slot::Fixed(6);
    def.constraints.spread_tokens = false;
    def.terrain_pool = std::collections::BTreeMap::from([(Terrain::Forest, 2), (Terrain::Hills, 2), (Terrain::Fields, 1)]);
    def.token_pool = std::collections::BTreeMap::from([(3, 1), (4, 1), (5, 1), (9, 1), (10, 1)]);
    let colors = PALETTE.iter().take(2).map(|c| c.to_string()).collect();
    let settings = with(|s| s.starting_settlements = 1);
    let mut g = Game::try_with_def(names(2), colors, TimerMode::Normal, settings, "two", &def).unwrap();
    let p = g.current;
    let v = (0..g.buildings.len())
        .find(|&v| g.board.vertices[v].hexes.contains(&0) && g.board.vertices[v].hexes.contains(&1))
        .expect("the two volcanoes share a corner");
    g.apply(p, Action::BuildSettlement { vertex: v }).unwrap();
    assert_eq!(g.phase, Phase::VolcanoPick);
    g.apply(p, Action::VolcanoPick { resource: Resource::Grain }).unwrap();
    assert_eq!(g.phase, Phase::VolcanoPick, "one more volcano to pick for");
    assert_eq!(g.volcano_picks, vec![(p, 1)]);
    g.apply(p, Action::VolcanoPick { resource: Resource::Wool }).unwrap();
    assert_eq!(g.phase, Phase::Setup);
    assert_eq!(g.setup_vertex, Some(v));
    assert!(g.lava.is_none());
    invariants(&g);
}

#[test]
fn a_timed_out_starting_volcano_pick_is_made_at_random() {
    let t0 = Instant::now();
    let mut g = volcano_setup_game(2, 1);
    let p = g.current;
    let v = g.board.hexes[0].vertices[0];
    g.apply_at(p, Action::BuildSettlement { vertex: v }, t0).unwrap();
    let held = g.players[p].hand.total();
    assert_eq!(g.clock.unwrap().kind, ClockKind::Volcano);
    assert!(g.tick(t0 + secs(VOLCANO_SECS + 1)));
    assert_eq!(g.players[p].hand.total(), held + 1);
    assert_eq!(g.phase, Phase::Setup);
    assert_eq!(g.setup_vertex, Some(v));
    invariants(&g);
}

#[test]
fn a_volcano_is_a_numbered_hex_that_produces_nothing_by_itself() {
    let def = volcano_def();
    let report = crate::validate::validate(&def, &Settings::default());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let json = serde_json::to_string(&Terrain::Volcano).unwrap();
    assert_eq!(json, "\"volcano\"");
    assert_eq!(Terrain::Volcano.resource(), None);
    // a volcano dealt from the pool gets a number token like any producing hex
    let mut pooled = crate::builtin::flower();
    pooled.hexes[0].terrain = Slot::Random;
    pooled.terrain_pool = std::collections::BTreeMap::from([(Terrain::Volcano, 1), (Terrain::Forest, 3), (Terrain::Hills, 2), (Terrain::Fields, 1)]);
    pooled.robber_start = Some(crate::map_def::HexRef { q: 1, r: 0 });
    pooled.token_pool = std::collections::BTreeMap::from([(3, 1), (4, 1), (5, 1), (9, 1), (10, 1), (11, 1), (2, 1)]);
    let report = crate::validate::validate(&pooled, &Settings::default());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    for _ in 0..20 {
        let b = Board::from_def(&pooled, "pooled", &mut rand::thread_rng()).unwrap();
        let v = b.hexes.iter().find(|h| h.terrain == Terrain::Volcano).expect("one volcano is dealt");
        assert!(v.number.is_some());
    }
}

#[test]
fn a_volcano_roll_gives_every_building_its_free_picks_one_player_at_a_time_then_the_lava_flows() {
    let mut g = volcano_game(3);
    build_on_volcano(&mut g, 0, P0, false); // 1 card
    build_on_volcano(&mut g, 2, P1, true); // 2 cards
    build_on_volcano(&mut g, 4, P0, true); // 2 more for P0: 3 in all
    let start_bank = g.bank;
    g.resolve_roll(8);
    assert_eq!(g.phase, Phase::VolcanoPick);
    // in turn order from the player who rolled
    let first = g.order.iter().position(|&x| x == P0).unwrap();
    let expected: Vec<(PlayerId, u8)> = (0..3)
        .map(|i| g.order[(first + i) % 3])
        .filter_map(|p| match p {
            x if x == P0 => Some((P0, 3)),
            x if x == P1 => Some((P1, 2)),
            _ => None,
        })
        .collect();
    assert_eq!(g.volcano_picks, expected);
    assert!(g.lava.is_none(), "the lava waits for the picks");

    // nobody else may pick, and the bank must have the card
    let (first_p, _) = g.volcano_picks[0];
    let other = if first_p == P0 { P1 } else { P0 };
    assert!(g.apply(other, Action::VolcanoPick { resource: Resource::Ore }).is_err());
    assert!(g.apply(P2, Action::VolcanoPick { resource: Resource::Ore }).is_err());
    g.bank[Resource::Wool] = 0;
    assert!(g.apply(first_p, Action::VolcanoPick { resource: Resource::Wool }).is_err());
    g.bank[Resource::Wool] = start_bank[Resource::Wool];
    // a player can't act in the volcano's turn either
    assert!(g.apply(P0, Action::EndTurn).is_err());

    // everyone picks what they like, two of the same or two different
    while g.phase == Phase::VolcanoPick {
        let (q, left) = g.volcano_picks[0];
        let r = if left == 2 { Resource::Ore } else { Resource::Grain };
        g.apply(q, Action::VolcanoPick { resource: r }).unwrap();
        invariants(&g);
    }
    assert_eq!(g.players[P0].hand.total(), 3);
    assert_eq!(g.players[P1].hand.total(), 2);
    assert_eq!(g.players[P2].hand.total(), 0);
    assert_eq!(g.phase, Phase::Main);
    let lava = g.lava.expect("the lava flowed after the last pick");
    assert!((1..=6).contains(&lava.die));
    assert_eq!(lava.vertex, g.board.hexes[0].vertices[usize::from(lava.die) - 1]);
    assert_eq!(g.volcano_active, None);
    invariants(&g);
}

#[test]
fn a_roll_of_another_number_or_a_seven_leaves_the_volcano_quiet() {
    let mut g = volcano_game(2);
    build_on_volcano(&mut g, 0, P0, false);
    g.resolve_roll(9);
    assert_eq!(g.phase, Phase::Main);
    assert!(g.lava.is_none() && g.volcano_picks.is_empty());
    let mut g = volcano_game(2);
    build_on_volcano(&mut g, 0, P0, false);
    g.resolve_roll(7);
    assert!(g.lava.is_none() && g.volcano_picks.is_empty());
    assert_ne!(g.phase, Phase::VolcanoPick);
}

#[test]
fn a_volcano_nobody_is_on_does_not_erupt_and_is_not_logged() {
    for robber_on_it in [false, true] {
        let mut g = volcano_game(2);
        if robber_on_it {
            g.robber = 0;
        }
        g.resolve_roll(8);
        assert_eq!(g.phase, Phase::Main);
        assert!(g.lava.is_none(), "no lava from an empty volcano");
        assert!(g.volcano_active.is_none() && g.volcano_queue.is_empty() && g.volcano_picks.is_empty());
        assert!(
            !g.log.iter().any(|e| matches!(e, LogEvent::LavaFlow { .. } | LogEvent::VolcanoBlocked { .. } | LogEvent::VolcanoBankEmpty)),
            "nothing about the volcano is logged"
        );
        invariants(&g);
    }
}

#[test]
fn only_the_volcanoes_with_a_building_on_them_erupt() {
    let mut def = volcano_def();
    // a second volcano on the rim, also numbered 8
    def.hexes[1].terrain = Slot::Fixed(Terrain::Volcano);
    def.hexes[1].number = Slot::Fixed(8);
    def.constraints.spread_tokens = false;
    def.terrain_pool = std::collections::BTreeMap::from([(Terrain::Forest, 2), (Terrain::Hills, 2), (Terrain::Fields, 1)]);
    def.token_pool = std::collections::BTreeMap::from([(3, 1), (4, 1), (5, 1), (9, 1), (10, 1)]);
    let colors = PALETTE.iter().take(2).map(|c| c.to_string()).collect();
    let mut g = Game::try_with_def(names(2), colors, TimerMode::Normal, Settings::default(), "two", &def).unwrap();
    g.phase = Phase::Roll;
    g.current = P0;
    g.robber = 2;
    g.sync_clock(Instant::now());
    let volcanoes: Vec<usize> = (0..g.board.hexes.len()).filter(|&h| g.board.hexes[h].terrain == Terrain::Volcano).collect();
    assert_eq!(volcanoes.len(), 2);
    // a settlement only on the second volcano (a corner that only it touches)
    let b = *g.board.hexes[volcanoes[1]].vertices.iter().find(|&&v| g.board.vertices[v].hexes.len() == 1).unwrap();
    g.buildings[b] = Some(Building { owner: P1, city: false });
    g.players[P1].settlements_left -= 1;
    g.resolve_roll(8);
    assert_eq!(g.phase, Phase::VolcanoPick);
    assert_eq!(g.volcano_active, Some(volcanoes[1]), "the empty one was skipped");
    g.apply(P1, Action::VolcanoPick { resource: Resource::Wool }).unwrap();
    assert_eq!(g.phase, Phase::Main);
    assert!(matches!(g.lava, Some(l) if l.hex == volcanoes[1]));
    let flows = g.log.iter().filter(|e| matches!(e, LogEvent::LavaFlow { .. })).count();
    assert_eq!(flows, 1, "only one eruption is logged");
    invariants(&g);
}

#[test]
fn the_robber_on_a_volcano_stops_the_free_resources_but_not_the_eruption() {
    let mut demoted = 0;
    for _ in 0..120 {
        let mut g = volcano_game(2);
        let v = build_on_volcano(&mut g, 0, P0, true);
        g.robber = 0;
        g.resolve_roll(8);
        assert_eq!(g.phase, Phase::Main, "no picks while the robber sits on the volcano");
        assert_eq!(g.players[P0].hand.total(), 0);
        assert!(g.log.iter().any(|e| matches!(e, LogEvent::VolcanoBlocked { hex: 0 })));
        let lava = g.lava.expect("it still erupts");
        if lava.vertex == v {
            assert_eq!(g.buildings[v], Some(Building { owner: P0, city: false }));
            demoted += 1;
        } else {
            assert_eq!(g.buildings[v], Some(Building { owner: P0, city: true }));
        }
    }
    assert!(demoted > 0, "the lava should have hit corner 1 at least once in 120 tries");
}

#[test]
fn lava_destroys_a_settlement_and_returns_it_to_the_stock() {
    for die in 1..=6u8 {
        let mut g = volcano_game(2);
        for c in 0..6 {
            if c % 2 == 0 {
                // settlements on alternate corners, so the distance rule is respected
                build_on_volcano(&mut g, c, P1, false);
            }
        }
        let hit = usize::from(die) - 1;
        let before = g.players[P1].settlements_left;
        g.erupt_with(0, die);
        let v = g.board.hexes[0].vertices[hit];
        if hit % 2 == 0 {
            assert_eq!(g.buildings[v], None, "die {die}");
            assert_eq!(g.players[P1].settlements_left, before + 1);
            assert!(g.log.iter().any(|e| matches!(e, LogEvent::LavaFlow { effect: LavaEffect::Destroyed, owner: Some(o), .. } if *o == P1)));
        } else {
            assert_eq!(g.buildings[v], None);
            assert_eq!(g.players[P1].settlements_left, before);
        }
        invariants(&g);
    }
}

#[test]
fn lava_reduces_a_city_to_a_settlement() {
    let mut g = volcano_game(2);
    let v = build_on_volcano(&mut g, 2, P0, true);
    let (settlements, cities) = (g.players[P0].settlements_left, g.players[P0].cities_left);
    g.erupt_with(0, 3);
    assert_eq!(g.buildings[v], Some(Building { owner: P0, city: false }));
    assert_eq!(g.players[P0].settlements_left, settlements - 1);
    assert_eq!(g.players[P0].cities_left, cities + 1);
    assert!(g.damaged.is_empty(), "there was a settlement piece to use");
    assert_eq!(g.vp_public(P0), g.settings.settlement_points as usize);
    assert!(g.log.iter().any(|e| matches!(e, LogEvent::LavaFlow { effect: LavaEffect::Demoted, die: 3, .. })));
    invariants(&g);
}

#[test]
fn a_city_is_still_reduced_when_the_owner_has_all_their_settlements_out_and_must_be_repaired_first() {
    let mut g = volcano_game(2);
    let city = build_on_volcano(&mut g, 2, P0, true);
    // P0 has every other settlement piece on the board too
    let elsewhere: Vec<usize> = (0..g.buildings.len())
        .filter(|&v| g.buildings[v].is_none() && g.board.vertices[v].hexes.iter().all(|&h| h != 0))
        .collect();
    let mut placed = vec![];
    for v in elsewhere {
        if g.players[P0].settlements_left == 0 {
            break;
        }
        if g.settlement_free(v) {
            g.buildings[v] = Some(Building { owner: P0, city: false });
            g.players[P0].settlements_left -= 1;
            placed.push(v);
        }
    }
    assert_eq!(g.players[P0].settlements_left, 0, "the test board has room for them all");
    g.erupt_with(0, 3);
    assert_eq!(g.buildings[city], Some(Building { owner: P0, city: false }), "demoted all the same");
    assert_eq!(g.damaged, vec![city]);
    assert_eq!(g.players[P0].settlements_left, 0, "the loan takes nothing from the stock");
    assert!(g.has_damaged(P0));
    invariants(&g);

    // only the damaged settlement may become a city, and it needs the usual price
    into_main(&mut g, P0, Instant::now());
    deal(&mut g, P0, Res::new(0, 0, 6, 4, 0));
    assert_eq!(g.legal(P0).cities, vec![city]);
    let other = placed[0];
    assert!(g.apply(P0, Action::BuildCity { vertex: other }).is_err(), "repair first");
    g.apply(P0, Action::BuildCity { vertex: city }).unwrap();
    assert!(g.damaged.is_empty());
    assert_eq!(g.players[P0].settlements_left, 0, "a repair gives no settlement piece back");
    invariants(&g);
    // repaired: the other settlements may be upgraded again
    assert!(g.legal(P0).cities.contains(&other));
    g.apply(P0, Action::BuildCity { vertex: other }).unwrap();
    assert_eq!(g.players[P0].settlements_left, 1);
    invariants(&g);
}

#[test]
fn lava_that_destroys_a_settlement_on_loan_gives_no_piece_back_and_a_loan_is_paid_off_by_a_returned_piece() {
    // a loan destroyed
    let mut g = volcano_game(2);
    let v = build_on_volcano(&mut g, 2, P0, true);
    g.players[P0].settlements_left = 0;
    g.erupt_with(0, 3);
    assert_eq!(g.damaged, vec![v]);
    g.erupt_with(0, 3);
    assert_eq!(g.buildings[v], None);
    assert!(g.damaged.is_empty());
    assert_eq!(g.players[P0].settlements_left, 0);

    // a loan, then a different settlement destroyed: the returning piece pays the loan
    let mut g = volcano_game(2);
    let loan = build_on_volcano(&mut g, 2, P0, true);
    build_on_volcano(&mut g, 4, P0, false);
    g.players[P0].settlements_left = 0;
    g.erupt_with(0, 3);
    assert_eq!(g.damaged, vec![loan]);
    g.erupt_with(0, 5);
    assert!(g.damaged.is_empty(), "the loan is paid off");
    assert_eq!(g.players[P0].settlements_left, 0);
    assert_eq!(g.buildings[loan], Some(Building { owner: P0, city: false }));
}

#[test]
fn destroying_a_settlement_can_free_a_longest_road() {
    let mut g = volcano_game(2);
    let v = build_on_volcano(&mut g, 0, P1, false);
    // P0 roads on both sides of P1's settlement are cut by it
    let edges: Vec<usize> = g.board.vertices[v].edges.clone();
    assert!(edges.len() >= 2);
    g.roads[edges[0]] = Some(P0);
    g.roads[edges[1]] = Some(P0);
    g.players[P0].roads_left -= 2;
    assert_eq!(g.longest_road(P0), 1);
    g.erupt_with(0, 1);
    assert_eq!(g.buildings[v], None);
    assert_eq!(g.longest_road(P0), 2, "nothing blocks the road any more");
}

#[test]
fn a_player_who_runs_out_of_time_gets_random_picks_and_the_next_player_gets_a_fresh_clock() {
    let mut g = volcano_game(3);
    build_on_volcano(&mut g, 0, P0, true);
    build_on_volcano(&mut g, 2, P1, false);
    let t0 = Instant::now();
    g.sync_clock(t0);
    g.resolve_roll(8);
    g.sync_clock(t0);
    assert_eq!(g.phase, Phase::VolcanoPick);
    let (first, _) = g.volcano_picks[0];
    let timer = g.clock.expect("a clock is running");
    assert_eq!((timer.kind, timer.player), (ClockKind::Volcano, first));
    assert_eq!(timer.total, Duration::from_secs(VOLCANO_SECS));
    assert!(!g.tick(t0 + secs(VOLCANO_SECS - 1)));
    assert!(g.tick(t0 + secs(VOLCANO_SECS + 1)));
    // that player's picks were all made; the other player is up (or the lava has flowed)
    assert!(g.players[first].hand.total() > 0);
    assert!(g.log.iter().any(|e| matches!(e, LogEvent::TimedOut { kind: ClockKind::Volcano, player } if *player == first)));
    if g.phase == Phase::VolcanoPick {
        let (second, _) = g.volcano_picks[0];
        assert_ne!(second, first);
        assert_eq!(g.clock.unwrap().player, second);
        assert!(g.clock.unwrap().deadline > t0 + secs(VOLCANO_SECS));
    }
    invariants(&g);
}

#[test]
fn a_bank_that_runs_dry_drops_the_picks_still_owed() {
    let mut g = volcano_game(2);
    build_on_volcano(&mut g, 0, P0, true);
    build_on_volcano(&mut g, 2, P1, true);
    // the bank holds exactly one card
    for r in Resource::ALL {
        g.players[P1].hand[r] = g.bank[r];
        g.bank[r] = 0;
    }
    g.players[P1].hand[Resource::Ore] -= 1;
    g.bank[Resource::Ore] = 1;
    g.resolve_roll(8);
    assert_eq!(g.phase, Phase::VolcanoPick);
    let (q, _) = g.volcano_picks[0];
    assert!(g.apply(q, Action::VolcanoPick { resource: Resource::Grain }).is_err());
    g.apply(q, Action::VolcanoPick { resource: Resource::Ore }).unwrap();
    assert_eq!(g.phase, Phase::Main, "no cards left for anyone else's picks");
    assert!(g.log.iter().any(|e| matches!(e, LogEvent::VolcanoBankEmpty)));
    assert!(g.lava.is_some());
    invariants(&g);
}

#[test]
fn two_volcanoes_with_the_same_number_erupt_one_after_the_other() {
    let mut def = volcano_def();
    // another volcano on the rim, also numbered 8 (spread_tokens only matters for dealt numbers)
    def.hexes[1].terrain = Slot::Fixed(Terrain::Volcano);
    def.hexes[1].number = Slot::Fixed(8);
    def.constraints.spread_tokens = false;
    def.terrain_pool = std::collections::BTreeMap::from([(Terrain::Forest, 2), (Terrain::Hills, 2), (Terrain::Fields, 1)]);
    def.token_pool = std::collections::BTreeMap::from([(3, 1), (4, 1), (5, 1), (9, 1), (10, 1)]);
    let colors = PALETTE.iter().take(2).map(|c| c.to_string()).collect();
    let mut g = Game::try_with_def(names(2), colors, TimerMode::Normal, Settings::default(), "two", &def).unwrap();
    g.phase = Phase::Roll;
    g.current = P0;
    g.robber = 2;
    g.sync_clock(Instant::now());
    let volcanoes: Vec<usize> = (0..g.board.hexes.len()).filter(|&h| g.board.hexes[h].terrain == Terrain::Volcano).collect();
    assert_eq!(volcanoes.len(), 2);
    // a settlement on a corner of each
    // (a corner of the middle volcano that the second one doesn't touch, and one only the second touches)
    let a = *g.board.hexes[volcanoes[0]].vertices.iter().find(|&&v| !g.board.hexes[volcanoes[1]].vertices.contains(&v)).unwrap();
    let b = *g.board.hexes[volcanoes[1]].vertices.iter().find(|&&v| g.board.vertices[v].hexes.len() == 1).unwrap();
    g.buildings[a] = Some(Building { owner: P0, city: false });
    g.buildings[b] = Some(Building { owner: P1, city: false });
    g.players[P0].settlements_left -= 1;
    g.players[P1].settlements_left -= 1;
    g.resolve_roll(8);
    assert_eq!(g.phase, Phase::VolcanoPick);
    assert_eq!(g.volcano_active, Some(volcanoes[0]));
    assert_eq!(g.volcano_picks, vec![(P0, 1)]);
    g.apply(P0, Action::VolcanoPick { resource: Resource::Brick }).unwrap();
    // the first volcano has erupted and the second one now waits for its players
    assert!(matches!(g.lava, Some(l) if l.hex == volcanoes[0]) || g.volcano_active == Some(volcanoes[1]));
    assert_eq!(g.phase, Phase::VolcanoPick);
    assert_eq!(g.volcano_active, Some(volcanoes[1]));
    assert_eq!(g.volcano_picks, vec![(P1, 1)]);
    g.apply(P1, Action::VolcanoPick { resource: Resource::Wool }).unwrap();
    assert_eq!(g.phase, Phase::Main);
    assert!(matches!(g.lava, Some(l) if l.hex == volcanoes[1]));
    invariants(&g);
}

#[test]
fn volcano_views_and_actions_have_a_wire_format() {
    let mut g = volcano_game(2);
    build_on_volcano(&mut g, 0, P0, false);
    g.resolve_roll(8);
    g.sync_clock(Instant::now());
    let conn = all_connected(2);
    let lens = g.longest_roads();
    let v = serde_json::to_value(g.view(P0, &conn, &lens)).unwrap();
    assert_eq!(v["phase"], "volcano_pick");
    assert_eq!(v["volcano"], serde_json::json!({ "hex": 0, "player": 0, "left": 1, "setup": false }));
    assert_eq!(v["timer"]["kind"], "volcano");
    assert_eq!(v["lava"], serde_json::Value::Null);
    assert_eq!(v["damaged"], serde_json::json!([]));
    let a: Action = serde_json::from_str(r#"{"type":"volcano_pick","resource":2}"#).unwrap();
    assert!(matches!(a, Action::VolcanoPick { resource: Resource::Ore }));
    g.apply(P0, a).unwrap();
    let v = serde_json::to_value(g.view(P0, &conn, &lens)).unwrap();
    assert_eq!(v["phase"], "main");
    assert!(v["volcano"].is_null());
    assert_eq!(v["lava"]["hex"], 0);
    assert_eq!(v["board"]["hexes"][0][2], "volcano");
}

#[test]
fn random_games_on_a_volcano_map_keep_invariants() {
    let mut rng = rand::thread_rng();
    let mut erupted = 0;
    for game_no in 0..60 {
        let n = 2 + game_no % 2;
        let mut def = volcano_def();
        def.hexes[0].number = Slot::Fixed(if game_no % 2 == 0 { 6 } else { 5 });
        def.token_pool = std::collections::BTreeMap::from([(2, 1), (3, 1), (4, 1), (9, 1), (10, 1), (11, 1)]);
        let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
        let s = Settings { starting_settlements: 1, ..Settings::default() };
        let mut g = Game::try_with_def(names(n), colors, TimerMode::VeryFast, s, "volcano", &def).unwrap();
        play_random(&mut g, &mut rng);
        invariants(&g);
        if g.log.iter().any(|e| matches!(e, LogEvent::LavaFlow { .. })) {
            erupted += 1;
        }
    }
    // (a volcano only erupts with a building on it, which random play doesn't always give it)
    assert!(erupted > 15, "only {erupted} of 60 games saw an eruption");
}
