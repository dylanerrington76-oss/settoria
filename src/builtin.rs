//! The built-in maps: Standard, Earth and USA.
//!
//! They are ordinary map files, `maps/builtin/standard.json` and `maps/builtin/earth.json`, compiled into
//! the program with `include_str!` so the binary needs nothing next to it. They are read by exactly the
//! same code as a map someone writes (`MapDef::from_json`), so a custom map is never second-class.
//!
//! * **Standard**: the base-game board (19 hexes, 18 number tokens, 9 harbours), dealt afresh every game.
//! * **Earth**: 81 hexes on 7 islands, 27 harbours, and a Sahara of three desert hexes that is always in
//!   the same place.
//! * **USA**: 144 hexes in the shape of the United States, one island, 25 fixed harbour spots whose kinds are
//!   dealt each game, and one desert dealt anywhere.
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
pub const BUILTIN_MAPS: [(&str, &str); 3] = [
    ("standard", include_str!("../maps/builtin/standard.json")),
    ("earth", include_str!("../maps/builtin/earth.json")),
    ("usa", include_str!("../maps/builtin/usa.json")),
];

/// The built-in maps, read once.
fn parsed() -> &'static [(&'static str, MapDef)] {
    static PARSED: OnceLock<Vec<(&'static str, MapDef)>> = OnceLock::new();
    PARSED.get_or_init(|| {
        BUILTIN_MAPS
            .iter()
            .map(|&(id, text)| {
                // The files are compiled in, and the tests read every one, so this can only fire for a
                // binary built from a broken file.
                let def = MapDef::from_json(text).unwrap_or_else(|e| panic!("the built-in map `{id}` is not valid: {e}"));
                (id, def)
            })
            .collect()
    })
}

/// A built-in map by id (`"standard"`, `"earth"`).
pub fn builtin(id: &str) -> Option<&'static MapDef> {
    parsed().iter().find(|(i, _)| *i == id).map(|(_, def)| def)
}

/// The standard map as a value to change in a test.
#[cfg(test)]
pub fn standard_def() -> MapDef {
    builtin("standard").expect("built in").clone()
}

/// The Earth map as a value to change in a test.
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
