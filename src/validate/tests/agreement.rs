//! The validator against the board generator, and against damaged files.

use super::*;
use crate::board::Board;
use rand::seq::SliceRandom;
use rand::Rng;

// ---------------- the validator and the generator agree ----------------

fn is_playability(e: &MapError) -> bool {
    matches!(e, MapError::NoProducingHexes | MapError::NotEnoughSettlementRoom { .. } | MapError::TokenPlacementFailed { .. } | MapError::Internal(_))
}

/// Make one random change to a map: some small, some wild.
fn mutate(def: &mut MapDef, rng: &mut StdRng) {
    use rand::Rng;
    let terrains = [Terrain::Hills, Terrain::Forest, Terrain::Mountains, Terrain::Fields, Terrain::Pasture, Terrain::Desert];
    let kinds = [PortKind::Any, PortKind::Brick, PortKind::Lumber, PortKind::Ore, PortKind::Grain, PortKind::Wool];
    let n = def.hexes.len();
    match rng.gen_range(0..16) {
        0 if n > 1 => {
            def.hexes.remove(rng.gen_range(0..n));
        }
        1 => def.hexes.push(HexDef::at(rng.gen_range(-3..12), rng.gen_range(-3..12))),
        2 if n > 0 => {
            let i = rng.gen_range(0..n);
            def.hexes[i].terrain = if rng.gen_bool(0.3) { Slot::Random } else { Slot::Fixed(*terrains.choose(rng).unwrap()) };
        }
        3 if n > 0 => {
            let i = rng.gen_range(0..n);
            def.hexes[i].number = match rng.gen_range(0..4) {
                0 => Slot::Random,
                _ => Slot::Fixed(rng.gen_range(0..14)),
            };
        }
        4 => {
            let t = *terrains.choose(rng).unwrap();
            def.terrain_pool.insert(t, rng.gen_range(0..6));
        }
        5 => {
            def.token_pool.insert(rng.gen_range(0..14), rng.gen_range(0..4));
        }
        6 => {
            let k = *kinds.choose(rng).unwrap();
            match &mut def.ports {
                PortsDef::Fixed { pool, .. } | PortsDef::Ring { pool, .. } => {
                    pool.insert(k, rng.gen_range(0..4));
                }
            }
        }
        7 => {
            if let PortsDef::Fixed { spots, .. } = &mut def.ports {
                let hex = def.hexes.choose(rng).map(|h| (h.q, h.r)).unwrap_or((0, 0));
                spots.push(PortSpot::at(hex.0, hex.1, rng.gen_range(0..7)));
            }
        }
        8 => {
            if let PortsDef::Fixed { spots, .. } = &mut def.ports {
                if !spots.is_empty() {
                    let i = rng.gen_range(0..spots.len());
                    spots.remove(i);
                }
            }
        }
        9 => {
            if let PortsDef::Ring { spacing, .. } = &mut def.ports {
                if !spacing.is_empty() {
                    let i = rng.gen_range(0..spacing.len());
                    spacing[i] = rng.gen_range(0..6);
                }
            }
        }
        10 => {
            def.robber_start = if rng.gen_bool(0.3) { None } else { Some(HexRef { q: rng.gen_range(-2..10), r: rng.gen_range(-2..10) }) };
        }
        11 => def.constraints.spread_tokens = !def.constraints.spread_tokens,
        12 => def.constraints.deserts_avoid_fixed_deserts = !def.constraints.deserts_avoid_fixed_deserts,
        13 => def.players = PlayerRange { min: rng.gen_range(0..6), max: rng.gen_range(0..6) },
        14 if rng.gen_bool(0.2) => def.version = rng.gen_range(0..3),
        _ => {
            // swap two hexes' written-in contents
            if n > 1 {
                let (a, b) = (rng.gen_range(0..n), rng.gen_range(0..n));
                let (ta, na) = (def.hexes[a].terrain, def.hexes[a].number);
                def.hexes[a].terrain = def.hexes[b].terrain;
                def.hexes[a].number = def.hexes[b].number;
                def.hexes[b].terrain = ta;
                def.hexes[b].number = na;
            }
        }
    }
}

#[test]
fn the_validator_and_the_generator_never_disagree_about_a_map() {
    // Many randomly damaged maps. If the generator refuses one for a structural reason, the validator
    // must list that very problem first; if the validator finds no structural problem, the generator
    // must not refuse the map for one (bad luck with the tokens is the only thing it may fail on).
    let rounds = if cfg!(debug_assertions) { 300 } else { 4000 };
    let mut rng = StdRng::seed_from_u64(2024);
    let starts = [flower(), archipelago(), standard_def(), earth_def()];
    let (mut refused, mut accepted) = (0, 0);
    for round in 0..rounds {
        let mut def = starts[round % starts.len()].clone();
        for _ in 0..rng.gen_range(1..4) {
            mutate(&mut def, &mut rng);
        }
        let report = validate_with_rng(&def, &Settings::default(), &mut StdRng::seed_from_u64(round as u64));
        let structural: Vec<&MapError> = report.errors.iter().filter(|e| !is_playability(e)).collect();
        let built = Board::from_def(&def, "standard", &mut rng);
        let context = || format!("round {round}: {}", serde_json::to_string(&def).unwrap());
        match built {
            Err(MapError::TokenPlacementFailed { .. }) | Ok(_) => {
                assert!(structural.is_empty(), "the generator accepted a map the validator calls broken: {structural:?}\n{}", context());
                accepted += 1;
            }
            Err(e) => {
                assert_eq!(structural.first().copied(), Some(&e), "the validator disagrees about the first problem\n{}", context());
                refused += 1;
            }
        }
        // and a map the validator passes completely must be one the generator can build
        if report.errors.is_empty() {
            let mut ok = false;
            for _ in 0..30 {
                if Board::from_def(&def, "standard", &mut rng).is_ok() {
                    ok = true;
                    break;
                }
            }
            assert!(ok, "a map the validator passes could not be built in 30 tries\n{}", context());
        }
    }
    assert!(refused > rounds / 10 && accepted > rounds / 10, "the test didn't exercise both sides: {refused} refused, {accepted} accepted");
}

#[test]
fn damaged_json_never_panics() {
    use rand::Rng;
    let original = include_str!("../../../examples/maps/archipelago.json");
    let chars: Vec<char> = original.chars().collect();
    let junk: Vec<char> = "{}[]\",:0123456789-?abcxyz \n".chars().collect();
    let mut rng = StdRng::seed_from_u64(99);
    let rounds = if cfg!(debug_assertions) { 500 } else { 5000 };
    let (mut parsed, mut rejected) = (0, 0);
    for _ in 0..rounds {
        let mut text = chars.clone();
        for _ in 0..rng.gen_range(1..6) {
            let at = rng.gen_range(0..text.len().max(1));
            match rng.gen_range(0..4) {
                0 if !text.is_empty() => {
                    text.remove(at.min(text.len() - 1));
                }
                1 => text.insert(at.min(text.len()), *junk.choose(&mut rng).unwrap()),
                2 if !text.is_empty() => {
                    let i = at.min(text.len() - 1);
                    text[i] = *junk.choose(&mut rng).unwrap();
                }
                _ => text.truncate(at),
            }
        }
        let text: String = text.into_iter().collect();
        match MapDef::from_json(&text) {
            Ok(def) => {
                parsed += 1;
                let _ = validate_with_rng(&def, &Settings::default(), &mut rng);
            }
            Err(message) => {
                rejected += 1;
                assert!(!message.is_empty());
            }
        }
    }
    assert!(parsed > 0 && rejected > 0, "{parsed} parsed, {rejected} rejected");
}
