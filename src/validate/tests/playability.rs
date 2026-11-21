//! Layer 4: playability and advice, and the trial deals behind them.

use super::*;
use crate::validate::playability::{dry_run, DryRun, DRY_RUN_TRIALS, HOPELESS_AFTER};

// ---------------- layer 4: is it worth playing ----------------

#[test]
fn a_map_of_nothing_but_desert_produces_nothing() {
    let mut d = flower();
    for h in &mut d.hexes {
        h.terrain = Slot::Fixed(Terrain::Desert);
    }
    d.terrain_pool.clear();
    d.token_pool.clear();
    assert_only_error(&d, MapError::NoProducingHexes);
}

#[test]
fn a_map_too_small_for_the_starting_settlements_is_refused() {
    // one hex has 6 corners, of which only 3 can be settled; 4 players want 2 each
    let d = one_hex();
    assert_error(&d, MapError::NotEnoughSettlementRoom { needed: 8, available: 3, players: 4, per_player: 2 });
    // for two players that is only 4, still too many; for a single settlement each it fits
    let mut d2 = d.clone();
    d2.players = PlayerRange { min: 2, max: 2 };
    assert_error(&d2, MapError::NotEnoughSettlementRoom { needed: 4, available: 3, players: 2, per_player: 2 });
    let mut s = Settings::default();
    s.starting_settlements = 1;
    let r = validate_with_rng(&d2, &s, &mut StdRng::seed_from_u64(1));
    assert!(r.errors.is_empty(), "{:?}", r.errors);
}

#[test]
fn a_map_with_just_enough_room_gets_a_warning_and_with_plenty_none() {
    // the flower has room for 12; 4 players x 2 = 8 fits, but not twice over
    assert_warning(&flower(), MapWarning::TightSettlementRoom { needed: 8, available: 12 });
    // for two players it needs only 4, and 12 is three times that
    let mut d = flower();
    d.players = PlayerRange { min: 2, max: 2 };
    let r = check(&d);
    assert!(!r.warnings.iter().any(|w| matches!(w, MapWarning::TightSettlementRoom { .. })), "{:?}", r.warnings);
    // exactly 2x is enough: 12 / 2 = 6 needed, so 3 players x 2 settlements
    let mut d = flower();
    d.players = PlayerRange { min: 2, max: 3 };
    let r = check(&d);
    assert!(!r.warnings.iter().any(|w| matches!(w, MapWarning::TightSettlementRoom { .. })), "{:?}", r.warnings);
}

#[test]
fn missing_and_lopsided_resources_are_pointed_out() {
    // the flower has no mountains
    assert_warning(&flower(), MapWarning::MissingResource { terrain: Terrain::Mountains });
    // lopsided: 6 forest hexes against 1 hills: 1 * 3 < 6
    let mut d = standard_def();
    d.terrain_pool = BTreeMap::from([
        (Terrain::Forest, 6),
        (Terrain::Hills, 1),
        (Terrain::Mountains, 4),
        (Terrain::Fields, 4),
        (Terrain::Pasture, 3),
        (Terrain::Desert, 1),
    ]);
    assert_warning(&d, MapWarning::LopsidedResources { scarce: Terrain::Hills, scarce_hexes: 1, plentiful: Terrain::Forest, plentiful_hexes: 6 });
    // the real maps are balanced enough to draw no comment (checked in the_built_in_maps_are_clean)
}

#[test]
fn an_island_of_only_desert_is_pointed_out() {
    let mut d = archipelago();
    // the second island (6,0) (7,0) (6,1): make all of it desert, which also frees tokens
    d.hexes[11].terrain = Slot::Fixed(Terrain::Desert);
    d.hexes[12].terrain = Slot::Fixed(Terrain::Desert);
    d.hexes[13].terrain = Slot::Fixed(Terrain::Desert);
    d.terrain_pool.insert(Terrain::Desert, 0);
    d.terrain_pool.insert(Terrain::Forest, 2); // 10 hexes are left to deal: 14, less 3 desert and the written-in mountains
    d.token_pool = BTreeMap::from([(2, 1), (3, 1), (4, 2), (5, 1), (9, 1), (10, 2), (11, 1), (12, 1)]);
    d.constraints.deserts_avoid_fixed_deserts = false;
    assert_warning(&d, MapWarning::IslandWithoutResources { near: (6, 0), hexes: 3 });
}

#[test]
fn a_map_without_harbours_says_so() {
    assert_warning(&flower(), MapWarning::NoHarbours);
    assert!(!check(&standard_def()).warnings.contains(&MapWarning::NoHarbours));
}

#[test]
fn harbours_that_meet_at_a_corner_are_pointed_out() {
    // sides 0 and 1 of the east hex share the corner between them
    let d = with_ports(vec![PortSpot::at(1, 0, 0), PortSpot::at(1, 0, 1)], &[(PortKind::Any, 2)]);
    let r = check(&d);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.warnings.iter().filter(|w| matches!(w, MapWarning::HarboursShareCorner { .. })).count(), 1, "{:?}", r.warnings);
    // far apart: no comment
    let d = with_ports(vec![PortSpot::at(1, 0, 0), PortSpot::at(-1, 0, 3)], &[(PortKind::Any, 2)]);
    assert!(!check(&d).warnings.iter().any(|w| matches!(w, MapWarning::HarboursShareCorner { .. })));
    // ring harbours one coast edge apart
    let d = with_ring(vec![1, 17], 2);
    assert!(check(&d).warnings.iter().any(|w| matches!(w, MapWarning::HarboursShareCorner { .. })));
}

// ---------------- the trial deals ----------------

#[test]
fn trial_deals_are_summed_up_correctly() {
    assert_eq!(dry_run(|| Ok(())), DryRun::Reliable);
    let fail = || Err(MapError::TokenPlacementFailed { attempts: 3 });
    assert_eq!(dry_run(fail), DryRun::Hopeless);

    // a deal that fails now and then is flaky, and says how often
    let mut n = 0;
    let r = dry_run(|| {
        n += 1;
        if n == 4 || n == 9 { fail() } else { Ok(()) }
    });
    assert_eq!(r, DryRun::Flaky { failed: 2, trials: DRY_RUN_TRIALS });

    // failing at the start and then recovering is flaky, not hopeless
    let mut n = 0;
    let r = dry_run(|| {
        n += 1;
        if n <= 2 { fail() } else { Ok(()) }
    });
    assert_eq!(r, DryRun::Flaky { failed: 2, trials: DRY_RUN_TRIALS });

    // hopeless maps stop early instead of repeating a slow failure ten times
    let mut tries = 0;
    let _ = dry_run(|| {
        tries += 1;
        fail()
    });
    assert_eq!(tries, HOPELESS_AFTER);

    // anything else going wrong is not bad luck
    let r = dry_run(|| Err(MapError::Internal("x")));
    assert_eq!(r, DryRun::Broken(MapError::Internal("x")));
}

#[test]
fn a_token_pool_that_cannot_be_spread_out_is_an_error_not_a_hang() {
    // 18 tokens that are all 6s and 8s on the standard board: some two must touch
    let d = standard_with(|d| d.token_pool = BTreeMap::from([(6, 9), (8, 9)]));
    assert_only_error(&d, MapError::TokenPlacementFailed { attempts: HOPELESS_AFTER });
    // every token the same number
    let d = standard_with(|d| d.token_pool = BTreeMap::from([(5, 18)]));
    assert_only_error(&d, MapError::TokenPlacementFailed { attempts: HOPELESS_AFTER });
    // the same pool is fine once spreading is off
    let d = standard_with(|d| {
        d.token_pool = BTreeMap::from([(6, 9), (8, 9)]);
        d.constraints.spread_tokens = false;
    });
    assert!(check(&d).errors.is_empty());
}

#[test]
fn the_biggest_map_allowed_is_validated_quickly_even_when_hopeless() {
    let mut d = flower();
    d.hexes = (0..23).flat_map(|r| (0..23).map(move |q| HexDef::at(q - r / 2, r))).take(MAX_HEXES).collect();
    d.terrain_pool = BTreeMap::from([(Terrain::Forest, MAX_HEXES as u32 - 1), (Terrain::Desert, 1)]);
    d.token_pool = BTreeMap::from([(5, MAX_HEXES as u32 / 2), (9, MAX_HEXES as u32 / 2 - 1)]);
    let t = std::time::Instant::now();
    let r = check(&d);
    assert!(r.errors.contains(&MapError::TokenPlacementFailed { attempts: HOPELESS_AFTER }), "{:?}", r.errors);
    let limit = if cfg!(debug_assertions) { 90 } else { 5 };
    assert!(t.elapsed().as_secs() < limit, "validation took {:?}", t.elapsed());

    // and one that can be dealt, with every other check running too
    d.token_pool = BTreeMap::from([(3, 125), (4, 125), (5, 125), (9, 124)]);
    let t = std::time::Instant::now();
    let r = check(&d);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(t.elapsed().as_secs() < limit, "validation took {:?}", t.elapsed());
}
