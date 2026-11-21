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
//!
//! This file only runs the layers in order. Each concern has a file of its own in `validate/`:
//!
//! | file          | what it holds                                                                    |
//! |---------------|----------------------------------------------------------------------------------|
//! | `hexes`       | layer 1                                                                          |
//! | `rules`       | layer 2                                                                          |
//! | `content`     | layer 3                                                                          |
//! | `playability` | layer 4, the errors: nothing produces, no room for setup, tokens can't be dealt  |
//! | `quality`     | layer 4, the advice: every `MapWarning` except a flaky deal                      |
//! | `stats`       | the facts for the summary (hexes, islands, harbours, settlement room)            |
//! | `bipartite`   | the matching behind the settlement-room figure, which knows nothing of hexes     |
//! | `report`      | what comes out: `Report`, `MapStats`, `MapWarning`                               |
//! | `cli`         | the `validate` command and how it prints a `Report`                              |

mod bipartite;
mod cli;
mod content;
mod hexes;
mod playability;
mod quality;
pub(crate) mod report;
mod rules;
mod stats;
#[cfg(test)]
mod tests;

pub use cli::run_cli;
pub(crate) use content::check_content;
pub(crate) use hexes::check_hexes;
pub use report::Report;
pub(crate) use rules::check_rules;

use crate::board::Geometry;
use crate::game::Settings;
use crate::map_def::MapDef;
use playability::{check_playability, check_token_deal, settlements_needed};
use quality::quality_warnings;
use rand::Rng;
use stats::settlement_room;

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
    let mut map_stats = None;

    if hexes_usable {
        let geo = Geometry::build(def.hexes.iter().map(|h| (h.q, h.r)).collect());
        errors.extend(check_content(def, &geo));
        let room = settlement_room(&geo);
        let facts = stats::gather(def, &geo, room.unwrap_or(0));

        // Playability is only worth asking about a map that is otherwise sound.
        if errors.is_empty() {
            errors.extend(check_playability(def, settings, &facts, room));
            if errors.is_empty() {
                warnings = quality_warnings(def, &geo, &facts, settlements_needed(def, settings));
                match check_token_deal(def, rng) {
                    Ok(None) => {}
                    Ok(Some(warning)) => warnings.push(warning),
                    Err(error) => errors.push(error),
                }
            }
        }
        map_stats = Some(facts);
    }

    Report::new(errors, warnings, map_stats)
}
