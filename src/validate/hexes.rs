//! Layer 1: can the hex list be used at all?

use crate::map_def::{MapDef, MapError, MAP_FORMAT_VERSION, MAX_COORD, MAX_HEXES};
use std::collections::HashSet;

/// Can the hex list be used at all? Empty means yes. (A wrong format version is reported alone: nothing
/// else in a file of another version can be trusted to mean what it says here.)
pub(crate) fn check_hexes(def: &MapDef) -> Vec<MapError> {
    if def.version != MAP_FORMAT_VERSION {
        return vec![MapError::UnsupportedVersion { found: def.version, supported: MAP_FORMAT_VERSION }];
    }
    if def.hexes.is_empty() {
        return vec![MapError::NoHexes];
    }
    if def.hexes.len() > MAX_HEXES {
        return vec![MapError::TooManyHexes { count: def.hexes.len(), max: MAX_HEXES }];
    }
    let mut errors = vec![];
    let mut seen = HashSet::new();
    // a range test, not `abs()`: i32::MIN.abs() overflows
    let in_range = |v: i32| (-MAX_COORD..=MAX_COORD).contains(&v);
    for h in &def.hexes {
        if !in_range(h.q) || !in_range(h.r) {
            errors.push(MapError::CoordOutOfRange { q: h.q, r: h.r });
        } else if !seen.insert((h.q, h.r)) {
            errors.push(MapError::DuplicateHex { q: h.q, r: h.r });
        }
    }
    errors
}
