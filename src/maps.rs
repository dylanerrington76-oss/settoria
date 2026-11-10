//! The maps the server knows about, by id.
//!
//! The registry starts with the built-in maps (the files in `maps/builtin/`, compiled in). Maps read from
//! the maps folder at startup, and maps the host uploads while the server runs, are added to it (see
//! `map_loader`); the lobby lists whatever is in it and a game is started from an entry's `MapDef`.
//! Uploaded maps live in memory only: they are gone when the server stops, and at most `MAX_UPLOADS` are kept.
//!
//! It is shared as `SharedMaps`, an `Arc<RwLock<..>>` of its own. That keeps it apart from the room's lock:
//! looking a map up, or validating and adding an uploaded one, never makes a player's move wait, and a slow
//! upload never holds up the table. (Validating happens before the lock is taken; the lock is only held to
//! add the finished entry.)

use crate::builtin::BUILTIN_MAPS;
use crate::game::Settings;
use crate::map_def::{MapDef, MAX_NAME_CHARS};
use crate::map_files::{id_from_stem, name_key, name_set, unique_id, unique_name};
use serde::Serialize;
use std::sync::{Arc, RwLock};

/// Where a map came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapSource {
    /// Compiled into the program (`maps/builtin/`).
    Builtin,
    /// Read from the maps folder at startup.
    Folder,
    /// Uploaded by the host while the server runs.
    Upload,
}

/// One map the server can play.
#[derive(Clone, Debug)]
pub struct MapEntry {
    /// What the lobby and the game log call it. Unique in a registry.
    pub id: String,
    pub name: String,
    pub author: String,
    pub source: MapSource,
    /// Shared, so starting a game doesn't copy the map or hold the registry's lock.
    pub def: Arc<MapDef>,
}

/// What the lobby tells every player about one map: enough to list it and to know whether it suits the
/// table, without sending the map itself.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct MapInfo {
    pub id: String,
    pub name: String,
    /// Empty if the map doesn't say.
    pub author: String,
    pub hexes: usize,
    pub players_min: usize,
    pub players_max: usize,
    /// One of the maps compiled into the program (as opposed to one someone added).
    pub builtin: bool,
    /// The rules the map suggests. Offered to the host, never applied automatically.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_settings: Option<Settings>,
}

impl MapEntry {
    pub fn info(&self) -> MapInfo {
        MapInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            author: self.author.clone(),
            hexes: self.def.hexes.len(),
            players_min: self.def.players.min,
            players_max: self.def.players.max,
            builtin: self.source == MapSource::Builtin,
            recommended_settings: self.def.recommended_settings,
        }
    }
}

/// Why `MapRegistry::insert` said no.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    /// A map with this id is already there. Nothing is ever replaced, built-in maps least of all.
    DuplicateId(String),
    /// An id is 1 to 64 characters from `a-z A-Z 0-9 - _ : .`.
    BadId(String),
    /// Built-in maps are not added at runtime.
    BuiltinSource,
    /// The host has already uploaded `MAX_UPLOADS` maps.
    TooManyUploads,
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::DuplicateId(id) => write!(f, "there is already a map with the id `{id}`"),
            RegistryError::BadId(id) => write!(f, "`{id}` is not a usable map id (1 to 64 letters, digits and - _ : .)"),
            RegistryError::BuiltinSource => write!(f, "built-in maps can't be added while the server runs"),
            RegistryError::TooManyUploads => write!(f, "{MAX_UPLOADS} maps have already been uploaded; restart the server to clear them"),
        }
    }
}

/// The longest map id.
pub const MAX_ID_LEN: usize = 64;

/// The most maps the host can upload while the server runs. They are kept in memory, so this is what bounds it.
pub const MAX_UPLOADS: usize = 10;

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ID_LEN && id.chars().all(|c| c.is_ascii_alphanumeric() || "-_:.".contains(c))
}

/// The maps, in the order the lobby shows them: the built-in ones first, then the rest as they were added.
#[derive(Debug)]
pub struct MapRegistry {
    entries: Vec<MapEntry>,
}

impl MapRegistry {
    /// A registry holding just the built-in maps.
    pub fn with_builtins() -> MapRegistry {
        let entries = BUILTIN_MAPS
            .iter()
            .map(|&(id, _)| {
                let def = crate::builtin::builtin(id).expect("every built-in map has a file").clone();
                MapEntry { id: id.to_string(), name: def.name.clone(), author: def.author.clone(), source: MapSource::Builtin, def: Arc::new(def) }
            })
            .collect();
        MapRegistry { entries }
    }

    pub fn entries(&self) -> &[MapEntry] {
        &self.entries
    }

    pub fn get(&self, id: &str) -> Option<&MapEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// How many maps came from `source`.
    pub fn count(&self, source: MapSource) -> usize {
        self.entries.iter().filter(|e| e.source == source).count()
    }

    /// Add a map. The caller has already read it and checked it with `validate`; this only keeps the list
    /// tidy: ids are unique and well-formed, nothing already there is ever replaced, and no more than
    /// `MAX_UPLOADS` maps are uploaded.
    pub fn insert(&mut self, id: impl Into<String>, source: MapSource, def: MapDef) -> Result<&MapEntry, RegistryError> {
        let id = id.into();
        if source == MapSource::Builtin {
            return Err(RegistryError::BuiltinSource);
        }
        if !valid_id(&id) {
            return Err(RegistryError::BadId(id));
        }
        if self.get(&id).is_some() {
            return Err(RegistryError::DuplicateId(id));
        }
        if source == MapSource::Upload && self.count(MapSource::Upload) >= MAX_UPLOADS {
            return Err(RegistryError::TooManyUploads);
        }
        self.entries.push(MapEntry { id, name: def.name.clone(), author: def.author.clone(), source, def: Arc::new(def) });
        Ok(self.entries.last().expect("just pushed"))
    }

    /// Add a map whose id and name may already be in use, by changing them instead of refusing it. The id is
    /// made from `id_hint` (a file name without its extension, say; anything not allowed in an id becomes `-`)
    /// and the name is the map's own, trimmed. If either is taken it gets a number: `earth-2`, `Earth (2)`.
    /// Names are compared without regard to case or surrounding spaces, so two buttons never look the same.
    /// The caller can see what it ended up with in the entry that comes back. The decision and the insert
    /// happen under one `&mut self`, so two maps added at once can't take the same id.
    pub fn add_unique(&mut self, id_hint: &str, source: MapSource, mut def: MapDef) -> Result<&MapEntry, RegistryError> {
        // Say no before doing any work for a map that can't be kept.
        if source == MapSource::Upload && self.count(MapSource::Upload) >= MAX_UPLOADS {
            return Err(RegistryError::TooManyUploads);
        }
        def.name = def.name.trim().to_string();
        def.author = def.author.trim().to_string();
        let names = name_set(self.entries.iter().map(|e| e.name.as_str()));
        def.name = unique_name(&def.name, MAX_NAME_CHARS, |n| names.contains(&name_key(n)));
        let id = unique_id(&id_from_stem(id_hint, MAX_ID_LEN), MAX_ID_LEN, |i| self.get(i).is_some());
        self.insert(id, source, def)
    }
}

impl Default for MapRegistry {
    fn default() -> Self {
        MapRegistry::with_builtins()
    }
}

/// The registry as the server shares it.
pub type SharedMaps = Arc<RwLock<MapRegistry>>;

/// A new shared registry holding the built-in maps.
pub fn shared_with_builtins() -> SharedMaps {
    Arc::new(RwLock::new(MapRegistry::with_builtins()))
}

/// Look a map up by id. Holds the registry's lock only for the lookup. A panic elsewhere while the lock was
/// held can't make this fail: the registry only ever changes by adding a whole entry, so what is there is
/// always sound.
pub fn find(maps: &SharedMaps, id: &str) -> Option<Arc<MapDef>> {
    let registry = maps.read().unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.get(id).map(|e| e.def.clone())
}

/// What the lobby shows of every map, in lobby order.
pub fn infos(maps: &SharedMaps) -> Vec<MapInfo> {
    let registry = maps.read().unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.entries().iter().map(MapEntry::info).collect()
}

/// Is every upload slot used? A quick answer for a request that would only be turned away anyway.
pub fn uploads_full(maps: &SharedMaps) -> bool {
    let registry = maps.read().unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.count(MapSource::Upload) >= MAX_UPLOADS
}

/// The ids of every map, in lobby order.
pub fn ids(maps: &SharedMaps) -> Vec<String> {
    let registry = maps.read().unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.entries().iter().map(|e| e.id.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::{flower, DEFAULT_MAP};
    use crate::validate::validate;

    #[test]
    fn it_starts_with_the_built_in_maps_in_lobby_order() {
        let r = MapRegistry::with_builtins();
        let listed: Vec<(&str, &str, MapSource)> = r.entries().iter().map(|e| (e.id.as_str(), e.name.as_str(), e.source)).collect();
        assert_eq!(listed, vec![("standard", "Standard", MapSource::Builtin), ("earth", "Earth", MapSource::Builtin), ("usa", "USA", MapSource::Builtin)]);
    }

    #[test]
    fn the_default_map_is_the_first_built_in_map() {
        let r = MapRegistry::with_builtins();
        assert_eq!(r.entries()[0].id, DEFAULT_MAP);
        assert!(r.get(DEFAULT_MAP).is_some());
    }

    #[test]
    fn the_built_in_maps_are_the_files_and_are_sound() {
        let r = MapRegistry::with_builtins();
        for (id, text) in BUILTIN_MAPS {
            let entry = r.get(id).expect("listed");
            assert_eq!(*entry.def, MapDef::from_json(text).unwrap(), "{id}");
            assert_eq!(entry.name.to_lowercase(), id, "a built-in map's id is its name in lower case");
            // the maps the game ships are held to the same standard as anyone's: no errors, no warnings
            let report = validate(&entry.def, &entry.def.recommended_settings.unwrap_or_default());
            assert!(report.errors.is_empty() && report.warnings.is_empty(), "{id}: {:?} {:?}", report.errors, report.warnings);
        }
    }

    #[test]
    fn maps_are_found_by_id() {
        let maps = shared_with_builtins();
        assert_eq!(find(&maps, "earth").unwrap().hexes.len(), 81);
        assert_eq!(find(&maps, "standard").unwrap().hexes.len(), 19);
        assert_eq!(find(&maps, "usa").unwrap().hexes.len(), 144);
        assert!(find(&maps, "atlantis").is_none());
        assert!(find(&maps, "").is_none());
        assert!(find(&maps, "Earth").is_none(), "ids are case-sensitive");
        assert_eq!(ids(&maps), vec!["standard", "earth", "usa"]);
    }

    #[test]
    fn a_lookup_shares_the_map_instead_of_copying_it() {
        let maps = shared_with_builtins();
        let (a, b) = (find(&maps, "earth").unwrap(), find(&maps, "earth").unwrap());
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn added_maps_go_after_the_built_in_ones_and_are_found() {
        let mut r = MapRegistry::with_builtins();
        let entry = r.insert("user:flower", MapSource::Folder, flower()).unwrap();
        assert_eq!((entry.id.as_str(), entry.name.as_str(), entry.source), ("user:flower", "Flower", MapSource::Folder));
        r.insert("up-1", MapSource::Upload, flower()).unwrap();
        let order: Vec<&str> = r.entries().iter().map(|e| e.id.as_str()).collect();
        assert_eq!(order, vec!["standard", "earth", "usa", "user:flower", "up-1"]);
        assert_eq!(r.get("up-1").unwrap().source, MapSource::Upload);
    }

    #[test]
    fn nothing_is_ever_replaced_least_of_all_a_built_in_map() {
        let mut r = MapRegistry::with_builtins();
        assert_eq!(r.insert("earth", MapSource::Upload, flower()).err(), Some(RegistryError::DuplicateId("earth".into())));
        assert_eq!(r.get("earth").unwrap().def.hexes.len(), 81, "Earth is still Earth");
        r.insert("mine", MapSource::Upload, flower()).unwrap();
        assert_eq!(r.insert("mine", MapSource::Folder, flower()).err(), Some(RegistryError::DuplicateId("mine".into())));
        assert_eq!(r.entries().len(), 4);
        // built-in maps aren't added at runtime at all
        assert_eq!(r.insert("another", MapSource::Builtin, flower()).err(), Some(RegistryError::BuiltinSource));
    }

    #[test]
    fn ids_must_be_tidy() {
        let mut r = MapRegistry::with_builtins();
        for good in ["a", "user:my-map_2.v3", &"x".repeat(64)] {
            assert!(r.insert(good, MapSource::Upload, flower()).is_ok(), "{good}");
        }
        for bad in ["", " ", "has space", "slash/inside", "back\\slash", "new\nline", "quote\"", "ünïcode", "tab\t", &"x".repeat(65), "../etc/passwd"] {
            assert_eq!(r.insert(bad, MapSource::Upload, flower()).err(), Some(RegistryError::BadId(bad.to_string())), "{bad:?}");
        }
        assert_eq!(r.entries().len(), 3 + 3, "none of the bad ones got in");
    }

    #[test]
    fn a_lookup_still_works_after_a_panic_while_the_lock_was_held() {
        let maps = shared_with_builtins();
        let poisoner = maps.clone();
        let _ = std::thread::spawn(move || {
            let _held = poisoner.write().unwrap();
            panic!("a panic while holding the lock, on purpose");
        })
        .join();
        assert!(maps.read().is_err(), "the lock really is poisoned");
        assert_eq!(find(&maps, "earth").unwrap().hexes.len(), 81);
        assert_eq!(ids(&maps), vec!["standard", "earth", "usa"]);
    }

    #[test]
    fn error_messages_say_what_to_do() {
        assert!(RegistryError::DuplicateId("earth".into()).to_string().contains("earth"));
        assert!(RegistryError::BadId("a b".into()).to_string().contains("1 to 64"));
    }

    fn named(name: &str) -> MapDef {
        let mut def = flower();
        def.name = name.into();
        def
    }

    #[test]
    fn a_taken_id_or_name_is_suffixed_not_refused() {
        let mut r = MapRegistry::with_builtins();
        // a file called earth.json: its id and its name both clash with the built-in Earth
        let e = r.add_unique("earth", MapSource::Folder, named("Earth")).unwrap();
        assert_eq!((e.id.as_str(), e.name.as_str(), e.def.name.as_str()), ("earth-2", "Earth (2)", "Earth (2)"));
        let e = r.add_unique("earth", MapSource::Folder, named("EARTH")).unwrap();
        assert_eq!((e.id.as_str(), e.name.as_str()), ("earth-3", "EARTH (3)"), "names clash whatever their case, so `Earth (2)` counts as taken");
        let e = r.add_unique("Earth", MapSource::Upload, named("Earth")).unwrap();
        assert_eq!((e.id.as_str(), e.name.as_str()), ("earth-4", "Earth (4)"));
        // the same id with a different name, and the same name with a different id
        let e = r.add_unique("standard", MapSource::Folder, named("Not standard")).unwrap();
        assert_eq!((e.id.as_str(), e.name.as_str()), ("standard-2", "Not standard"));
        let e = r.add_unique("other", MapSource::Folder, named("Standard")).unwrap();
        assert_eq!((e.id.as_str(), e.name.as_str()), ("other", "Standard (2)"));
        // nothing that was there changed
        assert_eq!(r.get("earth").unwrap().def.hexes.len(), 81);
        assert_eq!(r.get("earth").unwrap().name, "Earth");
        let order: Vec<&str> = r.entries().iter().map(|e| e.id.as_str()).collect();
        assert_eq!(order, vec!["standard", "earth", "usa", "earth-2", "earth-3", "earth-4", "standard-2", "other"]);
    }

    #[test]
    fn an_id_hint_is_made_safe_and_a_name_is_trimmed() {
        let mut r = MapRegistry::with_builtins();
        let mut def = named("  Padded  ");
        def.author = "  me ".into();
        let e = r.add_unique("My Map (v2)", MapSource::Folder, def).unwrap();
        assert_eq!((e.id.as_str(), e.name.as_str(), e.author.as_str()), ("my-map-v2", "Padded", "me"));
        assert_eq!(e.def.author, "me");
        for hint in ["../../etc/passwd", "", "日本語", "a b\nc", &"z".repeat(300)] {
            let e = r.add_unique(hint, MapSource::Folder, named(&format!("Map for {}", hint.len()))).unwrap();
            assert!(valid_id(&e.id), "{hint:?} -> {:?}", e.id);
        }
    }

    #[test]
    fn a_suffix_never_pushes_a_name_or_id_past_its_limit() {
        let mut r = MapRegistry::with_builtins();
        let long_name = "n".repeat(MAX_NAME_CHARS);
        let long_id = "i".repeat(100);
        r.add_unique(&long_id, MapSource::Folder, named(&long_name)).unwrap();
        let e = r.add_unique(&long_id, MapSource::Folder, named(&long_name)).unwrap();
        assert_eq!(e.id.len(), MAX_ID_LEN);
        assert!(e.id.ends_with("-2"));
        assert_eq!(e.name.chars().count(), MAX_NAME_CHARS);
        assert!(e.name.ends_with(" (2)"));
    }

    #[test]
    fn no_more_than_ten_maps_can_be_uploaded_and_nothing_else_counts() {
        let mut r = MapRegistry::with_builtins();
        for i in 0..3 {
            r.add_unique(&format!("folder{i}"), MapSource::Folder, named(&format!("Folder {i}"))).unwrap();
        }
        for i in 0..MAX_UPLOADS {
            r.add_unique("up", MapSource::Upload, named(&format!("Upload {i}"))).unwrap();
        }
        assert_eq!(r.count(MapSource::Upload), MAX_UPLOADS);
        let before = r.entries().len();
        assert_eq!(r.add_unique("one-more", MapSource::Upload, named("One more")).err(), Some(RegistryError::TooManyUploads));
        assert_eq!(r.insert("raw", MapSource::Upload, flower()).err(), Some(RegistryError::TooManyUploads));
        assert_eq!(r.entries().len(), before, "a refused upload adds nothing");
        // the cap is on uploads: built-in and folder maps are not affected, and the lobby still lists everything
        assert!(r.add_unique("another-folder", MapSource::Folder, named("Another folder")).is_ok());
        assert!(RegistryError::TooManyUploads.to_string().contains("10"));
    }

    #[test]
    fn the_shared_registry_says_when_the_upload_slots_are_used_up() {
        let maps = shared_with_builtins();
        assert!(!uploads_full(&maps));
        for i in 0..MAX_UPLOADS {
            maps.write().unwrap().add_unique("up", MapSource::Upload, named(&format!("Upload {i}"))).unwrap();
        }
        assert!(uploads_full(&maps));
        assert_eq!(infos(&maps).len(), 3 + MAX_UPLOADS);
    }

    #[test]
    fn the_lobby_is_told_what_it_needs_about_each_map() {
        let maps = shared_with_builtins();
        let infos = infos(&maps);
        assert_eq!(infos.len(), 3);
        assert_eq!(
            infos[0],
            MapInfo {
                id: "standard".into(),
                name: "Standard".into(),
                author: String::new(),
                hexes: 19,
                players_min: 2,
                players_max: 4,
                builtin: true,
                recommended_settings: None
            }
        );
        assert_eq!((infos[1].id.as_str(), infos[1].hexes, infos[1].builtin), ("earth", 81, true));
    }

    #[test]
    fn an_added_map_is_described_by_its_own_definition_and_is_not_built_in() {
        let mut def = flower();
        def.author = "someone".into();
        def.players = crate::map_def::PlayerRange { min: 3, max: 4 };
        let mut suggested = Settings::default();
        suggested.points_to_win = 12;
        def.recommended_settings = Some(suggested);
        let mut r = MapRegistry::with_builtins();
        let info = r.insert("user:flower", MapSource::Upload, def).unwrap().info();
        assert_eq!((info.name.as_str(), info.author.as_str(), info.hexes, info.players_min, info.players_max), ("Flower", "someone", 7, 3, 4));
        assert!(!info.builtin);
        assert_eq!(info.recommended_settings.unwrap().points_to_win, 12);
    }

    #[test]
    fn a_map_info_is_sent_as_plain_json_and_leaves_out_what_it_does_not_have() {
        let info = MapRegistry::with_builtins().get("earth").unwrap().info();
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "id": "earth", "name": "Earth", "author": "", "hexes": 81, "players_min": 2, "players_max": 8, "builtin": true })
        );
        assert!(json.get("recommended_settings").is_none(), "no suggestion, no key");
    }
}
