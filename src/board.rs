//! Board geometry, built from a `MapDef` (see `map_def.rs`).
//!
//! `Board::from_def` deals the random parts of a map (terrain, number tokens, harbour kinds), builds the
//! hex/vertex/edge geometry and places the harbours. Anything wrong with the map comes back as a
//! `MapError`: generation never panics on a bad map and never loops forever. The two built-in maps
//! (Standard and Earth) are `MapDef`s too (see `builtin.rs`).
//!
//! Vertices and edges are derived from hex corner coordinates, so the same
//! numeric ids are used by the server rules and by the browser renderer.

use crate::map_def::*;
use crate::validate;
use rand::seq::SliceRandom;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Resource indices, used everywhere (hands, bank, costs, UI).
pub const BRICK: usize = 0;
pub const LUMBER: usize = 1;
pub const ORE: usize = 2;
pub const GRAIN: usize = 3;
pub const WOOL: usize = 4;

/// A bundle of resources, indexed [brick, lumber, ore, grain, wool].
pub type Res = [u8; 5];

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Terrain {
    Hills,
    Forest,
    Mountains,
    Fields,
    Pasture,
    Desert,
}

impl Terrain {
    /// The name used in map files and messages.
    pub fn name(self) -> &'static str {
        match self {
            Terrain::Forest => "forest",
            Terrain::Hills => "hills",
            Terrain::Mountains => "mountains",
            Terrain::Fields => "fields",
            Terrain::Pasture => "pasture",
            Terrain::Desert => "desert",
        }
    }
}

impl Terrain {
    pub fn resource(self) -> Option<usize> {
        match self {
            Terrain::Hills => Some(BRICK),
            Terrain::Forest => Some(LUMBER),
            Terrain::Mountains => Some(ORE),
            Terrain::Fields => Some(GRAIN),
            Terrain::Pasture => Some(WOOL),
            Terrain::Desert => None,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct Hex {
    pub q: i32,
    pub r: i32,
    pub x: f64,
    pub y: f64,
    pub terrain: Terrain,
    pub number: Option<u8>,
    #[serde(skip)]
    pub vertices: [usize; 6],
}

#[derive(Clone, Serialize)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
    #[serde(skip)]
    pub hexes: Vec<usize>,
    #[serde(skip)]
    pub neighbors: Vec<usize>,
    #[serde(skip)]
    pub edges: Vec<usize>,
}

#[derive(Clone, Serialize)]
pub struct Edge {
    pub a: usize,
    pub b: usize,
}

#[derive(Clone, Serialize)]
pub struct Port {
    pub a: usize,
    pub b: usize,
    /// `None` = 3:1 any resource, `Some(r)` = 2:1 for resource `r`.
    pub resource: Option<usize>,
}

#[derive(Clone, Serialize)]
pub struct Board {
    /// Which map this is a board of: its id in the map registry (`"standard"`, `"earth"`, ...).
    pub map: String,
    /// The map's drawing hints, passed through for the browser (`MapDef::style`). Nothing in the rules reads them.
    pub style: MapStyle,
    pub hexes: Vec<Hex>,
    pub vertices: Vec<Vertex>,
    pub edges: Vec<Edge>,
    pub ports: Vec<Port>,
    /// The hex the robber starts on: the map's `robber_start`, else a random desert.
    pub desert: usize,
}

impl Edge {
    pub fn other(&self, v: usize) -> usize {
        if self.a == v {
            self.b
        } else {
            self.a
        }
    }
}

fn key(x: f64, y: f64) -> (i64, i64) {
    ((x * 100.0).round() as i64, (y * 100.0).round() as i64)
}

pub(crate) fn hex_distance(a: (i32, i32), b: (i32, i32)) -> i32 {
    let dq = a.0 - b.0;
    let dr = a.1 - b.1;
    (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
}

// ----------------------------------------------------------------------
// Dealing the random parts of a map
// ----------------------------------------------------------------------

/// The dealt terrain and number token of every hex, in the order of the map's `hexes`.
struct Layout {
    terrains: Vec<Terrain>,
    numbers: Vec<Option<u8>>,
}

pub(crate) fn is_hot(n: Option<u8>) -> bool {
    matches!(n, Some(6) | Some(8))
}

/// Two tokens may not sit on touching hexes if they are the same number, or if both are a 6 or 8.
pub(crate) fn tokens_clash(a: u8, b: u8) -> bool {
    a == b || (is_hot(Some(a)) && is_hot(Some(b)))
}

/// Number tokens are 2-12 and never 7 (a 7 is the robber, not a harvest).
pub(crate) fn valid_token(n: u8) -> bool {
    (2..=12).contains(&n) && n != 7
}

/// Total of a pool, summed so that huge counts in a hostile file cannot overflow. Always check this
/// against the number of hexes *before* expanding a pool into a list.
pub(crate) fn pool_total<K>(pool: &std::collections::BTreeMap<K, u32>) -> u64 {
    pool.values().map(|&c| c as u64).sum()
}

/// Fresh deals `deal_tokens` tries before giving up.
///
/// The limits are measured, not guessed. Standard and Earth are dealt on the first try with under one
/// swap per token (at most 22 in 1000 runs), and a deliberately hard pool (4 different numbers across
/// 250 touching hexes) needed at most about 12 swaps per token. A map that blows through
/// `DEAL_RESTARTS * DEAL_SWAPS_PER_TOKEN` swaps per token almost certainly can't be dealt at all, and the
/// server is stuck waiting while it tries, so failing fast matters more than trying harder: at these
/// limits a hopeless 250-hex map is refused in about a tenth of a second (it took ~2s at 20 * 50).
const DEAL_RESTARTS: usize = 3;
/// Swaps tried per token on each deal before starting that deal over.
const DEAL_SWAPS_PER_TOKEN: usize = 25;

/// Deal `tokens` out at random, one to each of `cells` (indices into `coords`), so that no two touching
/// hexes clash (see `tokens_clash`) - counting the `fixed` tokens written into the map as well. (Fixed
/// tokens that clash with each other are the map's own fault; `validate::check_content` reports those.)
/// Returns a token per hex: the fixed ones, the dealt ones, and `None` for everything else.
/// With `spread` off the deal is simply shuffled.
///
/// Starts from a random deal and swaps tokens around the clashing hexes until none are left
/// (random reshuffling alone would almost never succeed on a board as big as Earth), starting
/// again from a fresh deal if a run gets stuck. It gives up after `DEAL_RESTARTS` deals: a map whose
/// pool can't be spread out is an error, not an endless loop.
fn deal_tokens<R: Rng>(
    rng: &mut R,
    coords: &[(i32, i32)],
    fixed: &[Option<u8>],
    cells: &[usize],
    tokens: &[u8],
    spread: bool,
) -> Result<Vec<Option<u8>>, MapError> {
    debug_assert_eq!(cells.len(), tokens.len());
    let m = cells.len();
    let mut numbers: Vec<Option<u8>> = fixed.to_vec();
    let mut place = |dealt: &[u8]| {
        for (&cell, &token) in cells.iter().zip(dealt) {
            numbers[cell] = Some(token);
        }
    };
    let mut dealt = tokens.to_vec();

    if !spread || m == 0 {
        dealt.shuffle(rng);
        place(&dealt);
        return Ok(numbers);
    }

    // touching[k]: which of `cells` (by position) touch cells[k]; pinned[k]: the fixed tokens around it.
    let touching: Vec<Vec<usize>> = (0..m)
        .map(|k| {
            (0..m)
                .filter(|&j| j != k && hex_distance(coords[cells[k]], coords[cells[j]]) == 1)
                .collect()
        })
        .collect();
    let pinned: Vec<Vec<u8>> = (0..m)
        .map(|k| {
            (0..coords.len())
                .filter(|&h| hex_distance(coords[cells[k]], coords[h]) == 1)
                .filter_map(|h| fixed[h])
                .collect()
        })
        .collect();
    // How many neighbours of position `k` clash with the token it currently holds.
    let clashes = |dealt: &[u8], k: usize| {
        touching[k].iter().filter(|&&j| tokens_clash(dealt[k], dealt[j])).count()
            + pinned[k].iter().filter(|&&f| tokens_clash(dealt[k], f)).count()
    };

    for _ in 0..DEAL_RESTARTS {
        dealt.shuffle(rng);
        for _ in 0..DEAL_SWAPS_PER_TOKEN * m {
            let bad: Vec<usize> = (0..m).filter(|&k| clashes(&dealt, k) > 0).collect();
            if bad.is_empty() {
                place(&dealt);
                return Ok(numbers);
            }
            let k = bad[rng.gen_range(0..bad.len())];
            // Swap with the hex that leaves the fewest clashes, or now and then a random one to
            // shake loose from a dead end.
            let mut best: Vec<usize> = vec![];
            let mut best_score = usize::MAX;
            for j in 0..m {
                if j == k || dealt[j] == dealt[k] {
                    continue; // swapping equal tokens changes nothing
                }
                dealt.swap(k, j);
                let score = clashes(&dealt, k) + clashes(&dealt, j);
                dealt.swap(k, j);
                if score < best_score {
                    best_score = score;
                    best.clear();
                }
                if score == best_score {
                    best.push(j);
                }
            }
            if best.is_empty() {
                break; // every token is the same number: nothing to swap
            }
            let j = if rng.gen_range(0..10) == 0 {
                rng.gen_range(0..m)
            } else {
                best[rng.gen_range(0..best.len())]
            };
            dealt.swap(k, j);
        }
    }
    Err(MapError::TokenPlacementFailed { attempts: DEAL_RESTARTS })
}

/// Where a desert from the terrain pool may go: a hex whose terrain is random and that has no number
/// written on it (and, if the map asks, that doesn't touch a desert written into the map).
pub(crate) fn desert_spots(def: &MapDef, coords: &[(i32, i32)]) -> Vec<usize> {
    let fixed_deserts: Vec<usize> = (0..def.hexes.len()).filter(|&i| def.hexes[i].terrain == Slot::Fixed(Terrain::Desert)).collect();
    (0..def.hexes.len())
        .filter(|&i| def.hexes[i].terrain.is_random() && !matches!(def.hexes[i].number, Slot::Fixed(_)))
        .filter(|&i| !def.constraints.deserts_avoid_fixed_deserts || fixed_deserts.iter().all(|&d| hex_distance(coords[i], coords[d]) > 1))
        .collect()
}

/// Deal terrain and number tokens to every hex of a map that has passed `validate::check_content`.
///
/// What can still go wrong is the luck of the deal (`TokenPlacementFailed`). The pool-size checks are
/// repeated here as a guard anyway, because a pool is expanded into a list and a huge count in a hostile
/// file would be a huge list.
fn deal_layout<R: Rng>(def: &MapDef, coords: &[(i32, i32)], rng: &mut R) -> Result<Layout, MapError> {
    let n = def.hexes.len();

    // --- terrain -----------------------------------------------------------
    let mut terrains: Vec<Option<Terrain>> = def
        .hexes
        .iter()
        .map(|h| match h.terrain {
            Slot::Fixed(t) => Some(t),
            Slot::Random => None,
        })
        .collect();
    let random_hexes: Vec<usize> = (0..n).filter(|&i| terrains[i].is_none()).collect();
    let pool = pool_total(&def.terrain_pool);
    if pool != random_hexes.len() as u64 {
        return Err(MapError::TerrainPoolMismatch { random_hexes: random_hexes.len(), pool });
    }
    // From here on the pool holds exactly `random_hexes.len()` (at most MAX_HEXES) terrains.

    // Deserts from the pool go first, only where a number isn't written in and (if asked) away from
    // the deserts written into the map; the rest of the pool fills what is left.
    let deserts_wanted = def.terrain_pool.get(&Terrain::Desert).copied().unwrap_or(0) as usize;
    let mut spots = desert_spots(def, coords);
    if spots.len() < deserts_wanted {
        return Err(MapError::NotEnoughDesertSpots { wanted: deserts_wanted, available: spots.len() });
    }
    spots.shuffle(rng);
    for &i in spots.iter().take(deserts_wanted) {
        terrains[i] = Some(Terrain::Desert);
    }
    let mut bag: Vec<Terrain> = def
        .terrain_pool
        .iter()
        .filter(|(&t, _)| t != Terrain::Desert)
        .flat_map(|(&t, &count)| std::iter::repeat(t).take(count as usize))
        .collect();
    bag.shuffle(rng);
    let mut bag = bag.into_iter();
    for &i in &random_hexes {
        if terrains[i].is_none() {
            terrains[i] = Some(bag.next().ok_or(MapError::Internal("terrain bag ran out"))?);
        }
    }
    let terrains: Vec<Terrain> = terrains
        .into_iter()
        .map(|t| t.ok_or(MapError::Internal("a hex was left without terrain")))
        .collect::<Result<_, _>>()?;

    // --- number tokens -------------------------------------------------------
    let mut fixed: Vec<Option<u8>> = vec![None; n];
    for (i, h) in def.hexes.iter().enumerate() {
        if let Slot::Fixed(v) = h.number {
            fixed[i] = Some(v);
        }
    }
    let cells: Vec<usize> = (0..n).filter(|&i| terrains[i] != Terrain::Desert && fixed[i].is_none()).collect();
    let pool = pool_total(&def.token_pool);
    if pool != cells.len() as u64 {
        return Err(MapError::TokenPoolMismatch { needed: cells.len(), pool });
    }
    let tokens: Vec<u8> = def
        .token_pool
        .iter()
        .flat_map(|(&v, &count)| std::iter::repeat(v).take(count as usize))
        .collect();
    let numbers = deal_tokens(rng, coords, &fixed, &cells, &tokens, def.constraints.spread_tokens)?;
    Ok(Layout { terrains, numbers })
}

/// Deal a map's terrain and tokens once and throw the result away. This is the part of building a board
/// that depends on luck, so `validate` runs it a few times to see whether the map can be dealt reliably.
/// Only for a map that has passed `validate::check_content`.
pub(crate) fn dry_deal<R: Rng>(def: &MapDef, rng: &mut R) -> Result<(), MapError> {
    let coords: Vec<(i32, i32)> = def.hexes.iter().map(|h| (h.q, h.r)).collect();
    deal_layout(def, &coords, rng).map(|_| ())
}

// ----------------------------------------------------------------------
// Geometry
// ----------------------------------------------------------------------

/// The six hexes that touch a hex, as axial steps, in the same order as the hex's sides: step `i` leads
/// across side `i` (0 east, 1 south-east, 2 south-west, 3 west, 4 north-west, 5 north-east).
pub(crate) const NEIGHBOR_STEPS: [(i32, i32); 6] = [(1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, -1)];

/// The corners (vertices) and sides (edges) of a set of hexes, numbered in order of first appearance.
/// It depends only on where the hexes are, not on their terrain or tokens, so `validate` can build it
/// without dealing anything.
pub(crate) struct Geometry {
    pub coords: Vec<(i32, i32)>,
    /// Index into `coords` of the hex at each position.
    pub at: HashMap<(i32, i32), usize>,
    /// Where each hex's centre is.
    pub centres: Vec<(f64, f64)>,
    /// Each hex's six corners, clockwise from the upper right.
    pub hex_vertices: Vec<[usize; 6]>,
    pub vertices: Vec<Vertex>,
    pub edges: Vec<Edge>,
    pub edge_ids: HashMap<(usize, usize), usize>,
    /// How many hexes border each edge: 1 = coast, 2 = inland.
    pub edge_hex_count: Vec<u8>,
}

impl Geometry {
    /// `coords` must be distinct and within `MAX_COORD` (see `validate::check_hexes`).
    pub fn build(coords: Vec<(i32, i32)>) -> Geometry {
        let at: HashMap<(i32, i32), usize> = coords.iter().enumerate().map(|(i, &c)| (c, i)).collect();
        let sqrt3 = 3f64.sqrt();
        let mut vertex_ids: HashMap<(i64, i64), usize> = HashMap::new();
        let mut vertices: Vec<Vertex> = Vec::new();
        let mut edge_ids: HashMap<(usize, usize), usize> = HashMap::new();
        let mut edges: Vec<Edge> = Vec::new();
        let mut edge_hex_count: Vec<u8> = Vec::new();
        let mut centres: Vec<(f64, f64)> = Vec::new();
        let mut hex_vertices: Vec<[usize; 6]> = Vec::new();

        for (hi, &(q, r)) in coords.iter().enumerate() {
            let cx = sqrt3 * (q as f64 + r as f64 / 2.0);
            let cy = 1.5 * r as f64;
            let mut corner_ids = [0usize; 6];
            for (i, slot) in corner_ids.iter_mut().enumerate() {
                let ang = (60.0 * i as f64 - 30.0).to_radians();
                let (x, y) = (cx + ang.cos(), cy + ang.sin());
                let id = *vertex_ids.entry(key(x, y)).or_insert_with(|| {
                    vertices.push(Vertex {
                        x,
                        y,
                        hexes: vec![],
                        neighbors: vec![],
                        edges: vec![],
                    });
                    vertices.len() - 1
                });
                vertices[id].hexes.push(hi);
                *slot = id;
            }
            for i in 0..6 {
                let (a, b) = (corner_ids[i], corner_ids[(i + 1) % 6]);
                let k = (a.min(b), a.max(b));
                let eid = *edge_ids.entry(k).or_insert_with(|| {
                    edges.push(Edge { a: k.0, b: k.1 });
                    edge_hex_count.push(0);
                    edges.len() - 1
                });
                edge_hex_count[eid] += 1;
            }
            centres.push((cx, cy));
            hex_vertices.push(corner_ids);
        }
        for (ei, e) in edges.iter().enumerate() {
            vertices[e.a].edges.push(ei);
            vertices[e.b].edges.push(ei);
            vertices[e.a].neighbors.push(e.b);
            vertices[e.b].neighbors.push(e.a);
        }
        Geometry { coords, at, centres, hex_vertices, vertices, edges, edge_ids, edge_hex_count }
    }
}

// ----------------------------------------------------------------------
// Harbours
// ----------------------------------------------------------------------

/// The closed loops of coast edges (edges with land on one side only), each as edge ids in order round
/// the loop. One loop per island, plus one per lake. Every coast vertex has exactly two coast edges,
/// so walking from an edge always comes back to where it started.
pub(crate) fn coast_rings(geo: &Geometry) -> Vec<Vec<usize>> {
    let mut seen = vec![false; geo.edges.len()];
    let mut rings = vec![];
    for start in 0..geo.edges.len() {
        if geo.edge_hex_count[start] != 1 || seen[start] {
            continue;
        }
        seen[start] = true;
        let mut ring = vec![start];
        let mut at = geo.edges[start].b;
        while let Some(next) = geo.vertices[at].edges.iter().copied().find(|&e| geo.edge_hex_count[e] == 1 && !seen[e]) {
            seen[next] = true;
            ring.push(next);
            at = geo.edges[next].other(at);
        }
        rings.push(ring);
    }
    rings
}

/// The coast edge a harbour spot names, or why it doesn't name one.
pub(crate) fn port_edge(geo: &Geometry, spot: &PortSpot) -> Result<usize, MapError> {
    let PortSpot { q, r, side, .. } = *spot;
    if side >= 6 {
        return Err(MapError::PortSideInvalid { q, r, side });
    }
    let &hi = geo.at.get(&(q, r)).ok_or(MapError::PortHexMissing { q, r })?;
    let corners = &geo.hex_vertices[hi];
    let (a, b) = (corners[side], corners[(side + 1) % 6]);
    let eid = *geo.edge_ids.get(&(a.min(b), a.max(b))).ok_or(MapError::Internal("a hex edge is missing"))?;
    if geo.edge_hex_count[eid] != 1 {
        return Err(MapError::PortNotOnCoast { q, r, side });
    }
    Ok(eid)
}

/// Give each requested harbour its kind: fixed ones as written, `"?"` ones from the pool.
/// Returns `Port::resource` values (`None` = 3:1 any).
fn assign_port_kinds<R: Rng>(
    requested: &[Slot<PortKind>],
    pool: &std::collections::BTreeMap<PortKind, u32>,
    rng: &mut R,
) -> Result<Vec<Option<usize>>, MapError> {
    let random_spots = requested.iter().filter(|s| s.is_random()).count();
    let total = pool_total(pool);
    if total != random_spots as u64 {
        return Err(MapError::PortPoolMismatch { random_spots, pool: total });
    }
    let mut bag: Vec<PortKind> = pool.iter().flat_map(|(&k, &c)| std::iter::repeat(k).take(c as usize)).collect();
    bag.shuffle(rng);
    requested
        .iter()
        .map(|slot| match slot {
            Slot::Fixed(kind) => Ok(kind.resource()),
            Slot::Random => bag.pop().map(PortKind::resource).ok_or(MapError::Internal("harbour pool ran out")),
        })
        .collect()
}

/// Put the harbours on the coast. The map has already passed `validate::check_content`, so the guards
/// here (`ok_or` and so on) are safety nets rather than the checks themselves.
fn place_ports<R: Rng>(def: &MapDef, geo: &Geometry, rng: &mut R) -> Result<Vec<Port>, MapError> {
    let (sites, kinds) = match &def.ports {
        PortsDef::Fixed { spots, pool } => {
            let sites = spots.iter().map(|s| port_edge(geo, s)).collect::<Result<Vec<usize>, MapError>>()?;
            let requested: Vec<Slot<PortKind>> = spots.iter().map(|s| s.kind).collect();
            (sites, assign_port_kinds(&requested, pool, rng)?)
        }
        PortsDef::Ring { spacing, pool } => {
            let mut ring = coast_rings(geo).into_iter().next().ok_or(MapError::Internal("the map has no coast"))?;
            let offset = rng.gen_range(0..ring.len());
            ring.rotate_left(offset);
            let mut sites = Vec::with_capacity(spacing.len());
            let mut pos = 0usize;
            for &gap in spacing {
                sites.push(*ring.get(pos).ok_or(MapError::Internal("harbour spacing runs past the coast"))?);
                pos = pos.saturating_add(gap);
            }
            let requested = vec![Slot::Random; sites.len()];
            let kinds = assign_port_kinds(&requested, pool, rng)?;
            (sites, kinds)
        }
    };
    Ok(sites.iter().zip(kinds).map(|(&e, resource)| Port { a: geo.edges[e].a, b: geo.edges[e].b, resource }).collect())
}

// ----------------------------------------------------------------------
// The board
// ----------------------------------------------------------------------

impl Board {
    /// One of the built-in maps. They are known to be valid (the tests build each many times), so this
    /// can't fail; anything that isn't built in goes through `from_def` and its `Result`.
    #[cfg(test)]
    pub fn generate<R: Rng>(map: &str, rng: &mut R) -> Board {
        let def = crate::builtin::builtin(map).unwrap_or_else(|| panic!("`{map}` is not a built-in map"));
        Board::from_def(def, map, rng).expect("the built-in maps are valid")
    }

    /// Deal `def`'s random parts and build its geometry. Any problem with the map is an `Err`: the first
    /// one `validate` would report (see `validate::check_hexes`, `check_rules` and `check_content`), or
    /// `TokenPlacementFailed` if the tokens can't be spread out.
    ///
    /// `map` is the id the board is labelled with (what the browser is told it is a board of).
    pub fn from_def<R: Rng>(def: &MapDef, map: &str, rng: &mut R) -> Result<Board, MapError> {
        if let Some(e) = validate::check_hexes(def).into_iter().chain(validate::check_rules(def)).next() {
            return Err(e);
        }
        let geo = Geometry::build(def.hexes.iter().map(|h| (h.q, h.r)).collect());
        if let Some(e) = validate::check_content(def, &geo).into_iter().next() {
            return Err(e);
        }

        let Layout { terrains, numbers } = deal_layout(def, &geo.coords, rng)?;
        let ports = place_ports(def, &geo, rng)?;
        let desert = match def.robber_start {
            Some(HexRef { q, r }) => *geo.at.get(&(q, r)).ok_or(MapError::RobberStartMissing { q, r })?,
            None => {
                let deserts: Vec<usize> = (0..terrains.len()).filter(|&i| terrains[i] == Terrain::Desert).collect();
                *deserts.choose(rng).ok_or(MapError::NoRobberStart)?
            }
        };
        let hexes = (0..geo.coords.len())
            .map(|i| Hex {
                q: geo.coords[i].0,
                r: geo.coords[i].1,
                x: geo.centres[i].0,
                y: geo.centres[i].1,
                terrain: terrains[i],
                number: numbers[i],
                vertices: geo.hex_vertices[i],
            })
            .collect();
        Ok(Board {
            map: map.to_string(),
            style: def.style,
            hexes,
            vertices: geo.vertices,
            edges: geo.edges,
            ports,
            desert,
        })
    }

    /// Land masses: groups of hexes joined by shared edges.
    #[cfg(test)]
    pub fn islands(&self) -> Vec<Vec<usize>> {
        let mut seen = vec![false; self.hexes.len()];
        let mut out = vec![];
        for s in 0..self.hexes.len() {
            if seen[s] {
                continue;
            }
            seen[s] = true;
            let mut stack = vec![s];
            let mut island = vec![];
            while let Some(h) = stack.pop() {
                island.push(h);
                for o in 0..self.hexes.len() {
                    if !seen[o] && hex_distance((self.hexes[h].q, self.hexes[h].r), (self.hexes[o].q, self.hexes[o].r)) == 1 {
                        seen[o] = true;
                        stack.push(o);
                    }
                }
            }
            out.push(island);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::flower;

    // What Earth is supposed to be, written down here and not read from `maps/builtin/earth.json`, so that
    // the tests check the file against the facts and not against itself.
    /// The Sahara: always desert.
    const EARTH_SAHARA: [(i32, i32); 3] = [(6, 5), (7, 5), (8, 5)];
    /// Earth has 76 producing hexes: how many of each terrain.
    const EARTH_TERRAINS: [(Terrain, usize); 5] =
        [(Terrain::Hills, 16), (Terrain::Forest, 16), (Terrain::Pasture, 16), (Terrain::Fields, 14), (Terrain::Mountains, 14)];
    /// How many tokens of each number Earth has (the standard mix scaled up to 76 tokens).
    const EARTH_TOKENS: [(u8, usize); 10] = [(2, 4), (3, 8), (4, 8), (5, 9), (6, 9), (8, 9), (9, 9), (10, 8), (11, 8), (12, 4)];
    use std::collections::BTreeMap;

    /// No two touching hexes share a number, and no 6 or 8 touches another 6 or 8.
    fn assert_tokens_spread(b: &Board) {
        for (i, a) in b.hexes.iter().enumerate() {
            for c in b.hexes.iter().skip(i + 1) {
                if hex_distance((a.q, a.r), (c.q, c.r)) != 1 {
                    continue;
                }
                if let (Some(x), Some(y)) = (a.number, c.number) {
                    assert_ne!(x, y, "hexes ({},{}) and ({},{}) touch and both have a {x}", a.q, a.r, c.q, c.r);
                    assert!(!(is_hot(a.number) && is_hot(c.number)), "a 6/8 touches a 6/8");
                }
            }
        }
    }

    #[test]
    fn board_has_standard_shape() {
        for _ in 0..50 {
            let b = Board::generate("standard", &mut rand::thread_rng());
            assert_eq!(b.map, "standard");
            assert!(!b.style.sea_grid, "Standard has no sea grid");
            assert_eq!(b.hexes.len(), 19);
            assert_eq!(b.vertices.len(), 54);
            assert_eq!(b.edges.len(), 72);
            assert_eq!(b.ports.len(), 9);
            assert_eq!(b.ports.iter().filter(|p| p.resource.is_none()).count(), 4);
            // every resource has exactly one 2:1 harbour
            for r in 0..5 {
                assert_eq!(b.ports.iter().filter(|p| p.resource == Some(r)).count(), 1);
            }
            // port vertices are distinct
            let mut pv: Vec<usize> = b.ports.iter().flat_map(|p| [p.a, p.b]).collect();
            pv.sort();
            pv.dedup();
            assert_eq!(pv.len(), 18);
            // token multiset
            let mut nums: Vec<u8> = b.hexes.iter().filter_map(|h| h.number).collect();
            nums.sort();
            assert_eq!(nums, vec![2, 3, 3, 4, 4, 5, 5, 6, 6, 8, 8, 9, 9, 10, 10, 11, 11, 12]);
            assert_tokens_spread(&b);
            assert_eq!(b.hexes[b.desert].terrain, Terrain::Desert);
        }
    }

    fn at(b: &Board, q: i32, r: i32) -> usize {
        b.hexes.iter().position(|h| (h.q, h.r) == (q, r)).expect("hex exists")
    }

    #[test]
    fn earth_has_the_promised_shape() {
        for _ in 0..100 {
            let b = Board::generate("earth", &mut rand::thread_rng());
            assert_eq!(b.map, "earth");
            assert!(b.style.sea_grid, "Earth asks for the sea grid in its own file");
            assert_eq!(b.hexes.len(), 81);
            assert_eq!(b.ports.len(), 27);

            // 7 islands of 1, 1, 2, 5, 6, 28 and 38 hexes
            let mut sizes: Vec<usize> = b.islands().iter().map(|i| i.len()).collect();
            sizes.sort();
            assert_eq!(sizes, vec![1, 1, 2, 5, 6, 28, 38]);
        }
    }

    #[test]
    fn earth_deserts() {
        for _ in 0..300 {
            let b = Board::generate("earth", &mut rand::thread_rng());
            let sahara: Vec<usize> = EARTH_SAHARA.iter().map(|&(q, r)| at(&b, q, r)).collect();
            for &s in &sahara {
                assert_eq!(b.hexes[s].terrain, Terrain::Desert);
                assert_eq!(b.hexes[s].number, None);
            }
            let others: Vec<usize> = (0..81)
                .filter(|&i| b.hexes[i].terrain == Terrain::Desert && !sahara.contains(&i))
                .collect();
            assert_eq!(others.len(), 2, "two more deserts");
            for &o in &others {
                for &s in &sahara {
                    let (a, c) = (&b.hexes[o], &b.hexes[s]);
                    assert!(hex_distance((a.q, a.r), (c.q, c.r)) > 1, "random desert touches the Sahara");
                }
            }
            // only deserts lack a token, and the robber starts on a desert
            assert!(b.hexes.iter().all(|h| (h.terrain == Terrain::Desert) == h.number.is_none()));
            assert_eq!(b.hexes[b.desert].terrain, Terrain::Desert);
        }
    }

    #[test]
    fn earth_terrain_and_tokens() {
        for _ in 0..100 {
            let b = Board::generate("earth", &mut rand::thread_rng());
            for (t, count) in EARTH_TERRAINS {
                assert_eq!(b.hexes.iter().filter(|h| h.terrain == t).count(), count);
            }
            let mut nums: Vec<u8> = b.hexes.iter().filter_map(|h| h.number).collect();
            assert_eq!(nums.len(), 76);
            nums.sort();
            let mut want: Vec<u8> = EARTH_TOKENS.iter().flat_map(|&(n, c)| std::iter::repeat(n).take(c)).collect();
            want.sort();
            assert_eq!(nums, want);
            assert_tokens_spread(&b);
        }
    }

    #[test]
    fn touching_hexes_never_share_a_number() {
        for _ in 0..2000 {
            assert_tokens_spread(&Board::generate("standard", &mut rand::thread_rng()));
        }
        for _ in 0..300 {
            assert_tokens_spread(&Board::generate("earth", &mut rand::thread_rng()));
        }
    }

    #[test]
    fn earth_harbours() {
        let mut layouts = vec![];
        for _ in 0..100 {
            let b = Board::generate("earth", &mut rand::thread_rng());
            assert_eq!(b.ports.iter().filter(|p| p.resource.is_none()).count(), 12);
            for r in 0..5 {
                assert_eq!(b.ports.iter().filter(|p| p.resource == Some(r)).count(), 3);
            }
            // every harbour is on a coast edge, and no edge holds two
            let mut count = vec![0; b.edges.len()];
            for h in &b.hexes {
                for i in 0..6 {
                    let (a, c) = (h.vertices[i], h.vertices[(i + 1) % 6]);
                    let e = b.edges.iter().position(|e| (e.a, e.b) == (a.min(c), a.max(c))).unwrap();
                    count[e] += 1;
                }
            }
            let mut used: Vec<usize> = b
                .ports
                .iter()
                .map(|p| {
                    let e = b.edges.iter().position(|e| (e.a, e.b) == (p.a.min(p.b), p.a.max(p.b))).unwrap();
                    assert_eq!(count[e], 1);
                    e
                })
                .collect();
            used.sort();
            used.dedup();
            assert_eq!(used.len(), 27);
            // the spots never move between games
            layouts.push(b.ports.iter().map(|p| (p.a, p.b)).collect::<Vec<_>>());
        }
        assert!(layouts.windows(2).all(|w| w[0] == w[1]));
    }

    // What USA is supposed to be, written down here and not read from `maps/builtin/usa.json`.
    /// USA has 143 producing hexes: how many of each terrain (plus one desert, dealt anywhere).
    const USA_TERRAINS: [(Terrain, usize); 5] =
        [(Terrain::Hills, 29), (Terrain::Forest, 29), (Terrain::Pasture, 29), (Terrain::Fields, 28), (Terrain::Mountains, 28)];
    /// How many tokens of each number USA has (the standard mix scaled up to 143 tokens).
    const USA_TOKENS: [(u8, usize); 10] = [(2, 8), (3, 16), (4, 16), (5, 15), (6, 16), (8, 16), (9, 16), (10, 16), (11, 16), (12, 8)];

    #[test]
    fn usa_has_the_promised_shape() {
        for _ in 0..50 {
            let b = Board::generate("usa", &mut rand::thread_rng());
            assert_eq!(b.map, "usa");
            assert!(!b.style.sea_grid, "USA does not ask for the sea grid");
            assert_eq!(b.hexes.len(), 144);
            assert_eq!(b.hexes.iter().filter(|h| h.terrain == Terrain::Desert).count(), 1, "exactly one desert");
            assert!(b.hexes.iter().all(|h| (h.terrain == Terrain::Desert) == h.number.is_none()));
            assert_eq!(b.hexes[b.desert].terrain, Terrain::Desert);
            // twelve rows, from the top of Washington to the tip of Florida
            let rows: std::collections::BTreeSet<i32> = b.hexes.iter().map(|h| h.r).collect();
            assert_eq!(rows.len(), 12);
        }
    }

    #[test]
    fn usa_terrain_and_tokens() {
        for _ in 0..50 {
            let b = Board::generate("usa", &mut rand::thread_rng());
            for (t, count) in USA_TERRAINS {
                assert_eq!(b.hexes.iter().filter(|h| h.terrain == t).count(), count);
            }
            let mut nums: Vec<u8> = b.hexes.iter().filter_map(|h| h.number).collect();
            assert_eq!(nums.len(), 143);
            nums.sort();
            let mut want: Vec<u8> = USA_TOKENS.iter().flat_map(|&(n, c)| std::iter::repeat(n).take(c)).collect();
            want.sort();
            assert_eq!(nums, want);
            assert_tokens_spread(&b);
        }
    }

    #[test]
    fn usa_desert_is_dealt_anywhere() {
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..200 {
            let b = Board::generate("usa", &mut rand::thread_rng());
            let d = &b.hexes[b.desert];
            seen.insert((d.q, d.r));
        }
        assert!(seen.len() > 20, "the desert should land in many places, not {}", seen.len());
    }

    #[test]
    fn usa_harbours() {
        let mut layouts = vec![];
        let mut kinds_vary = false;
        let mut first_kinds: Option<Vec<Option<usize>>> = None;
        for _ in 0..100 {
            let b = Board::generate("usa", &mut rand::thread_rng());
            assert_eq!(b.ports.len(), 25);
            assert_eq!(b.ports.iter().filter(|p| p.resource.is_none()).count(), 10);
            for r in 0..5 {
                assert_eq!(b.ports.iter().filter(|p| p.resource == Some(r)).count(), 3);
            }
            // every harbour is on a coast edge, and no edge holds two
            let mut count = vec![0; b.edges.len()];
            for h in &b.hexes {
                for i in 0..6 {
                    let (a, c) = (h.vertices[i], h.vertices[(i + 1) % 6]);
                    let e = b.edges.iter().position(|e| (e.a, e.b) == (a.min(c), a.max(c))).unwrap();
                    count[e] += 1;
                }
            }
            let mut used: Vec<usize> = b
                .ports
                .iter()
                .map(|p| {
                    let e = b.edges.iter().position(|e| (e.a, e.b) == (p.a.min(p.b), p.a.max(p.b))).unwrap();
                    assert_eq!(count[e], 1);
                    e
                })
                .collect();
            used.sort();
            used.dedup();
            assert_eq!(used.len(), 25);
            // the spots never move between games, the kinds are dealt anew
            layouts.push(b.ports.iter().map(|p| (p.a, p.b)).collect::<Vec<_>>());
            let kinds: Vec<Option<usize>> = b.ports.iter().map(|p| p.resource).collect();
            match &first_kinds {
                None => first_kinds = Some(kinds),
                Some(k) => kinds_vary |= *k != kinds,
            }
        }
        assert!(layouts.windows(2).all(|w| w[0] == w[1]));
        assert!(kinds_vary, "harbour kinds should be dealt differently from game to game");
    }

    // ------------------------------------------------------------------
    // The built-in maps still produce exactly the board they did before maps became data
    // ------------------------------------------------------------------

    /// FNV-1a over every hex (q, r, corner vertex ids) and every edge (a, b): a fingerprint of the whole
    /// geometry, including how vertices and edges are numbered.
    fn fingerprint(b: &Board) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut feed = |x: i64| {
            for byte in x.to_le_bytes() {
                h ^= byte as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        };
        for hx in &b.hexes {
            feed(hx.q as i64);
            feed(hx.r as i64);
            for v in hx.vertices {
                feed(v as i64);
            }
        }
        for e in &b.edges {
            feed(e.a as i64);
            feed(e.b as i64);
        }
        h
    }

    /// Recorded from the code as it was before `MapDef` existed.
    const GOLDEN_EARTH_PORTS: [(usize, usize); 27] = [
        (4, 5), (10, 19), (24, 25), (32, 37), (29, 52), (35, 66), (8, 44), (55, 83), (95, 101), (78, 115), (119, 138), (99, 100),
        (107, 108), (132, 133), (167, 168), (147, 171), (151, 173), (154, 155), (157, 158), (204, 226), (191, 196), (201, 211),
        (216, 238), (206, 207), (230, 231), (243, 256), (251, 252),
    ];

    #[test]
    fn earth_geometry_is_identical_to_before_maps_became_data() {
        for _ in 0..20 {
            let b = Board::generate("earth", &mut rand::thread_rng());
            assert_eq!((b.hexes.len(), b.vertices.len(), b.edges.len()), (81, 259, 333));
            assert_eq!(fingerprint(&b), 0x2f022a530e15220c);
            let ports: Vec<(usize, usize)> = b.ports.iter().map(|p| (p.a, p.b)).collect();
            assert_eq!(ports, GOLDEN_EARTH_PORTS);
        }
    }

    #[test]
    fn standard_geometry_is_identical_to_before_maps_became_data() {
        for _ in 0..20 {
            let b = Board::generate("standard", &mut rand::thread_rng());
            assert_eq!(fingerprint(&b), 0x990165024caf1fdf);
        }
    }

    #[test]
    fn sides_face_the_directions_the_docs_say() {
        // README and `PortSpot` promise: side 0 faces east, then clockwise (on screen, y pointing down):
        // 1 south-east, 2 south-west, 3 west, 4 north-west, 5 north-east; and `NEIGHBOR_STEPS[i]` is the
        // hex across side `i`. Check both against the real geometry, hex by hex.
        let facing = [(1.0, 0.0), (0.5, 1.0), (-0.5, 1.0), (-1.0, 0.0), (-0.5, -1.0), (0.5, -1.0)];
        for def in [crate::builtin::standard_def(), crate::builtin::earth_def(), flower(), crate::builtin::archipelago()] {
            let geo = Geometry::build(def.hexes.iter().map(|h| (h.q, h.r)).collect());
            for (hi, &(q, r)) in geo.coords.iter().enumerate() {
                let (cx, cy) = geo.centres[hi];
                for side in 0..6 {
                    let (a, b) = (geo.hex_vertices[hi][side], geo.hex_vertices[hi][(side + 1) % 6]);
                    // the middle of the side, relative to the hex's centre
                    let (mx, my) = ((geo.vertices[a].x + geo.vertices[b].x) / 2.0 - cx, (geo.vertices[a].y + geo.vertices[b].y) / 2.0 - cy);
                    // the same direction as the promised one: parallel (cross product 0) and pointing the same way
                    // (a hex is sqrt3 wide and 1.5 apart from the next row, so a step sideways is half of sqrt3)
                    let want = (facing[side].0 * 3f64.sqrt() / 2.0, facing[side].1 * 0.75);
                    let cross = mx * want.1 - my * want.0;
                    let dot = mx * want.0 + my * want.1;
                    assert!(cross.abs() < 1e-9 && dot > 0.0, "side {side} of ({q}, {r}) does not face the way the docs say");
                    // and the hex across it is the one `NEIGHBOR_STEPS` names: present exactly when the side is not coast
                    let (dq, dr) = NEIGHBOR_STEPS[side];
                    let across = geo.at.contains_key(&(q + dq, r + dr));
                    let eid = geo.edge_ids[&(a.min(b), a.max(b))];
                    assert_eq!(across, geo.edge_hex_count[eid] == 2, "side {side} of ({q}, {r})");
                }
            }
        }
    }

    #[test]
    fn standard_harbours_keep_their_spacing_round_the_coast() {
        for _ in 0..50 {
            let b = Board::generate("standard", &mut rand::thread_rng());
            let rings = coast_rings(&geometry_of(&b));
            assert_eq!(rings.len(), 1);
            assert_eq!(rings[0].len(), 30);
            // where each harbour sits along the coast, then the gaps between neighbours
            let mut at: Vec<usize> = b
                .ports
                .iter()
                .map(|p| rings[0].iter().position(|&e| (b.edges[e].a, b.edges[e].b) == (p.a.min(p.b), p.a.max(p.b))).unwrap())
                .collect();
            at.sort();
            let mut gaps: Vec<usize> = at.windows(2).map(|w| w[1] - w[0]).collect();
            gaps.push(30 - at[8] + at[0]);
            // the pattern 3,4,3,3,4,3,3,4,3 started from somewhere else round the loop
            let start = gaps.iter().position(|&g| g == 4).unwrap();
            gaps.rotate_left(start);
            assert_eq!(gaps, vec![4, 3, 3, 4, 3, 3, 4, 3, 3]);
        }
    }

    #[test]
    fn built_in_maps_survive_a_trip_through_json() {
        for id in ["standard", "earth"] {
            let def = crate::builtin::builtin(id).unwrap().clone();
            let json = serde_json::to_string(&def).unwrap();
            let back: MapDef = serde_json::from_str(&json).unwrap();
            assert_eq!(back, def);
            let b = Board::from_def(&back, id, &mut rand::thread_rng()).unwrap();
            assert_eq!(fingerprint(&b), fingerprint(&Board::generate(id, &mut rand::thread_rng())));
        }
    }

    // ------------------------------------------------------------------
    // Bad maps are errors, not panics or hangs
    // ------------------------------------------------------------------

    /// The geometry of a built board, rebuilt from where its hexes are (it numbers everything the same way).
    fn geometry_of(b: &Board) -> Geometry {
        Geometry::build(b.hexes.iter().map(|h| (h.q, h.r)).collect())
    }

    fn build(def: &MapDef) -> Result<Board, MapError> {
        Board::from_def(def, "standard", &mut rand::thread_rng())
    }

    fn err(def: &MapDef) -> MapError {
        build(def).err().expect("the map should be refused")
    }

    #[test]
    fn the_flower_is_a_valid_map() {
        let b = build(&flower()).unwrap();
        assert_eq!(b.hexes.len(), 7);
        assert_eq!(b.ports.len(), 0);
        assert_eq!(b.hexes[b.desert].terrain, Terrain::Desert);
    }

    #[test]
    fn size_and_shape_problems_are_reported() {
        let mut d = flower();
        d.version = 2;
        assert_eq!(err(&d), MapError::UnsupportedVersion { found: 2, supported: 1 });

        let mut d = flower();
        d.hexes.clear();
        assert_eq!(err(&d), MapError::NoHexes);

        let mut d = flower();
        d.hexes = (0..=MAX_HEXES as i32).map(|q| HexDef::at(q % 90, q / 90)).collect();
        assert_eq!(err(&d), MapError::TooManyHexes { count: MAX_HEXES + 1, max: MAX_HEXES });

        let mut d = flower();
        d.hexes[3].q = MAX_COORD + 1;
        assert_eq!(err(&d), MapError::CoordOutOfRange { q: MAX_COORD + 1, r: d.hexes[3].r });
        let mut d = flower();
        d.hexes[3].r = i32::MIN;
        assert!(matches!(err(&d), MapError::CoordOutOfRange { .. }), "extreme coordinates must not overflow");

        let mut d = flower();
        d.hexes[2] = d.hexes[1].clone();
        assert_eq!(err(&d), MapError::DuplicateHex { q: 1, r: 0 });

        for (min, max) in [(1, 4), (2, 9), (4, 2), (0, 0)] {
            let mut d = flower();
            d.players = PlayerRange { min, max };
            assert_eq!(err(&d), MapError::BadPlayerRange { min, max });
        }

        let mut d = flower();
        let mut s = crate::game::Settings::default();
        s.points_to_win = 1;
        d.recommended_settings = Some(s);
        assert!(matches!(err(&d), MapError::BadRecommendedSettings(_)));
    }

    #[test]
    fn terrain_pool_must_match_the_random_hexes() {
        let mut d = flower();
        d.terrain_pool.insert(Terrain::Forest, 3);
        assert_eq!(err(&d), MapError::TerrainPoolMismatch { random_hexes: 6, pool: 7 });
        let mut d = flower();
        d.terrain_pool.clear();
        assert_eq!(err(&d), MapError::TerrainPoolMismatch { random_hexes: 6, pool: 0 });
        // fixing a hex's terrain takes it out of the count
        let mut d = flower();
        d.hexes[1].terrain = Slot::Fixed(Terrain::Hills);
        d.terrain_pool.insert(Terrain::Hills, 1);
        assert!(build(&d).is_ok());
    }

    #[test]
    fn token_pool_must_match_and_hold_real_tokens() {
        let mut d = flower();
        d.token_pool.insert(3, 2);
        assert_eq!(err(&d), MapError::TokenPoolMismatch { needed: 6, pool: 7 });
        for bad in [0u8, 1, 7, 13, 255] {
            let mut d = flower();
            d.token_pool.remove(&3);
            d.token_pool.insert(bad, 1);
            assert_eq!(err(&d), MapError::InvalidToken { value: bad, at: None });
        }
        let mut d = flower();
        d.hexes[1].number = Slot::Fixed(7);
        assert_eq!(err(&d), MapError::InvalidToken { value: 7, at: Some((1, 0)) });
    }

    #[test]
    fn huge_pool_counts_are_refused_before_anything_is_allocated() {
        // u32::MAX of a terrain would be a 4 GB list if it were expanded; the total is checked first.
        let mut d = flower();
        d.terrain_pool.insert(Terrain::Forest, u32::MAX);
        d.terrain_pool.insert(Terrain::Hills, u32::MAX);
        assert!(matches!(err(&d), MapError::TerrainPoolMismatch { .. }));
        let mut d = flower();
        d.token_pool.insert(5, u32::MAX);
        assert!(matches!(err(&d), MapError::TokenPoolMismatch { .. }));
        let mut d = flower();
        d.ports = PortsDef::Fixed { spots: vec![PortSpot::at(1, 0, 0)], pool: BTreeMap::from([(PortKind::Any, u32::MAX), (PortKind::Ore, u32::MAX)]) };
        assert!(matches!(err(&d), MapError::PortPoolMismatch { .. }));
    }

    #[test]
    fn deserts_and_the_robber() {
        // a number on a written-in desert
        let mut d = flower();
        d.hexes[0].number = Slot::Fixed(6);
        assert_eq!(err(&d), MapError::NumberOnDesert { q: 0, r: 0 });

        // no desert anywhere: the robber needs somewhere to start
        let mut d = flower();
        d.hexes[0].terrain = Slot::Fixed(Terrain::Mountains);
        d.token_pool.insert(6, 1); // the old centre now produces, so it needs a token too
        assert_eq!(err(&d), MapError::NoRobberStart);
        d.robber_start = Some(HexRef { q: 5, r: 5 });
        assert_eq!(err(&d), MapError::RobberStartMissing { q: 5, r: 5 });
        d.robber_start = Some(HexRef { q: 1, r: 0 });
        let b = build(&d).unwrap();
        assert_eq!((b.hexes[b.desert].q, b.hexes[b.desert].r), (1, 0));
        assert!(b.hexes.iter().all(|h| h.terrain != Terrain::Desert));
    }

    #[test]
    fn deserts_from_the_pool_need_room() {
        // 2 deserts to deal, but a number is written on all but one of the random hexes
        let mut d = flower();
        d.terrain_pool = BTreeMap::from([(Terrain::Desert, 2), (Terrain::Forest, 4)]);
        for i in 2..7 {
            d.hexes[i].number = Slot::Fixed(3 + 6 * (i as u8 % 2)); // 9 and 3, alternating round the ring
        }
        d.token_pool = BTreeMap::from([(4, 1)]);
        assert!(matches!(err(&d), MapError::NotEnoughDesertSpots { wanted: 2, available: 1 }));
    }

    #[test]
    fn fixed_numbers_are_kept_and_the_dealt_ones_keep_clear_of_them() {
        for _ in 0..200 {
            let mut d = flower();
            // a 6 at the east hex; the tokens left to deal include an 8 that must not end up beside it
            d.hexes[1].number = Slot::Fixed(6);
            d.token_pool = BTreeMap::from([(8, 1), (3, 1), (4, 1), (5, 1), (9, 1)]);
            let b = build(&d).unwrap();
            assert_eq!(b.hexes[1].number, Some(6));
            assert_tokens_spread(&b);
            // the 8 may only be on a hex that doesn't touch the 6: the three hexes away from it
            let eight = b.hexes.iter().position(|h| h.number == Some(8)).unwrap();
            assert!(hex_distance((b.hexes[eight].q, b.hexes[eight].r), (1, 0)) > 1);
        }
    }

    #[test]
    fn fixed_numbers_that_cannot_touch_are_refused() {
        let mut d = flower();
        d.hexes[1].number = Slot::Fixed(6); // (1, 0)
        d.hexes[2].number = Slot::Fixed(8); // (1, -1) touches it
        d.token_pool = BTreeMap::from([(3, 1), (4, 1), (5, 1), (9, 1)]);
        assert!(matches!(err(&d), MapError::FixedTokensClash { .. }));
        // ...unless spreading is switched off
        d.constraints.spread_tokens = false;
        assert!(build(&d).is_ok());
    }

    #[test]
    fn spreading_can_be_switched_off() {
        let mut d = flower();
        d.token_pool = BTreeMap::from([(6, 6)]);
        assert!(matches!(err(&d), MapError::TokenPlacementFailed { .. }));
        d.constraints.spread_tokens = false;
        let b = build(&d).unwrap();
        assert!(b.hexes.iter().filter(|h| h.number == Some(6)).count() == 6);
    }

    #[test]
    fn an_impossible_token_pool_fails_quickly_instead_of_hanging() {
        // 18 tokens that are all 6s and 8s on the 19-hex board: some 6/8 must touch.
        let mut d = crate::builtin::standard_def();
        d.token_pool = BTreeMap::from([(6, 9), (8, 9)]);
        d.terrain_pool = BTreeMap::from([(Terrain::Forest, 18), (Terrain::Desert, 1)]);
        let t = std::time::Instant::now();
        assert!(matches!(err(&d), MapError::TokenPlacementFailed { .. }));
        assert!(t.elapsed().as_secs() < 10, "gave up too slowly: {:?}", t.elapsed());

        // every token the same number
        let mut d = crate::builtin::earth_def();
        d.token_pool = BTreeMap::from([(5, 76)]);
        let t = std::time::Instant::now();
        assert!(matches!(err(&d), MapError::TokenPlacementFailed { .. }));
        assert!(t.elapsed().as_secs() < 10, "gave up too slowly: {:?}", t.elapsed());

        // the biggest map allowed, with a pool that can't be spread: 2 numbers across ~250 touching hexes
        let mut d = flower();
        d.hexes.clear();
        for r in 0..16 {
            for q in 0..16 {
                d.hexes.push(HexDef::at(q - r / 2, r));
            }
        }
        assert_eq!(d.hexes.len(), 256);
        d.hexes.truncate(MAX_HEXES);
        d.terrain_pool = BTreeMap::from([(Terrain::Forest, MAX_HEXES as u32 - 1), (Terrain::Desert, 1)]);
        d.token_pool = BTreeMap::from([(5, 125), (9, MAX_HEXES as u32 - 126)]);
        let t = std::time::Instant::now();
        assert!(matches!(err(&d), MapError::TokenPlacementFailed { .. }));
        assert!(t.elapsed().as_secs() < 10, "gave up too slowly on a big map: {:?}", t.elapsed());
    }

    #[test]
    fn harbours_must_be_where_they_can_be() {
        let flower_with = |spots: Vec<PortSpot>, pool: &[(PortKind, u32)]| {
            let mut d = flower();
            d.ports = PortsDef::Fixed { spots, pool: pool.iter().copied().collect() };
            d
        };
        let any1 = [(PortKind::Any, 1)];

        // a good one: the east side of the east hex is coast
        assert_eq!(build(&flower_with(vec![PortSpot::at(1, 0, 0)], &any1)).unwrap().ports.len(), 1);
        // not part of the map
        assert_eq!(err(&flower_with(vec![PortSpot::at(9, 9, 0)], &any1)), MapError::PortHexMissing { q: 9, r: 9 });
        // sides run 0 to 5
        assert_eq!(err(&flower_with(vec![PortSpot::at(1, 0, 6)], &any1)), MapError::PortSideInvalid { q: 1, r: 0, side: 6 });
        assert_eq!(err(&flower_with(vec![PortSpot::at(1, 0, usize::MAX)], &any1)), MapError::PortSideInvalid { q: 1, r: 0, side: usize::MAX });
        // every side of the middle hex is shared with another hex, so none is coast
        assert_eq!(err(&flower_with(vec![PortSpot::at(0, 0, 2)], &any1)), MapError::PortNotOnCoast { q: 0, r: 0, side: 2 });
        // the same edge twice
        let twice = vec![PortSpot::at(1, 0, 0), PortSpot::at(1, 0, 0)];
        assert_eq!(err(&flower_with(twice, &[(PortKind::Any, 2)])), MapError::PortEdgeTaken { q: 1, r: 0, side: 0 });
        // pool too small / too large
        let two = vec![PortSpot::at(1, 0, 0), PortSpot::at(1, 0, 1)];
        assert_eq!(err(&flower_with(two.clone(), &any1)), MapError::PortPoolMismatch { random_spots: 2, pool: 1 });
        assert_eq!(err(&flower_with(two, &[(PortKind::Any, 3)])), MapError::PortPoolMismatch { random_spots: 2, pool: 3 });
    }

    #[test]
    fn a_harbour_with_a_written_in_kind_does_not_use_the_pool() {
        let mut d = flower();
        d.ports = PortsDef::Fixed {
            spots: vec![
                PortSpot { q: 1, r: 0, side: 0, kind: Slot::Fixed(PortKind::Ore) },
                PortSpot::at(1, 0, 1),
            ],
            pool: BTreeMap::from([(PortKind::Any, 1)]),
        };
        let b = build(&d).unwrap();
        assert_eq!(b.ports[0].resource, Some(ORE));
        assert_eq!(b.ports[1].resource, None);
    }

    #[test]
    fn ring_harbours_need_one_coast_with_matching_spacing() {
        let ring = |spacing: Vec<usize>, any: u32| {
            let mut d = flower();
            d.ports = PortsDef::Ring { spacing, pool: BTreeMap::from([(PortKind::Any, any)]) };
            d
        };
        // the flower's coast is 18 edges long
        let b = build(&ring(vec![6, 6, 6], 3)).unwrap();
        assert_eq!(b.ports.len(), 3);
        assert_eq!(err(&ring(vec![6, 6], 2)), MapError::BadPortSpacing { sum: 12, coast: 18 });
        assert_eq!(err(&ring(vec![0, 18], 2)), MapError::BadPortSpacing { sum: 18, coast: 18 });
        assert_eq!(err(&ring(vec![usize::MAX, usize::MAX], 2)), MapError::BadPortSpacing { sum: u64::MAX, coast: 18 });
        assert_eq!(err(&ring(vec![6, 6, 6], 2)), MapError::PortPoolMismatch { random_spots: 3, pool: 2 });

        // two islands have two coasts
        let mut d = flower();
        d.hexes = vec![
            HexDef { q: 0, r: 0, terrain: Slot::Fixed(Terrain::Forest), number: Slot::Fixed(5) },
            HexDef { q: 5, r: 0, terrain: Slot::Fixed(Terrain::Desert), number: Slot::Random },
        ];
        d.terrain_pool.clear();
        d.token_pool.clear();
        d.ports = PortsDef::Ring { spacing: vec![6, 6], pool: BTreeMap::from([(PortKind::Any, 2)]) };
        assert_eq!(err(&d), MapError::CoastNotOneRing { rings: 2 });
        // fixed harbours are fine on islands
        d.ports = PortsDef::Fixed { spots: vec![PortSpot::at(0, 0, 0), PortSpot::at(5, 0, 0)], pool: BTreeMap::from([(PortKind::Any, 2)]) };
        assert_eq!(build(&d).unwrap().ports.len(), 2);
    }

    #[test]
    fn coast_rings_finds_one_loop_per_island_and_per_lake() {
        let count_for = |d: &MapDef| {
            let b = build(d).unwrap();
            coast_rings(&geometry_of(&b)).iter().map(|r| r.len()).collect::<Vec<_>>()
        };
        assert_eq!(count_for(&flower()), vec![18]);
        // a ring of six with a lake in the middle: an outer coast of 18 and an inner one of 6
        let mut d = flower();
        d.hexes.remove(0);
        d.terrain_pool = BTreeMap::from([(Terrain::Forest, 6)]);
        d.token_pool = BTreeMap::from([(3, 1), (4, 1), (5, 1), (9, 1), (10, 1), (11, 1)]);
        d.robber_start = Some(HexRef { q: 1, r: 0 });
        let mut sizes = count_for(&d);
        sizes.sort();
        assert_eq!(sizes, vec![6, 18]);
    }
}
