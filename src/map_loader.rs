//! Getting maps into the registry: from the maps folder when the server starts, and from an upload while
//! it runs. Both go through the same steps, `check_and_add`: parse, validate, then register.
//!
//! A map that is no good is never an error for the server. The folder loader logs it (file name and every
//! issue) and goes on to the next file; an upload gets the list of issues back.
//!
//! Reading the files and making ids and names is in `map_files`; keeping the maps is `maps`.

use crate::map_def::{MapDef, MAX_MAP_BYTES};
use crate::map_files::{file_label, file_stem, id_from_stem, list_json_files, printable, read_capped, summary};
use crate::maps::{MapSource, RegistryError, SharedMaps, MAX_ID_LEN};
use crate::validate::{validate, Report};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

/// Where maps are read from at startup unless `MAPS_DIR` says otherwise.
pub const DEFAULT_MAPS_DIR: &str = "maps";

/// The most files read from the maps folder.
pub const MAX_FOLDER_FILES: usize = 10;

/// The most issues written to the log for one file (a hopeless file can have a hundred).
const LOGGED_ISSUES: usize = 20;

/// A map that was accepted, as the registry now holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Added {
    pub id: String,
    pub name: String,
    /// The name was already in use, so a number was added to it.
    pub name_changed: bool,
    /// The id was already in use, so a number was added to it.
    pub id_changed: bool,
    /// Things worth knowing about a map that is playable all the same.
    pub warnings: Vec<String>,
}

/// Why a map was not added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddError {
    /// The text isn't a good map. Every issue, in the words the person who wrote it needs.
    Invalid(Vec<String>),
    /// A good map, but there is no room for it (the upload limit).
    Full(String),
    /// Something that should not happen.
    Internal(String),
}

impl AddError {
    pub fn issues(self) -> Vec<String> {
        match self {
            AddError::Invalid(issues) => issues,
            AddError::Full(why) | AddError::Internal(why) => vec![why],
        }
    }
}

/// What `validate` found wrong, as sentences.
fn issues_of(report: &Report) -> Vec<String> {
    let mut issues: Vec<String> = report.errors.iter().map(|e| e.to_string()).collect();
    if report.truncated {
        issues.push("(there were more problems than are shown)".to_string());
    }
    issues
}

/// Parse and validate a map's JSON. A map is judged by the rules it suggests for itself (or the standard
/// ones), as `validate` on the command line does.
fn check_text(text: &str) -> Result<(MapDef, Vec<String>), Vec<String>> {
    let def = MapDef::from_json(text).map_err(|e| vec![e])?;
    let settings = def.recommended_settings.unwrap_or_default();
    let report = validate(&def, &settings);
    if !report.is_ok() {
        return Err(issues_of(&report));
    }
    let warnings = report.warnings.iter().map(|w| w.to_string()).collect();
    Ok((def, warnings))
}

/// Check a map's JSON and, if it is a good map, add it to the registry.
/// `id_hint` is what the id is made from (a file's name without `.json`); `None` makes it from the map's
/// name. A taken id or name gets a number (see `MapRegistry::add_unique`). The checking, which can take a
/// moment on a big map, happens before the registry's lock is taken; the lock is held only to add the entry.
/// It never panics: a bug in the checks is reported as an issue with that map.
pub fn check_and_add(maps: &SharedMaps, text: &str, source: MapSource, id_hint: Option<&str>) -> Result<Added, AddError> {
    let checked = match catch_unwind(AssertUnwindSafe(|| check_text(text))) {
        Ok(checked) => checked,
        Err(_) => return Err(AddError::Internal("something went wrong while checking the map".to_string())),
    };
    let (def, warnings) = checked.map_err(AddError::Invalid)?;

    let asked_name = def.name.trim().to_string();
    let hint = id_hint.map(str::to_string).unwrap_or_else(|| def.name.clone());
    let wanted_id = id_from_stem(&hint, MAX_ID_LEN);

    let mut registry = maps.write().unwrap_or_else(|poisoned| poisoned.into_inner());
    let outcome = match registry.add_unique(&hint, source, def) {
        Ok(entry) => Ok(Added {
            id: entry.id.clone(),
            name: entry.name.clone(),
            name_changed: entry.name != asked_name,
            id_changed: entry.id != wanted_id,
            warnings,
        }),
        Err(RegistryError::TooManyUploads) => Err(AddError::Full(RegistryError::TooManyUploads.to_string())),
        Err(other) => Err(AddError::Internal(other.to_string())),
    };
    outcome
}

/// What loading the maps folder did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FolderReport {
    /// How many maps were added.
    pub loaded: usize,
    /// Each file that was left out, with every issue it had.
    pub skipped: Vec<(String, Vec<String>)>,
    /// The folder itself couldn't be read (it is missing, say).
    pub folder_problem: Option<String>,
}

impl FolderReport {
    /// The one line for the startup banner, e.g. `Loaded 3 maps, skipped 1 (foo.json: the map has no hexes)`.
    pub fn summary(&self, dir: &Path) -> String {
        if let Some(why) = &self.folder_problem {
            return format!("Loaded 0 maps (can't read the maps folder '{}': {})", dir.display(), why);
        }
        let skipped: Vec<(String, String)> = self
            .skipped
            .iter()
            .map(|(file, issues)| {
                let first = issues.first().map(|i| printable(i)).unwrap_or_default();
                let why = if issues.len() > 1 { format!("{first} (+{} more)", issues.len() - 1) } else { first };
                (file.clone(), why)
            })
            .collect();
        summary(self.loaded, &skipped)
    }
}

/// Read one file and add its map.
fn load_file(path: &Path, maps: &SharedMaps) -> Result<Added, Vec<String>> {
    let text = read_capped(path, MAX_MAP_BYTES).map_err(|why| vec![why])?;
    check_and_add(maps, &text, MapSource::Folder, Some(&file_stem(path))).map_err(AddError::issues)
}

/// Add every good map in the `*.json` files of `dir` (not of folders under it), in file-name order. A file
/// that is too big, can't be read, isn't a map or isn't a good one is logged to stderr with its name and its
/// issues and skipped; nothing a file contains can stop the others from loading, or the server from starting.
pub fn load_dir(dir: &Path, maps: &SharedMaps) -> FolderReport {
    let mut report = FolderReport::default();
    let files = match list_json_files(dir) {
        Ok(files) => files,
        Err(e) => {
            eprintln!("maps: can't read the maps folder '{}': {e}", dir.display());
            report.folder_problem = Some(e.to_string());
            return report;
        }
    };

    for path in files.iter().take(MAX_FOLDER_FILES) {
        let label = file_label(path);
        match load_file(path, maps) {
            Ok(added) => {
                report.loaded += 1;
                if added.name_changed || added.id_changed {
                    eprintln!("maps: {label} is loaded as \"{}\" (id `{}`), because its name or id was already in use", added.name, added.id);
                }
                for warning in &added.warnings {
                    eprintln!("maps: {label}: warning: {}", printable(warning));
                }
            }
            Err(issues) => {
                for issue in issues.iter().take(LOGGED_ISSUES) {
                    eprintln!("maps: skipped {label}: {}", printable(issue));
                }
                if issues.len() > LOGGED_ISSUES {
                    eprintln!("maps: skipped {label}: ...and {} more issues", issues.len() - LOGGED_ISSUES);
                }
                report.skipped.push((label, issues));
            }
        }
    }

    if files.len() > MAX_FOLDER_FILES {
        let left_out = files.len() - MAX_FOLDER_FILES;
        let why = format!("only the first {MAX_FOLDER_FILES} files in the folder are read");
        eprintln!("maps: {left_out} more files not read: {why}");
        report.skipped.push((format!("{left_out} more files"), vec![why]));
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::{archipelago, flower};
    use crate::maps::{infos, shared_with_builtins, MAX_UPLOADS};
    use std::fs;
    use std::path::PathBuf;

    /// A new empty folder for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("settoria-map-loader-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn json_of(def: &MapDef) -> String {
        serde_json::to_string(def).unwrap()
    }

    fn named(name: &str) -> MapDef {
        let mut def = flower();
        def.name = name.into();
        def
    }

    fn ids_and_names(maps: &SharedMaps) -> Vec<(String, String)> {
        infos(maps).into_iter().map(|i| (i.id, i.name)).collect()
    }

    fn pair(id: &str, name: &str) -> (String, String) {
        (id.to_string(), name.to_string())
    }

    #[test]
    fn good_maps_are_loaded_and_bad_ones_are_skipped_with_their_issues() {
        let dir = scratch("mixed");
        fs::write(dir.join("good.json"), json_of(&named("Good"))).unwrap();
        fs::write(dir.join("broken.json"), "{ this is not json").unwrap();
        fs::write(dir.join("empty.json"), "").unwrap();
        fs::write(dir.join("nohexes.json"), r#"{"version":1,"name":"No hexes","hexes":[]}"#).unwrap();
        fs::write(dir.join("typo.json"), r#"{"version":1,"name":"Typo","hexs":[]}"#).unwrap();
        fs::write(dir.join("huge.json"), " ".repeat(MAX_MAP_BYTES + 1)).unwrap();
        fs::write(dir.join("binary.json"), [0xFFu8, 0xFE, 0x00, 0x9F]).unwrap();
        fs::write(dir.join("notes.txt"), "not a map").unwrap();
        fs::write(dir.join(".hidden.json"), "{ nope").unwrap();
        fs::create_dir(dir.join("builtin")).unwrap();
        fs::write(dir.join("builtin").join("inside.json"), json_of(&named("Inside"))).unwrap();
        fs::create_dir(dir.join("dir.json")).unwrap();

        let maps = shared_with_builtins();
        let report = load_dir(&dir, &maps);

        assert_eq!(report.loaded, 1);
        assert_eq!(ids_and_names(&maps), vec![pair("standard", "Standard"), pair("earth", "Earth"), pair("usa", "USA"), pair("good", "Good")]);
        let skipped: Vec<&str> = report.skipped.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(skipped, vec!["binary.json", "broken.json", "dir.json", "empty.json", "huge.json", "nohexes.json", "typo.json"]);
        let why = |file: &str| report.skipped.iter().find(|(f, _)| f == file).unwrap().1.join(" / ");
        assert!(why("binary.json").contains("UTF-8"), "{}", why("binary.json"));
        assert!(why("broken.json").contains("line 1"), "{}", why("broken.json"));
        assert!(why("dir.json").contains("not a regular file"));
        assert!(why("huge.json").contains("limited to"));
        assert!(why("nohexes.json").contains("no hexes"));
        assert!(why("typo.json").contains("hexs"), "{}", why("typo.json"));
        assert!(report.folder_problem.is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_summary_says_what_happened() {
        let dir = scratch("summary");
        fs::write(dir.join("a.json"), json_of(&named("A"))).unwrap();
        fs::write(dir.join("b.json"), json_of(&named("B"))).unwrap();
        fs::write(dir.join("c.json"), json_of(&named("C"))).unwrap();
        fs::write(dir.join("foo.json"), r#"{"version":1,"name":"Foo","hexes":[]}"#).unwrap();
        let report = load_dir(&dir, &shared_with_builtins());
        assert_eq!(report.summary(&dir), "Loaded 3 maps, skipped 1 (foo.json: the map has no hexes)");

        let clean = scratch("summary-clean");
        fs::write(clean.join("a.json"), json_of(&named("A"))).unwrap();
        assert_eq!(load_dir(&clean, &shared_with_builtins()).summary(&clean), "Loaded 1 map");
        assert_eq!(load_dir(&scratch("summary-none"), &shared_with_builtins()).summary(&clean), "Loaded 0 maps");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&clean);
    }

    #[test]
    fn a_file_with_several_issues_is_summed_up_by_its_first() {
        let dir = scratch("several");
        // no name, no hexes' content, a bad player range: several things at once
        fs::write(dir.join("many.json"), r#"{"version":1,"name":" ","players":{"min":1,"max":9},"hexes":[{"q":0,"r":0}]}"#).unwrap();
        let report = load_dir(&dir, &shared_with_builtins());
        assert_eq!(report.skipped.len(), 1);
        assert!(report.skipped[0].1.len() >= 2, "{:?}", report.skipped);
        let line = report.summary(&dir);
        assert!(line.starts_with("Loaded 0 maps, skipped 1 (many.json: "), "{line}");
        assert!(line.contains("more)"), "{line}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_folder_is_reported_and_the_built_in_maps_stay() {
        let maps = shared_with_builtins();
        let dir = std::env::temp_dir().join(format!("settoria-no-such-folder-{}", std::process::id()));
        let report = load_dir(&dir, &maps);
        assert_eq!(report.loaded, 0);
        assert!(report.folder_problem.is_some());
        assert!(report.summary(&dir).starts_with("Loaded 0 maps (can't read the maps folder"), "{}", report.summary(&dir));
        assert_eq!(ids_and_names(&maps), vec![pair("standard", "Standard"), pair("earth", "Earth"), pair("usa", "USA")]);
    }

    #[test]
    fn a_folder_that_is_really_a_file_is_reported_too() {
        let dir = scratch("notafolder");
        let file = dir.join("maps");
        fs::write(&file, "x").unwrap();
        let report = load_dir(&file, &shared_with_builtins());
        assert!(report.folder_problem.is_some());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_ids_and_names_are_suffixed() {
        let dir = scratch("dupes");
        // earth.json clashes with the built-in Earth (id and name); the other two share a name with each other
        fs::write(dir.join("earth.json"), json_of(&named("Earth"))).unwrap();
        fs::write(dir.join("one.json"), json_of(&named("Twin"))).unwrap();
        fs::write(dir.join("two.json"), json_of(&named("twin"))).unwrap();
        // two file names that make the same id
        fs::write(dir.join("My Map.json"), json_of(&named("Mine"))).unwrap();
        fs::write(dir.join("my-map.json"), json_of(&named("Mine too"))).unwrap();
        let maps = shared_with_builtins();
        let report = load_dir(&dir, &maps);
        assert_eq!(report.loaded, 5, "{:?}", report.skipped);
        assert!(report.skipped.is_empty());
        let listed = ids_and_names(&maps);
        for want in [
            pair("earth-2", "Earth (2)"),
            pair("one", "Twin"),
            pair("two", "twin (2)"),
            pair("my-map", "Mine"),
            pair("my-map-2", "Mine too"),
        ] {
            assert!(listed.contains(&want), "{want:?} missing from {listed:?}");
        }
        assert_eq!(listed[1], pair("earth", "Earth"), "the built-in map is untouched");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_same_folder_gives_the_same_ids_on_every_start() {
        let dir = scratch("stable");
        for name in ["zeta", "alpha", "Mid"] {
            fs::write(dir.join(format!("{name}.json")), json_of(&named(name))).unwrap();
        }
        let first = shared_with_builtins();
        let second = shared_with_builtins();
        load_dir(&dir, &first);
        load_dir(&dir, &second);
        assert_eq!(ids_and_names(&first), ids_and_names(&second));
        let ids: Vec<String> = ids_and_names(&first).into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, vec!["standard", "earth", "usa", "alpha", "mid", "zeta"].into_iter().map(String::from).collect::<Vec<_>>());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_name_that_is_too_long_or_not_plain_text_is_skipped() {
        let dir = scratch("names");
        fs::write(dir.join("long.json"), json_of(&named(&"n".repeat(500)))).unwrap();
        fs::write(dir.join("lines.json"), json_of(&named("two\nlines"))).unwrap();
        let mut authored = named("Authored");
        authored.author = "a".repeat(100);
        fs::write(dir.join("author.json"), json_of(&authored)).unwrap();
        let report = load_dir(&dir, &shared_with_builtins());
        assert_eq!(report.loaded, 0);
        let why = |file: &str| report.skipped.iter().find(|(f, _)| f == file).unwrap().1.join(" / ");
        assert!(why("long.json").contains("500 characters"), "{}", why("long.json"));
        assert!(why("lines.json").contains("line break"), "{}", why("lines.json"));
        assert!(why("author.json").contains("author"), "{}", why("author.json"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_first_hundred_files_are_read() {
        let dir = scratch("many");
        for i in 0..(MAX_FOLDER_FILES + 3) {
            fs::write(dir.join(format!("m{i:03}.json")), json_of(&named(&format!("Map {i}")))).unwrap();
        }
        let maps = shared_with_builtins();
        let report = load_dir(&dir, &maps);
        assert_eq!(report.loaded, MAX_FOLDER_FILES);
        assert_eq!(infos(&maps).len(), 3 + MAX_FOLDER_FILES);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].0, "3 more files");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_warning_does_not_stop_a_map_loading() {
        let dir = scratch("warn");
        // the example map is playable; whatever warnings it has don't keep it out
        fs::write(dir.join("archipelago.json"), json_of(&archipelago())).unwrap();
        let maps = shared_with_builtins();
        let report = load_dir(&dir, &maps);
        assert_eq!(report.loaded, 1, "{:?}", report.skipped);
        assert!(ids_and_names(&maps).contains(&pair("archipelago", "Archipelago")));
        let _ = fs::remove_dir_all(&dir);
    }

    // ---------------- check_and_add: the part an upload uses ----------------

    #[test]
    fn an_uploaded_map_is_named_after_itself_and_added_as_an_upload() {
        let maps = shared_with_builtins();
        let added = check_and_add(&maps, &json_of(&named("Flower Island")), MapSource::Upload, None).unwrap();
        assert_eq!((added.id.as_str(), added.name.as_str(), added.name_changed, added.id_changed), ("flower-island", "Flower Island", false, false));
        let again = check_and_add(&maps, &json_of(&named("Flower Island")), MapSource::Upload, None).unwrap();
        assert_eq!((again.id.as_str(), again.name.as_str(), again.name_changed, again.id_changed), ("flower-island-2", "Flower Island (2)", true, true));
        assert!(infos(&maps).iter().all(|i| i.builtin == (i.id == "standard" || i.id == "earth" || i.id == "usa")));
    }

    #[test]
    fn a_bad_upload_gets_every_issue_and_adds_nothing() {
        let maps = shared_with_builtins();
        let mut def = named(" ");
        def.players = crate::map_def::PlayerRange { min: 1, max: 9 };
        let issues = match check_and_add(&maps, &json_of(&def), MapSource::Upload, None) {
            Err(AddError::Invalid(issues)) => issues,
            other => panic!("{other:?}"),
        };
        assert!(issues.len() >= 2, "{issues:?}");
        assert!(issues.iter().any(|i| i.contains("needs a name")));
        assert!(issues.iter().any(|i| i.contains("player range")));
        assert_eq!(infos(&maps).len(), 3);

        for junk in ["", "null", "[]", "42", "\"text\"", "{", "{\"version\":1}", "\u{0}\u{0}", "{\"version\":99,\"name\":\"x\",\"hexes\":[]}"] {
            assert!(matches!(check_and_add(&maps, junk, MapSource::Upload, None), Err(AddError::Invalid(_))), "{junk:?}");
        }
        assert_eq!(infos(&maps).len(), 3);
    }

    #[test]
    fn an_oversized_text_is_refused_even_if_it_reaches_this_far() {
        let maps = shared_with_builtins();
        let big = format!("{{\"version\":1,\"name\":\"x\",\"hexes\":[],\"pad\":\"{}\"}}", "x".repeat(MAX_MAP_BYTES));
        match check_and_add(&maps, &big, MapSource::Upload, None) {
            Err(AddError::Invalid(issues)) => assert!(issues[0].contains("limited to"), "{issues:?}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_eleventh_upload_is_turned_away_as_full() {
        let maps = shared_with_builtins();
        for i in 0..MAX_UPLOADS {
            check_and_add(&maps, &json_of(&named(&format!("Up {i}"))), MapSource::Upload, None).unwrap();
        }
        match check_and_add(&maps, &json_of(&named("Too many")), MapSource::Upload, None) {
            Err(AddError::Full(why)) => assert!(why.contains("10"), "{why}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(infos(&maps).len(), 3 + MAX_UPLOADS);
        // folder maps are not uploads
        assert!(check_and_add(&maps, &json_of(&named("From a file")), MapSource::Folder, Some("from-a-file")).is_ok());
    }
}