//! The longest roads kept by `Game::longest_roads` follow every change to the roads and buildings.
//! (The random games in `random_play.rs` check them against a fresh search after every move, through `invariants`.)

use crate::game::{Action, COST_ROAD, Game, Phase};
use crate::types::PerPlayer;
use rand::thread_rng;
use std::time::Instant;
use super::support::{deal, into_main, names, run_setup, volcano_game, build_on_volcano, P0, P1};

fn fresh(g: &Game) -> PerPlayer<usize> {
    PerPlayer::from_fn(g.n, |p| g.longest_road(p))
}

#[test]
fn a_setup_road_replaces_the_kept_lengths() {
    // Setup roads don't run the award check, so placing one must clear the kept lengths itself.
    let mut g = Game::new(names(2));
    let p = g.current;
    g.apply(p, Action::BuildSettlement { vertex: 0 }).unwrap();
    assert_eq!(g.longest_roads()[p], 0, "kept now");
    let edge = g.board.vertices[0].edges[0];
    g.apply(p, Action::BuildRoad { edge }).unwrap();
    assert_eq!(g.longest_roads()[p], 1);
    assert_eq!(g.longest_roads(), fresh(&g));
}

#[test]
fn the_award_check_leaves_the_lengths_for_the_broadcast_to_reuse() {
    let mut g = Game::new(names(2));
    run_setup(&mut g, &mut thread_rng());
    let p = g.current;
    into_main(&mut g, p, Instant::now());
    deal(&mut g, p, COST_ROAD);
    g.longest_roads(); // a broadcast after the last move
    let edge = g.legal(p).roads[0];
    g.apply(p, Action::BuildRoad { edge }).unwrap();
    assert_eq!(g.phase, Phase::Main);
    let kept = g.road_lens.borrow().clone();
    assert_eq!(kept, Some(fresh(&g)), "the build's own award check already searched, and left the result");
}

#[test]
fn lava_destroying_a_settlement_replaces_the_kept_lengths() {
    let mut g = volcano_game(2);
    let v = build_on_volcano(&mut g, 0, P1, false);
    let edges = g.board.vertices[v].edges.clone();
    g.roads[edges[0]] = Some(P0);
    g.roads[edges[1]] = Some(P0);
    assert_eq!(g.longest_roads()[P0], 1, "P1's settlement cuts the road in two");
    g.erupt_with(0, 1);
    assert_eq!(g.buildings[v], None);
    assert_eq!(g.longest_roads()[P0], 2, "and the lava joins it up");
    assert_eq!(g.longest_roads(), fresh(&g));
}
