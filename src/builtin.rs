//! The built-in maps: Standard, Earth and USA.
//!
//! They are ordinary map files, `maps/builtin/standard.json` and `maps/builtin/earth.json`, compiled into
//! the program with `include_str!` so the binary needs nothing next to it. They are read by exactly the
//! same code as a map someone writes (`MapDef::from_json`), so a custom map is never second-class.
//!
//! The order of the hexes in a file decides how corners and sides are numbered, which is why the files
//! list them in a fixed order. Nothing depends on that except the numbering itself (a test pins it).

use crate::map_def::*;
use std::sync::OnceLock;

#[cfg(test)]
use crate::board::Terrain;
#[cfg(test)]
use std::collections::BTreeMap;

/// The map a table starts with, and the only one the game log doesn't announce.
pub const DEFAULT_MAP: &str = "standard";

/// Each built-in map's id and the text of its file. The id is what the lobby and the game log call it.
pub const BUILTIN_MAPS: [(&str, &str); 6] = [
    ("standard", include_str!("../maps/builtin/standard.json")),
    ("earth", include_str!("../maps/builtin/earth.json")),
    ("usa", include_str!("../maps/builtin/usa.json")),
    ("texas", include_str!("../maps/builtin/texas.json")),
    ("europe", include_str!("../maps/builtin/europe.json")),
    ("maritime southeast asia", include_str!("../maps/builtin/maritime_southeast_asia.json")),
];

/// The built-in maps, read once.
fn parsed() -> &'static [(&'static str, MapDef)] {
    static PARSED: OnceLock<Vec<(&'static str, MapDef)>> = OnceLock::new();
    PARSED.get_or_init(|| {
        BUILTIN_MAPS
            .iter()
            .map(|&(id, text)| {
                let def = MapDef::from_json(text).unwrap_or_else(|e| panic!("failed to load built-in map `{id}`: {e}"));
                (id, def)
            })
            .collect()
    })
}

/// A built-in map by id (`"standard"`, `"earth"`).
pub fn builtin(id: &str) -> Option<&'static MapDef> {
    parsed().iter().find(|(i, _)| *i == id).map(|(_, def)| def)
}

/// The ids of the built-in maps in lobby order, followed by `after`: what the lobby lists when `after` are
/// the maps that have been added since. Tests use it so that they don't each count the built-in maps.
#[cfg(test)]
pub fn ids_then<'a>(after: &[&'a str]) -> Vec<&'a str> {
    let mut ids: Vec<&'a str> = BUILTIN_MAPS.iter().map(|&(id, _)| id).collect();
    ids.extend_from_slice(after);
    ids
}

#[cfg(test)]
pub fn standard_def() -> MapDef {
    builtin("standard").expect("built in").clone()
}

#[cfg(test)]
pub fn earth_def() -> MapDef {
    builtin("earth").expect("built in").clone()
}

// ----------------------------------------------------------------------
// Maps for tests
// ----------------------------------------------------------------------

/// Seven hexes: a desert in the middle and six around it. Valid as it stands, with no harbours.
#[cfg(test)]
pub fn flower() -> MapDef {
    let mut hexes = vec![HexDef { q: 0, r: 0, terrain: Slot::Fixed(Terrain::Desert), number: Slot::Random }];
    for (q, r) in [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)] {
        hexes.push(HexDef::at(q, r));
    }
    MapDef {
        version: MAP_FORMAT_VERSION,
        name: "Flower".into(),
        author: String::new(),
        players: PlayerRange::default(),
        hexes,
        terrain_pool: BTreeMap::from([(Terrain::Forest, 2), (Terrain::Hills, 2), (Terrain::Fields, 1), (Terrain::Pasture, 1)]),
        token_pool: BTreeMap::from([(3, 1), (4, 1), (5, 1), (9, 1), (10, 1), (11, 1)]),
        ports: PortsDef::default(),
        robber_start: None,
        constraints: Constraints::default(),
        recommended_settings: None,
        style: MapStyle::default(),
    }
}

/// The example map in `examples/maps/archipelago.json`: written the way a person would write one.
#[cfg(test)]
pub fn archipelago() -> MapDef {
    MapDef::from_json(include_str!("../examples/maps/archipelago.json")).expect("the example map is valid JSON for a MapDef")
}
