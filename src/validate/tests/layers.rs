//! Layers 1 to 3: the hex list, the rules and the content.

use super::*;

// ---------------- layer 1: the hex list ----------------

#[test]
fn a_wrong_version_is_reported_and_nothing_else_about_the_file() {
    let mut d = flower();
    d.version = 9;
    d.terrain_pool.clear(); // would be a problem, but not one worth reporting in a file of another version
    let r = check(&d);
    assert_eq!(r.errors, vec![MapError::UnsupportedVersion { found: 9, supported: 1 }]);
    assert_eq!(r.stats, None);
}

#[test]
fn an_empty_or_oversized_hex_list_stops_there() {
    let mut d = flower();
    d.hexes.clear();
    assert_only_error(&d, MapError::NoHexes);
    assert_eq!(check(&d).stats, None);

    let mut d = flower();
    d.hexes = (0..=MAX_HEXES as i32).map(|i| HexDef::at(i % 90, i / 90)).collect();
    assert_only_error(&d, MapError::TooManyHexes { count: MAX_HEXES + 1, max: MAX_HEXES });
}

#[test]
fn every_bad_coordinate_and_duplicate_is_reported() {
    let mut d = flower();
    d.hexes[1].q = MAX_COORD + 1;
    d.hexes[2].r = i32::MIN;
    d.hexes[4] = d.hexes[3].clone();
    let r = check(&d);
    assert_eq!(
        r.errors,
        vec![
            MapError::CoordOutOfRange { q: MAX_COORD + 1, r: 0 },
            MapError::CoordOutOfRange { q: 1, r: i32::MIN },
            MapError::DuplicateHex { q: 0, r: -1 },
        ]
    );
    assert_eq!(r.stats, None, "no summary of a hex list that makes no sense");
}

// ---------------- layer 2: the rules ----------------

#[test]
fn the_player_range_and_recommended_settings_are_checked() {
    for (min, max) in [(1, 4), (2, 9), (4, 2), (0, 0)] {
        let mut d = flower();
        d.players = PlayerRange { min, max };
        assert_error(&d, MapError::BadPlayerRange { min, max });
    }
    let mut d = flower();
    let mut s = Settings::default();
    s.points_to_win = 1;
    d.recommended_settings = Some(s);
    assert!(matches!(check(&d).errors[..], [MapError::BadRecommendedSettings(_)]));
}

#[test]
fn the_name_and_author_must_be_short_plain_text() {
    let mut d = flower();
    for name in ["", "   ", "\t"] {
        d.name = name.into();
        assert_only_error(&d, MapError::EmptyName);
    }
    // the limit is in characters (not bytes) and spaces around the text don't count
    for name in ["x".repeat(MAX_NAME_CHARS), "é".repeat(MAX_NAME_CHARS), format!("  {}  ", "x".repeat(MAX_NAME_CHARS))] {
        d.name = name;
        assert!(check(&d).errors.is_empty(), "{:?}", check(&d).errors);
    }
    d.name = "x".repeat(MAX_NAME_CHARS + 1);
    assert_only_error(&d, MapError::TextTooLong { field: "name", len: MAX_NAME_CHARS + 1, max: MAX_NAME_CHARS });
    d.name = "Flower".into();
    d.author = "y".repeat(MAX_AUTHOR_CHARS + 1);
    assert_only_error(&d, MapError::TextTooLong { field: "author", len: MAX_AUTHOR_CHARS + 1, max: MAX_AUTHOR_CHARS });
    // line breaks, escapes and invisible characters that reorder or hide text
    for bad in ["two\nlines", "tab\there", "esc\u{1b}[31m", "zero\u{200b}width", "flip\u{202e}ed", "bom\u{feff}x"] {
        d.name = bad.into();
        d.author = String::new();
        assert_only_error(&d, MapError::BadCharacters { field: "name" });
        d.name = "Flower".into();
        d.author = bad.into();
        assert_only_error(&d, MapError::BadCharacters { field: "author" });
    }
}

#[test]
fn rule_problems_do_not_hide_content_problems() {
    let mut d = flower();
    d.players = PlayerRange { min: 9, max: 9 };
    d.terrain_pool.clear();
    let r = check(&d);
    assert_eq!(
        r.errors,
        vec![MapError::BadPlayerRange { min: 9, max: 9 }, MapError::TerrainPoolMismatch { random_hexes: 6, pool: 0 }]
    );
}

// ---------------- layer 3: the content ----------------

#[test]
fn terrain_pool_must_match_the_random_hexes() {
    let mut d = flower();
    d.terrain_pool.insert(Terrain::Forest, 3);
    assert_only_error(&d, MapError::TerrainPoolMismatch { random_hexes: 6, pool: 7 });
    d.terrain_pool.clear();
    assert_only_error(&d, MapError::TerrainPoolMismatch { random_hexes: 6, pool: 0 });
}

#[test]
fn a_terrain_pool_problem_is_not_followed_by_a_made_up_token_count() {
    // how many tokens are needed depends on how many deserts get dealt, which this can't say
    let mut d = flower();
    d.terrain_pool = BTreeMap::from([(Terrain::Desert, 50)]);
    assert_only_error(&d, MapError::TerrainPoolMismatch { random_hexes: 6, pool: 50 });
}

#[test]
fn deserts_from_the_pool_need_room_to_go() {
    // 2 deserts to deal but numbers are written on all but one of the random hexes
    let mut d = flower();
    d.terrain_pool = BTreeMap::from([(Terrain::Desert, 2), (Terrain::Forest, 4)]);
    for i in 2..7 {
        d.hexes[i].number = Slot::Fixed(if i % 2 == 0 { 9 } else { 3 });
    }
    d.token_pool.clear();
    assert_only_error(&d, MapError::NotEnoughDesertSpots { wanted: 2, available: 1 });
    // the same map is fine with one desert fewer to place (and then no token is needed: the 1 random hex is the desert)
    d.terrain_pool = BTreeMap::from([(Terrain::Desert, 1), (Terrain::Forest, 5)]);
    assert!(check(&d).errors.is_empty(), "{:?}", check(&d).errors);
}

#[test]
fn fixed_numbers_must_be_real_tokens_and_not_on_a_desert() {
    for bad in [0u8, 1, 7, 13, 255] {
        let mut d = flower();
        d.hexes[1].number = Slot::Fixed(bad);
        d.token_pool = BTreeMap::from([(3, 1), (4, 1), (5, 1), (9, 1), (10, 1)]);
        assert_only_error(&d, MapError::InvalidToken { value: bad, at: Some((1, 0)) });
    }
    let mut d = flower();
    d.hexes[0].number = Slot::Fixed(6);
    assert_only_error(&d, MapError::NumberOnDesert { q: 0, r: 0 });
}

#[test]
fn the_token_pool_must_match_and_hold_real_tokens() {
    let mut d = flower();
    d.token_pool.insert(3, 2);
    assert_only_error(&d, MapError::TokenPoolMismatch { needed: 6, pool: 7 });
    for bad in [0u8, 1, 7, 13, 255] {
        let mut d = flower();
        d.token_pool.remove(&3);
        d.token_pool.insert(bad, 1);
        assert_only_error(&d, MapError::InvalidToken { value: bad, at: None });
    }
}

#[test]
fn written_in_numbers_and_deserts_change_how_many_tokens_are_needed() {
    // 19 hexes, 1 desert dealt, so 18 tokens - unless one hex has its number written in: then 17
    let mut d = standard_def();
    d.hexes[0].number = Slot::Fixed(6);
    assert_only_error(&d, MapError::TokenPoolMismatch { needed: 17, pool: 18 });
    d.token_pool.insert(6, 1); // one of the two 6s is now written into the map
    assert!(check(&d).errors.is_empty(), "{:?}", check(&d).errors);
    // a desert written into the map is one hex that needs no token and takes none of the pool
    let mut d = standard_def();
    d.hexes[0].terrain = Slot::Fixed(Terrain::Desert);
    d.terrain_pool.insert(Terrain::Desert, 0);
    assert!(check(&d).errors.is_empty(), "{:?}", check(&d).errors);
}

#[test]
fn written_in_numbers_that_may_not_touch_are_reported_unless_spreading_is_off() {
    let mut d = flower();
    d.hexes[1].number = Slot::Fixed(6); // (1, 0)
    d.hexes[2].number = Slot::Fixed(8); // (1, -1) touches it
    d.token_pool = BTreeMap::from([(3, 1), (4, 1), (5, 1), (9, 1)]);
    assert_only_error(&d, MapError::FixedTokensClash { a: (1, 0), b: (1, -1) });
    // two equal numbers touching
    d.hexes[2].number = Slot::Fixed(6);
    assert_only_error(&d, MapError::FixedTokensClash { a: (1, 0), b: (1, -1) });
    d.constraints.spread_tokens = false;
    assert!(check(&d).errors.is_empty());
}

#[test]
fn each_bad_harbour_is_reported() {
    let any = [(PortKind::Any, 1)];
    assert!(check(&with_ports(vec![PortSpot::at(1, 0, 0)], &any)).errors.is_empty());
    assert_only_error(&with_ports(vec![PortSpot::at(9, 9, 0)], &any), MapError::PortHexMissing { q: 9, r: 9 });
    assert_only_error(&with_ports(vec![PortSpot::at(1, 0, 6)], &any), MapError::PortSideInvalid { q: 1, r: 0, side: 6 });
    assert_only_error(&with_ports(vec![PortSpot::at(0, 0, 2)], &any), MapError::PortNotOnCoast { q: 0, r: 0, side: 2 });
    assert_only_error(
        &with_ports(vec![PortSpot::at(1, 0, 0), PortSpot::at(1, 0, 0)], &[(PortKind::Any, 2)]),
        MapError::PortEdgeTaken { q: 1, r: 0, side: 0 },
    );
    // two bad ones in one map are both reported
    let r = check(&with_ports(vec![PortSpot::at(9, 9, 0), PortSpot::at(0, 0, 2)], &[(PortKind::Any, 2)]));
    assert_eq!(r.errors, vec![MapError::PortHexMissing { q: 9, r: 9 }, MapError::PortNotOnCoast { q: 0, r: 0, side: 2 }]);
}

#[test]
fn the_harbour_pool_must_match_the_harbours_of_random_kind() {
    let two = vec![PortSpot::at(1, 0, 0), PortSpot::at(1, 0, 1)];
    assert_only_error(&with_ports(two.clone(), &[(PortKind::Any, 1)]), MapError::PortPoolMismatch { random_spots: 2, pool: 1 });
    assert_only_error(&with_ports(two, &[(PortKind::Any, 3)]), MapError::PortPoolMismatch { random_spots: 2, pool: 3 });
    // a harbour whose kind is written in doesn't draw from the pool
    let mixed = vec![PortSpot { q: 1, r: 0, side: 0, kind: Slot::Fixed(PortKind::Ore) }, PortSpot::at(1, 0, 1)];
    assert!(check(&with_ports(mixed, &[(PortKind::Any, 1)])).errors.is_empty());
}

#[test]
fn ring_harbours_need_one_coast_with_matching_spacing() {
    // the flower's coast is 18 edges
    assert!(check(&with_ring(vec![6, 6, 6], 3)).errors.is_empty());
    assert_only_error(&with_ring(vec![6, 6], 2), MapError::BadPortSpacing { sum: 12, coast: 18 });
    assert_only_error(&with_ring(vec![0, 18], 2), MapError::BadPortSpacing { sum: 18, coast: 18 });
    assert_only_error(&with_ring(vec![usize::MAX, usize::MAX], 2), MapError::BadPortSpacing { sum: u64::MAX, coast: 18 });
    assert_only_error(&with_ring(vec![6, 6, 6], 2), MapError::PortPoolMismatch { random_spots: 3, pool: 2 });

    // two islands are two coasts
    let mut d = flower();
    d.hexes = vec![
        HexDef { q: 0, r: 0, terrain: Slot::Fixed(Terrain::Forest), number: Slot::Fixed(5) },
        HexDef { q: 5, r: 0, terrain: Slot::Fixed(Terrain::Desert), number: Slot::Random },
    ];
    d.terrain_pool.clear();
    d.token_pool.clear();
    d.ports = PortsDef::Ring { spacing: vec![6, 6], pool: BTreeMap::from([(PortKind::Any, 2)]) };
    assert_only_error(&d, MapError::CoastNotOneRing { rings: 2 });
}

#[test]
fn the_robber_needs_somewhere_to_start() {
    let mut d = flower();
    d.robber_start = Some(HexRef { q: 5, r: 5 });
    assert_only_error(&d, MapError::RobberStartMissing { q: 5, r: 5 });
    d.robber_start = Some(HexRef { q: 1, r: 0 });
    assert!(check(&d).errors.is_empty());

    // no desert anywhere (the old centre now needs a token too)
    let mut d = flower();
    d.hexes[0].terrain = Slot::Fixed(Terrain::Mountains);
    d.token_pool.insert(6, 1);
    assert_only_error(&d, MapError::NoRobberStart);
    d.robber_start = Some(HexRef { q: 1, r: 0 });
    assert!(check(&d).errors.is_empty());
    // a desert dealt from the pool counts
    let mut d = flower();
    d.hexes[0].terrain = Slot::Random;
    d.terrain_pool.insert(Terrain::Desert, 1);
    assert!(check(&d).errors.is_empty(), "{:?}", check(&d).errors);
}

#[test]
fn independent_problems_are_all_reported_in_one_pass() {
    let mut d = flower();
    d.players = PlayerRange { min: 4, max: 2 }; // 1
    d.terrain_pool.insert(Terrain::Hills, 5); // 2
    d.hexes[0].number = Slot::Fixed(6); // 3 (the centre is a desert)
    d.token_pool.insert(7, 1); // 4: and the pool is no longer one token per remaining hex, which can't be said (see above)
    d.ports = PortsDef::Fixed { spots: vec![PortSpot::at(2, 2, 0), PortSpot::at(0, 0, 0)], pool: BTreeMap::new() }; // 5, 6, 7
    d.robber_start = Some(HexRef { q: 8, r: 8 }); // 8
    let r = check(&d);
    assert_eq!(
        r.errors,
        vec![
            MapError::BadPlayerRange { min: 4, max: 2 },
            MapError::TerrainPoolMismatch { random_hexes: 6, pool: 9 },
            MapError::NumberOnDesert { q: 0, r: 0 },
            MapError::InvalidToken { value: 7, at: None },
            MapError::PortHexMissing { q: 2, r: 2 },
            MapError::PortNotOnCoast { q: 0, r: 0, side: 0 },
            MapError::PortPoolMismatch { random_spots: 2, pool: 0 },
            MapError::RobberStartMissing { q: 8, r: 8 },
        ]
    );
}

#[test]
fn a_hopeless_file_gets_a_bounded_report() {
    let mut d = flower();
    d.ports = PortsDef::Fixed { spots: (0..500).map(|i| PortSpot::at(50 + i, 0, 0)).collect(), pool: BTreeMap::new() };
    let r = check(&d);
    assert_eq!(r.errors.len(), MAX_ISSUES);
    assert!(r.truncated);
}
