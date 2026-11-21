//! Layer 3: the terrain, number tokens, harbours and robber start, against the hexes of a usable hex list.
//!
//! These checks need the map's geometry (which side of which hex is coast), built by `Geometry::build`.
//! The sections run in a fixed order, and so do the errors they report.

use crate::board::{coast_rings, desert_spots, hex_distance, is_hot, pool_total, port_edge, tokens_clash, valid_token, Geometry, Terrain};
use crate::map_def::{HexRef, MapDef, MapError, PortsDef, Slot};
use std::collections::HashSet;

/// Everything that is wrong with the terrain, tokens, harbours and robber start, given a usable hex list.
/// Independent problems are all reported; one problem is only reported once, not again for each thing
/// that follows from it (a terrain pool of the wrong size doesn't also get its tokens counted, because
/// how many tokens are needed depends on how many deserts get dealt).
pub(crate) fn check_content(def: &MapDef, geo: &Geometry) -> Vec<MapError> {
    let mut errors = vec![];
    let terrain_pool_fits = check_terrain(def, geo, &mut errors);
    check_tokens(def, geo, terrain_pool_fits, &mut errors);
    check_harbours(def, geo, &mut errors);
    check_robber(def, geo, &mut errors);
    errors
}

fn is_fixed_desert(def: &MapDef, hex: usize) -> bool {
    def.hexes[hex].terrain == Slot::Fixed(Terrain::Desert)
}

/// How many deserts the terrain pool deals.
fn deserts_wanted(def: &MapDef) -> usize {
    def.terrain_pool.get(&Terrain::Desert).copied().unwrap_or(0) as usize
}

// --- terrain ---------------------------------------------------------------

/// The terrain pool against the hexes that take a random terrain, and room for the deserts it deals.
/// Says whether the pool is the right size, which the token count depends on.
fn check_terrain(def: &MapDef, geo: &Geometry, errors: &mut Vec<MapError>) -> bool {
    let random_terrain = def.hexes.iter().filter(|h| h.terrain.is_random()).count();
    let terrain_total = pool_total(&def.terrain_pool);
    let terrain_pool_fits = terrain_total == random_terrain as u64;
    if !terrain_pool_fits {
        errors.push(MapError::TerrainPoolMismatch { random_hexes: random_terrain, pool: terrain_total });
    } else {
        let wanted = deserts_wanted(def);
        let available = desert_spots(def, &geo.coords).len();
        if available < wanted {
            errors.push(MapError::NotEnoughDesertSpots { wanted, available });
        }
    }
    terrain_pool_fits
}

// --- number tokens -----------------------------------------------------------

fn check_tokens(def: &MapDef, geo: &Geometry, terrain_pool_fits: bool, errors: &mut Vec<MapError>) {
    check_written_numbers(def, geo, errors);
    check_token_pool(def, terrain_pool_fits, errors);
    if def.constraints.spread_tokens {
        check_token_spread(def, geo, errors);
    }
}

/// The numbers written into the map must be real tokens, and not on a desert.
fn check_written_numbers(def: &MapDef, geo: &Geometry, errors: &mut Vec<MapError>) {
    for (i, h) in def.hexes.iter().enumerate() {
        if let Slot::Fixed(v) = h.number {
            if !valid_token(v) {
                errors.push(MapError::InvalidToken { value: v, at: Some(geo.coords[i]) });
            } else if is_fixed_desert(def, i) {
                errors.push(MapError::NumberOnDesert { q: h.q, r: h.r });
            }
        }
    }
}

/// The token pool must hold real tokens, and (once the terrain pool is known to fit) as many as are needed.
fn check_token_pool(def: &MapDef, terrain_pool_fits: bool, errors: &mut Vec<MapError>) {
    if terrain_pool_fits {
        // Producing hexes still waiting for a token: those with a fixed terrain, plus the random-terrain
        // ones less the ones that turn out to be deserts (deserts only go where no number is written).
        let waiting_fixed =
            def.hexes.iter().filter(|h| h.number.is_random() && matches!(h.terrain, Slot::Fixed(t) if t != Terrain::Desert)).count();
        let waiting_random = def.hexes.iter().filter(|h| h.number.is_random() && h.terrain.is_random()).count();
        let needed = waiting_fixed + waiting_random - deserts_wanted(def).min(waiting_random);
        let pool = pool_total(&def.token_pool);
        if pool != needed as u64 {
            errors.push(MapError::TokenPoolMismatch { needed, pool });
        }
    }
    for &v in def.token_pool.keys() {
        if !valid_token(v) {
            errors.push(MapError::InvalidToken { value: v, at: None });
        }
    }
}

/// With spreading on, tokens that may not touch must not be written in next to each other.
fn check_token_spread(def: &MapDef, geo: &Geometry, errors: &mut Vec<MapError>) {
    let n = def.hexes.len();
    let fixed: Vec<Option<u8>> = (0..n)
        .map(|i| match def.hexes[i].number {
            Slot::Fixed(v) if valid_token(v) && !is_fixed_desert(def, i) => Some(v),
            _ => None,
        })
        .collect();
    check_written_tokens_clash(geo, &fixed, errors);
    check_volcanoes_touching_hot(def, geo, &fixed, errors);
}

fn check_written_tokens_clash(geo: &Geometry, fixed: &[Option<u8>], errors: &mut Vec<MapError>) {
    let coords = &geo.coords;
    for i in 0..fixed.len() {
        for j in i + 1..fixed.len() {
            if let (Some(a), Some(b)) = (fixed[i], fixed[j]) {
                if hex_distance(coords[i], coords[j]) == 1 && tokens_clash(a, b) {
                    errors.push(MapError::FixedTokensClash { a: coords[i], b: coords[j] });
                }
            }
        }
    }
}

/// A volcano written into the map is always a 6 or 8, whatever number it is given, so it clashes with
/// any 6 or 8 it touches. (Two fixed 6/8s that touch are already reported by `check_written_tokens_clash`.)
fn check_volcanoes_touching_hot(def: &MapDef, geo: &Geometry, fixed: &[Option<u8>], errors: &mut Vec<MapError>) {
    let (hexes, coords) = (&def.hexes, &geo.coords);
    let is_volcano = |i: usize| hexes[i].terrain == Slot::Fixed(Terrain::Volcano);
    let written_hot = |i: usize| is_hot(fixed[i]);
    let will_be_hot = |i: usize| is_volcano(i) || written_hot(i);
    for i in 0..hexes.len() {
        for j in i + 1..hexes.len() {
            if hex_distance(coords[i], coords[j]) != 1 || !will_be_hot(i) || !will_be_hot(j) || (written_hot(i) && written_hot(j)) {
                continue;
            }
            // The one of the two whose own number is not already a 6 or 8 is the one that has to change.
            let culprit = if written_hot(i) { j } else { i };
            let e = MapError::VolcanoTouchesHot { q: hexes[culprit].q, r: hexes[culprit].r };
            if !errors.contains(&e) {
                errors.push(e);
            }
        }
    }
}

// --- harbours ------------------------------------------------------------------

fn check_harbours(def: &MapDef, geo: &Geometry, errors: &mut Vec<MapError>) {
    match &def.ports {
        PortsDef::Fixed { spots, pool } => {
            let mut used = HashSet::new();
            for s in spots {
                match port_edge(geo, s) {
                    Ok(edge) => {
                        if !used.insert(edge) {
                            errors.push(MapError::PortEdgeTaken { q: s.q, r: s.r, side: s.side });
                        }
                    }
                    Err(e) => errors.push(e),
                }
            }
            let random_spots = spots.iter().filter(|s| s.kind.is_random()).count();
            let total = pool_total(pool);
            if total != random_spots as u64 {
                errors.push(MapError::PortPoolMismatch { random_spots, pool: total });
            }
        }
        PortsDef::Ring { spacing, pool } => {
            let rings = coast_rings(geo);
            if rings.len() != 1 {
                errors.push(MapError::CoastNotOneRing { rings: rings.len() });
            } else {
                // saturating: a hostile file can't wrap the sum round to the coast length
                let sum = spacing.iter().fold(0u64, |total, &s| total.saturating_add(s as u64));
                if spacing.contains(&0) || sum != rings[0].len() as u64 {
                    errors.push(MapError::BadPortSpacing { sum, coast: rings[0].len() });
                }
            }
            let total = pool_total(pool);
            if total != spacing.len() as u64 {
                errors.push(MapError::PortPoolMismatch { random_spots: spacing.len(), pool: total });
            }
        }
    }
}

// --- the robber --------------------------------------------------------------------

fn check_robber(def: &MapDef, geo: &Geometry, errors: &mut Vec<MapError>) {
    match def.robber_start {
        Some(HexRef { q, r }) => {
            if !geo.at.contains_key(&(q, r)) {
                errors.push(MapError::RobberStartMissing { q, r });
            }
        }
        None => {
            let any_desert = (0..def.hexes.len()).any(|i| is_fixed_desert(def, i)) || deserts_wanted(def) > 0;
            if !any_desert {
                errors.push(MapError::NoRobberStart);
            }
        }
    }
}
