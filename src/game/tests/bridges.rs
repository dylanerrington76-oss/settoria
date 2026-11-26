//! Bridges: roads over water.

use crate::game::{Action, Building, COST_BRIDGE, Game, PALETTE, Phase, Settings, TimerMode};
use crate::log_event::LogEvent;
use crate::types::{Res, Resource};
use std::time::Instant;
use super::support::{deal, into_main, invariants, names, P0, P1, with};

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
    // The browser works out the land edges itself and is sent the bridge sites as pairs of corners, in
    // edge order (see board_wire.rs), so the one at edge `e` is entry `e - land_edges`.
    let v = serde_json::to_value(&g.board).unwrap();
    let land = g.board.edges.iter().filter(|e| !e.bridge).count();
    assert!(e >= land, "bridge sites come after the land edges");
    assert_eq!(v["bridges"][e - land], serde_json::json!([g.board.edges[e].a, g.board.edges[e].b]));
    assert_eq!(v["bridges"].as_array().unwrap().len(), g.board.edges.len() - land);
    assert!(v.get("edges").is_none() && v.get("vertices").is_none(), "corners and sides are not sent");
}
