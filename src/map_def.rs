//! The data description of a map (`MapDef`) and the errors that come from turning one into a board.
//!
//! A `MapDef` says *what* the map is: which hexes exist, which terrain and number tokens are fixed and
//! which are dealt at random, where the harbours go. `Board::from_def` (in `board.rs`) turns it into the
//! vertex/edge geometry the rules and the renderer use. Nothing in here knows about geometry.
//!
//! The two built-in maps are `MapDef`s too (see `builtin.rs`); custom maps will be the same type read
//! from JSON. Everything here is therefore written for **untrusted input**: unknown fields are
//! rejected, counts are checked before anything is allocated from them, and every problem is a
//! `MapError` rather than a panic.
//!
//! In JSON a value that is dealt at random is written `"?"`:
//!
//! ```json
//! {
//!   "version": 1,
//!   "name": "Tiny",
//!   "hexes": [ { "q": 0, "r": 0, "terrain": "desert" }, { "q": 1, "r": 0, "number": 6 }, { "q": 0, "r": 1 } ],
//!   "terrain_pool": { "forest": 1, "hills": 1 },
//!   "token_pool": { "5": 1 }
//! }
//! ```

use crate::board::Terrain;
use crate::types::Resource;
use crate::game::Settings;
use serde::de::{self, Deserializer, IntoDeserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};
use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

/// The only file format version this build reads.
pub const MAP_FORMAT_VERSION: u32 = 1;
/// Most hexes a map may have (Earth has 81). Keeps generation and rendering cheap.
pub const MAX_HEXES: usize = 250;
/// Hex coordinates must lie in `-MAX_COORD..=MAX_COORD`.
pub const MAX_COORD: i32 = 100;
/// Biggest map file that is read. Earth is about 3 KB as JSON.
pub const MAX_MAP_BYTES: usize = 256 * 1024;
/// Longest map `name`, in characters. The lobby lists maps as small buttons, so a long name breaks the layout.
pub const MAX_NAME_CHARS: usize = 32;
/// Longest map `author`, in characters (it is shown in a tooltip and a few other places).
pub const MAX_AUTHOR_CHARS: usize = 32;

// ----------------------------------------------------------------------
// Slot: a value that is either written down or dealt at random
// ----------------------------------------------------------------------

/// `Fixed(x)` is written as `x`; `Random` is written as the string `"?"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot<T> {
    Fixed(T),
    Random,
}

impl<T> Slot<T> {
    pub fn random() -> Self {
        Slot::Random
    }
    pub fn is_random(&self) -> bool {
        matches!(self, Slot::Random)
    }
}

impl<T: Serialize> Serialize for Slot<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Slot::Fixed(v) => v.serialize(s),
            Slot::Random => s.serialize_str("?"),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Slot<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct SlotVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for SlotVisitor<T> {
            type Value = Slot<T>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a value, or \"?\" for random")
            }
            // Delegating to `T` (rather than an untagged enum) keeps its own error messages,
            // e.g. "unknown variant `forst`, expected one of ...".
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Slot<T>, E> {
                if v == "?" {
                    Ok(Slot::Random)
                } else {
                    T::deserialize(v.into_deserializer()).map(Slot::Fixed)
                }
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Slot<T>, E> {
                T::deserialize(v.into_deserializer()).map(Slot::Fixed)
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Slot<T>, E> {
                T::deserialize(v.into_deserializer()).map(Slot::Fixed)
            }
        }
        d.deserialize_any(SlotVisitor(PhantomData))
    }
}

// ----------------------------------------------------------------------
// The pieces of a map
// ----------------------------------------------------------------------

/// What a harbour offers.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PortKind {
    /// 3:1 on anything.
    Any,
    Brick,
    Lumber,
    Ore,
    Grain,
    Wool,
}

impl PortKind {
    /// `None` = 3:1 any resource, `Some(r)` = 2:1 for resource `r` (as in `Port::resource`).
    pub fn resource(self) -> Option<Resource> {
        match self {
            PortKind::Any => None,
            PortKind::Brick => Some(Resource::Brick),
            PortKind::Lumber => Some(Resource::Lumber),
            PortKind::Ore => Some(Resource::Ore),
            PortKind::Grain => Some(Resource::Grain),
            PortKind::Wool => Some(Resource::Wool),
        }
    }
}

/// One land hex. Leave `terrain` / `number` out (or write `"?"`) to have them dealt from the pools.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HexDef {
    pub q: i32,
    pub r: i32,
    #[serde(default = "Slot::random", skip_serializing_if = "Slot::is_random")]
    pub terrain: Slot<Terrain>,
    /// A fixed token (2-12, not 7), or random. A desert never has one, whatever this says.
    #[serde(default = "Slot::random", skip_serializing_if = "Slot::is_random")]
    pub number: Slot<u8>,
}

impl HexDef {
    /// A hex whose terrain and number are both dealt at random.
    #[cfg(test)]
    pub fn at(q: i32, r: i32) -> HexDef {
        HexDef { q, r, terrain: Slot::Random, number: Slot::Random }
    }
}

/// A hex by axial coordinates.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HexRef {
    pub q: i32,
    pub r: i32,
}

/// A harbour on the coast edge of one hex. `side` counts round the hex from corner `side` to corner
/// `side + 1`, with corner 0 at the upper right, going clockwise: 0 east, 1 south-east, 2 south-west,
/// 3 west, 4 north-west, 5 north-east.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PortSpot {
    pub q: i32,
    pub r: i32,
    pub side: usize,
    #[serde(default = "Slot::random", skip_serializing_if = "Slot::is_random")]
    pub kind: Slot<PortKind>,
}

impl PortSpot {
    #[cfg(test)]
    pub fn at(q: i32, r: i32, side: usize) -> PortSpot {
        PortSpot { q, r, side, kind: Slot::Random }
    }
}

/// Where the harbours go. Spots whose kind is `"?"` draw from `pool`; the pool must hold exactly as
/// many harbours as there are such spots.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum PortsDef {
    /// Harbours at the listed spots.
    Fixed {
        #[serde(default)]
        spots: Vec<PortSpot>,
        #[serde(default)]
        pool: BTreeMap<PortKind, u32>,
    },
    /// The coast must be one closed ring (a single island). Harbours go on its edges at the given gaps
    /// (`spacing[i]` coast edges between harbour `i` and harbour `i + 1`, wrapping round, so they must
    /// add up to the length of the coast), starting at a random point.
    Ring {
        spacing: Vec<usize>,
        #[serde(default)]
        pool: BTreeMap<PortKind, u32>,
    },
}

impl Default for PortsDef {
    /// No harbours.
    fn default() -> Self {
        PortsDef::Fixed { spots: vec![], pool: BTreeMap::new() }
    }
}

/// The fewest and most players the engine can seat at one table.
pub const MIN_PLAYERS: usize = 2;
pub const MAX_PLAYERS: usize = 8;

/// How many players the map is meant for
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlayerRange {
    pub min: usize,
    pub max: usize,
}

impl Default for PlayerRange {
    fn default() -> Self {
        PlayerRange { min: 2, max: 4 }
    }
}

/// Optional rules about how the random parts are dealt.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Constraints {
    /// No two touching hexes share a number, and no 6/8 touches another 6/8 (default: on).
    pub spread_tokens: bool,
    /// Deserts dealt from the terrain pool never touch a desert written into the map.
    pub deserts_avoid_fixed_deserts: bool,
}

impl Default for Constraints {
    fn default() -> Self {
        Constraints { spread_tokens: true, deserts_avoid_fixed_deserts: false }
    }
}

/// Hints for how the browser draws the map. They never affect the rules or how the board is dealt; the
/// server passes them through to the browser as they are (`Board::style`), so the browser never has to
/// recognise a map by its name.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct MapStyle {
    /// Draw a faint hex grid across the open sea. Off by default.
    pub sea_grid: bool,
}

impl MapStyle {
    /// Used to leave `"style"` out of a map written back to JSON when it says nothing.
    fn is_default(&self) -> bool {
        *self == MapStyle::default()
    }
}

/// A whole map.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MapDef {
    /// Must be `MAP_FORMAT_VERSION`.
    pub version: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
    #[serde(default)]
    pub players: PlayerRange,
    pub hexes: Vec<HexDef>,
    /// Terrain dealt to the hexes whose terrain is `"?"`. Counts must add up to exactly that many
    /// hexes. May include `desert`.
    #[serde(default)]
    pub terrain_pool: BTreeMap<Terrain, u32>,
    /// Number tokens dealt to the producing hexes whose number is `"?"` (token value -> how many).
    /// Counts must add up to exactly that many hexes.
    #[serde(default)]
    pub token_pool: BTreeMap<u8, u32>,
    #[serde(default)]
    pub ports: PortsDef,
    /// Where the robber starts. Default: a random desert (a map with no desert must set this).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub robber_start: Option<HexRef>,
    #[serde(default)]
    pub constraints: Constraints,
    /// The full rule set this map suggests (fields left out are the standard defaults). The lobby may
    /// offer it to the host; it is never applied automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_settings: Option<Settings>,
    /// How the browser should draw the map, e.g. `{ "sea_grid": true }`. Purely cosmetic.
    #[serde(default, skip_serializing_if = "MapStyle::is_default")]
    pub style: MapStyle,
}

impl MapDef {
    /// Read a map from the text of a JSON file. Refuses files over `MAX_MAP_BYTES`, ignores a byte-order
    /// mark (Windows editors add one), and on a syntax or field error says which line and column.
    /// This only reads the file: `validate` says whether the map is any good.
    pub fn from_json(text: &str) -> Result<MapDef, String> {
        if text.len() > MAX_MAP_BYTES {
            return Err(format!("the file is {} bytes, but maps are limited to {} bytes", text.len(), MAX_MAP_BYTES));
        }
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        serde_json::from_str(text).map_err(|e| e.to_string())
    }
}

// ----------------------------------------------------------------------
// Errors
// ----------------------------------------------------------------------

/// Why a map could not be turned into a board (or a game on it could not be started).
/// `Display` is written for the person who made the map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapError {
    UnsupportedVersion { found: u32, supported: u32 },
    NoHexes,
    TooManyHexes { count: usize, max: usize },
    CoordOutOfRange { q: i32, r: i32 },
    DuplicateHex { q: i32, r: i32 },
    BadPlayerRange { min: usize, max: usize },
    /// The lobby has a number of players the map is not meant for.
    PlayerCount { players: usize, min: usize, max: usize },
    BadRecommendedSettings(String),
    /// The map has no name (or only spaces).
    EmptyName,
    /// `field` ("name" or "author") is longer than `max` characters.
    TextTooLong { field: &'static str, len: usize, max: usize },
    /// `field` has a line break, a control character or an invisible formatting character in it.
    BadCharacters { field: &'static str },
    TerrainPoolMismatch { random_hexes: usize, pool: u64 },
    TokenPoolMismatch { needed: usize, pool: u64 },
    /// `at` is the hex it was written on, or `None` when it is in `token_pool`.
    InvalidToken { value: u8, at: Option<(i32, i32)> },
    NumberOnDesert { q: i32, r: i32 },
    NotEnoughDesertSpots { wanted: usize, available: usize },
    /// `terrain_pool` has more volcanoes than there are hexes that can hold the 6 or 8 a volcano needs.
    NotEnoughVolcanoSpots { wanted: usize, available: usize },
    /// A volcano is always a 6 or 8, but this one touches another hex that is (or must be) a 6 or 8, and
    /// those may not touch (`constraints.spread_tokens`).
    VolcanoTouchesHot { q: i32, r: i32 },
    FixedTokensClash { a: (i32, i32), b: (i32, i32) },
    TokenPlacementFailed { attempts: usize },
    PortHexMissing { q: i32, r: i32 },
    PortSideInvalid { q: i32, r: i32, side: usize },
    PortNotOnCoast { q: i32, r: i32, side: usize },
    PortEdgeTaken { q: i32, r: i32, side: usize },
    PortPoolMismatch { random_spots: usize, pool: u64 },
    CoastNotOneRing { rings: usize },
    BadPortSpacing { sum: u64, coast: usize },
    RobberStartMissing { q: i32, r: i32 },
    NoRobberStart,
    /// Every hex is a desert, so nothing would ever be produced. (Found by `validate`, not needed to build.)
    NoProducingHexes,
    /// Even placed perfectly, the starting settlements don't fit under the distance rule.
    /// (Found by `validate`, not needed to build.)
    NotEnoughSettlementRoom { needed: usize, available: usize, players: usize, per_player: usize },
    /// A bug in the generator rather than in the map.
    Internal(&'static str),
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use MapError::*;
        match self {
            UnsupportedVersion { found, supported } => write!(f, "map format version {found} is not supported (this server reads version {supported})"),
            NoHexes => write!(f, "the map has no hexes"),
            TooManyHexes { count, max } => write!(f, "the map has {count} hexes, but the most allowed is {max}"),
            CoordOutOfRange { q, r } => write!(f, "hex ({q}, {r}) is outside the allowed coordinate range of -{MAX_COORD} to {MAX_COORD}"),
            DuplicateHex { q, r } => write!(f, "hex ({q}, {r}) is listed more than once"),
            BadPlayerRange { min, max } => write!(f, "the player range {min} to {max} is not allowed (it must lie within {MIN_PLAYERS} to {MAX_PLAYERS}, with min at most max)"),
            PlayerCount { players, min, max } => write!(f, "this map is for {min} to {max} players, but {players} are seated"),
            BadRecommendedSettings(why) => write!(f, "the map's recommended settings are invalid: {why}"),
            EmptyName => write!(f, "the map needs a name"),
            TextTooLong { field, len, max } => write!(f, "the {field} is {len} characters long, but the most allowed is {max}"),
            BadCharacters { field } => write!(f, "the {field} has a line break, control character or invisible character in it; use plain text"),
            TerrainPoolMismatch { random_hexes, pool } => write!(f, "{random_hexes} hexes need a random terrain, but terrain_pool holds {pool}"),
            TokenPoolMismatch { needed, pool } => write!(f, "{needed} hexes need a random number token, but token_pool holds {pool}"),
            InvalidToken { value, at: Some((q, r)) } => write!(f, "hex ({q}, {r}) has number {value}, but tokens are 2 to 12 except 7"),
            InvalidToken { value, at: None } => write!(f, "token_pool has number {value}, but tokens are 2 to 12 except 7"),
            NumberOnDesert { q, r } => write!(f, "hex ({q}, {r}) is a desert, so it cannot have a number"),
            NotEnoughDesertSpots { wanted, available } => write!(f, "terrain_pool has {wanted} deserts to place, but only {available} hexes can take one"),
            NotEnoughVolcanoSpots { wanted, available } => write!(
                f,
                "terrain_pool has {wanted} volcanoes to place, but only {available} hexes can take one (a volcano is always a 6 or 8, and those may not touch)"
            ),
            VolcanoTouchesHot { q, r } => write!(f, "hex ({q}, {r}) is a volcano, so it is always a 6 or 8, but it touches another 6 or 8 (turn off spread_tokens to allow that)"),
            FixedTokensClash { a, b } => write!(f, "touching hexes ({}, {}) and ({}, {}) have fixed numbers that are not allowed to touch", a.0, a.1, b.0, b.1),
            TokenPlacementFailed { attempts } => write!(f, "could not spread the number tokens out after {attempts} attempts; the token pool may be impossible on this layout"),
            PortHexMissing { q, r } => write!(f, "a harbour is on hex ({q}, {r}), which is not part of the map"),
            PortSideInvalid { q, r, side } => write!(f, "the harbour on hex ({q}, {r}) has side {side}, but sides are 0 to 5"),
            PortNotOnCoast { q, r, side } => write!(f, "the harbour on hex ({q}, {r}) side {side} is not on the coast"),
            PortEdgeTaken { q, r, side } => write!(f, "hex ({q}, {r}) side {side} has two harbours"),
            PortPoolMismatch { random_spots, pool } => write!(f, "{random_spots} harbours have a random kind, but the harbour pool holds {pool}"),
            CoastNotOneRing { rings } => write!(f, "ring harbour placement needs the coast to be one closed ring, but it has {rings}"),
            BadPortSpacing { sum, coast } => write!(f, "harbour spacing must be all at least 1 and add up to the coast length of {coast}, but adds up to {sum}"),
            RobberStartMissing { q, r } => write!(f, "robber_start ({q}, {r}) is not part of the map"),
            NoRobberStart => write!(f, "the map has no desert, so it must say where the robber starts (robber_start)"),
            NoProducingHexes => write!(f, "every hex is a desert, so no resources would ever be produced"),
            NotEnoughSettlementRoom { needed, available, players, per_player } => write!(
                f,
                "{players} players placing {per_player} settlements each need room for {needed}, but under the distance rule the map has room for only {available}"
            ),
            Internal(what) => write!(f, "internal error while building the map ({what})"),
        }
    }
}

impl std::error::Error for MapError {}

// ----------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<MapDef, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn slots_read_values_and_question_marks() {
        let s: Slot<Terrain> = serde_json::from_str("\"forest\"").unwrap();
        assert_eq!(s, Slot::Fixed(Terrain::Forest));
        let s: Slot<Terrain> = serde_json::from_str("\"?\"").unwrap();
        assert_eq!(s, Slot::Random);
        let s: Slot<u8> = serde_json::from_str("6").unwrap();
        assert_eq!(s, Slot::Fixed(6));
        let s: Slot<u8> = serde_json::from_str("\"?\"").unwrap();
        assert_eq!(s, Slot::Random);
        assert_eq!(serde_json::to_string(&Slot::<u8>::Random).unwrap(), "\"?\"");
        assert_eq!(serde_json::to_string(&Slot::Fixed(Terrain::Hills)).unwrap(), "\"hills\"");
    }

    #[test]
    fn slots_keep_helpful_error_messages() {
        let e = serde_json::from_str::<Slot<Terrain>>("\"forst\"").unwrap_err().to_string();
        assert!(e.contains("unknown variant") && e.contains("forest"), "{e}");
        let e = serde_json::from_str::<Slot<u8>>("300").unwrap_err().to_string();
        assert!(e.contains("300"), "{e}");
        assert!(serde_json::from_str::<Slot<u8>>("-1").is_err());
        assert!(serde_json::from_str::<Slot<u8>>("true").is_err());
        assert!(serde_json::from_str::<Slot<u8>>("\"six\"").is_err());
    }

    #[test]
    fn a_small_map_reads_with_defaults_filled_in() {
        let m = parse(
            r#"{
                "version": 1, "name": "Tiny",
                "hexes": [ {"q":0,"r":0,"terrain":"desert"}, {"q":1,"r":0,"number":6}, {"q":0,"r":1} ],
                "terrain_pool": {"forest":1,"hills":1},
                "token_pool": {"5":1}
            }"#,
        )
        .unwrap();
        assert_eq!(m.name, "Tiny");
        assert_eq!(m.author, "");
        assert_eq!(m.players, PlayerRange { min: 2, max: 4 });
        assert_eq!(m.hexes[0].terrain, Slot::Fixed(Terrain::Desert));
        assert_eq!(m.hexes[0].number, Slot::Random);
        assert_eq!(m.hexes[1].terrain, Slot::Random);
        assert_eq!(m.hexes[1].number, Slot::Fixed(6));
        assert_eq!(m.terrain_pool[&Terrain::Forest], 1);
        assert_eq!(m.token_pool[&5], 1);
        assert_eq!(m.ports, PortsDef::default());
        assert_eq!(m.constraints, Constraints { spread_tokens: true, deserts_avoid_fixed_deserts: false });
        assert_eq!(m.robber_start, None);
        assert_eq!(m.recommended_settings, None);
        assert_eq!(m.style, MapStyle { sea_grid: false });
    }

    #[test]
    fn the_style_hints_are_read_and_checked() {
        let m = parse(r#"{"version":1,"name":"x","hexes":[],"style":{"sea_grid":true}}"#).unwrap();
        assert!(m.style.sea_grid);
        // `style` may be written but say nothing, and any hint left out is off
        assert!(!parse(r#"{"version":1,"name":"x","hexes":[],"style":{}}"#).unwrap().style.sea_grid);
        // an unknown hint is a typo, not something to ignore
        assert!(parse(r#"{"version":1,"name":"x","hexes":[],"style":{"sea_gird":true}}"#).is_err());
        assert!(parse(r#"{"version":1,"name":"x","hexes":[],"style":{"sea_grid":"yes"}}"#).is_err());
        // and a map that doesn't ask for anything doesn't write a `style` back out
        let plain = serde_json::to_string(&parse(r#"{"version":1,"name":"x","hexes":[]}"#).unwrap()).unwrap();
        assert!(!plain.contains("style"), "{plain}");
        let styled = serde_json::to_string(&m).unwrap();
        assert!(styled.contains(r#""style":{"sea_grid":true}"#), "{styled}");
    }

    #[test]
    fn typos_and_junk_are_rejected_not_ignored() {
        // misspelt field names
        assert!(parse(r#"{"version":1,"name":"x","hexs":[]}"#).is_err());
        assert!(parse(r#"{"version":1,"name":"x","hexes":[{"q":0,"r":0,"terain":"forest"}]}"#).is_err());
        // wrong types and missing required fields
        assert!(parse(r#"{"version":1,"hexes":[]}"#).is_err());
        assert!(parse(r#"{"version":"1","name":"x","hexes":[]}"#).is_err());
        assert!(parse(r#"{"version":1,"name":"x","hexes":[{"q":0.5,"r":0}]}"#).is_err());
        assert!(parse("not json").is_err());
        assert!(parse("").is_err());
        // an unknown terrain name in the pool
        assert!(parse(r#"{"version":1,"name":"x","hexes":[],"terrain_pool":{"swamp":1}}"#).is_err());
        // an unknown harbour mode
        assert!(parse(r#"{"version":1,"name":"x","hexes":[],"ports":{"mode":"random"}}"#).is_err());
    }

    #[test]
    fn ports_read_in_both_modes() {
        let m = parse(
            r#"{"version":1,"name":"x","hexes":[],
                "ports":{"mode":"fixed","spots":[{"q":1,"r":2,"side":3},{"q":4,"r":5,"side":0,"kind":"ore"}],"pool":{"any":1}}}"#,
        )
        .unwrap();
        match m.ports {
            PortsDef::Fixed { spots, pool } => {
                assert_eq!(spots[0], PortSpot::at(1, 2, 3));
                assert_eq!(spots[1].kind, Slot::Fixed(PortKind::Ore));
                assert_eq!(pool[&PortKind::Any], 1);
            }
            other => panic!("{other:?}"),
        }
        let m = parse(r#"{"version":1,"name":"x","hexes":[],"ports":{"mode":"ring","spacing":[3,4],"pool":{"wool":2}}}"#).unwrap();
        assert_eq!(m.ports, PortsDef::Ring { spacing: vec![3, 4], pool: BTreeMap::from([(PortKind::Wool, 2)]) });
    }

    #[test]
    fn a_map_survives_a_round_trip_through_json() {
        let mut m = MapDef {
            version: MAP_FORMAT_VERSION,
            name: "Round trip".into(),
            author: "me".into(),
            players: PlayerRange { min: 3, max: 4 },
            hexes: vec![
                HexDef::at(0, 0),
                HexDef { q: 1, r: 0, terrain: Slot::Fixed(Terrain::Desert), number: Slot::Random },
                HexDef { q: 0, r: 1, terrain: Slot::Random, number: Slot::Fixed(9) },
            ],
            terrain_pool: BTreeMap::from([(Terrain::Forest, 1), (Terrain::Mountains, 1)]),
            token_pool: BTreeMap::from([(4, 1)]),
            ports: PortsDef::Fixed {
                spots: vec![PortSpot::at(0, 0, 4), PortSpot { q: 1, r: 0, side: 1, kind: Slot::Fixed(PortKind::Any) }],
                pool: BTreeMap::from([(PortKind::Grain, 1)]),
            },
            robber_start: Some(HexRef { q: 0, r: 1 }),
            constraints: Constraints { spread_tokens: false, deserts_avoid_fixed_deserts: true },
            recommended_settings: None,
            style: MapStyle { sea_grid: true },
        };
        let mut custom = Settings::default();
        custom.points_to_win = 12;
        m.recommended_settings = Some(custom);

        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<MapDef>(&json).unwrap(), m);
        // a hex with nothing fixed is written as just its coordinates
        assert!(json.contains(r#"{"q":0,"r":0}"#), "{json}");
    }

    #[test]
    fn messages_name_the_problem_and_where_it_is() {
        let msg = MapError::PortNotOnCoast { q: 3, r: 4, side: 2 }.to_string();
        assert!(msg.contains("(3, 4)") && msg.contains("side 2") && msg.contains("coast"), "{msg}");
        let msg = MapError::TerrainPoolMismatch { random_hexes: 81, pool: 80 }.to_string();
        assert!(msg.contains("81") && msg.contains("80"), "{msg}");
    }
}
