//! The settlement-room figure, the matching it comes from, and the islands.

use super::*;
use crate::board::{Board, Geometry};
use crate::validate::bipartite::max_independent_set_size;
use crate::validate::quality::ROOM_SLACK;
use crate::validate::stats::{islands, settlement_room};
use rand::seq::SliceRandom;

// ---------------- the settlement-room figure ----------------

/// The most corners, no two neighbours, found the slow obvious way.
fn brute_force_room(adj: &[u64], available: u64) -> usize {
    if available == 0 {
        return 0;
    }
    let v = available.trailing_zeros() as usize;
    let without = brute_force_room(adj, available & !(1 << v));
    let with = 1 + brute_force_room(adj, available & !(adj[v] | (1 << v)));
    without.max(with)
}

/// The fewest settlements after which nothing more fits (a minimum independent dominating set):
/// the worst any order of legal placements can do.
fn worst_case_fill(adj: &[u64], undominated: u64, best: &mut usize, size: usize) {
    if size >= *best {
        return;
    }
    if undominated == 0 {
        *best = size;
        return;
    }
    // the lowest corner still free must end up covered: by itself or by a neighbour being settled
    let v = undominated.trailing_zeros() as usize;
    let mut candidates = adj[v] | (1 << v);
    while candidates != 0 {
        let u = candidates.trailing_zeros() as usize;
        candidates &= candidates - 1;
        if undominated & (1 << u) != 0 {
            worst_case_fill(adj, undominated & !(adj[u] | (1 << u)), best, size + 1);
        }
    }
}

fn adjacency(geo: &Geometry) -> Vec<u64> {
    (0..geo.vertices.len()).map(|v| geo.vertices[v].neighbors.iter().fold(0u64, |m, &w| m | (1 << w))).collect()
}

fn small_shapes() -> Vec<(&'static str, Vec<(i32, i32)>)> {
    vec![
        ("single hex", vec![(0, 0)]),
        ("two hexes", vec![(0, 0), (1, 0)]),
        ("triangle", vec![(0, 0), (1, 0), (0, 1)]),
        ("flower", coords_of(&flower())),
        ("ring round a lake", vec![(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)]),
        ("comb", vec![(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (0, 1), (2, 1), (4, 1)]),
        ("strip of 6", (0..6).map(|q| (q, 0)).collect()),
        ("two islands", vec![(0, 0), (1, 0), (5, 0), (5, 1)]),
        ("archipelago", coords_of(&archipelago())),
        ("block 3x3", (0..3).flat_map(|r| (0..3).map(move |q| (q - r / 2, r))).collect()),
    ]
}

#[test]
fn settlement_room_is_exact() {
    for (name, coords) in small_shapes() {
        let geo = Geometry::build(coords);
        let adj = adjacency(&geo);
        assert!(adj.len() <= 64, "{name} is too big to check this way");
        let all = if adj.len() == 64 { u64::MAX } else { (1u64 << adj.len()) - 1 };
        assert_eq!(settlement_room(&geo), Some(brute_force_room(&adj, all)), "{name}");
    }
    // a single hex: corners 0, 2, 4 (or 1, 3, 5)
    assert_eq!(settlement_room(&Geometry::build(vec![(0, 0)])), Some(3));
}

#[test]
fn worst_case_placement_still_fits_when_there_is_slack() {
    // The claim behind ROOM_SLACK: however badly people place, they can always fit at least half of
    // what the best packing fits. Computed exactly (not sampled) for every small shape.
    for (name, coords) in small_shapes() {
        let geo = Geometry::build(coords);
        let adj = adjacency(&geo);
        let all = if adj.len() == 64 { u64::MAX } else { (1u64 << adj.len()) - 1 };
        let mut worst = usize::MAX;
        worst_case_fill(&adj, all, &mut worst, 0);
        let best = settlement_room(&geo).unwrap();
        assert!(worst * ROOM_SLACK >= best, "{name}: the worst placements fit only {worst} of a best {best}");
        assert!(worst <= best);
    }
}

#[test]
fn random_placement_fills_between_half_and_all_of_the_room() {
    let mut rng = StdRng::seed_from_u64(3);
    let mut shapes: Vec<(String, Vec<(i32, i32)>)> = small_shapes().into_iter().map(|(n, c)| (n.to_string(), c)).collect();
    shapes.push(("Standard".into(), coords_of(&standard_def())));
    shapes.push(("Earth".into(), coords_of(&earth_def())));
    shapes.push(("block 15x15".into(), (0..15).flat_map(|r| (0..15).map(move |q| (q - r / 2, r))).collect()));
    for (name, coords) in shapes {
        let geo = Geometry::build(coords);
        let best = settlement_room(&geo).unwrap();
        for _ in 0..200 {
            let mut order: Vec<usize> = (0..geo.vertices.len()).collect();
            order.shuffle(&mut rng);
            let mut blocked = vec![false; order.len()];
            let mut placed = 0;
            for v in order {
                if !blocked[v] {
                    placed += 1;
                    blocked[v] = true;
                    for &w in &geo.vertices[v].neighbors {
                        blocked[w] = true;
                    }
                }
            }
            assert!(placed <= best, "{name}: placed {placed}, but {best} is supposed to be the most");
            assert!(placed * ROOM_SLACK >= best, "{name}: random placement filled only {placed} of {best}");
        }
    }
}

#[test]
fn islands_are_counted_by_shared_edges() {
    let g = Geometry::build(vec![(0, 0), (1, 0), (5, 0), (5, 1), (9, 9)]);
    let mut sizes: Vec<usize> = islands(&g).iter().map(|i| i.len()).collect();
    sizes.sort();
    assert_eq!(sizes, vec![1, 2, 2]);
    // agrees with the board's own (test-only) count on the built-in maps
    for map in ["standard", "earth"] {
        let b = Board::generate(map, &mut rand::thread_rng());
        let g = Geometry::build(b.hexes.iter().map(|h| (h.q, h.r)).collect());
        assert_eq!(islands(&g).len(), b.islands().len());
    }
}

// ---------------- the matching, on graphs that are not maps ----------------

/// Neighbour lists for a graph of `n` vertices with these edges.
fn graph(n: usize, edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut adjacency = vec![vec![]; n];
    for &(a, b) in edges {
        adjacency[a].push(b);
        adjacency[b].push(a);
    }
    adjacency
}

#[test]
fn the_largest_independent_set_of_small_bipartite_graphs() {
    assert_eq!(max_independent_set_size(&[]), Some(0), "no vertices");
    assert_eq!(max_independent_set_size(&graph(4, &[])), Some(4), "no edges: all of them");
    assert_eq!(max_independent_set_size(&graph(4, &[(0, 1), (1, 2), (2, 3)])), Some(2), "a path of 4");
    assert_eq!(max_independent_set_size(&graph(5, &[(0, 1), (0, 2), (0, 3), (0, 4)])), Some(4), "a star: all its leaves");
    assert_eq!(max_independent_set_size(&graph(6, &[(0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (5, 0)])), Some(3), "a cycle of 6");
    assert_eq!(max_independent_set_size(&graph(5, &[(0, 2), (0, 3), (0, 4), (1, 2), (1, 3), (1, 4)])), Some(3), "2 against 3");
    assert_eq!(max_independent_set_size(&graph(5, &[(0, 1), (2, 3), (3, 4)])), Some(3), "separate pieces add up");
    // the second vertex on the left can only be matched by moving the first one's partner out of its way
    assert_eq!(max_independent_set_size(&graph(4, &[(0, 2), (0, 3), (1, 2)])), Some(2), "needs a rerouted match");
}

#[test]
fn a_graph_with_an_odd_cycle_is_not_bipartite() {
    assert_eq!(max_independent_set_size(&graph(3, &[(0, 1), (1, 2), (2, 0)])), None);
    // found in a piece the search only reaches after finishing another
    assert_eq!(max_independent_set_size(&graph(5, &[(0, 1), (2, 3), (3, 4), (4, 2)])), None);
}

#[test]
fn the_largest_independent_set_agrees_with_brute_force_on_random_bipartite_graphs() {
    use rand::Rng;
    let mut rng = StdRng::seed_from_u64(11);
    for _ in 0..300 {
        let (left, right) = (rng.gen_range(0..7), rng.gen_range(0..7));
        let n = left + right;
        let edges: Vec<(usize, usize)> = (0..left).flat_map(|l| (left..n).map(move |r| (l, r))).filter(|_| rng.gen_bool(0.3)).collect();
        let adjacency = graph(n, &edges);
        let masks: Vec<u64> = adjacency.iter().map(|ns| ns.iter().fold(0u64, |m, &w| m | (1 << w))).collect();
        assert_eq!(max_independent_set_size(&adjacency), Some(brute_force_room(&masks, (1u64 << n) - 1)), "{edges:?}");
    }
}
