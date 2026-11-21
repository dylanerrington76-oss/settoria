//! Layer 2: the map's name and author, its player range and its recommended settings. None of this
//! depends on the hexes.

use crate::map_def::{MapDef, MapError, PlayerRange, MAX_AUTHOR_CHARS, MAX_NAME_CHARS, MAX_PLAYERS, MIN_PLAYERS};

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
