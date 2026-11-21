//! The largest set of vertices in a bipartite graph with no two of them joined by an edge.
//!
//! That is a hard problem in general, but for a bipartite graph the answer is exact and quick: all the
//! vertices, less the size of a maximum matching (König's theorem). Nothing here knows about hexes; the
//! graph is just a list of neighbours for each vertex.

use std::collections::VecDeque;

/// The size of the largest set of vertices, no two of them neighbours. `adjacency[v]` lists the neighbours
/// of `v` (and every edge is listed from both ends). `None` if the graph isn't bipartite.
pub(super) fn max_independent_set_size(adjacency: &[Vec<usize>]) -> Option<usize> {
    let side = two_colour(adjacency)?;
    Some(adjacency.len() - max_matching_size(adjacency, &side))
}

/// Give every vertex one of two colours so that each edge joins one of each, or `None` if some edge can't
/// be (an odd cycle).
fn two_colour(adjacency: &[Vec<usize>]) -> Option<Vec<bool>> {
    let mut side = vec![false; adjacency.len()];
    let mut seen = vec![false; adjacency.len()];
    let mut queue = VecDeque::new();
    for start in 0..adjacency.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        queue.push_back(start);
        while let Some(v) = queue.pop_front() {
            for &w in &adjacency[v] {
                if !seen[w] {
                    seen[w] = true;
                    side[w] = !side[v];
                    queue.push_back(w);
                } else if side[w] == side[v] {
                    return None;
                }
            }
        }
    }
    Some(side)
}

/// The size of a maximum matching (the most edges, no two sharing a vertex), by augmenting paths from the
/// vertices on the `false` side.
fn max_matching_size(adjacency: &[Vec<usize>], side: &[bool]) -> usize {
    let mut matching = Matching { adjacency, owner: vec![None; adjacency.len()], stamp: vec![0; adjacency.len()] };
    let mut matched = 0;
    for (round, left) in (0..adjacency.len()).filter(|&v| !side[v]).enumerate() {
        if matching.augment(left, round + 1) {
            matched += 1;
        }
    }
    matched
}

struct Matching<'a> {
    adjacency: &'a [Vec<usize>],
    /// For each vertex on the `true` side, the vertex on the `false` side it is matched with so far.
    owner: Vec<Option<usize>>,
    /// `stamp[v] == round` once `v` has been tried in the search of that round, so it isn't tried again.
    stamp: Vec<usize>,
}

impl Matching<'_> {
    /// Try to match `left`, moving earlier matches out of the way along an augmenting path if need be.
    fn augment(&mut self, left: usize, round: usize) -> bool {
        let adjacency = self.adjacency;
        for &right in &adjacency[left] {
            if self.stamp[right] == round {
                continue;
            }
            self.stamp[right] = round;
            let free = match self.owner[right] {
                None => true,
                Some(other) => self.augment(other, round),
            };
            if free {
                self.owner[right] = Some(left);
                return true;
            }
        }
        false
    }
}
