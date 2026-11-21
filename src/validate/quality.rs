//! Layer 4, the advice: warnings about a map that can be played, but could be better.
//!
//! Only asked of a map that is playable, and in this order: settlement room, resources, islands of nothing
//! but desert, harbours.

use super::report::{MapStats, MapWarning};
use super::stats::islands;
use crate::board::{coast_rings, port_edge, Geometry, Terrain};
use crate::map_def::{MapDef, PortsDef, Slot};
use std::collections::HashMap;

/// If the map has room for at least this many times the settlements setup needs, setup can finish
/// however carelessly people place, and `validate` says nothing about it.
///
/// Measured, not guessed. The worst any order of legal placements can do (settle so that as few corners
/// as possible get used up, then run out of room) is computed exactly in the
/// `worst_case_placement_still_fits_when_there_is_slack` test, for a dozen map shapes: it is never below
/// half the best possible packing, and exactly half on the flower and on a ring of hexes round a lake.
/// So with 2x the needed room even the worst placements leave enough. (Random placement does much better:
/// about 80% of the best packing on average, and at least half in every shape tried.)
pub(super) const ROOM_SLACK: usize = 2;

/// Everything worth saying about a playable map. `needed` is how many settlements setup has to place.
pub(super) fn quality_warnings(def: &MapDef, geo: &Geometry, stats: &MapStats, needed: usize) -> Vec<MapWarning> {
    let mut warnings = vec![];
    warnings.extend(tight_settlement_room(stats, needed));
    warnings.extend(resource_warnings(stats));
    warnings.extend(desert_island_warnings(def, geo));
    warnings.extend(harbour_warnings(def, geo, stats));
    warnings
}

fn tight_settlement_room(stats: &MapStats, needed: usize) -> Option<MapWarning> {
    let fits = stats.settlement_room >= needed;
    (fits && stats.settlement_room < needed * ROOM_SLACK)
        .then_some(MapWarning::TightSettlementRoom { needed, available: stats.settlement_room })
}

/// Resources: how many hexes of each of the five producing terrains.
fn resource_warnings(stats: &MapStats) -> Vec<MapWarning> {
    let mut warnings = vec![];
    let resources = [Terrain::Forest, Terrain::Hills, Terrain::Mountains, Terrain::Fields, Terrain::Pasture];
    let count = |t: Terrain| stats.terrain.get(&t).copied().unwrap_or(0);
    let missing: Vec<Terrain> = resources.iter().copied().filter(|&t| count(t) == 0).collect();
    for &terrain in &missing {
        warnings.push(MapWarning::MissingResource { terrain });
    }
    if missing.is_empty() {
        let scarce = *resources.iter().min_by_key(|&&t| count(t)).expect("five resources");
        let plentiful = *resources.iter().max_by_key(|&&t| count(t)).expect("five resources");
        if count(scarce) * 3 < count(plentiful) {
            warnings.push(MapWarning::LopsidedResources {
                scarce,
                scarce_hexes: count(scarce),
                plentiful,
                plentiful_hexes: count(plentiful),
            });
        }
    }
    warnings
}

/// An island that is certainly all desert (only terrain written into the map is certain).
fn desert_island_warnings(def: &MapDef, geo: &Geometry) -> Vec<MapWarning> {
    islands(geo)
        .into_iter()
        .filter(|island| island.iter().all(|&i| def.hexes[i].terrain == Slot::Fixed(Terrain::Desert)))
        .map(|island| MapWarning::IslandWithoutResources { near: geo.coords[island[0]], hexes: island.len() })
        .collect()
}

fn harbour_warnings(def: &MapDef, geo: &Geometry, stats: &MapStats) -> Vec<MapWarning> {
    let mut warnings = vec![];
    if stats.harbours == 0 {
        warnings.push(MapWarning::NoHarbours);
    }
    let mut at_corner: HashMap<usize, usize> = HashMap::new();
    for e in harbour_edges(def, geo) {
        for v in [geo.edges[e].a, geo.edges[e].b] {
            *at_corner.entry(v).or_insert(0) += 1;
        }
    }
    let mut shared: Vec<usize> = at_corner.into_iter().filter(|&(_, c)| c >= 2).map(|(v, _)| v).collect();
    shared.sort();
    for v in shared {
        warnings.push(MapWarning::HarboursShareCorner { near: geo.coords[geo.vertices[v].hexes[0]] });
    }
    warnings
}

/// The coast edge of each harbour. For a ring the starting point is random in a real game, but which
/// harbours meet at a corner doesn't depend on it, so start at 0.
fn harbour_edges(def: &MapDef, geo: &Geometry) -> Vec<usize> {
    match &def.ports {
        PortsDef::Fixed { spots, .. } => spots.iter().filter_map(|s| port_edge(geo, s).ok()).collect(),
        PortsDef::Ring { spacing, .. } => {
            let Some(ring) = coast_rings(geo).into_iter().next() else { return vec![] };
            let mut pos = 0usize;
            let mut edges = vec![];
            for &gap in spacing {
                if let Some(&e) = ring.get(pos) {
                    edges.push(e);
                }
                pos = pos.saturating_add(gap);
            }
            edges
        }
    }
}
