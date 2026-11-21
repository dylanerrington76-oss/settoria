//! Tests of the validator, as a whole and by part. Nearly all of them go through `validate_with_rng`, so
//! they check what a map's author would see; the files mirror the layers and parts of the validator:
//!
//! - `layers`: the hex list, the rules and the content (layers 1 to 3), and that a report stays bounded
//! - `playability`: layer 4, errors and advice, and the trial deals
//! - `settlement_room`: the settlement-room figure, the matching behind it, and the islands
//! - `agreement`: the validator against the board generator, and against damaged files
//! - `cli`: the `validate` command
//!
//! What they share is here.

mod agreement;
mod cli;
mod layers;
mod playability;
mod settlement_room;

use super::report::{MapWarning, Report, MAX_ISSUES};
use super::validate_with_rng;
use crate::board::Terrain;
use crate::builtin::{archipelago, earth_def, flower, standard_def};
use crate::game::Settings;
use crate::map_def::*;
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::collections::BTreeMap;

fn check(def: &MapDef) -> Report {
    validate_with_rng(def, &Settings::default(), &mut StdRng::seed_from_u64(7))
}

/// A report must contain this error; the failure message shows everything it did contain.
#[track_caller]
fn assert_error(def: &MapDef, want: MapError) {
    let r = check(def);
    assert!(r.errors.contains(&want), "wanted {want:?} among {:?}", r.errors);
}

#[track_caller]
fn assert_only_error(def: &MapDef, want: MapError) {
    let r = check(def);
    assert_eq!(r.errors, vec![want], "wanted exactly one problem");
}

#[track_caller]
fn assert_warning(def: &MapDef, want: MapWarning) {
    let r = check(def);
    assert!(r.errors.is_empty(), "the map should be playable but has {:?}", r.errors);
    assert!(r.warnings.contains(&want), "wanted {want:?} among {:?}", r.warnings);
}

fn coords_of(def: &MapDef) -> Vec<(i32, i32)> {
    def.hexes.iter().map(|h| (h.q, h.r)).collect()
}

/// The standard map, changed by `f`.
fn standard_with(f: impl FnOnce(&mut MapDef)) -> MapDef {
    let mut d = standard_def();
    f(&mut d);
    d
}

fn with_ports(spots: Vec<PortSpot>, pool: &[(PortKind, u32)]) -> MapDef {
    let mut d = flower();
    d.ports = PortsDef::Fixed { spots, pool: pool.iter().copied().collect() };
    d
}

fn with_ring(spacing: Vec<usize>, any: u32) -> MapDef {
    let mut d = flower();
    d.ports = PortsDef::Ring { spacing, pool: BTreeMap::from([(PortKind::Any, any)]) };
    d
}

/// A map of a single forest hex, which is far too small for the starting settlements.
fn one_hex() -> MapDef {
    let mut d = flower();
    d.hexes = vec![HexDef { q: 0, r: 0, terrain: Slot::Fixed(Terrain::Forest), number: Slot::Fixed(5) }];
    d.terrain_pool.clear();
    d.token_pool.clear();
    d.robber_start = Some(HexRef { q: 0, r: 0 });
    d
}

// ---------------- the built-in maps and the example ----------------

#[test]
fn the_built_in_maps_are_clean() {
    for (name, def) in [("Standard", standard_def()), ("Earth", earth_def())] {
        let r = check(&def);
        assert!(r.errors.is_empty(), "{name}: {:?}", r.errors);
        assert!(r.warnings.is_empty(), "{name} has warnings: {:?}", r.warnings);
        assert!(!r.truncated);
    }
}

#[test]
fn the_example_map_is_clean_and_described_correctly() {
    let r = check(&archipelago());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    let s = r.stats.unwrap();
    assert_eq!((s.hexes, s.islands, s.harbours), (14, 2, 7));
    assert_eq!(s.producing_hexes, 12); // a written-in desert, and one dealt from the pool
}

#[test]
fn stats_describe_the_map() {
    let s = check(&earth_def()).stats.unwrap();
    assert_eq!((s.hexes, s.islands, s.producing_hexes, s.harbours), (81, 7, 76, 27));
    assert_eq!(s.terrain[&Terrain::Desert], 5);
    assert_eq!(s.terrain[&Terrain::Hills], 16);
    assert_eq!(s.terrain.values().sum::<usize>(), 81);
    assert!(s.settlement_room > 80);

    let s = check(&standard_def()).stats.unwrap();
    assert_eq!((s.hexes, s.islands, s.producing_hexes, s.harbours), (19, 1, 18, 9));
}
