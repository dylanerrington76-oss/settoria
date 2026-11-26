//! Turn order and the countdowns: lengths, bonuses, pauses, and what happens when time runs out.

use crate::game::{Action, ClockKind, Game, PALETTE, Phase, Settings, TimerMode};
use crate::types::{DevCard, Res, Resource};
use std::time::Instant;
use super::support::{all_connected, into_main, names, P0, P1, P2, pick, run_setup, secs, timed};

/// Seconds left on the running clock at `now`.
fn left(g: &Game, now: Instant) -> f64 {
    g.clock.unwrap().deadline.saturating_duration_since(now).as_secs_f64()
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
