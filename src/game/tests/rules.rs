//! The basic rules: setup, the distance rule, the two awards, harbour ratios, and the robber.

use crate::game::{Action, Building, Game, Phase};
use crate::types::{PlayerId, Resource};
use super::support::{all_connected, line_of_roads, names, P0, P1, P2, pick};

#[test]
fn setup_order_and_resources() {
    let mut rng = rand::thread_rng();
    let mut g = Game::new(names(3));
    let mut order = vec![];
    while g.phase == Phase::Setup {
        let p = g.current;
        if g.setup_vertex.is_none() {
            order.push(p);
        }
        let l = g.legal(p);
        if g.setup_vertex.is_some() {
            g.apply(p, Action::BuildRoad { edge: pick(&l.roads, &mut rng).unwrap() }).unwrap();
        } else {
            g.apply(p, Action::BuildSettlement { vertex: pick(&l.settlements, &mut rng).unwrap() })
                .unwrap();
        }
    }
    assert_eq!(order, vec![P0, P1, P2, P2, P1, P0]);
    assert_eq!(g.phase, Phase::Roll);
    assert_eq!(g.current, P0);
    for p in PlayerId::range(3) {
        assert_eq!(g.players[p].settlements_left, 3);
        assert_eq!(g.players[p].roads_left, 13);
    }
}

#[test]
fn not_your_turn_is_rejected() {
    let mut g = Game::new(names(2));
    assert!(g.apply(P1, Action::BuildSettlement { vertex: 0 }).is_err());
    assert!(g.apply(P0, Action::Roll).is_err());
}

#[test]
fn distance_rule_enforced() {
    let mut g = Game::new(names(2));
    g.apply(P0, Action::BuildSettlement { vertex: 0 }).unwrap();
    let e = g.board.vertices[0].edges[0];
    g.apply(P0, Action::BuildRoad { edge: e }).unwrap();
    let nb = g.board.vertices[0].neighbors[0];
    assert!(g.apply(P1, Action::BuildSettlement { vertex: nb }).is_err());
}

#[test]
fn longest_road_award_and_break() {
    let mut g = Game::new(names(3));
    g.phase = Phase::Main;
    let path = line_of_roads(&mut g, P0, 5);
    assert_eq!(g.longest_road(P0), 5);
    g.update_longest_road();
    assert_eq!(g.longest_road_holder, Some(P0));
    assert_eq!(g.vp_public(P0), 2);

    // An opponent settlement in the middle of the path breaks it.
    let mid = {
        let (e1, e2) = (&g.board.edges[path[2]], &g.board.edges[path[3]]);
        [e1.a, e1.b].into_iter().find(|v| *v == e2.a || *v == e2.b).unwrap()
    };
    g.buildings[mid] = Some(Building { owner: P1, city: false });
    assert!(g.longest_road(P0) < 5);
    g.update_longest_road();
    assert_eq!(g.longest_road_holder, None);
}

#[test]
fn longest_roads_match_the_per_player_search_and_feed_every_view() {
    let mut g = Game::new(names(3));
    g.phase = Phase::Main;
    let path = line_of_roads(&mut g, P0, 5);
    // One road for P1 on the last edge, which the search for P0's path (from vertex 0) never reaches.
    let far = g.roads.len() - 1;
    assert!(!path.contains(&far));
    g.roads[far] = Some(P1);

    let lens = g.longest_roads();
    for p in PlayerId::range(3) {
        assert_eq!(lens[p], g.longest_road(p));
    }
    assert_eq!((lens[P0], lens[P1], lens[P2]), (5, 1, 0));

    // Every seat's view reports the shared lengths, even though no award update has run.
    for you in PlayerId::range(3) {
        let v = g.view(you, &all_connected(3), &lens);
        for p in PlayerId::range(3) {
            assert_eq!(v.players[p.index()].longest_road, lens[p]);
        }
    }
}

#[test]
fn largest_army_requires_three_and_must_be_beaten() {
    let mut g = Game::new(names(3));
    g.players[P0].knights = 3;
    g.update_largest_army(P0);
    assert_eq!(g.largest_army_holder, Some(P0));
    g.players[P1].knights = 3;
    g.update_largest_army(P1);
    assert_eq!(g.largest_army_holder, Some(P0));
    g.players[P1].knights = 4;
    g.update_largest_army(P1);
    assert_eq!(g.largest_army_holder, Some(P1));
}

#[test]
fn bank_trade_ratios_with_ports() {
    let mut g = Game::new(names(2));
    assert_eq!(g.ratio(P0, Resource::Brick), 4);
    let port = g.board.ports[0].clone();
    g.buildings[port.a] = Some(Building { owner: P0, city: false });
    match port.resource {
        None => assert_eq!(g.ratio(P0, Resource::Brick), 3),
        Some(r) => {
            assert_eq!(g.ratio(P0, r), 2);
            let other = Resource::from_index((r.index() + 1) % 5).unwrap();
            assert_eq!(g.ratio(P0, other), 4);
        }
    }
}

#[test]
fn robber_blocks_production() {
    for _ in 0..30 {
        let mut g = Game::new(names(2));
        // Find a hex + corner where no other hex touching that corner shares the number,
        // so the robber on that hex must stop all production for the corner.
        let (h, v) = (0..19)
            .filter(|&h| g.board.hexes[h].number.is_some())
            .flat_map(|h| g.board.hexes[h].vertices.map(|v| (h, v)))
            .find(|&(h, v)| {
                g.board.vertices[v]
                    .hexes
                    .iter()
                    .all(|&o| o == h || g.board.hexes[o].number != g.board.hexes[h].number)
            })
            .expect("some corner always qualifies");
        g.buildings[v] = Some(Building { owner: P0, city: true });
        let roll = g.board.hexes[h].number.unwrap();
        g.robber = h;
        g.distribute(roll);
        assert_eq!(g.players[P0].hand.total(), 0, "robber must block the hex");
        g.robber = g.board.desert;
        g.distribute(roll);
        assert_eq!(g.players[P0].hand.total(), 2, "a city on one producing hex pays 2");
    }
}
