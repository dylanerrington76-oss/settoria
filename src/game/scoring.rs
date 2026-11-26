//! Points, the Longest Road and Largest Army awards, and winning.

use crate::log_event::LogEvent;
use crate::types::{DevCard, PerPlayer, PlayerId};
use super::{Game, Phase, VpBreakdown};

impl Game {
    pub fn longest_road(&self, p: PlayerId) -> usize {
        let mut used = vec![false; self.roads.len()];
        let mut best = 0;
        for v in 0..self.board.vertices.len() {
            if self.board.vertices[v]
                .edges
                .iter()
                .any(|&e| self.roads[e] == Some(p))
            {
                best = best.max(self.road_dfs(p, v, &mut used, 0, false));
            }
        }
        best
    }

    /// `longest_road` for every player. The search enumerates every trail in a road network, so it
    /// is the costly part of a view; callers that need all the lengths (the award check, and a
    /// broadcast that builds one view per seat) compute them once with this and share the result.
    pub fn longest_roads(&self) -> PerPlayer<usize> {
        PerPlayer::from_fn(self.n, |p| self.longest_road(p))
    }

    fn road_dfs(&self, p: PlayerId, v: usize, used: &mut Vec<bool>, len: usize, arrived: bool) -> usize {
        // An opponent's settlement/city interrupts a road that arrives at it.
        if arrived {
            if let Some(b) = self.buildings[v] {
                if b.owner != p {
                    return len;
                }
            }
        }
        let mut best = len;
        for &e in &self.board.vertices[v].edges {
            if self.roads[e] == Some(p) && !used[e] {
                used[e] = true;
                let next = self.board.edges[e].other(v);
                best = best.max(self.road_dfs(p, next, used, len + 1, true));
                used[e] = false;
            }
        }
        best
    }

    /// Points visible to everyone: buildings and the two awards, but not Victory Point cards.
    pub fn vp_public(&self, p: PlayerId) -> usize {
        self.vp_breakdown(p).public()
    }

    /// Every point `p` holds, hidden Victory Point cards included.
    pub fn vp_total(&self, p: PlayerId) -> usize {
        self.vp_breakdown(p).total()
    }

    /// The parts of `vp_total(p)`: points from settlements, cities, Victory Point cards and the two awards.
    /// The one place points are counted; `vp_public` and `vp_total` are sums of these parts.
    pub fn vp_breakdown(&self, p: PlayerId) -> VpBreakdown {
        let s = &self.settings;
        let (mut settlements, mut cities) = (0, 0);
        for b in self.buildings.iter().flatten().filter(|b| b.owner == p) {
            if b.city {
                cities += s.city_points as usize;
            } else {
                settlements += s.settlement_points as usize;
            }
        }
        let pl = &self.players[p];
        let vp_cards = pl.dev[DevCard::VictoryPoint] as usize + pl.dev_new[DevCard::VictoryPoint] as usize;
        let award = |holder: Option<PlayerId>, points: u8| if holder == Some(p) { points as usize } else { 0 };
        VpBreakdown {
            settlements,
            cities,
            dev_cards: vp_cards * s.vp_card_points as usize,
            largest_army: award(self.largest_army_holder, s.largest_army_points),
            longest_road: award(self.longest_road_holder, s.longest_road_points),
        }
    }

    pub(super) fn update_longest_road(&mut self) {
        let lens = self.longest_roads();
        let max = *lens.iter().max().unwrap();
        let top: Vec<PlayerId> = PlayerId::range(self.n).filter(|&p| lens[p] == max).collect();
        let old = self.longest_road_holder;
        let new = if max < self.settings.longest_road_min as usize {
            None
        } else if old.map_or(false, |h| top.contains(&h)) {
            old
        } else if top.len() == 1 {
            Some(top[0])
        } else {
            None
        };
        if new != old {
            self.longest_road_holder = new;
            match new {
                Some(p) => self.log(LogEvent::LongestRoadTaken { player: p, roads: lens[p] }),
                None => self.log(LogEvent::LongestRoadLost),
            }
        }
    }

    pub(super) fn update_largest_army(&mut self, p: PlayerId) {
        let k = self.players[p].knights;
        if k < self.settings.largest_army_min || self.largest_army_holder == Some(p) {
            return;
        }
        let beats = match self.largest_army_holder {
            None => true,
            Some(h) => k > self.players[h].knights,
        };
        if beats {
            self.largest_army_holder = Some(p);
            self.log(LogEvent::LargestArmyTaken { player: p, knights: k });
        }
    }

    pub(super) fn check_win(&mut self) {
        self.check_win_for(None);
    }

    /// Declare the winner if somebody has reached the points target. A classic game only looks at the player whose
    /// turn it is. In Speed mode there are no turns, so anybody who has the points wins the moment they have them:
    /// `actor` (whoever's action this was) first if several have, else whoever has the most.
    pub(super) fn check_win_for(&mut self, actor: Option<PlayerId>) {
        if self.phase == Phase::Setup || self.phase == Phase::GameOver {
            return;
        }
        let target = self.settings.points_to_win as usize;
        let p = if self.settings.speed {
            let reached: Vec<PlayerId> = PlayerId::range(self.n).filter(|&q| self.vp_total(q) >= target).collect();
            match actor {
                _ if reached.is_empty() => return,
                Some(a) if reached.contains(&a) => a,
                _ => reached
                    .iter()
                    .copied()
                    .max_by_key(|&q| (self.vp_total(q), std::cmp::Reverse(q.index())))
                    .unwrap_or(reached[0]),
            }
        } else {
            self.current
        };
        if self.vp_total(p) >= target {
            self.winner = Some(p);
            self.phase = Phase::GameOver;
            self.trades.clear();
            let points = self.vp_total(p);
            self.log(LogEvent::Won { player: p, points });
        }
    }
}
