//! Where things may go: free corners and sides, and the hexes the robber may move to.

use crate::types::{PerPlayer, PlayerId};
use super::{Game, Legal, Phase};

impl Game {
    /// Empty vertex with no building on any adjacent vertex (distance rule).
    pub fn settlement_free(&self, v: usize) -> bool {
        self.buildings[v].is_none()
            && self.board.vertices[v]
                .neighbors
                .iter()
                .all(|&n| self.buildings[n].is_none())
    }

    pub fn settlement_connected(&self, p: PlayerId, v: usize) -> bool {
        self.settlement_free(v)
            && self.board.vertices[v]
                .edges
                .iter()
                .any(|&e| self.roads[e] == Some(p))
    }

    /// Edge is empty and touches one of `p`'s buildings or roads
    /// (an opponent's building blocks continuing through its vertex).
    pub fn road_connected(&self, p: PlayerId, e: usize) -> bool {
        if self.roads[e].is_some() {
            return false;
        }
        let edge = &self.board.edges[e];
        for v in [edge.a, edge.b] {
            match self.buildings[v] {
                Some(b) if b.owner == p => return true,
                Some(_) => continue,
                None => {
                    if self.board.vertices[v]
                        .edges
                        .iter()
                        .any(|&o| o != e && self.roads[o] == Some(p))
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Where `p` could build a road: bridge sites are not roads (see `legal_bridges`).
    pub fn legal_roads(&self, p: PlayerId) -> Vec<usize> {
        if self.players[p].roads_left == 0 {
            return vec![];
        }
        (0..self.roads.len())
            .filter(|&e| !self.board.edges[e].bridge && self.road_connected(p, e))
            .collect()
    }

    /// Where `p` could build a bridge: a bridge site that touches their roads or buildings, the way a road
    /// must. Empty when the game has bridges off or `p` has no road pieces left.
    pub fn legal_bridges(&self, p: PlayerId) -> Vec<usize> {
        if !self.settings.bridges || self.players[p].roads_left == 0 {
            return vec![];
        }
        (0..self.roads.len())
            .filter(|&e| self.board.edges[e].bridge && self.road_connected(p, e))
            .collect()
    }

    pub fn legal(&self, p: PlayerId) -> Legal {
        let mut l = Legal::default();
        // In Speed mode everybody builds at once while the table is open (`Phase::Main`); everything else (setup
        // and the robber) is still one player's job.
        let open_table = self.settings.speed && self.phase == Phase::Main;
        if p != self.current && !open_table {
            return l;
        }
        match self.phase {
            Phase::Setup => {
                if let Some(v) = self.setup_vertex {
                    l.roads = self.board.vertices[v]
                        .edges
                        .iter()
                        .copied()
                        .filter(|&e| self.roads[e].is_none() && !self.board.edges[e].bridge)
                        .collect();
                } else {
                    l.settlements = (0..self.buildings.len())
                        .filter(|&v| self.settlement_free(v))
                        .collect();
                }
            }
            Phase::Main => {
                let pl = &self.players[p];
                l.roads = self.legal_roads(p);
                l.bridges = self.legal_bridges(p);
                if pl.settlements_left > 0 {
                    l.settlements = (0..self.buildings.len())
                        .filter(|&v| self.settlement_connected(p, v))
                        .collect();
                }
                if pl.cities_left > 0 {
                    let repairing = self.has_damaged(p);
                    l.cities = (0..self.buildings.len())
                        .filter(|&v| {
                            matches!(self.buildings[v], Some(b) if b.owner == p && !b.city)
                                && (!repairing || self.damaged.contains(&v))
                        })
                        .collect();
                }
            }
            Phase::RoadBuilding => l.roads = self.legal_roads(p),
            Phase::MoveRobber => {
                l.hexes = self.robber_hexes(p)
            }
            _ => {}
        }
        l
    }

    /// Whether `p` has a settlement on loan after lava hit one of their cities (see `Game::damaged`).
    pub fn has_damaged(&self, p: PlayerId) -> bool {
        self.damaged.iter().any(|&v| matches!(self.buildings[v], Some(b) if b.owner == p))
    }

    /// Hexes where `p` may put the robber. With Friendly Robber on, a hex is off limits
    /// if any other player with `Settings::friendly_robber_max_vp` or fewer visible points has a
    /// building on it (unless that would leave nowhere to go).
    pub fn robber_hexes(&self, p: PlayerId) -> Vec<usize> {
        let all: Vec<usize> = (0..self.board.hexes.len()).filter(|&h| h != self.robber).collect();
        if !self.settings.friendly_robber {
            return all;
        }
        let max_vp = self.settings.friendly_robber_max_vp();
        let protected = PerPlayer::from_fn(self.n, |q| q != p && self.vp_public(q) <= max_vp);
        let open: Vec<usize> = all
            .iter()
            .copied()
            .filter(|&h| {
                !self.board.hexes[h]
                    .vertices
                    .iter()
                    .any(|&v| matches!(self.buildings[v], Some(b) if protected[b.owner]))
            })
            .collect();
        if open.is_empty() {
            all
        } else {
            open
        }
    }
}
