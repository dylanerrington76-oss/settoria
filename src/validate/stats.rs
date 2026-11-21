//! Facts about a map: how many hexes, islands and harbours it has, and how many settlements fit on it.

use super::bipartite::max_independent_set_size;
use super::report::MapStats;
use crate::board::{Geometry, Terrain, NEIGHBOR_STEPS};
use crate::map_def::{MapDef, PortsDef, Slot};
use std::collections::BTreeMap;

/// The facts for a summary line. `room` is the figure `settlement_room` gave, or 0 if it gave none.
pub(super) fn gather(def: &MapDef, geo: &Geometry, room: usize) -> MapStats {
    let terrain = terrain_counts(def);
    let deserts = terrain.get(&Terrain::Desert).copied().unwrap_or(0);
    MapStats {
        hexes: def.hexes.len(),
        islands: islands(geo).len(),
        producing_hexes: def.hexes.len().saturating_sub(deserts),
        harbours: harbour_count(def),
        settlement_room: room,
        terrain,
    }
}

/// The most settlements that fit on the map under the distance rule (no two on corners joined by an edge):
/// the size of the largest set of corners, no two of them neighbours.
///
/// The corners of a hex map form a bipartite graph (a honeycomb has two kinds of corner, and every edge
/// joins one of each), which is what makes the answer exact and quick (see `bipartite`). `None` only if the
/// graph somehow isn't bipartite, which can't happen for hexes.
pub(super) fn settlement_room(geo: &Geometry) -> Option<usize> {
    let corners: Vec<Vec<usize>> = geo.vertices.iter().map(|v| v.neighbors.clone()).collect();
    max_independent_set_size(&corners)
}

/// Groups of hexes joined by shared edges, as lists of hex indices.
pub(super) fn islands(geo: &Geometry) -> Vec<Vec<usize>> {
    let mut seen = vec![false; geo.coords.len()];
    let mut out = vec![];
    for start in 0..geo.coords.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![start];
        let mut island = vec![];
        while let Some(h) = stack.pop() {
            island.push(h);
            for (dq, dr) in NEIGHBOR_STEPS {
                if let Some(&o) = geo.at.get(&(geo.coords[h].0 + dq, geo.coords[h].1 + dr)) {
                    if !seen[o] {
                        seen[o] = true;
                        stack.push(o);
                    }
                }
            }
        }
        out.push(island);
    }
    out
}

fn terrain_counts(def: &MapDef) -> BTreeMap<Terrain, usize> {
    let mut counts = BTreeMap::new();
    for h in &def.hexes {
        if let Slot::Fixed(t) = h.terrain {
            *counts.entry(t).or_insert(0) += 1;
        }
    }
    for (&t, &c) in &def.terrain_pool {
        *counts.entry(t).or_insert(0) += c as usize;
    }
    counts
}

fn harbour_count(def: &MapDef) -> usize {
    match &def.ports {
        PortsDef::Fixed { spots, .. } => spots.len(),
        PortsDef::Ring { spacing, .. } => spacing.len(),
    }
}
