//! Building roads, bridges, settlements and cities, and what each costs.

use crate::log_event::LogEvent;
use crate::types::{PlayerId, Res};
use super::{Building, Game, Phase};

pub const COST_ROAD: Res = Res::new(1, 1, 0, 0, 0);
pub const COST_SETTLEMENT: Res = Res::new(1, 1, 0, 1, 1);
pub const COST_CITY: Res = Res::new(0, 0, 3, 2, 0);
/// A bridge: 1 brick, 1 lumber and 1 ore.
pub const COST_BRIDGE: Res = Res::new(1, 1, 1, 0, 0);

impl Game {
    /// Put one of `p`'s road pieces on `edge`. Callers have already checked the placement is legal and
    /// taken any payment; this is shared by setup roads, Road Building, bought roads and bridges.
    fn place_road(&mut self, p: PlayerId, edge: usize) {
        self.roads[edge] = Some(p);
        self.players[p].roads_left -= 1;
    }

    pub(super) fn do_build_road(&mut self, p: PlayerId, edge: usize) -> Result<(), String> {
        if edge >= self.roads.len() {
            return Err("Unknown road location".into());
        }
        match self.phase {
            Phase::Setup => {
                let v = self.setup_vertex.ok_or("Place a settlement first")?;
                let e = &self.board.edges[edge];
                if self.roads[edge].is_some() || e.bridge || (e.a != v && e.b != v) {
                    return Err("The road must touch your new settlement".into());
                }
                self.place_road(p, edge);
                self.setup_vertex = None;
                self.advance_setup();
                Ok(())
            }
            Phase::RoadBuilding => {
                if self.players[p].roads_left == 0 || self.board.edges[edge].bridge || !self.road_connected(p, edge) {
                    return Err("You can't build a road there".into());
                }
                self.place_road(p, edge);
                self.road_building_left -= 1;
                self.add_bonus();
                self.update_longest_road();
                if self.road_building_left == 0 || self.legal_roads(p).is_empty() {
                    self.road_building_left = 0;
                    self.phase = self.resume_phase();
                }
                Ok(())
            }
            Phase::Main => {
                if self.players[p].roads_left == 0 {
                    return Err("You have no roads left".into());
                }
                // (a Road Building card in Speed mode leaves free roads to place, whenever the player likes)
                let free = self.players[p].free_roads > 0;
                if !free && !self.players[p].hand.covers(&COST_ROAD) {
                    return Err("A road costs 1 brick and 1 lumber".into());
                }
                if self.board.edges[edge].bridge {
                    return Err("That is water: a bridge goes there, not a road".into());
                }
                if !self.road_connected(p, edge) {
                    return Err("You can't build a road there".into());
                }
                if free {
                    self.players[p].free_roads -= 1;
                } else {
                    self.pay(p, &COST_ROAD);
                }
                self.place_road(p, edge);
                self.log(LogEvent::BuiltRoad { player: p });
                self.add_bonus();
                self.update_longest_road();
                Ok(())
            }
            _ => Err("You can't build a road right now".into()),
        }
    }

    /// Build a bridge: a road over water, paid for with `COST_BRIDGE`. Main phase only (so not in setup and
    /// not with a Road Building card). It takes one of the player's road pieces, joins their road network
    /// like any road and counts as one road for Longest Road.
    pub(super) fn do_build_bridge(&mut self, p: PlayerId, edge: usize) -> Result<(), String> {
        if edge >= self.roads.len() {
            return Err("Unknown bridge location".into());
        }
        if self.phase != Phase::Main {
            return Err("You can't build a bridge right now".into());
        }
        if !self.settings.bridges {
            return Err("Bridges are off in this game".into());
        }
        if self.players[p].roads_left == 0 {
            return Err("You have no roads left".into());
        }
        if !self.board.edges[edge].bridge {
            return Err("A bridge can only be built over water".into());
        }
        if !self.players[p].hand.covers(&COST_BRIDGE) {
            return Err("A bridge costs 1 brick, 1 lumber and 1 ore".into());
        }
        if !self.road_connected(p, edge) {
            return Err("You can't build a bridge there".into());
        }
        self.pay(p, &COST_BRIDGE);
        self.place_road(p, edge);
        self.log(LogEvent::BuiltBridge { player: p });
        self.add_bonus();
        self.update_longest_road();
        Ok(())
    }

    pub(super) fn do_build_settlement(&mut self, p: PlayerId, v: usize) -> Result<(), String> {
        if v >= self.buildings.len() {
            return Err("Unknown settlement location".into());
        }
        match self.phase {
            Phase::Setup => {
                if self.setup_vertex.is_some() {
                    return Err("Place a road next to your new settlement".into());
                }
                if !self.settlement_free(v) {
                    return Err("You can't build a settlement there".into());
                }
                self.buildings[v] = Some(Building { owner: p, city: false });
                self.players[p].settlements_left -= 1;
                self.setup_vertex = Some(v);
                let mut volcanoes = vec![];
                if self.setup_idx / self.n == self.settings.starting_settlements as usize - 1 {
                    // Each player's last starting settlement: collect one card per adjacent hex. A volcano or
                    // a gold field makes nothing by itself, so it gives a free card of the player's choice instead.
                    let hexes = self.board.vertices[v].hexes.clone();
                    for h in hexes {
                        if let Some(r) = self.board.hexes[h].terrain.resource() {
                            if self.bank[r] > 0 {
                                self.bank[r] -= 1;
                                self.players[p].hand[r] += 1;
                            }
                        } else if self.board.hexes[h].terrain.gives_free_pick() {
                            volcanoes.push(h);
                        }
                    }
                }
                self.log(LogEvent::PlacedSettlement { player: p });
                if !volcanoes.is_empty() {
                    // One pick for each volcano or gold field, then back to setup for the road (see `advance_volcano`).
                    self.setup_pick = true;
                    self.volcano_queue = volcanoes;
                    self.volcano_active = None;
                    self.volcano_picks.clear();
                    self.advance_volcano();
                }
                Ok(())
            }
            Phase::Main => {
                if self.players[p].settlements_left == 0 {
                    return Err("You have no settlements left".into());
                }
                if !self.players[p].hand.covers(&COST_SETTLEMENT) {
                    return Err("A settlement costs 1 brick, 1 lumber, 1 grain and 1 wool".into());
                }
                if !self.settlement_connected(p, v) {
                    return Err("A settlement must connect to your road and be 2+ steps from any other settlement".into());
                }
                self.pay(p, &COST_SETTLEMENT);
                self.buildings[v] = Some(Building { owner: p, city: false });
                self.players[p].settlements_left -= 1;
                self.log(LogEvent::BuiltSettlement { player: p });
                self.add_bonus();
                self.update_longest_road();
                Ok(())
            }
            _ => Err("You can't build a settlement right now".into()),
        }
    }

    pub(super) fn do_build_city(&mut self, p: PlayerId, v: usize) -> Result<(), String> {
        if self.phase != Phase::Main {
            return Err("You can't build a city right now".into());
        }
        if v >= self.buildings.len() {
            return Err("Unknown city location".into());
        }
        if self.players[p].cities_left == 0 {
            return Err("You have no cities left".into());
        }
        if !self.players[p].hand.covers(&COST_CITY) {
            return Err("A city costs 3 ore and 2 grain".into());
        }
        match self.buildings[v] {
            Some(b) if b.owner == p && !b.city => {}
            _ => return Err("A city must replace one of your settlements".into()),
        }
        let repairing = self.damaged.iter().position(|&d| d == v);
        if repairing.is_none() && self.has_damaged(p) {
            return Err("Lava damaged one of your cities: repair that settlement before building another city".into());
        }
        self.pay(p, &COST_CITY);
        self.buildings[v] = Some(Building { owner: p, city: true });
        self.players[p].cities_left -= 1;
        match repairing {
            // A settlement on loan holds no piece, so there is none to give back.
            Some(i) => {
                self.damaged.remove(i);
            }
            None => self.players[p].settlements_left += 1,
        }
        self.log(LogEvent::BuiltCity { player: p });
        self.add_bonus();
        Ok(())
    }
}
