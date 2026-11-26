//! The structured game log, and how a view numbers its window of it.

use crate::game::{Action, Building, ClockKind, Game, Phase, TimerMode};
use crate::log_event::LogEvent;
use crate::types::{PerPlayer, PlayerId, Res, Resource};
use std::time::Instant;
use super::support::{all_connected, into_main, names, P0, P1, pick, run_setup, secs, timed};

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

/// The view carries only the last 50 events; `log_start` numbers the first of them so a client can tell
/// which lines it already has after the window slides (and after old events are dropped from the front).
#[test]
fn log_lines_keep_their_numbers_as_the_window_slides() {
    let mut g = Game::new((0..2).map(|i| format!("P{i}")).collect());
    let connected = PerPlayer::from_fn(2, |_| true);
    let me = PlayerId::new(0);
    let mut last_start = None;
    for _ in 0..300 {
        g.log(LogEvent::LongestRoadLost);
        let lens = g.longest_roads();
        let v = g.view(me, &connected, &lens);
        let total = g.log_dropped + g.log.len();
        assert_eq!(v.log_start + v.log.len(), total, "the window always ends at the newest event");
        if let Some(prev) = last_start {
            assert!(v.log_start >= prev, "numbers never go backwards");
        }
        last_start = Some(v.log_start);
    }
    assert!(g.log_dropped > 0, "the test must push events off the front of the log");
}
