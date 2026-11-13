//! Is this map any good? Every problem in one pass, and a command-line tool to ask.
//!
//! Checking happens in layers, and each layer only runs if the one before it can be trusted:
//!
//! 1. **The hex list** (`check_hexes`): format version, size, coordinates, duplicates. Nothing else can be
//!    checked until the hexes themselves make sense.
//! 2. **The rules** (`check_rules`): name and author, player range and recommended settings. Independent of
//!    the hexes.
//! 3. **The content** (`check_content`): pools against hex counts, tokens, harbours, robber start.
//!    These need the map's geometry (which side of which hex is coast), built by `Geometry::build`.
//! 4. **Playability**, only once nothing above found a problem: is there room for the starting
//!    settlements, does anything produce, can the tokens actually be dealt?
//!
//! Layers 1 to 3 are the *only* place these rules live. `Board::from_def` calls the same functions and
//! refuses a map with the first problem they find, so the generator and the validator cannot drift apart.
//! Layer 4 is advice about whether the map is worth playing, so it is not needed to build a board.
//!
//! Problems come in two kinds: `MapError`s, which make the map unusable, and `MapWarning`s, which are
//! worth knowing about but leave it playable.

use crate::board::{
    coast_rings, desert_spots, dry_deal, hex_distance, is_hot, pool_total, port_edge, tokens_clash, valid_token, Geometry, Terrain,
    NEIGHBOR_STEPS,
};
use crate::game::Settings;
use crate::map_def::*;
use rand::Rng;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::io::Write;

// ----------------------------------------------------------------------
// What a validation produces
// ----------------------------------------------------------------------

/// Worth knowing about, but the map can still be played.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapWarning {
    /// Setup would fit if everyone placed perfectly, but not with room to spare, so people placing
    /// carelessly could run out of corners before everyone has their settlements.
    TightSettlementRoom { needed: usize, available: usize },
    /// No hex of this terrain exists, so the resource is only available by trading with the bank.
    MissingResource { terrain: Terrain },
    /// One resource is far scarcer than another.
    LopsidedResources { scarce: Terrain, scarce_hexes: usize, plentiful: Terrain, plentiful_hexes: usize },
    /// A whole island of nothing but desert, near this hex.
    IslandWithoutResources { near: (i32, i32), hexes: usize },
    NoHarbours,
    /// Two harbours meet at a corner, near this hex (so one settlement there uses both, and their
    /// pictures overlap).
    HarboursShareCorner { near: (i32, i32) },
    /// Some trial deals of the number tokens failed: starting a game on this map sometimes fails and
    /// has to be tried again.
    DealOftenFails { failed: usize, trials: usize },
}

impl fmt::Display for MapWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use MapWarning::*;
        match self {
            TightSettlementRoom { needed, available } => write!(
                f,
                "room for {available} settlements, and setup needs {needed}: it fits if everyone places well, but careless placement could run out of room"
            ),
            MissingResource { terrain } => write!(f, "no {} hexes, so that resource can only be had by trading with the bank", terrain.name()),
            LopsidedResources { scarce, scarce_hexes, plentiful, plentiful_hexes } => write!(
                f,
                "{} is scarce ({scarce_hexes} hexes) next to {} ({plentiful_hexes} hexes)",
                scarce.name(),
                plentiful.name()
            ),
            IslandWithoutResources { near, hexes } => write!(f, "the island near ({}, {}) has {hexes} hexes and all of them are desert", near.0, near.1),
            NoHarbours => write!(f, "the map has no harbours, so players can only trade 4:1 with the bank"),
            HarboursShareCorner { near } => write!(f, "two harbours meet at a corner near hex ({}, {})", near.0, near.1),
            DealOftenFails { failed, trials } => write!(
                f,
                "the number tokens could not be spread out in {failed} of {trials} trial deals, so starting a game here will sometimes fail"
            ),
        }
    }
}

/// Facts about the map, for a summary line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapStats {
    pub hexes: usize,
    pub islands: usize,
    /// Hexes that are not deserts.
    pub producing_hexes: usize,
    pub harbours: usize,
    /// The most settlements that fit under the distance rule.
    pub settlement_room: usize,
    /// How many hexes of each terrain the map ends up with (what is written in, plus the terrain pool).
    pub terrain: BTreeMap<Terrain, usize>,
}

/// Everything `validate` found. The map is playable if `errors` is empty.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Report {
    pub errors: Vec<MapError>,
    pub warnings: Vec<MapWarning>,
    /// Only there when the hex list made sense.
    pub stats: Option<MapStats>,
    /// There were more problems than `MAX_ISSUES`; the rest were dropped.
    pub truncated: bool,
}

impl Report {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// The most errors (and the most warnings) a report holds, so a hopeless file can't make a huge report.
pub const MAX_ISSUES: usize = 100;

// ----------------------------------------------------------------------
// Layer 1: the hex list
// ----------------------------------------------------------------------

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

// ----------------------------------------------------------------------
// Layer 2: the rules
// ----------------------------------------------------------------------

/// A character that has no business in a map's name or author: control characters (line breaks, tabs,
/// escapes) and the invisible formatting ones (zero-width spaces, left-to-right/right-to-left marks and
/// overrides, the byte-order mark), which can reorder or hide text in the lobby.
fn is_layout_breaking(c: char) -> bool {
    c.is_control() || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

/// Check a label shown in the lobby. Spaces around it don't count (the registry stores it trimmed).
fn check_label(field: &'static str, text: &str, max: usize, errors: &mut Vec<MapError>) {
    let text = text.trim();
    let len = text.chars().count();
    if len > max {
        errors.push(MapError::TextTooLong { field, len, max });
    }
    if text.chars().any(is_layout_breaking) {
        errors.push(MapError::BadCharacters { field });
    }
}

/// The name and author, the player range and the recommended settings, which don't depend on the hexes.
pub(crate) fn check_rules(def: &MapDef) -> Vec<MapError> {
    let mut errors = vec![];
    if def.name.trim().is_empty() {
        errors.push(MapError::EmptyName);
    }
    check_label("name", &def.name, MAX_NAME_CHARS, &mut errors);
    check_label("author", &def.author, MAX_AUTHOR_CHARS, &mut errors);
    let PlayerRange { min, max } = def.players;
    if min < MIN_PLAYERS || max > MAX_PLAYERS || min > max {
        errors.push(MapError::BadPlayerRange { min, max });
    }
    if let Some(s) = &def.recommended_settings {
        if let Err(why) = s.validate() {
            errors.push(MapError::BadRecommendedSettings(why));
        }
    }
    errors
}

// ----------------------------------------------------------------------
// Layer 3: the content
// ----------------------------------------------------------------------

/// Everything that is wrong with the terrain, tokens, harbours and robber start, given a usable hex list.
/// Independent problems are all reported; one problem is only reported once, not again for each thing
/// that follows from it (a terrain pool of the wrong size doesn't also get its tokens counted, because
/// how many tokens are needed depends on how many deserts get dealt).
pub(crate) fn check_content(def: &MapDef, geo: &Geometry) -> Vec<MapError> {
    let mut errors = vec![];
    let hexes = &def.hexes;
    let coords = &geo.coords;
    let n = hexes.len();
    let is_fixed_desert = |i: usize| hexes[i].terrain == Slot::Fixed(Terrain::Desert);

    // --- terrain ---------------------------------------------------------------
    let random_terrain = hexes.iter().filter(|h| h.terrain.is_random()).count();
    let terrain_total = pool_total(&def.terrain_pool);
    let terrain_pool_fits = terrain_total == random_terrain as u64;
    if !terrain_pool_fits {
        errors.push(MapError::TerrainPoolMismatch { random_hexes: random_terrain, pool: terrain_total });
    }
    let deserts_wanted = def.terrain_pool.get(&Terrain::Desert).copied().unwrap_or(0) as usize;
    if terrain_pool_fits {
        let available = desert_spots(def, coords).len();
        if available < deserts_wanted {
            errors.push(MapError::NotEnoughDesertSpots { wanted: deserts_wanted, available });
        }
    }

    // --- number tokens -----------------------------------------------------------
    for (i, h) in hexes.iter().enumerate() {
        if let Slot::Fixed(v) = h.number {
            if !valid_token(v) {
                errors.push(MapError::InvalidToken { value: v, at: Some(coords[i]) });
            } else if is_fixed_desert(i) {
                errors.push(MapError::NumberOnDesert { q: h.q, r: h.r });
            }
        }
    }
    if terrain_pool_fits {
        // Producing hexes still waiting for a token: those with a fixed terrain, plus the random-terrain
        // ones less the ones that turn out to be deserts (deserts only go where no number is written).
        let waiting_fixed = hexes.iter().filter(|h| h.number.is_random() && matches!(h.terrain, Slot::Fixed(t) if t != Terrain::Desert)).count();
        let waiting_random = hexes.iter().filter(|h| h.number.is_random() && h.terrain.is_random()).count();
        let needed = waiting_fixed + waiting_random - deserts_wanted.min(waiting_random);
        let pool = pool_total(&def.token_pool);
        if pool != needed as u64 {
            errors.push(MapError::TokenPoolMismatch { needed, pool });
        }
    }
    for &v in def.token_pool.keys() {
        if !valid_token(v) {
            errors.push(MapError::InvalidToken { value: v, at: None });
        }
    }
    if def.constraints.spread_tokens {
        let fixed: Vec<Option<u8>> = (0..n)
            .map(|i| match hexes[i].number {
                Slot::Fixed(v) if valid_token(v) && !is_fixed_desert(i) => Some(v),
                _ => None,
            })
            .collect();
        for i in 0..n {
            for j in i + 1..n {
                if let (Some(a), Some(b)) = (fixed[i], fixed[j]) {
                    if hex_distance(coords[i], coords[j]) == 1 && tokens_clash(a, b) {
                        errors.push(MapError::FixedTokensClash { a: coords[i], b: coords[j] });
                    }
                }
            }
        }
        // A volcano written into the map is always a 6 or 8, whatever number it is given, so it clashes with
        // any 6 or 8 it touches. (Two fixed 6/8s that touch are already reported just above.)
        let is_volcano = |i: usize| hexes[i].terrain == Slot::Fixed(Terrain::Volcano);
        let written_hot = |i: usize| is_hot(fixed[i]);
        let will_be_hot = |i: usize| is_volcano(i) || written_hot(i);
        for i in 0..n {
            for j in i + 1..n {
                if hex_distance(coords[i], coords[j]) != 1 || !will_be_hot(i) || !will_be_hot(j) || (written_hot(i) && written_hot(j)) {
                    continue;
                }
                // The one of the two whose own number is not already a 6 or 8 is the one that has to change.
                let culprit = if written_hot(i) { j } else { i };
                let e = MapError::VolcanoTouchesHot { q: hexes[culprit].q, r: hexes[culprit].r };
                if !errors.contains(&e) {
                    errors.push(e);
                }
            }
        }
    }

    // --- harbours ------------------------------------------------------------------
    match &def.ports {
        PortsDef::Fixed { spots, pool } => {
            let mut used = HashSet::new();
            for s in spots {
                match port_edge(geo, s) {
                    Ok(edge) => {
                        if !used.insert(edge) {
                            errors.push(MapError::PortEdgeTaken { q: s.q, r: s.r, side: s.side });
                        }
                    }
                    Err(e) => errors.push(e),
                }
            }
            let random_spots = spots.iter().filter(|s| s.kind.is_random()).count();
            let total = pool_total(pool);
            if total != random_spots as u64 {
                errors.push(MapError::PortPoolMismatch { random_spots, pool: total });
            }
        }
        PortsDef::Ring { spacing, pool } => {
            let rings = coast_rings(geo);
            if rings.len() != 1 {
                errors.push(MapError::CoastNotOneRing { rings: rings.len() });
            } else {
                // saturating: a hostile file can't wrap the sum round to the coast length
                let sum = spacing.iter().fold(0u64, |total, &s| total.saturating_add(s as u64));
                if spacing.contains(&0) || sum != rings[0].len() as u64 {
                    errors.push(MapError::BadPortSpacing { sum, coast: rings[0].len() });
                }
            }
            let total = pool_total(pool);
            if total != spacing.len() as u64 {
                errors.push(MapError::PortPoolMismatch { random_spots: spacing.len(), pool: total });
            }
        }
    }

    // --- the robber --------------------------------------------------------------------
    match def.robber_start {
        Some(HexRef { q, r }) => {
            if !geo.at.contains_key(&(q, r)) {
                errors.push(MapError::RobberStartMissing { q, r });
            }
        }
        None => {
            let any_desert = (0..n).any(is_fixed_desert) || deserts_wanted > 0;
            if !any_desert {
                errors.push(MapError::NoRobberStart);
            }
        }
    }
    errors
}

// ----------------------------------------------------------------------
// Layer 4: is it worth playing?
// ----------------------------------------------------------------------

/// The most settlements that fit on the map under the distance rule (no two on corners joined by an edge):
/// the size of the largest set of corners, no two of them neighbours.
///
/// That is a hard problem in general, but the corners of a hex map form a bipartite graph (a honeycomb
/// has two kinds of corner, and every edge joins one of each), and for those the answer is exact and
/// quick: all the corners, less the size of a maximum matching (Konig's theorem). `None` only if the
/// graph somehow isn't bipartite, which can't happen for hexes.
pub(crate) fn settlement_room(geo: &Geometry) -> Option<usize> {
    let nv = geo.vertices.len();
    // Two-colour the corners.
    let mut side = vec![u8::MAX; nv];
    let mut queue = VecDeque::new();
    for start in 0..nv {
        if side[start] != u8::MAX {
            continue;
        }
        side[start] = 0;
        queue.push_back(start);
        while let Some(v) = queue.pop_front() {
            for &w in &geo.vertices[v].neighbors {
                if side[w] == u8::MAX {
                    side[w] = 1 - side[v];
                    queue.push_back(w);
                } else if side[w] == side[v] {
                    return None;
                }
            }
        }
    }
    // Maximum matching by augmenting paths, from the corners coloured 0.
    fn augment(l: usize, geo: &Geometry, owner: &mut [usize], stamp: &mut [usize], round: usize) -> bool {
        for &r in &geo.vertices[l].neighbors {
            if stamp[r] == round {
                continue;
            }
            stamp[r] = round;
            if owner[r] == usize::MAX || augment(owner[r], geo, owner, stamp, round) {
                owner[r] = l;
                return true;
            }
        }
        false
    }
    let mut owner = vec![usize::MAX; nv];
    let mut stamp = vec![0usize; nv];
    let mut matched = 0;
    for (round, l) in (0..nv).filter(|&v| side[v] == 0).enumerate() {
        if augment(l, geo, &mut owner, &mut stamp, round + 1) {
            matched += 1;
        }
    }
    Some(nv - matched)
}

/// Groups of hexes joined by shared edges, as lists of hex indices.
fn islands(geo: &Geometry) -> Vec<Vec<usize>> {
    let mut seen = vec![false; geo.coords.len()];
    let mut out = vec![];
    for start in 0..geo.coords.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![start];
        let mut island = vec![];
        while let Some(h) = stack.pop() {
            island.push(h);
            for (dq, dr) in NEIGHBOR_STEPS {
                if let Some(&o) = geo.at.get(&(geo.coords[h].0 + dq, geo.coords[h].1 + dr)) {
                    if !seen[o] {
                        seen[o] = true;
                        stack.push(o);
                    }
                }
            }
        }
        out.push(island);
    }
    out
}

/// If the map has room for at least this many times the settlements setup needs, setup can finish
/// however carelessly people place, and `validate` says nothing about it.
///
/// Measured, not guessed. The worst any order of legal placements can do (settle so that as few corners
/// as possible get used up, then run out of room) is computed exactly in the
/// `worst_case_placement_still_fits_when_there_is_slack` test, for a dozen map shapes: it is never below
/// half the best possible packing, and exactly half on the flower and on a ring of hexes round a lake.
/// So with 2x the needed room even the worst placements leave enough. (Random placement does much better:
/// about 80% of the best packing on average, and at least half in every shape tried.)
const ROOM_SLACK: usize = 2;
/// Trial deals `validate` makes of the terrain and tokens.
const DRY_RUN_TRIALS: usize = 10;
/// If this many trial deals in a row fail from the start, the map is hopeless and the rest are skipped.
const HOPELESS_AFTER: usize = 3;

/// How the trial deals went.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DryRun {
    Reliable,
    /// The first few deals all failed: the tokens can't be spread out on this map.
    Hopeless,
    Flaky { failed: usize, trials: usize },
    /// A deal failed for a reason other than bad luck.
    Broken(MapError),
}

/// Run `trial` up to `DRY_RUN_TRIALS` times and say how reliable it is.
pub(crate) fn dry_run(mut trial: impl FnMut() -> Result<(), MapError>) -> DryRun {
    let mut failed = 0;
    for done in 1..=DRY_RUN_TRIALS {
        match trial() {
            Ok(()) => {}
            Err(MapError::TokenPlacementFailed { .. }) => {
                failed += 1;
                if done == HOPELESS_AFTER && failed == HOPELESS_AFTER {
                    return DryRun::Hopeless;
                }
            }
            Err(other) => return DryRun::Broken(other),
        }
    }
    if failed == 0 {
        DryRun::Reliable
    } else {
        DryRun::Flaky { failed, trials: DRY_RUN_TRIALS }
    }
}

fn terrain_counts(def: &MapDef) -> BTreeMap<Terrain, usize> {
    let mut counts = BTreeMap::new();
    for h in &def.hexes {
        if let Slot::Fixed(t) = h.terrain {
            *counts.entry(t).or_insert(0) += 1;
        }
    }
    for (&t, &c) in &def.terrain_pool {
        *counts.entry(t).or_insert(0) += c as usize;
    }
    counts
}

fn harbour_count(def: &MapDef) -> usize {
    match &def.ports {
        PortsDef::Fixed { spots, .. } => spots.len(),
        PortsDef::Ring { spacing, .. } => spacing.len(),
    }
}

/// The coast edge of each harbour. For a ring the starting point is random in a real game, but which
/// harbours meet at a corner doesn't depend on it, so start at 0.
fn harbour_edges(def: &MapDef, geo: &Geometry) -> Vec<usize> {
    match &def.ports {
        PortsDef::Fixed { spots, .. } => spots.iter().filter_map(|s| port_edge(geo, s).ok()).collect(),
        PortsDef::Ring { spacing, .. } => {
            let Some(ring) = coast_rings(geo).into_iter().next() else { return vec![] };
            let mut pos = 0usize;
            let mut edges = vec![];
            for &gap in spacing {
                if let Some(&e) = ring.get(pos) {
                    edges.push(e);
                }
                pos = pos.saturating_add(gap);
            }
            edges
        }
    }
}

fn quality_warnings(def: &MapDef, geo: &Geometry, stats: &MapStats, needed: usize) -> Vec<MapWarning> {
    let mut warnings = vec![];

    if stats.settlement_room >= needed && stats.settlement_room < needed * ROOM_SLACK {
        warnings.push(MapWarning::TightSettlementRoom { needed, available: stats.settlement_room });
    }

    // Resources: how many hexes of each of the five producing terrains.
    let resources = [Terrain::Forest, Terrain::Hills, Terrain::Mountains, Terrain::Fields, Terrain::Pasture];
    let count = |t: Terrain| stats.terrain.get(&t).copied().unwrap_or(0);
    let missing: Vec<Terrain> = resources.iter().copied().filter(|&t| count(t) == 0).collect();
    for &terrain in &missing {
        warnings.push(MapWarning::MissingResource { terrain });
    }
    if missing.is_empty() {
        let scarce = *resources.iter().min_by_key(|&&t| count(t)).expect("five resources");
        let plentiful = *resources.iter().max_by_key(|&&t| count(t)).expect("five resources");
        if count(scarce) * 3 < count(plentiful) {
            warnings.push(MapWarning::LopsidedResources {
                scarce,
                scarce_hexes: count(scarce),
                plentiful,
                plentiful_hexes: count(plentiful),
            });
        }
    }

    // An island that is certainly all desert (only terrain written into the map is certain).
    for island in islands(geo) {
        if island.iter().all(|&i| def.hexes[i].terrain == Slot::Fixed(Terrain::Desert)) {
            warnings.push(MapWarning::IslandWithoutResources { near: geo.coords[island[0]], hexes: island.len() });
        }
    }

    if stats.harbours == 0 {
        warnings.push(MapWarning::NoHarbours);
    }
    let mut at_corner: HashMap<usize, usize> = HashMap::new();
    for e in harbour_edges(def, geo) {
        for v in [geo.edges[e].a, geo.edges[e].b] {
            *at_corner.entry(v).or_insert(0) += 1;
        }
    }
    let mut shared: Vec<usize> = at_corner.into_iter().filter(|&(_, c)| c >= 2).map(|(v, _)| v).collect();
    shared.sort();
    for v in shared {
        warnings.push(MapWarning::HarboursShareCorner { near: geo.coords[geo.vertices[v].hexes[0]] });
    }
    warnings
}

// ----------------------------------------------------------------------
// validate
// ----------------------------------------------------------------------

/// Check a map, and say everything that is wrong or worth knowing. `settings` are the rules it would be
/// played with (they decide how many starting settlements must fit).
pub fn validate(def: &MapDef, settings: &Settings) -> Report {
    validate_with_rng(def, settings, &mut rand::thread_rng())
}

/// `validate` with the randomness (for the trial deals) supplied, so a test can repeat a run.
pub fn validate_with_rng<R: Rng>(def: &MapDef, settings: &Settings, rng: &mut R) -> Report {
    let mut errors = check_hexes(def);
    let hexes_usable = errors.is_empty();
    errors.extend(check_rules(def));
    let mut warnings = vec![];
    let mut stats = None;

    if hexes_usable {
        let geo = Geometry::build(def.hexes.iter().map(|h| (h.q, h.r)).collect());
        errors.extend(check_content(def, &geo));
        let room = settlement_room(&geo);
        let terrain = terrain_counts(def);
        let deserts = terrain.get(&Terrain::Desert).copied().unwrap_or(0);
        let s = MapStats {
            hexes: def.hexes.len(),
            islands: islands(&geo).len(),
            producing_hexes: def.hexes.len().saturating_sub(deserts),
            harbours: harbour_count(def),
            settlement_room: room.unwrap_or(0),
            terrain,
        };

        // Playability is only worth asking about a map that is otherwise sound.
        if errors.is_empty() {
            let needed = def.players.max * settings.starting_settlements as usize;
            if s.producing_hexes == 0 {
                errors.push(MapError::NoProducingHexes);
            }
            match room {
                None => errors.push(MapError::Internal("the corners of the map are not a bipartite graph")),
                Some(available) if available < needed => errors.push(MapError::NotEnoughSettlementRoom {
                    needed,
                    available,
                    players: def.players.max,
                    per_player: settings.starting_settlements as usize,
                }),
                Some(_) => {}
            }
            if errors.is_empty() {
                warnings = quality_warnings(def, &geo, &s, needed);
                match dry_run(|| dry_deal(def, rng)) {
                    DryRun::Reliable => {}
                    DryRun::Hopeless => errors.push(MapError::TokenPlacementFailed { attempts: HOPELESS_AFTER }),
                    DryRun::Flaky { failed, trials } => warnings.push(MapWarning::DealOftenFails { failed, trials }),
                    DryRun::Broken(e) => errors.push(e),
                }
            }
        }
        stats = Some(s);
    }

    let truncated = errors.len() > MAX_ISSUES || warnings.len() > MAX_ISSUES;
    errors.truncate(MAX_ISSUES);
    warnings.truncate(MAX_ISSUES);
    Report { errors, warnings, stats, truncated }
}

// ----------------------------------------------------------------------
// The command line: `catan-lan validate map.json ...`
// ----------------------------------------------------------------------

const USAGE: &str = "usage: catan-lan validate <map.json> [more.json ...]

Checks map files and says everything that is wrong with them (errors) or worth knowing (warnings).
Exit status: 0 if every map is playable, 1 if any has errors, 2 if a file can't be read or parsed.";

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Write one map's report.
fn print_report(out: &mut dyn Write, path: &str, def: &MapDef, report: &Report) -> std::io::Result<()> {
    let by = if def.author.is_empty() { String::new() } else { format!(" by {}", def.author) };
    writeln!(out, "{path}: \"{}\"{by}", def.name)?;
    if let Some(s) = &report.stats {
        writeln!(
            out,
            "  {} on {} ({} producing), {}, room for {}",
            plural(s.hexes, "hex", "hexes"),
            plural(s.islands, "island", "islands"),
            s.producing_hexes,
            plural(s.harbours, "harbour", "harbours"),
            plural(s.settlement_room, "settlement", "settlements")
        )?;
        let terrain: Vec<String> = s.terrain.iter().map(|(t, c)| format!("{} {c}", t.name())).collect();
        writeln!(out, "  terrain: {}", terrain.join(", "))?;
    }
    for e in &report.errors {
        writeln!(out, "  error: {e}")?;
    }
    for w in &report.warnings {
        writeln!(out, "  warning: {w}")?;
    }
    if report.truncated {
        writeln!(out, "  (there were more problems than are shown)")?;
    }
    let (e, w) = (report.errors.len(), report.warnings.len());
    if report.is_ok() {
        writeln!(out, "  OK{}", if w > 0 { format!(", {}", plural(w, "warning", "warnings")) } else { String::new() })
    } else {
        writeln!(out, "  FAILED: {}{}", plural(e, "error", "errors"), if w > 0 { format!(", {}", plural(w, "warning", "warnings")) } else { String::new() })
    }
}

/// Run the `validate` command. Reports go to `out`; problems with the files themselves and usage
/// go to `err`. Returns the exit status.
pub fn run_cli(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        let _ = writeln!(err, "{USAGE}");
        return 2;
    }
    let mut status = 0;
    for path in args {
        let text = match std::fs::metadata(path) {
            Ok(meta) if meta.len() > MAX_MAP_BYTES as u64 => {
                let _ = writeln!(err, "{path}: the file is {} bytes, but maps are limited to {MAX_MAP_BYTES} bytes", meta.len());
                status = status.max(2);
                continue;
            }
            Ok(_) => match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(e) => {
                    let _ = writeln!(err, "{path}: can't read the file: {e}");
                    status = status.max(2);
                    continue;
                }
            },
            Err(e) => {
                let _ = writeln!(err, "{path}: can't read the file: {e}");
                status = status.max(2);
                continue;
            }
        };
        let def = match MapDef::from_json(&text) {
            Ok(d) => d,
            Err(e) => {
                let _ = writeln!(err, "{path}: not a valid map file: {e}");
                status = status.max(2);
                continue;
            }
        };
        let settings = def.recommended_settings.unwrap_or_default();
        let report = validate(&def, &settings);
        let _ = print_report(out, path, &def, &report);
        if !report.is_ok() {
            status = status.max(1);
        }
    }
    status
}

// ----------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::builtin::{archipelago, earth_def, flower, standard_def};
    use rand::rngs::StdRng;
    use rand::seq::SliceRandom;
    use rand::SeedableRng;

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

    fn with_ports(spots: Vec<PortSpot>, pool: &[(PortKind, u32)]) -> MapDef {
        let mut d = flower();
        d.ports = PortsDef::Fixed { spots, pool: pool.iter().copied().collect() };
        d
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

    fn with_ring(spacing: Vec<usize>, any: u32) -> MapDef {
        let mut d = flower();
        d.ports = PortsDef::Ring { spacing, pool: BTreeMap::from([(PortKind::Any, any)]) };
        d
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

    fn one_hex() -> MapDef {
        let mut d = flower();
        d.hexes = vec![HexDef { q: 0, r: 0, terrain: Slot::Fixed(Terrain::Forest), number: Slot::Fixed(5) }];
        d.terrain_pool.clear();
        d.token_pool.clear();
        d.robber_start = Some(HexRef { q: 0, r: 0 });
        d
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
        d.hexes = (0..16).flat_map(|r| (0..16).map(move |q| HexDef::at(q - r / 2, r))).take(MAX_HEXES).collect();
        d.terrain_pool = BTreeMap::from([(Terrain::Forest, MAX_HEXES as u32 - 1), (Terrain::Desert, 1)]);
        d.token_pool = BTreeMap::from([(5, 125), (9, MAX_HEXES as u32 - 126)]);
        let t = std::time::Instant::now();
        let r = check(&d);
        assert!(r.errors.contains(&MapError::TokenPlacementFailed { attempts: HOPELESS_AFTER }), "{:?}", r.errors);
        let limit = if cfg!(debug_assertions) { 90 } else { 5 };
        assert!(t.elapsed().as_secs() < limit, "validation took {:?}", t.elapsed());

        // and one that can be dealt, with every other check running too
        d.token_pool = BTreeMap::from([(3, 63), (4, 62), (5, 62), (9, 62)]);
        let t = std::time::Instant::now();
        let r = check(&d);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert!(t.elapsed().as_secs() < limit, "validation took {:?}", t.elapsed());
    }

    // ---------------- the settlement-room figure ----------------

    /// The most corners, no two neighbours, found the slow obvious way.
    fn brute_force_room(adj: &[u64], available: u64) -> usize {
        if available == 0 {
            return 0;
        }
        let v = available.trailing_zeros() as usize;
        let without = brute_force_room(adj, available & !(1 << v));
        let with = 1 + brute_force_room(adj, available & !(adj[v] | (1 << v)));
        without.max(with)
    }

    /// The fewest settlements after which nothing more fits (a minimum independent dominating set):
    /// the worst any order of legal placements can do.
    fn worst_case_fill(adj: &[u64], undominated: u64, best: &mut usize, size: usize) {
        if size >= *best {
            return;
        }
        if undominated == 0 {
            *best = size;
            return;
        }
        // the lowest corner still free must end up covered: by itself or by a neighbour being settled
        let v = undominated.trailing_zeros() as usize;
        let mut candidates = adj[v] | (1 << v);
        while candidates != 0 {
            let u = candidates.trailing_zeros() as usize;
            candidates &= candidates - 1;
            if undominated & (1 << u) != 0 {
                worst_case_fill(adj, undominated & !(adj[u] | (1 << u)), best, size + 1);
            }
        }
    }

    fn adjacency(geo: &Geometry) -> Vec<u64> {
        (0..geo.vertices.len()).map(|v| geo.vertices[v].neighbors.iter().fold(0u64, |m, &w| m | (1 << w))).collect()
    }

    fn small_shapes() -> Vec<(&'static str, Vec<(i32, i32)>)> {
        vec![
            ("single hex", vec![(0, 0)]),
            ("two hexes", vec![(0, 0), (1, 0)]),
            ("triangle", vec![(0, 0), (1, 0), (0, 1)]),
            ("flower", coords_of(&flower())),
            ("ring round a lake", vec![(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)]),
            ("comb", vec![(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (0, 1), (2, 1), (4, 1)]),
            ("strip of 6", (0..6).map(|q| (q, 0)).collect()),
            ("two islands", vec![(0, 0), (1, 0), (5, 0), (5, 1)]),
            ("archipelago", coords_of(&archipelago())),
            ("block 3x3", (0..3).flat_map(|r| (0..3).map(move |q| (q - r / 2, r))).collect()),
        ]
    }

    #[test]
    fn settlement_room_is_exact() {
        for (name, coords) in small_shapes() {
            let geo = Geometry::build(coords);
            let adj = adjacency(&geo);
            assert!(adj.len() <= 64, "{name} is too big to check this way");
            let all = if adj.len() == 64 { u64::MAX } else { (1u64 << adj.len()) - 1 };
            assert_eq!(settlement_room(&geo), Some(brute_force_room(&adj, all)), "{name}");
        }
        // a single hex: corners 0, 2, 4 (or 1, 3, 5)
        assert_eq!(settlement_room(&Geometry::build(vec![(0, 0)])), Some(3));
    }

    #[test]
    fn worst_case_placement_still_fits_when_there_is_slack() {
        // The claim behind ROOM_SLACK: however badly people place, they can always fit at least half of
        // what the best packing fits. Computed exactly (not sampled) for every small shape.
        for (name, coords) in small_shapes() {
            let geo = Geometry::build(coords);
            let adj = adjacency(&geo);
            let all = if adj.len() == 64 { u64::MAX } else { (1u64 << adj.len()) - 1 };
            let mut worst = usize::MAX;
            worst_case_fill(&adj, all, &mut worst, 0);
            let best = settlement_room(&geo).unwrap();
            assert!(worst * ROOM_SLACK >= best, "{name}: the worst placements fit only {worst} of a best {best}");
            assert!(worst <= best);
        }
    }

    #[test]
    fn random_placement_fills_between_half_and_all_of_the_room() {
        let mut rng = StdRng::seed_from_u64(3);
        let mut shapes: Vec<(String, Vec<(i32, i32)>)> = small_shapes().into_iter().map(|(n, c)| (n.to_string(), c)).collect();
        shapes.push(("Standard".into(), coords_of(&standard_def())));
        shapes.push(("Earth".into(), coords_of(&earth_def())));
        shapes.push(("block 15x15".into(), (0..15).flat_map(|r| (0..15).map(move |q| (q - r / 2, r))).collect()));
        for (name, coords) in shapes {
            let geo = Geometry::build(coords);
            let best = settlement_room(&geo).unwrap();
            for _ in 0..200 {
                let mut order: Vec<usize> = (0..geo.vertices.len()).collect();
                order.shuffle(&mut rng);
                let mut blocked = vec![false; order.len()];
                let mut placed = 0;
                for v in order {
                    if !blocked[v] {
                        placed += 1;
                        blocked[v] = true;
                        for &w in &geo.vertices[v].neighbors {
                            blocked[w] = true;
                        }
                    }
                }
                assert!(placed <= best, "{name}: placed {placed}, but {best} is supposed to be the most");
                assert!(placed * ROOM_SLACK >= best, "{name}: random placement filled only {placed} of {best}");
            }
        }
    }

    #[test]
    fn islands_are_counted_by_shared_edges() {
        let g = Geometry::build(vec![(0, 0), (1, 0), (5, 0), (5, 1), (9, 9)]);
        let mut sizes: Vec<usize> = islands(&g).iter().map(|i| i.len()).collect();
        sizes.sort();
        assert_eq!(sizes, vec![1, 2, 2]);
        // agrees with the board's own (test-only) count on the built-in maps
        for map in ["standard", "earth"] {
            let b = Board::generate(map, &mut rand::thread_rng());
            let g = Geometry::build(b.hexes.iter().map(|h| (h.q, h.r)).collect());
            assert_eq!(islands(&g).len(), b.islands().len());
        }
    }

    // ---------------- the validator and the generator agree ----------------

    fn is_playability(e: &MapError) -> bool {
        matches!(e, MapError::NoProducingHexes | MapError::NotEnoughSettlementRoom { .. } | MapError::TokenPlacementFailed { .. } | MapError::Internal(_))
    }

    /// Make one random change to a map: some small, some wild.
    fn mutate(def: &mut MapDef, rng: &mut StdRng) {
        use rand::Rng;
        let terrains = [Terrain::Hills, Terrain::Forest, Terrain::Mountains, Terrain::Fields, Terrain::Pasture, Terrain::Desert];
        let kinds = [PortKind::Any, PortKind::Brick, PortKind::Lumber, PortKind::Ore, PortKind::Grain, PortKind::Wool];
        let n = def.hexes.len();
        match rng.gen_range(0..16) {
            0 if n > 1 => {
                def.hexes.remove(rng.gen_range(0..n));
            }
            1 => def.hexes.push(HexDef::at(rng.gen_range(-3..12), rng.gen_range(-3..12))),
            2 if n > 0 => {
                let i = rng.gen_range(0..n);
                def.hexes[i].terrain = if rng.gen_bool(0.3) { Slot::Random } else { Slot::Fixed(*terrains.choose(rng).unwrap()) };
            }
            3 if n > 0 => {
                let i = rng.gen_range(0..n);
                def.hexes[i].number = match rng.gen_range(0..4) {
                    0 => Slot::Random,
                    _ => Slot::Fixed(rng.gen_range(0..14)),
                };
            }
            4 => {
                let t = *terrains.choose(rng).unwrap();
                def.terrain_pool.insert(t, rng.gen_range(0..6));
            }
            5 => {
                def.token_pool.insert(rng.gen_range(0..14), rng.gen_range(0..4));
            }
            6 => {
                let k = *kinds.choose(rng).unwrap();
                match &mut def.ports {
                    PortsDef::Fixed { pool, .. } | PortsDef::Ring { pool, .. } => {
                        pool.insert(k, rng.gen_range(0..4));
                    }
                }
            }
            7 => {
                if let PortsDef::Fixed { spots, .. } = &mut def.ports {
                    let hex = def.hexes.choose(rng).map(|h| (h.q, h.r)).unwrap_or((0, 0));
                    spots.push(PortSpot::at(hex.0, hex.1, rng.gen_range(0..7)));
                }
            }
            8 => {
                if let PortsDef::Fixed { spots, .. } = &mut def.ports {
                    if !spots.is_empty() {
                        let i = rng.gen_range(0..spots.len());
                        spots.remove(i);
                    }
                }
            }
            9 => {
                if let PortsDef::Ring { spacing, .. } = &mut def.ports {
                    if !spacing.is_empty() {
                        let i = rng.gen_range(0..spacing.len());
                        spacing[i] = rng.gen_range(0..6);
                    }
                }
            }
            10 => {
                def.robber_start = if rng.gen_bool(0.3) { None } else { Some(HexRef { q: rng.gen_range(-2..10), r: rng.gen_range(-2..10) }) };
            }
            11 => def.constraints.spread_tokens = !def.constraints.spread_tokens,
            12 => def.constraints.deserts_avoid_fixed_deserts = !def.constraints.deserts_avoid_fixed_deserts,
            13 => def.players = PlayerRange { min: rng.gen_range(0..6), max: rng.gen_range(0..6) },
            14 if rng.gen_bool(0.2) => def.version = rng.gen_range(0..3),
            _ => {
                // swap two hexes' written-in contents
                if n > 1 {
                    let (a, b) = (rng.gen_range(0..n), rng.gen_range(0..n));
                    let (ta, na) = (def.hexes[a].terrain, def.hexes[a].number);
                    def.hexes[a].terrain = def.hexes[b].terrain;
                    def.hexes[a].number = def.hexes[b].number;
                    def.hexes[b].terrain = ta;
                    def.hexes[b].number = na;
                }
            }
        }
    }

    #[test]
    fn the_validator_and_the_generator_never_disagree_about_a_map() {
        // Many randomly damaged maps. If the generator refuses one for a structural reason, the validator
        // must list that very problem first; if the validator finds no structural problem, the generator
        // must not refuse the map for one (bad luck with the tokens is the only thing it may fail on).
        let rounds = if cfg!(debug_assertions) { 300 } else { 4000 };
        let mut rng = StdRng::seed_from_u64(2024);
        let starts = [flower(), archipelago(), standard_def(), earth_def()];
        let (mut refused, mut accepted) = (0, 0);
        for round in 0..rounds {
            let mut def = starts[round % starts.len()].clone();
            for _ in 0..rng.gen_range(1..4) {
                mutate(&mut def, &mut rng);
            }
            let report = validate_with_rng(&def, &Settings::default(), &mut StdRng::seed_from_u64(round as u64));
            let structural: Vec<&MapError> = report.errors.iter().filter(|e| !is_playability(e)).collect();
            let built = Board::from_def(&def, "standard", &mut rng);
            let context = || format!("round {round}: {}", serde_json::to_string(&def).unwrap());
            match built {
                Err(MapError::TokenPlacementFailed { .. }) | Ok(_) => {
                    assert!(structural.is_empty(), "the generator accepted a map the validator calls broken: {structural:?}\n{}", context());
                    accepted += 1;
                }
                Err(e) => {
                    assert_eq!(structural.first().copied(), Some(&e), "the validator disagrees about the first problem\n{}", context());
                    refused += 1;
                }
            }
            // and a map the validator passes completely must be one the generator can build
            if report.errors.is_empty() {
                let mut ok = false;
                for _ in 0..30 {
                    if Board::from_def(&def, "standard", &mut rng).is_ok() {
                        ok = true;
                        break;
                    }
                }
                assert!(ok, "a map the validator passes could not be built in 30 tries\n{}", context());
            }
        }
        assert!(refused > rounds / 10 && accepted > rounds / 10, "the test didn't exercise both sides: {refused} refused, {accepted} accepted");
    }

    #[test]
    fn damaged_json_never_panics() {
        use rand::Rng;
        let original = include_str!("../examples/maps/archipelago.json");
        let chars: Vec<char> = original.chars().collect();
        let junk: Vec<char> = "{}[]\",:0123456789-?abcxyz \n".chars().collect();
        let mut rng = StdRng::seed_from_u64(99);
        let rounds = if cfg!(debug_assertions) { 500 } else { 5000 };
        let (mut parsed, mut rejected) = (0, 0);
        for _ in 0..rounds {
            let mut text = chars.clone();
            for _ in 0..rng.gen_range(1..6) {
                let at = rng.gen_range(0..text.len().max(1));
                match rng.gen_range(0..4) {
                    0 if !text.is_empty() => {
                        text.remove(at.min(text.len() - 1));
                    }
                    1 => text.insert(at.min(text.len()), *junk.choose(&mut rng).unwrap()),
                    2 if !text.is_empty() => {
                        let i = at.min(text.len() - 1);
                        text[i] = *junk.choose(&mut rng).unwrap();
                    }
                    _ => text.truncate(at),
                }
            }
            let text: String = text.into_iter().collect();
            match MapDef::from_json(&text) {
                Ok(def) => {
                    parsed += 1;
                    let _ = validate_with_rng(&def, &Settings::default(), &mut rng);
                }
                Err(message) => {
                    rejected += 1;
                    assert!(!message.is_empty());
                }
            }
        }
        assert!(parsed > 0 && rejected > 0, "{parsed} parsed, {rejected} rejected");
    }

    // ---------------- the command line ----------------

    struct Dir(std::path::PathBuf);

    impl Dir {
        fn new(name: &str) -> Dir {
            let p = std::env::temp_dir().join(format!("catan-validate-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
        fn file(&self, name: &str, text: &str) -> String {
            let p = self.0.join(name);
            std::fs::write(&p, text).unwrap();
            p.to_string_lossy().into_owned()
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn cli(args: &[&str]) -> (i32, String, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = run_cli(&args, &mut out, &mut err);
        (status, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    #[test]
    fn the_command_accepts_a_good_map() {
        let (status, out, err) = cli(&["examples/maps/archipelago.json"]);
        assert_eq!(status, 0, "{out}{err}");
        assert!(out.contains("\"Archipelago\" by Settoria examples"), "{out}");
        assert!(out.contains("14 hexes on 2 islands (12 producing), 7 harbours, room for"), "{out}");
        assert!(out.contains("terrain: "), "{out}");
        assert!(out.trim_end().ends_with("OK"), "{out}");
        assert_eq!(err, "");
    }

    #[test]
    fn the_command_lists_every_problem_with_where_it_is() {
        let dir = Dir::new("broken");
        let mut d = flower();
        d.terrain_pool.insert(Terrain::Hills, 5);
        d.hexes[0].number = Slot::Fixed(6);
        d.ports = PortsDef::Fixed { spots: vec![PortSpot::at(0, 0, 2)], pool: BTreeMap::from([(PortKind::Any, 1)]) };
        let path = dir.file("broken.json", &serde_json::to_string(&d).unwrap());
        let (status, out, err) = cli(&[&path]);
        assert_eq!(status, 1, "{out}{err}");
        assert!(out.contains("error: 6 hexes need a random terrain, but terrain_pool holds 9"), "{out}");
        assert!(out.contains("error: hex (0, 0) is a desert, so it cannot have a number"), "{out}");
        assert!(out.contains("error: the harbour on hex (0, 0) side 2 is not on the coast"), "{out}");
        assert!(out.contains("FAILED: 3 errors"), "{out}");
    }

    #[test]
    fn the_command_shows_warnings_without_failing() {
        let dir = Dir::new("warn");
        let path = dir.file("flower.json", &serde_json::to_string(&flower()).unwrap());
        let (status, out, _) = cli(&[&path]);
        assert_eq!(status, 0, "{out}");
        assert!(out.contains("warning: no mountains hexes"), "{out}");
        assert!(out.contains("warning: the map has no harbours"), "{out}");
        assert!(out.contains("warning: room for 12 settlements, and setup needs 8"), "{out}");
        assert!(out.trim_end().ends_with("OK, 3 warnings"), "{out}");
    }

    #[test]
    fn the_command_uses_the_maps_own_recommended_settings() {
        let dir = Dir::new("settings");
        // the one-hex map can't hold 4 players x 2, but with 1 settlement each and 2 players it fits
        let mut d = one_hex();
        d.players = PlayerRange { min: 2, max: 2 };
        let mut s = Settings::default();
        s.starting_settlements = 1;
        d.recommended_settings = Some(s);
        let path = dir.file("tiny.json", &serde_json::to_string(&d).unwrap());
        let (status, out, err) = cli(&[&path]);
        assert_eq!(status, 0, "{out}{err}");
        d.recommended_settings = None;
        let path = dir.file("tiny2.json", &serde_json::to_string(&d).unwrap());
        assert_eq!(cli(&[&path]).0, 1);
    }

    #[test]
    fn the_command_says_where_a_syntax_or_field_mistake_is() {
        let dir = Dir::new("syntax");
        let text = std::fs::read_to_string("examples/maps/archipelago.json").unwrap().replace("\"terrain_pool\"", "\"terain_pool\"");
        let path = dir.file("typo.json", &text);
        let (status, out, err) = cli(&[&path]);
        assert_eq!(status, 2, "{out}{err}");
        assert!(err.contains("not a valid map file") && err.contains("unknown field `terain_pool`") && err.contains("line "), "{err}");
        assert_eq!(out, "");

        let path = dir.file("cut.json", "{ \"version\": 1, \"name\": ");
        let (status, _, err) = cli(&[&path]);
        assert_eq!(status, 2);
        assert!(err.contains("line 1 column"), "{err}");

        let path = dir.file("terrain.json", &std::fs::read_to_string("examples/maps/archipelago.json").unwrap().replace("\"mountains\"", "\"mountians\""));
        let (_, _, err) = cli(&[&path]);
        assert!(err.contains("unknown variant `mountians`"), "{err}");
    }

    #[test]
    fn the_command_copes_with_files_it_cannot_use() {
        let dir = Dir::new("files");
        let (status, _, err) = cli(&["/no/such/map.json"]);
        assert_eq!(status, 2);
        assert!(err.contains("/no/such/map.json: can't read the file"), "{err}");

        let big = dir.file("big.json", &" ".repeat(MAX_MAP_BYTES + 1));
        let (status, _, err) = cli(&[&big]);
        assert_eq!(status, 2);
        assert!(err.contains("limited to"), "{err}");

        // a byte-order mark, as some Windows editors write, is ignored
        let bom = dir.file("bom.json", &format!("\u{feff}{}", std::fs::read_to_string("examples/maps/archipelago.json").unwrap()));
        assert_eq!(cli(&[&bom]).0, 0);

        let (status, _, err) = cli(&[]);
        assert_eq!(status, 2);
        assert!(err.contains("usage: catan-lan validate"), "{err}");
        assert_eq!(cli(&["--help"]).0, 2);
    }

    #[test]
    fn with_several_files_every_one_is_checked_and_the_worst_status_wins() {
        let dir = Dir::new("several");
        let mut bad = flower();
        bad.terrain_pool.clear();
        let bad = dir.file("bad.json", &serde_json::to_string(&bad).unwrap());
        let unreadable = dir.file("junk.json", "nonsense");
        let good = "examples/maps/archipelago.json";

        let (status, out, err) = cli(&[good, &bad]);
        assert_eq!(status, 1);
        assert!(out.contains("Archipelago") && out.contains("FAILED"), "{out}");
        let (status, out, err2) = cli(&[good, &bad, &unreadable, good]);
        assert_eq!(status, 2, "{out}{err}{err2}");
        assert_eq!(out.matches("\"Archipelago\"").count(), 2, "files after a bad one are still checked: {out}");
        assert!(err2.contains("junk.json: not a valid map file"), "{err2}");
    }
}
