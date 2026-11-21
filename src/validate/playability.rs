//! Layer 4, the errors: is the map worth playing at all? Is there room for the starting settlements, does
//! anything produce, can the tokens actually be dealt?
//!
//! Only asked of a map that nothing in layers 1 to 3 found a problem with. (The advice that leaves a map
//! playable is in `quality`.)

use super::report::{MapStats, MapWarning};
use crate::board::dry_deal;
use crate::game::Settings;
use crate::map_def::{MapDef, MapError};
use rand::Rng;

/// Trial deals `validate` makes of the terrain and tokens.
pub(super) const DRY_RUN_TRIALS: usize = 10;
/// If this many trial deals in a row fail from the start, the map is hopeless and the rest are skipped.
pub(super) const HOPELESS_AFTER: usize = 3;

/// How many settlements setup has to place: every player's starting settlements, for the most players.
pub(super) fn settlements_needed(def: &MapDef, settings: &Settings) -> usize {
    def.players.max * settings.starting_settlements as usize
}

/// Does anything produce, and is there room for the starting settlements? `room` is what `settlement_room`
/// found, which is `None` only if the corners of the map are somehow not a bipartite graph.
pub(super) fn check_playability(def: &MapDef, settings: &Settings, stats: &MapStats, room: Option<usize>) -> Vec<MapError> {
    let mut errors = vec![];
    if stats.producing_hexes == 0 {
        errors.push(MapError::NoProducingHexes);
    }
    let needed = settlements_needed(def, settings);
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
    errors
}

/// Deal the map's terrain and tokens a few times. A deal that never works is an error, and one that works
/// only now and then is a warning: starting a game here would sometimes have to be tried again.
pub(super) fn check_token_deal<R: Rng>(def: &MapDef, rng: &mut R) -> Result<Option<MapWarning>, MapError> {
    match dry_run(|| dry_deal(def, rng)) {
        DryRun::Reliable => Ok(None),
        DryRun::Hopeless => Err(MapError::TokenPlacementFailed { attempts: HOPELESS_AFTER }),
        DryRun::Flaky { failed, trials } => Ok(Some(MapWarning::DealOftenFails { failed, trials })),
        DryRun::Broken(e) => Err(e),
    }
}

/// How the trial deals went.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum DryRun {
    Reliable,
    /// The first few deals all failed: the tokens can't be spread out on this map.
    Hopeless,
    Flaky { failed: usize, trials: usize },
    /// A deal failed for a reason other than bad luck.
    Broken(MapError),
}

/// Run `trial` up to `DRY_RUN_TRIALS` times and say how reliable it is.
pub(super) fn dry_run(mut trial: impl FnMut() -> Result<(), MapError>) -> DryRun {
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
