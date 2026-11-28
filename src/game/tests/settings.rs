//! The host's rules (`Settings`): ranges, scaling with the table, points, piece limits, Friendly Robber.

use crate::game::{Action, Building, Game, PALETTE, Phase, Settings, TimerMode, VpBreakdown};
use crate::log_event::LogEvent;
use crate::types::{ByDev, DevCard, PlayerId, Res, Resource};
use rand::Rng;
use std::time::Instant;
use super::support::{all_connected, invariants, line_of_roads, names, P0, P1, P2, pick, play_random, secs, with};

/// The first vertex of some hex, plus that hex, for placing test buildings.
fn some_hex_vertex(g: &Game) -> (usize, usize) {
    let h = (0..g.board.hexes.len()).find(|&h| h != g.robber).unwrap();
    (h, g.board.hexes[h].vertices[0])
}

#[test]
fn defaults_are_the_standard_rules_with_15_points() {
    let d = Settings::default();
    assert_eq!(d.validate(), Ok(()));
    assert_eq!(
        (d.points_to_win, d.discard_limit, d.friendly_robber, d.max_roads, d.max_cities, d.max_settlements),
        (15, 7, false, 15, 4, 5)
    );
    assert_eq!((d.dev_cards, d.resource_max, d.settlement_points, d.city_points), (25, 19, 1, 2));
    assert_eq!((d.longest_road_points, d.largest_army_points, d.vp_card_points), (2, 2, 1));
    assert_eq!((d.longest_road_min, d.largest_army_min, d.starting_settlements, d.bank_ratio), (5, 3, 2, 4));
    assert!(d.changes().is_empty(), "defaults log nothing");
    assert_eq!(d.deck_counts(), ByDev([14, 2, 2, 2, 5, 0]));
    assert_eq!((d.variable_discard, d.discard_step_rolls, d.discard_limit_cap, d.alchemist), (false, 10, 10, false));
    // A partial JSON object fills in the rest from the defaults.
    let s: Settings = serde_json::from_str("{\"points_to_win\":12}").unwrap();
    assert_eq!((s.points_to_win, s.max_roads), (12, 15));
}

#[test]
fn invalid_settings_are_rejected() {
    assert!(with(|s| s.points_to_win = 2).validate().is_err());
    assert!(with(|s| s.discard_limit = 0).validate().is_err());
    assert!(with(|s| s.bank_ratio = 0).validate().is_err());
    assert!(with(|s| s.longest_road_min = 1).validate().is_err());
    assert!(with(|s| s.largest_army_min = 0).validate().is_err());
    assert!(with(|s| s.starting_settlements = 0).validate().is_err());
    assert!(with(|s| s.starting_settlements = 6).validate().is_err());
    assert!(with(|s| s.max_roads = 0).validate().is_err());
    assert!(with(|s| s.resource_max = 0).validate().is_err());
    // every starting settlement needs a road and a settlement piece
    assert!(with(|s| { s.starting_settlements = 3; s.max_roads = 2 }).validate().is_err());
    assert!(with(|s| { s.starting_settlements = 3; s.max_settlements = 2 }).validate().is_err());
    assert!(with(|s| { s.starting_settlements = 3; s.max_roads = 3; s.max_settlements = 3 }).validate().is_ok());
    // too many starting settlements for the board
    let five = with(|s| s.starting_settlements = 5);
    assert!(five.check_players(4).is_ok());
    assert!(with(|s| { s.starting_settlements = 5; s.max_settlements = 6 }).check_players(13).is_err());
    assert!(Settings::default().check_players(4).is_ok());
    // twelve players with the most starting settlements each is what the limit is sized for
    assert!(five.check_players(12).is_ok());
    assert!(Settings::default().check_players(12).is_ok());
}

#[test]
fn the_bank_and_deck_grow_with_the_table_unless_the_host_chose_otherwise() {
    let d = Settings::default();
    for n in 2..=4 {
        assert_eq!(d.scaled_for(n), d, "{n} players play the standard rules");
    }
    for (n, bank, deck) in [(5, 24, 34), (6, 24, 34), (7, 30, 44), (8, 30, 44), (9, 36, 54), (10, 36, 54), (11, 42, 60), (12, 42, 60)] {
        let s = d.scaled_for(n);
        assert_eq!((s.resource_max, s.dev_cards), (bank, deck), "{n} players");
        assert_eq!(Settings { resource_max: d.resource_max, dev_cards: d.dev_cards, ..s }, d, "nothing else changes");
        assert_eq!(s.validate(), Ok(()));
        assert_eq!(s.deck_counts().0.iter().sum::<usize>(), deck as usize);
    }
    // a value the host changed is left alone, the other one still grows
    let s = with(|s| { s.resource_max = 12; s.dev_cards = 10 }).scaled_for(8);
    assert_eq!((s.resource_max, s.dev_cards), (12, 10));
    let s = with(|s| s.dev_cards = 40).scaled_for(6);
    assert_eq!((s.resource_max, s.dev_cards), (24, 40));
}

#[test]
fn up_to_twelve_players_can_play_on_the_big_maps() {
    let mut rng = rand::thread_rng();
    for n in 5..=12usize {
        for map in ["earth", "usa"] {
            let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
            let mut g = Game::with_options(names(n), colors, TimerMode::VeryFast, Settings::default(), map);
            let scaled = Settings::default().scaled_for(n);
            assert_eq!((g.n, g.settings), (n, scaled), "{map} with {n}");
            assert_eq!(g.deck.len(), scaled.dev_cards as usize);
            assert_eq!(g.bank, Res::splat(scaled.resource_max));
            assert!(!g.log.iter().any(|e| matches!(e, LogEvent::CustomRules { .. })), "scaling isn't a custom rule");
            play_random(&mut g, &mut rng);
            invariants(&g);
        }
    }
}

#[test]
fn dev_deck_has_the_chosen_size_in_standard_proportions() {
    for total in 0..=60u8 {
        let s = with(|s| s.dev_cards = total);
        let c = s.deck_counts();
        assert_eq!(c.0.iter().sum::<usize>(), total as usize, "deck of {total}");
        let g = Game::with_settings(names(2), s);
        assert_eq!(g.deck.len(), total as usize);
    }
    assert_eq!(with(|s| s.dev_cards = 50).deck_counts(), ByDev([28, 4, 4, 4, 10, 0]));
    assert_eq!(with(|s| s.dev_cards = 10).deck_counts(), ByDev([5, 1, 1, 1, 2, 0]));
    // an empty deck can't be bought from
    let mut g = Game::with_settings(names(2), with(|s| s.dev_cards = 0));
    g.phase = Phase::Main;
    g.players[P0].hand = Res::new(0, 0, 1, 1, 1);
    assert!(g.apply(P0, Action::BuyDev).is_err());
}

#[test]
fn piece_limits_and_bank_size_come_from_the_settings() {
    let s = with(|s| { s.max_roads = 9; s.max_settlements = 3; s.max_cities = 2; s.resource_max = 7 });
    let g = Game::with_settings(names(3), s);
    for p in &g.players {
        assert_eq!((p.roads_left, p.settlements_left, p.cities_left), (9, 3, 2));
    }
    assert_eq!(g.bank, Res::splat(7));
    invariants(&g);
}

#[test]
fn points_to_win_and_point_values_are_configurable() {
    let s = with(|s| {
        s.points_to_win = 12;
        s.settlement_points = 2;
        s.city_points = 3;
        s.longest_road_points = 1;
        s.largest_army_points = 4;
        s.vp_card_points = 2;
    });
    let mut g = Game::with_settings(names(3), s);
    g.phase = Phase::Main;
    let vs: Vec<usize> = (0..54).step_by(3).take(3).collect();
    g.buildings[vs[0]] = Some(Building { owner: P0, city: false });
    g.buildings[vs[1]] = Some(Building { owner: P0, city: true });
    assert_eq!(g.vp_public(P0), 2 + 3);
    g.longest_road_holder = Some(P0);
    assert_eq!(g.vp_public(P0), 2 + 3 + 1);
    g.largest_army_holder = Some(P0);
    assert_eq!(g.vp_public(P0), 2 + 3 + 1 + 4);
    g.players[P0].dev[DevCard::VictoryPoint] = 1;
    g.players[P0].dev_new[DevCard::VictoryPoint] = 1;
    assert_eq!(g.vp_public(P0), 10, "victory point cards stay hidden");
    assert_eq!(g.vp_total(P0), 10 + 2 * 2);
    assert_eq!(
        g.vp_breakdown(P0),
        VpBreakdown { settlements: 2, cities: 3, dev_cards: 2 * 2, largest_army: 4, longest_road: 1 }
    );
    let b = g.vp_breakdown(P0);
    assert_eq!(b.settlements + b.cities + b.dev_cards + b.largest_army + b.longest_road, g.vp_total(P0));
    // 14 points is over 12: the current player wins on the spot
    g.current = P0;
    g.check_win();
    assert_eq!(g.winner, Some(P0));
    assert_eq!(g.phase, Phase::GameOver);

    // one point short of the target does not win
    let mut g = Game::with_settings(names(2), with(|s| s.points_to_win = 4));
    g.phase = Phase::Main;
    g.buildings[0] = Some(Building { owner: P0, city: true });
    g.buildings[10] = Some(Building { owner: P0, city: false });
    g.check_win();
    assert_eq!(g.winner, None, "3 points, 4 needed");
    g.buildings[20] = Some(Building { owner: P0, city: false });
    g.check_win();
    assert_eq!(g.winner, Some(P0));
}

#[test]
fn default_target_is_15_not_10() {
    let mut g = Game::new(names(2));
    g.phase = Phase::Main;
    g.players[P0].dev[DevCard::VictoryPoint] = 12;
    g.check_win();
    assert_eq!(g.winner, None, "12 points is not enough at the default 15");
    g.players[P0].dev[DevCard::VictoryPoint] = 15;
    g.check_win();
    assert_eq!(g.winner, Some(P0));
}

#[test]
fn discard_limit_decides_who_discards() {
    let mut g = Game::with_settings(names(3), with(|s| s.discard_limit = 4));
    g.phase = Phase::Roll;
    g.players[P0].hand = Res::new(2, 2, 1, 0, 0); // 5 > 4: discards 2
    g.players[P1].hand = Res::new(2, 2, 0, 0, 0); // exactly 4: keeps all
    g.players[P2].hand = Res::new(3, 3, 3, 3, 0); // 12: discards 6
    g.resolve_roll(7);
    assert_eq!(g.phase, Phase::Discard);
    let mut d = g.discards.clone();
    d.sort();
    assert_eq!(d, vec![(P0, 2), (P2, 6)]);

    // With a high limit nobody discards and the robber moves straight away.
    let mut g = Game::with_settings(names(2), with(|s| s.discard_limit = 12));
    g.phase = Phase::Roll;
    g.players[P0].hand = Res::new(3, 3, 3, 3, 0);
    g.resolve_roll(7);
    assert_eq!(g.phase, Phase::MoveRobber);
}

#[test]
fn award_minimums_are_configurable() {
    // Longest Road from 3
    let mut g = Game::with_settings(names(3), with(|s| s.longest_road_min = 3));
    g.phase = Phase::Main;
    line_of_roads(&mut g, P0, 3);
    g.update_longest_road();
    assert_eq!(g.longest_road_holder, Some(P0));
    // ...and a 5-road line does not qualify when 7 are needed
    let mut g = Game::with_settings(names(3), with(|s| s.longest_road_min = 7));
    g.phase = Phase::Main;
    line_of_roads(&mut g, P0, 5);
    g.update_longest_road();
    assert_eq!(g.longest_road_holder, None);

    // Largest Army from 2, and from 5
    let mut g = Game::with_settings(names(3), with(|s| s.largest_army_min = 2));
    g.players[P0].knights = 2;
    g.update_largest_army(P0);
    assert_eq!(g.largest_army_holder, Some(P0));
    let mut g = Game::with_settings(names(3), with(|s| s.largest_army_min = 5));
    g.players[P0].knights = 4;
    g.update_largest_army(P0);
    assert_eq!(g.largest_army_holder, None);
    g.players[P0].knights = 5;
    g.update_largest_army(P0);
    assert_eq!(g.largest_army_holder, Some(P0));
}

#[test]
fn bank_ratio_is_the_base_and_harbours_only_improve_it() {
    let mut g = Game::with_settings(names(2), with(|s| s.bank_ratio = 5));
    assert_eq!(g.ratio(P0, Resource::Brick), 5);
    let port = g.board.ports.iter().find(|p| p.resource.is_none()).unwrap().clone();
    g.buildings[port.a] = Some(Building { owner: P0, city: false });
    assert_eq!(g.ratio(P0, Resource::Brick), 3, "a 3:1 harbour beats a 5:1 base");

    // a 2:1 base is already better than a 3:1 harbour
    let mut g = Game::with_settings(names(2), with(|s| s.bank_ratio = 2));
    g.buildings[port.a] = Some(Building { owner: P0, city: false });
    assert_eq!(g.ratio(P0, Resource::Brick), 2, "a 3:1 harbour never makes a 2:1 base worse");
    // and a 1:1 base is better than any harbour
    let mut g = Game::with_settings(names(2), with(|s| s.bank_ratio = 1));
    g.buildings[port.a] = Some(Building { owner: P0, city: false });
    assert_eq!(g.ratio(P0, Resource::Brick), 1);

    // the trade itself uses the ratio
    let mut g = Game::with_settings(names(2), with(|s| s.bank_ratio = 3));
    g.phase = Phase::Main;
    g.players[P0].hand = Res::new(3, 0, 0, 0, 0);
    g.bank = Res::new(16, 19, 19, 19, 19);
    g.apply(P0, Action::BankTrade { give: Resource::Brick, get: Resource::Ore }).unwrap();
    assert_eq!(g.players[P0].hand, Res::new(0, 0, 1, 0, 0));
    assert_eq!(g.bank[Resource::Brick], 19);
    assert_eq!(g.view(P0, &all_connected(2), &g.longest_roads()).ratios, Res::splat(3));
}

/// Run the whole setup phase with random legal moves; returns (player, vertex, step) per settlement.
fn setup_log(g: &mut Game, rng: &mut impl Rng) -> Vec<(PlayerId, usize, usize)> {
    let mut placed = vec![];
    while g.phase == Phase::Setup {
        let p = g.current;
        let l = g.legal(p);
        if let Some(_) = g.setup_vertex {
            g.apply(p, Action::BuildRoad { edge: pick(&l.roads, rng).unwrap() }).unwrap();
        } else {
            let v = pick(&l.settlements, rng).unwrap();
            placed.push((p, v, g.setup_idx));
            g.apply(p, Action::BuildSettlement { vertex: v }).unwrap();
        }
    }
    placed
}

#[test]
fn starting_settlements_snake_draft_one_road_each() {
    let mut rng = rand::thread_rng();
    for start in 1..=4u8 {
        for n in 2..=4usize {
            let mut g = Game::with_settings(names(n), with(|s| s.starting_settlements = start));
            let placed = setup_log(&mut g, &mut rng);
            // snake order: forward, backward, forward...
            let expect: Vec<PlayerId> = (0..start as usize)
                .flat_map(|r| if r % 2 == 0 { PlayerId::range(n).collect::<Vec<_>>() } else { PlayerId::range(n).collect::<Vec<_>>().into_iter().rev().collect() })
                .collect();
            assert_eq!(placed.iter().map(|x| x.0).collect::<Vec<_>>(), expect, "start={start} n={n}");
            assert_eq!(g.phase, Phase::Roll);
            assert_eq!(g.current, P0, "the first placer takes the first turn");
            for p in PlayerId::range(n) {
                let s = g.buildings.iter().flatten().filter(|b| b.owner == p).count() as u8;
                let r = g.roads.iter().filter(|r| **r == Some(p)).count() as u8;
                assert_eq!((s, r), (start, start), "one road per starting settlement");
                assert_eq!(g.players[p].settlements_left, 5 - start);
                assert_eq!(g.players[p].roads_left, 15 - start);
            }
            // only each player's LAST settlement pays out
            for p in PlayerId::range(n) {
                let last = placed.iter().filter(|x| x.0 == p).last().unwrap().1;
                let mut want = Res::ZERO;
                for &h in &g.board.vertices[last].hexes {
                    if let Some(r) = g.board.hexes[h].terrain.resource() {
                        want[r] += 1;
                    }
                }
                assert_eq!(g.players[p].hand, want, "start={start} n={n} player {}", p.index());
            }
            invariants(&g);
        }
    }
}

#[test]
fn friendly_robber_protects_low_scoring_players() {
    let mut g = Game::with_settings(names(3), with(|s| s.friendly_robber = true));
    g.phase = Phase::MoveRobber;
    g.current = P0;
    let (_, v) = some_hex_vertex(&g);
    g.buildings[v] = Some(Building { owner: P1, city: false });
    let touching: Vec<usize> = g.board.vertices[v].hexes.clone();
    let allowed = g.robber_hexes(P0);
    for &h in &touching {
        assert!(!allowed.contains(&h), "hex {h} touches a 1-point player");
    }
    assert!(allowed.len() >= 15 && !allowed.contains(&g.robber));
    assert_eq!(g.legal(P0).hexes, allowed, "the client is shown only legal hexes");
    let blocked = touching.iter().copied().find(|&h| h != g.robber).unwrap();
    let err = g.apply(P0, Action::MoveRobber { hex: blocked }).unwrap_err();
    assert!(err.contains("Friendly Robber"), "{err}");
    assert_eq!(g.phase, Phase::MoveRobber, "a refused move changes nothing");

    // your own buildings never protect a hex from you
    let mut mine = Game::with_settings(names(2), with(|s| s.friendly_robber = true));
    mine.buildings[v] = Some(Building { owner: P0, city: false });
    assert!(touching.iter().all(|&h| h == mine.robber || mine.robber_hexes(P0).contains(&h)));

    // at 3 visible points a player is fair game
    g.longest_road_holder = Some(P1); // +2
    assert_eq!(g.vp_public(P1), 3);
    assert!(g.robber_hexes(P0).contains(&blocked));
    g.apply(P0, Action::MoveRobber { hex: blocked }).unwrap();
    assert_eq!(g.robber, blocked);

    // switched off: everything but the current hex is allowed
    let mut off = Game::with_settings(names(3), Settings::default());
    off.buildings[v] = Some(Building { owner: P1, city: false });
    assert_eq!(off.robber_hexes(P0).len(), 18);
}

#[test]
fn friendly_robber_protection_follows_the_starting_settlements() {
    // Three starting settlements each: everyone starts on 3 points, so 3 points is still protected.
    let mut g = Game::with_settings(names(3), with(|s| { s.friendly_robber = true; s.starting_settlements = 3 }));
    assert_eq!(g.settings.friendly_robber_max_vp(), 3);
    g.phase = Phase::MoveRobber;
    g.current = P0;
    let (_, v) = some_hex_vertex(&g);
    let touching: Vec<usize> = g.board.vertices[v].hexes.clone();
    let blocked = touching.iter().copied().find(|&h| h != g.robber).unwrap();
    for owner in [P1, P2] {
        g.buildings[v] = Some(Building { owner, city: false });
        g.longest_road_holder = Some(owner); // 1 settlement + 2 for the road: 3 points
        assert_eq!(g.vp_public(owner), 3);
        assert!(!g.robber_hexes(P0).contains(&blocked), "a 3-point player is protected with 3 starting settlements");
        let err = g.apply(P0, Action::MoveRobber { hex: blocked }).unwrap_err();
        assert!(err.contains("3 points or fewer"), "{err}");
    }
    // one point more and they are fair game
    g.settings.largest_army_points = 1;
    g.largest_army_holder = Some(P2);
    assert_eq!(g.vp_public(P2), 4);
    assert!(g.robber_hexes(P0).contains(&blocked));

    // Four starting settlements, settlements worth 2 each: everyone starts on 8.
    let s = with(|s| { s.starting_settlements = 4; s.settlement_points = 2 });
    assert_eq!(s.friendly_robber_max_vp(), 8);
    // The standard two settlements still protect 2 points.
    assert_eq!(Settings::default().friendly_robber_max_vp(), 2);
}

#[test]
fn friendly_robber_never_leaves_the_game_stuck() {
    let mut g = Game::with_settings(names(2), with(|s| s.friendly_robber = true));
    g.phase = Phase::MoveRobber;
    // player 1 (1 point each building, but low) sits on every vertex
    for v in 0..g.buildings.len() {
        g.buildings[v] = Some(Building { owner: P1, city: false });
    }
    g.longest_road_holder = None;
    assert!(g.vp_public(P1) > g.settings.friendly_robber_max_vp());
    g.settings.settlement_points = 0; // now player 1 has 0 points and protects every hex
    assert_eq!(g.vp_public(P1), 0);
    assert_eq!(g.robber_hexes(P0).len(), 18, "if every hex is protected, any hex is allowed");
}

#[test]
fn timed_out_robber_respects_friendly_robber() {
    for _ in 0..40 {
        let t0 = Instant::now();
        let mut g = Game::with_settings(names(2), with(|s| s.friendly_robber = true));
        let (_, v) = some_hex_vertex(&g);
        g.buildings[v] = Some(Building { owner: P1, city: false });
        g.phase = Phase::MoveRobber;
        g.current = P0;
        g.sync_clock(t0);
        assert!(g.tick(t0 + secs(100)));
        let touching = g.board.vertices[v].hexes.clone();
        assert!(!touching.contains(&g.robber), "auto-placed robber on a protected hex");
    }
}

#[test]
fn custom_rules_are_announced_in_the_log() {
    let g = Game::with_settings(names(2), with(|s| { s.points_to_win = 12; s.friendly_robber = true; s.bank_ratio = 3 }));
    let line = g
        .log
        .iter()
        .find_map(|l| match l {
            LogEvent::CustomRules { changes } => Some(changes.join(", ")),
            _ => None,
        })
        .expect("a custom-rules event");
    assert!(line.contains("points to win 12") && line.contains("Friendly Robber on") && line.contains("bank trade 3:1"), "{line}");
    assert!(Game::new(names(2)).log.iter().all(|l| !matches!(l, LogEvent::CustomRules { .. })));
    let v = g.view(P0, &all_connected(2), &g.longest_roads());
    assert_eq!(v.settings.points_to_win, 12);
    let json = serde_json::to_string(&v).unwrap();
    assert!(json.contains("\"settings\":{\"points_to_win\":12"));
}
