//! Settoria Speed: no turns, a roll every `SPEED_ROLL_SECS`, everyone builds and trades at any time.

use crate::game::clock::{ROBBER_SECS, SPEED_ROLL_SECS, STEAL_SECS};
use crate::game::{Action, Building, ClockKind, COST_DEV, COST_SETTLEMENT, Game, PALETTE, Phase, Settings, TimerMode};
use crate::log_event::LogEvent;
use crate::types::{DevCard, PerPlayer, PlayerId, Res, Resource};
use rand::Rng;
use rand::seq::SliceRandom;
use std::time::{Duration, Instant};

const P0: PlayerId = PlayerId::new(0);

const P1: PlayerId = PlayerId::new(1);

const P2: PlayerId = PlayerId::new(2);

fn secs(d: u64) -> Duration {
    Duration::from_secs(d)
}

/// Seconds left on the running clock at `now`.
fn left(g: &Game, now: Instant) -> f64 {
    g.clock.unwrap().deadline.saturating_duration_since(now).as_secs_f64()
}

/// A Speed game of `n` players with setup done (taking the first legal spot each time), in seat order.
fn speed_game_with(n: usize, edit: impl FnOnce(&mut Settings)) -> (Game, Instant) {
    let t0 = Instant::now();
    let colors: Vec<String> = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
    let names: Vec<String> = (0..n).map(|i| format!("P{i}")).collect();
    let mut settings = Settings { speed: true, ..Settings::default() };
    edit(&mut settings);
    let mut g = Game::build(names, colors, TimerMode::Normal, settings, "standard", false, t0);
    while g.phase == Phase::Setup {
        let p = g.current;
        let l = g.legal(p);
        let act = if g.setup_vertex.is_some() {
            Action::BuildRoad { edge: l.roads[0] }
        } else {
            Action::BuildSettlement { vertex: l.settlements[0] }
        };
        g.apply_at(p, act, t0).unwrap();
    }
    (g, t0)
}

fn speed_game(n: usize) -> (Game, Instant) {
    speed_game_with(n, |_| {})
}

/// Carry out whatever a 7 or a Knight leaves to do (discards, the robber, the victim) at `now`.
fn settle(g: &mut Game, now: Instant) {
    for _ in 0..40 {
        match g.phase {
            Phase::Discard => {
                let Some(&(q, need)) = g.discards.first() else { return };
                let mut cards = Res::ZERO;
                let mut still = need as u32;
                for r in Resource::ALL {
                    let take = (g.players[q].hand[r] as u32).min(still);
                    cards[r] = take as u8;
                    still -= take;
                }
                g.apply_at(q, Action::Discard { cards }, now).unwrap();
            }
            Phase::MoveRobber => {
                let p = g.current;
                let hex = g.legal(p).hexes[0];
                g.apply_at(p, Action::MoveRobber { hex }, now).unwrap();
            }
            Phase::Steal => {
                let p = g.current;
                let victim = g.steal_candidates[0];
                g.apply_at(p, Action::Steal { victim }, now).unwrap();
            }
            _ => return,
        }
    }
    panic!("the robber sequence did not finish");
}

/// A free corner that `a` and `b` each have a road beside, so either could settle it.
fn contested_spot(g: &mut Game, a: PlayerId, b: PlayerId) -> usize {
    let v = (0..g.buildings.len())
        .find(|&v| g.settlement_free(v) && g.board.vertices[v].edges.len() >= 2)
        .expect("a free corner with two sides");
    let e = g.board.vertices[v].edges.clone();
    g.roads[e[0]] = Some(a);
    g.roads[e[1]] = Some(b);
    v
}

#[test]
fn setup_is_still_turn_based_and_then_the_table_opens_with_a_20_second_countdown() {
    let t0 = Instant::now();
    let colors: Vec<String> = PALETTE.iter().take(3).map(|c| c.to_string()).collect();
    let names: Vec<String> = (0..3).map(|i| format!("P{i}")).collect();
    let settings = Settings { speed: true, ..Settings::default() };
    let mut g = Game::build(names, colors, TimerMode::Normal, settings, "standard", false, t0);
    assert_eq!(g.phase, Phase::Setup);
    let spot = g.legal(g.current).settlements[0];
    let not_yet = PlayerId::new((g.current.index() + 1) % 3);
    assert!(g.apply_at(not_yet, Action::BuildSettlement { vertex: spot }, t0).is_err(), "setup waits for its player");
    assert_eq!(g.clock.unwrap().kind, ClockKind::SetupSettlement);

    let (g, t0) = speed_game(3);
    assert_eq!(g.phase, Phase::Main);
    let c = g.clock.unwrap();
    assert_eq!((c.kind, c.total), (ClockKind::AutoRoll, secs(SPEED_ROLL_SECS)));
    assert!((left(&g, t0) - 20.0).abs() < 0.01);
}

#[test]
fn the_server_rolls_every_20_seconds_and_the_countdown_starts_again_after_each_roll() {
    let (mut g, t0) = speed_game(3);
    assert!(!g.tick(t0 + secs(19)));
    assert!(g.dice.is_none());
    let mut now = t0;
    for i in 1..=30u32 {
        now += secs(21);
        assert!(g.tick(now), "roll {i}");
        assert_eq!(g.roll_counts.iter().sum::<u32>(), i);
        settle(&mut g, now);
        assert_eq!(g.phase, Phase::Main);
        let c = g.clock.unwrap();
        assert_eq!(c.kind, ClockKind::AutoRoll);
        assert!((left(&g, now) - 20.0).abs() < 0.01, "a fresh 20 seconds after roll {i}");
    }
    assert!(!g.log.iter().any(|e| matches!(e, LogEvent::TimedOut { .. })), "an automatic roll is nobody running out of time");
    assert!(g.log.iter().any(|e| matches!(e, LogEvent::DiceRolled { .. })));
}

#[test]
fn building_does_not_move_the_countdown() {
    let (mut g, t0) = speed_game(3);
    let v = contested_spot(&mut g, P1, P2);
    g.players[P1].hand = COST_SETTLEMENT;
    g.apply_at(P1, Action::BuildSettlement { vertex: v }, t0 + secs(5)).unwrap();
    assert!((left(&g, t0 + secs(5)) - 15.0).abs() < 0.01, "no bonus, no reset");
}

#[test]
fn anyone_can_build_at_any_time_and_the_first_request_for_a_spot_wins() {
    let (mut g, t0) = speed_game(3);
    assert_eq!(g.current, P0, "it is nobody's turn: P1 and P2 are not the player `current`");
    let v = contested_spot(&mut g, P1, P2);
    g.players[P1].hand = COST_SETTLEMENT;
    g.players[P2].hand = COST_SETTLEMENT;
    assert!(g.legal(P1).settlements.contains(&v) && g.legal(P2).settlements.contains(&v));
    // Both click Build on the same corner "at once": the server handles one request first.
    g.apply_at(P2, Action::BuildSettlement { vertex: v }, t0).unwrap();
    let second = g.apply_at(P1, Action::BuildSettlement { vertex: v }, t0);
    assert!(second.is_err(), "the second request fails");
    assert_eq!(g.buildings[v], Some(Building { owner: P2, city: false }));
    assert_eq!(g.players[P2].hand.total(), 0, "the winner paid");
    assert_eq!(g.players[P1].hand, COST_SETTLEMENT, "the loser paid nothing");
    assert!(!g.legal(P1).settlements.contains(&v));
}

#[test]
fn there_are_no_rolls_or_turns_to_ask_for() {
    let (mut g, t0) = speed_game(2);
    assert!(g.apply_at(P0, Action::Roll, t0).is_err());
    assert!(g.apply_at(P0, Action::EndTurn, t0).is_err());
    assert!(g.apply_at(P1, Action::EndTurn, t0).is_err());
    assert!(g.dice.is_none());
}

#[test]
fn the_robber_moves_down_the_line_one_seven_at_a_time() {
    let (mut g, t0) = speed_game(3);
    let order = g.order.clone();
    let mut now = t0;
    for k in 0..7usize {
        let lead = order[k % 3];
        assert_eq!(g.current, lead, "before 7 number {k} `current` is the player who will place it");
        g.resolve_roll(7);
        g.sync_clock(now);
        g.roll_left = None; // (what `tick` does after an automatic roll)
        assert_eq!(g.current, lead);
        assert_eq!(g.phase, Phase::MoveRobber);
        // nobody else may place it, or build, while it moves
        let other = PlayerId::new((lead.index() + 1) % 3);
        let hex = g.legal(lead).hexes[0];
        assert!(g.apply_at(other, Action::MoveRobber { hex }, now).is_err());
        assert!(g.apply_at(other, Action::BuildRoad { edge: 0 }, now).is_err(), "everyone waits");
        assert!(g.legal(other).roads.is_empty() && g.legal(other).settlements.is_empty());
        now += secs(3);
        settle(&mut g, now);
        assert_eq!(g.phase, Phase::Main);
        assert_eq!(g.rotation, k + 1);
        assert_eq!(g.current, order[(k + 1) % 3]);
        assert_eq!(g.clock.unwrap().kind, ClockKind::AutoRoll);
        assert!((left(&g, now) - 20.0).abs() < 0.01, "the countdown starts again once the 7 is dealt with");
    }
}

#[test]
fn while_someone_discards_everyone_waits_and_the_20_seconds_do_not_start() {
    let (mut g, t0) = speed_game(3);
    g.players[P1].hand = Res::new(3, 3, 1, 1, 1); // 9 cards: more than 7, so 4 must go
    g.resolve_roll(7);
    g.sync_clock(t0 + secs(20));
    g.roll_left = None; // (what `tick` does after an automatic roll)
    assert_eq!(g.phase, Phase::Discard);
    assert_eq!(g.discards, vec![(P1, 4)]);
    assert_eq!(g.clock.unwrap().kind, ClockKind::Discard);
    // the others can neither build nor trade, and have no legal spots
    g.players[P2].hand = COST_SETTLEMENT;
    assert!(g.apply_at(P2, Action::BuildRoad { edge: 0 }, t0 + secs(21)).is_err());
    assert!(g.apply_at(P2, Action::BankTrade { give: Resource::Brick, get: Resource::Ore }, t0 + secs(21)).is_err());
    assert!(g.apply_at(P2, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 0, 1, 0, 0) }, t0 + secs(21)).is_err());
    // no roll happens however long they take (the 20 s countdown is not running)
    assert!(!g.log.iter().any(|e| matches!(e, LogEvent::DiceRolled { .. })));
    g.apply_at(P1, Action::Discard { cards: Res::new(3, 1, 0, 0, 0) }, t0 + secs(25)).unwrap();
    assert_eq!(g.phase, Phase::MoveRobber);
    settle(&mut g, t0 + secs(26));
    assert_eq!(g.phase, Phase::Main);
    assert!((left(&g, t0 + secs(26)) - 20.0).abs() < 0.01);
}

#[test]
fn a_knight_freezes_the_countdown_and_its_owner_places_the_robber() {
    let (mut g, t0) = speed_game(3);
    g.players[P2].dev[DevCard::Knight] = 1;
    let lead = g.current;
    assert_ne!(lead, P2);
    g.apply_at(P2, Action::PlayKnight, t0 + secs(8)).unwrap();
    assert_eq!((g.phase, g.current), (Phase::MoveRobber, P2));
    assert_eq!(g.rotation, 0, "a Knight is not part of the rotation of 7s");
    // time passes while the robber is being placed
    settle(&mut g, t0 + secs(14));
    assert_eq!(g.phase, Phase::Main);
    assert_eq!(g.current, lead);
    assert!((left(&g, t0 + secs(14)) - 12.0).abs() < 0.01, "the 12 seconds that were left, not a fresh 20");
    assert_eq!(g.players[P2].knights, 1);
}

#[test]
fn a_robber_that_nobody_places_is_placed_by_the_clock() {
    let (mut g, t0) = speed_game(3);
    g.resolve_roll(7);
    g.sync_clock(t0);
    g.roll_left = None; // (what `tick` does after an automatic roll)
    let lead_before = g.current;
    assert_eq!(g.clock.unwrap().kind, ClockKind::Robber);
    assert!(g.tick(t0 + secs(ROBBER_SECS + 1)));
    if g.phase == Phase::Steal {
        assert!(g.tick(t0 + secs(ROBBER_SECS + 1 + STEAL_SECS + 1)));
    }
    assert_eq!(g.phase, Phase::Main);
    assert_ne!(g.current, lead_before);
}

#[test]
fn development_cards_are_limited_to_one_per_roll_and_new_ones_wait_for_the_next_roll() {
    let (mut g, t0) = speed_game(3);
    g.players[P1].dev[DevCard::YearOfPlenty] = 2;
    let plenty = Action::PlayYearOfPlenty { a: Resource::Brick, b: Resource::Lumber };
    g.apply_at(P1, plenty.clone(), t0).unwrap();
    assert!(g.apply_at(P1, plenty.clone(), t0).is_err(), "one development card between two rolls");
    assert!(g.dev_played_by(P1) && !g.dev_played_by(P0));
    // a bought card isn't playable yet
    g.players[P0].hand = COST_DEV;
    g.apply_at(P0, Action::BuyDev, t0).unwrap();
    assert_eq!(g.players[P0].dev_new.total(), 1);
    assert_eq!(g.players[P0].dev.total(), 0);
    // the next roll opens a new round
    let now = t0 + secs(21);
    assert!(g.tick(now));
    settle(&mut g, now);
    assert_eq!(g.players[P0].dev_new.total(), 0);
    assert_eq!(g.players[P0].dev.total(), 1);
    assert!(!g.dev_played_by(P1));
    g.apply_at(P1, plenty, now).unwrap();
}

#[test]
fn the_monopoly_lock_is_per_player_and_a_roll_clears_it() {
    let (mut g, t0) = speed_game(3);
    g.players[P1].traded_away[Resource::Brick] = 2;
    assert!(g.monopoly_locked_for(P1, Resource::Brick));
    assert!(!g.monopoly_locked_for(P0, Resource::Brick));
    assert!(!g.monopoly_locked_for(P1, Resource::Lumber));
    let connected = PerPlayer::from_fn(3, |_| true);
    let lens = g.longest_roads();
    assert!(g.view(P1, &connected, &lens).monopoly_locked[Resource::Brick]);
    assert!(!g.view(P0, &connected, &lens).monopoly_locked[Resource::Brick]);
    g.players[P1].dev[DevCard::Monopoly] = 1;
    assert!(g.apply_at(P1, Action::PlayMonopoly { resource: Resource::Brick }, t0).is_err());
    let now = t0 + secs(21);
    g.tick(now);
    settle(&mut g, now);
    assert!(!g.monopoly_locked_for(P1, Resource::Brick));
}

#[test]
fn any_player_can_offer_and_each_has_a_limit_of_their_own() {
    let (mut g, t0) = speed_game(3);
    g.players[P1].hand = Res::new(1, 0, 0, 0, 0);
    g.players[P2].hand = Res::new(0, 0, 2, 0, 0);
    let wants = [Res::new(0, 0, 1, 0, 0), Res::new(0, 1, 0, 0, 0), Res::new(0, 0, 0, 1, 0), Res::new(0, 0, 0, 0, 1)];
    for w in wants {
        g.apply_at(P1, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: w }, t0).unwrap();
    }
    assert!(g.apply_at(P1, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 0, 2, 0, 0) }, t0).is_err(), "five offers from one player");
    assert!(g.apply_at(P2, Action::OfferTrade { give: Res::new(0, 0, 1, 0, 0), want: Res::new(1, 0, 0, 0, 0) }, t0).is_ok(), "another player's offers are counted separately");
    assert_eq!(g.trades.len(), 5);
    // only the owner can withdraw an offer
    let first = g.trades[0].id;
    assert!(g.apply_at(P0, Action::CancelTrade { id: first }, t0).is_err());
    assert!(g.apply_at(P2, Action::CancelTrade { id: first }, t0).is_err());
    g.apply_at(P1, Action::CancelTrade { id: first }, t0).unwrap();
    assert_eq!(g.trades.len(), 4);
}

#[test]
fn two_players_trade_while_a_third_is_the_player_the_robber_will_go_to() {
    let (mut g, t0) = speed_game(3);
    g.players[P1].hand = Res::new(1, 0, 0, 0, 0);
    g.players[P2].hand = Res::new(0, 0, 1, 0, 0);
    g.apply_at(P1, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 0, 1, 0, 0) }, t0).unwrap();
    let id = g.trades[0].id;
    assert!(g.apply_at(P1, Action::ConfirmTrade { id, with: P2 }, t0).is_err(), "not accepted yet");
    g.apply_at(P2, Action::RespondTrade { id, accept: true }, t0).unwrap();
    g.apply_at(P1, Action::ConfirmTrade { id, with: P2 }, t0).unwrap();
    assert_eq!(g.players[P1].hand, Res::new(0, 0, 1, 0, 0));
    assert_eq!(g.players[P2].hand, Res::new(1, 0, 0, 0, 0));
    assert_eq!(g.players[P1].traded_away[Resource::Brick], 1);
    assert_eq!(g.players[P2].traded_away[Resource::Ore], 0, "only what the proposer gave counts, as in a classic game");
    assert_eq!(g.players[P1].stats.trades, 1);
}

#[test]
fn road_building_in_speed_mode_credits_two_free_roads() {
    let (mut g, t0) = speed_game(2);
    g.players[P1].dev[DevCard::RoadBuilding] = 1;
    g.players[P1].hand = Res::ZERO;
    g.apply_at(P1, Action::PlayRoadBuilding, t0).unwrap();
    assert_eq!(g.phase, Phase::Main, "nobody waits for the roads");
    assert_eq!(g.players[P1].free_roads, 2);
    let connected = PerPlayer::from_fn(2, |_| true);
    let lens = g.longest_roads();
    assert_eq!(g.view(P1, &connected, &lens).road_building_left, 2);
    assert_eq!(g.view(P0, &connected, &lens).road_building_left, 0);
    let edge = g.legal(P1).roads[0];
    g.apply_at(P1, Action::BuildRoad { edge }, t0).unwrap();
    assert_eq!(g.players[P1].free_roads, 1);
    assert_eq!(g.players[P1].hand, Res::ZERO, "free");
    let edge = g.legal(P1).roads[0];
    g.apply_at(P1, Action::BuildRoad { edge }, t0).unwrap();
    let edge = g.legal(P1).roads[0];
    assert!(g.apply_at(P1, Action::BuildRoad { edge }, t0).is_err(), "the third road has to be paid for");
}

#[test]
fn reaching_the_points_target_wins_at_once_whoever_it_is() {
    let (mut g, t0) = speed_game_with(3, |s| s.points_to_win = 3);
    let v = contested_spot(&mut g, P1, P2);
    g.players[P2].hand = COST_SETTLEMENT;
    assert_eq!(g.vp_total(P2), 2);
    g.apply_at(P2, Action::BuildSettlement { vertex: v }, t0).unwrap();
    assert_eq!(g.winner, Some(P2));
    assert_eq!(g.phase, Phase::GameOver);
    assert!(g.apply_at(P1, Action::BankTrade { give: Resource::Brick, get: Resource::Ore }, t0).is_err());
}

#[test]
fn speed_mode_is_announced_in_the_log_and_off_by_default() {
    assert!(!Settings::default().speed);
    assert!(Settings::default().changes().is_empty());
    assert_eq!(Settings { speed: true, ..Settings::default() }.changes(), vec!["Speed mode".to_string()]);
    let (g, _) = speed_game(2);
    assert!(g.log.iter().any(|e| matches!(e, LogEvent::CustomRules { changes } if changes.iter().any(|c| c == "Speed mode"))));
}

/// Random play with the clock jumping about: nothing panics, and cards are never created or lost.
#[test]
fn random_speed_games_conserve_cards_and_never_get_stuck() {
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(11);
    for game in 0..6 {
        let (mut g, t0) = speed_game_with(2 + game % 3, |s| s.points_to_win = 30);
        let mut now = t0;
        for _ in 0..600 {
            if g.phase == Phase::GameOver {
                break;
            }
            // sometimes a player is handed cards from the bank
            if rng.gen_bool(0.2) {
                let p = PlayerId::new(rng.gen_range(0..g.n));
                let r = *Resource::ALL.choose(&mut rng).unwrap();
                if g.bank[r] > 0 {
                    g.bank[r] -= 1;
                    g.players[p].hand[r] += 1;
                }
            }
            let p = PlayerId::new(rng.gen_range(0..g.n));
            let l = g.legal(p);
            let act = match rng.gen_range(0..9) {
                0 => l.settlements.choose(&mut rng).map(|&vertex| Action::BuildSettlement { vertex }),
                1 => l.cities.choose(&mut rng).map(|&vertex| Action::BuildCity { vertex }),
                2 | 3 => l.roads.choose(&mut rng).map(|&edge| Action::BuildRoad { edge }),
                4 => Some(Action::BuyDev),
                5 => Some(Action::BankTrade { give: *Resource::ALL.choose(&mut rng).unwrap(), get: *Resource::ALL.choose(&mut rng).unwrap() }),
                6 => {
                    let (a, b) = (*Resource::ALL.choose(&mut rng).unwrap(), *Resource::ALL.choose(&mut rng).unwrap());
                    let (mut give, mut want) = (Res::ZERO, Res::ZERO);
                    give[a] = 1;
                    want[b] = 1;
                    Some(Action::OfferTrade { give, want })
                }
                7 => g.trades.first().map(|t| Action::RespondTrade { id: t.id, accept: rng.gen_bool(0.5) }),
                _ => {
                    let kinds = [DevCard::Knight, DevCard::RoadBuilding, DevCard::YearOfPlenty, DevCard::Monopoly];
                    match *kinds.choose(&mut rng).unwrap() {
                        DevCard::Knight => Some(Action::PlayKnight),
                        DevCard::RoadBuilding => Some(Action::PlayRoadBuilding),
                        DevCard::YearOfPlenty => Some(Action::PlayYearOfPlenty { a: Resource::Ore, b: Resource::Grain }),
                        _ => Some(Action::PlayMonopoly { resource: *Resource::ALL.choose(&mut rng).unwrap() }),
                    }
                }
            };
            if let Some(a) = act {
                let _ = g.apply_at(p, a, now);
            }
            // a robber or discard left hanging is dealt with, now or by the clock
            if g.phase != Phase::Main && g.phase != Phase::GameOver {
                if rng.gen_bool(0.5) {
                    settle(&mut g, now);
                } else {
                    now += secs(40);
                    g.tick(now);
                }
            }
            if rng.gen_bool(0.1) {
                now += secs(21);
                g.tick(now);
            }
            for r in Resource::ALL {
                let held: u32 = g.players.iter().map(|pl| pl.hand[r] as u32).sum();
                assert_eq!(held + g.bank[r] as u32, g.settings.resource_max as u32, "{} not conserved", r.name());
            }
            if g.phase == Phase::Main {
                assert_eq!(g.clock.map(|c| c.kind), Some(ClockKind::AutoRoll));
            }
            for t in &g.trades {
                assert!(g.players[t.from].hand.covers(&t.give), "an open offer its proposer can't pay for");
            }
        }
    }
}
