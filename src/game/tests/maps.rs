//! Games on different maps, and the colours players choose.

use crate::board::Terrain;
use crate::game::{Game, PALETTE, palette_color, Phase, Settings, TimerMode};
use crate::log_event::LogEvent;
use crate::map_def::{MapDef, MapError};
use crate::types::PerPlayer;
use super::support::{all_connected, invariants, names, P0, play_random};

#[test]
fn the_map_decides_the_board_and_where_the_robber_starts() {
    let std = Game::new(names(2));
    assert_eq!(std.board.map, "standard");
    assert_eq!(std.board.hexes.len(), 19);
    assert!(!std.log.iter().any(|l| matches!(l, LogEvent::MapChosen { .. })), "the standard map isn't announced");

    for _ in 0..30 {
        let g = Game::with_map(names(3), "earth");
        assert_eq!(g.board.map, "earth");
        assert_eq!(g.board.hexes.len(), 81);
        assert_eq!(g.board.ports.len(), 27);
        assert_eq!(g.buildings.len(), g.board.vertices.len());
        assert_eq!(g.roads.len(), g.board.edges.len());
        assert_eq!(g.board.hexes[g.robber].terrain, Terrain::Desert);
        assert!(g.log.contains(&LogEvent::MapChosen { map: "earth".into(), name: "Earth".into() }));
    }
}

#[test]
fn earth_games_can_be_played_to_the_end() {
    let mut rng = rand::thread_rng();
    let mut finished = 0;
    for game_no in 0..40 {
        let n = 2 + game_no % 3;
        let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
        let mut g = Game::with_options(names(n), colors, TimerMode::VeryFast, Settings::default(), "earth");
        play_random(&mut g, &mut rng);
        invariants(&g);
        if g.phase == Phase::GameOver {
            finished += 1;
            let w = g.winner.unwrap();
            assert!(g.vp_total(w) >= g.settings.points_to_win as usize);
        }
    }
    assert!(finished > 5, "only {finished} of 40 random Earth games finished");
}

fn archipelago() -> MapDef {
    crate::builtin::archipelago()
}

fn start_on(def: &MapDef, n: usize) -> Result<Game, MapError> {
    let colors = PALETTE.iter().take(n).map(|c| c.to_string()).collect();
    Game::try_with_def(names(n), colors, TimerMode::VeryFast, Settings::default(), "archipelago", def)
}

#[test]
fn a_map_written_as_json_can_be_played_to_the_end() {
    let def = archipelago();
    let mut rng = rand::thread_rng();
    let mut finished = 0;
    for game_no in 0..40 {
        let n = 2 + game_no % 3;
        let mut g = start_on(&def, n).expect("the map can be played with 2, 3 or 4 players");
        assert_eq!(g.board.hexes.len(), 14);
        assert_eq!(g.board.ports.len(), 7);
        // the written-in parts are where the file put them
        let hex = |q, r| g.board.hexes.iter().find(|h| (h.q, h.r) == (q, r)).unwrap();
        assert_eq!((hex(1, 1).terrain, hex(1, 1).number), (Terrain::Mountains, Some(8)));
        assert_eq!(hex(6, 0).terrain, Terrain::Desert);
        play_random(&mut g, &mut rng);
        invariants(&g);
        if g.phase == Phase::GameOver {
            finished += 1;
            let w = g.winner.unwrap();
            assert!(g.vp_total(w) >= g.settings.points_to_win as usize);
        }
    }
    assert!(finished > 5, "only {finished} of 40 random games on the custom map finished");
}

#[test]
fn the_robber_starts_where_the_map_says_or_on_a_desert() {
    let mut def = archipelago();
    for _ in 0..30 {
        let g = start_on(&def, 3).unwrap();
        assert_eq!(g.board.hexes[g.robber].terrain, Terrain::Desert);
    }
    def.robber_start = Some(crate::map_def::HexRef { q: 2, r: 1 });
    let g = start_on(&def, 3).unwrap();
    assert_eq!((g.board.hexes[g.robber].q, g.board.hexes[g.robber].r), (2, 1));
}

#[test]
fn a_map_is_only_started_for_the_players_it_is_meant_for() {
    let mut def = archipelago();
    def.players = crate::map_def::PlayerRange { min: 3, max: 4 };
    assert_eq!(start_on(&def, 2).err(), Some(MapError::PlayerCount { players: 2, min: 3, max: 4 }));
    assert!(start_on(&def, 3).is_ok());
    assert!(start_on(&def, 4).is_ok());
    def.players = crate::map_def::PlayerRange { min: 2, max: 2 };
    assert_eq!(start_on(&def, 3).err(), Some(MapError::PlayerCount { players: 3, min: 2, max: 2 }));
    // a range the engine can't play is the map's mistake, whatever the lobby holds
    def.players = crate::map_def::PlayerRange { min: 0, max: 13 };
    assert_eq!(start_on(&def, 3).err(), Some(MapError::BadPlayerRange { min: 0, max: 13 }));
}

#[test]
fn a_map_that_cannot_be_built_is_an_error_not_a_panic() {
    let mut def = archipelago();
    def.token_pool.clear(); // 11 hexes want a token
    let e = start_on(&def, 3).err().expect("refused");
    assert_eq!(e, MapError::TokenPoolMismatch { needed: 11, pool: 0 });
    assert!(e.to_string().contains("11"), "{e}");

    let mut def = archipelago();
    def.ports = crate::map_def::PortsDef::Fixed { spots: vec![crate::map_def::PortSpot::at(1, 1, 0)], pool: [(crate::map_def::PortKind::Any, 1)].into() };
    assert!(matches!(start_on(&def, 3).err(), Some(MapError::PortNotOnCoast { q: 1, r: 1, side: 0 })));
}

#[test]
fn the_built_in_maps_start_through_the_fallible_path_too() {
    for map in ["standard", "earth"] {
        let colors = PALETTE.iter().take(3).map(|c| c.to_string()).collect();
        let g = Game::try_with_options(names(3), colors, TimerMode::Normal, Settings::default(), map).unwrap();
        assert_eq!(g.board.map, map);
    }
}

#[test]
fn players_keep_their_chosen_colors() {
    let g = Game::with_colors(names(2), vec!["#000000".into(), "#FFFFFF".into()]);
    let v = g.view(P0, &all_connected(2), &g.longest_roads());
    assert_eq!(v.players[0].color, "#000000");
    assert_eq!(v.players[1].color, "#FFFFFF");
    // `Game::new` hands out palette colors in seat order
    assert_eq!(Game::new(names(3)).colors, PerPlayer::from(vec![PALETTE[0].to_string(), PALETTE[1].to_string(), PALETTE[2].to_string()]));
}

#[test]
fn palette_lookup_is_exact_but_ignores_case() {
    assert_eq!(palette_color("#7bafd4"), Some("#7BAFD4"));
    assert_eq!(palette_color("69BE28"), None); // the leading # is required
    assert_eq!(palette_color("#123456"), None);
    let mut all = PALETTE.to_vec();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), PALETTE.len(), "palette has no duplicates");
}
