//! Whole games played by random legal moves, checking the invariants after every move.

use crate::game::{Game, PALETTE, Phase, Settings, TimerMode};
use rand::Rng;
use super::support::{invariants, names, play_random};

/// Random-play fuzzing. The harness occasionally moves cards from the bank
/// to a player (conserving totals) so that games progress quickly.
#[test]
fn random_games_keep_invariants() {
    let mut rng = rand::thread_rng();
    let mut finished = 0;
    for game_no in 0..60 {
        let n = 2 + game_no % 3;
        // Half the games use a shuffled order and the real constructor.
        let mut g = if game_no % 2 == 0 {
            let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
            Game::with_options(names(n), colors, TimerMode::VeryFast, Settings::default(), "standard")
        } else {
            Game::new(names(n))
        };
        play_random(&mut g, &mut rng);
        invariants(&g);
        if g.phase == Phase::GameOver {
            finished += 1;
            let w = g.winner.unwrap();
            assert!(g.vp_total(w) >= g.settings.points_to_win as usize);
        }
    }
    assert!(finished > 10, "only {finished} of 60 random games finished");
}

fn random_settings(rng: &mut impl Rng) -> Settings {
    let start = rng.gen_range(1..=3u8);
    let discard_limit = rng.gen_range(2..=10);
    let s = Settings {
        points_to_win: rng.gen_range(5..=15),
        discard_limit,
        variable_discard: rng.gen_bool(0.5),
        discard_step_rolls: rng.gen_range(1..=12),
        discard_limit_cap: rng.gen_range(discard_limit..=20),
        alchemist: rng.gen_bool(0.5),
        friendly_robber: rng.gen_bool(0.5),
        bridges: rng.gen_bool(0.5),
        max_roads: rng.gen_range(start.max(3)..=20),
        max_cities: rng.gen_range(0..=6),
        max_settlements: rng.gen_range(start..=8),
        dev_cards: rng.gen_range(0..=40),
        resource_max: rng.gen_range(4..=30),
        settlement_points: rng.gen_range(0..=3),
        city_points: rng.gen_range(0..=4),
        longest_road_points: rng.gen_range(0..=3),
        largest_army_points: rng.gen_range(0..=3),
        vp_card_points: rng.gen_range(0..=3),
        longest_road_min: rng.gen_range(2..=7),
        largest_army_min: rng.gen_range(1..=5),
        starting_settlements: start,
        bank_ratio: rng.gen_range(1..=5),
        // `play_random` drives a turn-based game, so these random-rules games stay turn-based.
        speed: false,
    };
    s.validate().unwrap();
    s
}

#[test]
fn random_games_with_random_settings_keep_invariants() {
    let mut rng = rand::thread_rng();
    let mut finished = 0;
    for game_no in 0..80 {
        let n = 2 + game_no % 3;
        let s = random_settings(&mut rng);
        s.check_players(n).unwrap();
        let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
        let map = if game_no % 2 == 0 { "standard" } else { "earth" };
        let mut g = Game::with_options(names(n), colors, TimerMode::VeryFast, s, map);
        play_random(&mut g, &mut rng);
        invariants(&g);
        if g.phase == Phase::GameOver {
            finished += 1;
            let w = g.winner.unwrap();
            assert!(g.vp_total(w) >= s.points_to_win as usize);
        }
    }
    assert!(finished > 10, "only {finished} of 80 random-rule games finished");
}
