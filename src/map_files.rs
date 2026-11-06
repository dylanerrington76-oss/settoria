//! The plain-text side of loading maps: finding and reading map files, and making ids and names that are
//! safe to show and unique.
//!
//! Nothing in here knows what a map is (it only uses the standard library), so it can be read, and tested,
//! on its own. `map_loader` puts it together with parsing, `validate` and the registry.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The first `max` characters of `text`.
pub fn truncate_chars(text: &str, max: usize) -> &str {
    match text.char_indices().nth(max) {
        Some((i, _)) => &text[..i],
        None => text,
    }
}

/// Text from a file name or a file's contents, made safe to print on one line: control characters (which
/// could start a new log line or send escape codes to the terminal) become `?`.
pub fn printable(text: &str) -> String {
    text.chars().map(|c| if c.is_control() { '?' } else { c }).collect()
}

/// A map id made from a file name without its extension: lower case, and anything but `a-z 0-9 _ .` becomes
/// `-` (runs of them become one), with no `-` or `.` at either end. An empty result is `map`.
pub fn id_from_stem(stem: &str, max_len: usize) -> String {
    let mut out = String::new();
    for c in stem.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    // Only ASCII is left, so cutting at a byte is cutting at a character.
    let cut = truncate_chars(out.trim_matches(|c| c == '-' || c == '.'), max_len);
    let cut = cut.trim_end_matches(|c| c == '-' || c == '.');
    if cut.is_empty() {
        "map".to_string()
    } else {
        cut.to_string()
    }
}

/// `base`, or if `taken` says it is, `base-2`, `base-3`, ... (the base is shortened to leave room, so the
/// result is never longer than `max_len`).
pub fn unique_id(base: &str, max_len: usize, taken: impl Fn(&str) -> bool) -> String {
    let base = truncate_chars(base, max_len);
    if !taken(base) {
        return base.to_string();
    }
    let mut n: u32 = 2;
    loop {
        let suffix = format!("-{n}");
        let room = max_len.saturating_sub(suffix.len());
        let candidate = format!("{}{}", truncate_chars(base, room), suffix);
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// `name`, or if `taken` says it is, `name (2)`, `name (3)`, ... (the name is shortened to leave room, so
/// the result is never longer than `max_chars` characters).
pub fn unique_name(name: &str, max_chars: usize, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_string();
    }
    let mut n: u32 = 2;
    loop {
        let suffix = format!(" ({n})");
        let room = max_chars.saturating_sub(suffix.chars().count());
        let candidate = format!("{}{}", truncate_chars(name, room).trim_end(), suffix);
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Names compare the way a person reads them: `Earth` and ` earth ` are the same name.
pub fn name_key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// A set of names (as `name_key`s) to ask "is this one taken?" of.
pub fn name_set<'a>(names: impl Iterator<Item = &'a str>) -> HashSet<String> {
    names.map(name_key).collect()
}

/// The `*.json` files directly inside `dir` (not in folders under it), sorted by name so that the order, and
/// so the ids they get, are the same on every start. The match on `.json` ignores case. Files whose names
/// start with `.` are left out: editors and macOS leave hidden scratch files beside real ones.
pub fn list_json_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = vec![];
    for entry in std::fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_json = path.extension().map_or(false, |e| e.eq_ignore_ascii_case("json"));
        if is_json && !name.starts_with('.') {
            files.push(path);
        }
    }
    // By name without regard to case (`Mid.json` between `alpha.json` and `zeta.json`); the exact name breaks ties.
    files.sort_by_key(|p| {
        let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
        (name.to_lowercase(), name)
    });
    Ok(files)
}

/// A file's name as shown in logs and messages.
pub fn file_label(path: &Path) -> String {
    printable(&path.file_name().unwrap_or_default().to_string_lossy())
}

/// A file's name without its extension: what its map's id is made from.
pub fn file_stem(path: &Path) -> String {
    path.file_stem().unwrap_or_default().to_string_lossy().into_owned()
}

/// Read a text file of at most `max_bytes`. The size is checked before reading, and again on what was read
/// (a file can grow in between, and a special file can claim to be small), so a huge or endless file is
/// never read whole. Only regular files are read: opening a named pipe would wait forever. The error is a
/// sentence for the person who owns the file.
pub fn read_capped(path: &Path, max_bytes: usize) -> Result<String, String> {
    let too_big = |what: String| format!("{what}, but maps are limited to {max_bytes} bytes");
    let meta = std::fs::metadata(path).map_err(|e| format!("can't read the file: {e}"))?;
    if !meta.is_file() {
        return Err("it is not a regular file".to_string());
    }
    if meta.len() > max_bytes as u64 {
        return Err(too_big(format!("the file is {} bytes", meta.len())));
    }
    let file = std::fs::File::open(path).map_err(|e| format!("can't read the file: {e}"))?;
    let mut bytes = Vec::new();
    file.take(max_bytes as u64 + 1).read_to_end(&mut bytes).map_err(|e| format!("can't read the file: {e}"))?;
    if bytes.len() > max_bytes {
        return Err(too_big("the file is bigger than that".to_string()));
    }
    String::from_utf8(bytes).map_err(|_| "the file is not UTF-8 text (save it as UTF-8)".to_string())
}

/// The startup summary, e.g. `Loaded 3 maps, skipped 1 (foo.json: the map has no hexes)`. `skipped` is each
/// skipped file with a short reason. Only the first few are named; the log above has all of them.
pub fn summary(loaded: usize, skipped: &[(String, String)]) -> String {
    const NAMED: usize = 5;
    const REASON_CHARS: usize = 120;
    let mut s = format!("Loaded {loaded} {}", if loaded == 1 { "map" } else { "maps" });
    if !skipped.is_empty() {
        let named: Vec<String> = skipped
            .iter()
            .take(NAMED)
            .map(|(file, why)| {
                let short = truncate_chars(why, REASON_CHARS);
                let dots = if short.len() < why.len() { "…" } else { "" };
                format!("{file}: {short}{dots}")
            })
            .collect();
        s.push_str(&format!(", skipped {} ({}", skipped.len(), named.join("; ")));
        if skipped.len() > NAMED {
            s.push_str(&format!("; and {} more", skipped.len() - NAMED));
        }
        s.push(')');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A new empty folder for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("settoria-map-files-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn truncation_is_by_characters() {
        assert_eq!(truncate_chars("hello", 3), "hel");
        assert_eq!(truncate_chars("hello", 5), "hello");
        assert_eq!(truncate_chars("hello", 50), "hello");
        assert_eq!(truncate_chars("héllo", 2), "hé");
        assert_eq!(truncate_chars("日本語", 2), "日本");
        assert_eq!(truncate_chars("abc", 0), "");
    }

    #[test]
    fn printable_removes_what_could_garble_a_log() {
        assert_eq!(printable("a\nb\u{1b}[31mc\td"), "a?b?[31mc?d");
        assert_eq!(printable("plain é 日本"), "plain é 日本");
    }

    #[test]
    fn ids_come_from_file_names() {
        assert_eq!(id_from_stem("archipelago", 64), "archipelago");
        assert_eq!(id_from_stem("My Map (v2)", 64), "my-map-v2");
        assert_eq!(id_from_stem("Earth", 64), "earth");
        assert_eq!(id_from_stem("a.b_c-d", 64), "a.b_c-d");
        assert_eq!(id_from_stem("--a   b--", 64), "a-b");
        assert_eq!(id_from_stem("../etc/passwd", 64), "etc-passwd");
        assert_eq!(id_from_stem("tab\there\nnewline", 64), "tab-here-newline");
        assert_eq!(id_from_stem("日本語", 64), "map");
        assert_eq!(id_from_stem("", 64), "map");
        assert_eq!(id_from_stem("...", 64), "map");
        assert_eq!(id_from_stem(&"x".repeat(100), 64), "x".repeat(64));
        assert_eq!(id_from_stem("aaaa-bbbb", 5), "aaaa", "no dash left dangling by the cut");
    }

    #[test]
    fn every_id_it_makes_is_one_the_registry_accepts() {
        let ok = |id: &str| !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || "-_:.".contains(c));
        for stem in ["", " ", "ünï", "a/b", "x".repeat(500).as_str(), "\u{0}\u{1}", "😀😀", "Map #1", "..", "-"] {
            assert!(ok(&id_from_stem(stem, 64)), "{stem:?} -> {:?}", id_from_stem(stem, 64));
        }
    }

    #[test]
    fn a_taken_id_gets_a_number() {
        let taken: HashSet<&str> = ["earth", "earth-2", "earth-3"].into_iter().collect();
        assert_eq!(unique_id("flower", 64, |i| taken.contains(i)), "flower");
        assert_eq!(unique_id("earth", 64, |i| taken.contains(i)), "earth-4");
    }

    #[test]
    fn a_suffixed_id_still_fits() {
        let full = "x".repeat(64);
        let id = unique_id(&full, 64, |i| i == full);
        assert_eq!(id.len(), 64);
        assert!(id.ends_with("-2"), "{id}");
        let again = unique_id(&full, 64, |i| i == full || i == id);
        assert_eq!(again.len(), 64);
        assert!(again.ends_with("-3"), "{again}");
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let taken = name_set(["Earth", "Flower", "Flower (2)"].into_iter());
        let is_taken = |n: &str| taken.contains(&name_key(n));
        assert_eq!(unique_name("Trio", 32, is_taken), "Trio");
        assert_eq!(unique_name("Flower", 32, is_taken), "Flower (3)");
        assert_eq!(unique_name("earth", 32, is_taken), "earth (2)", "names are compared without regard to case");
        assert_eq!(unique_name("  EARTH ", 32, is_taken), "  EARTH (2)", "spaces around a name don't make it different");
    }

    #[test]
    fn a_suffixed_name_still_fits() {
        let long = "n".repeat(32);
        let name = unique_name(&long, 32, |n| n == long);
        assert_eq!(name.chars().count(), 32);
        assert!(name.ends_with(" (2)"), "{name}");
        let wide = "é".repeat(32);
        let name = unique_name(&wide, 32, |n| n == wide);
        assert_eq!(name.chars().count(), 32);
        assert!(name.ends_with(" (2)"), "{name}");
        // a name that ends in a space where it was cut doesn't leave a double space
        let spaced = format!("{} tail", "n".repeat(27));
        let name = unique_name(&spaced, 32, |n| n == spaced);
        assert!(!name.contains("  "), "{name:?}");
        assert!(name.chars().count() <= 32);
    }

    #[test]
    fn only_json_files_directly_in_the_folder_are_listed_in_name_order() {
        let dir = scratch("list");
        for name in ["b.json", "a.json", "C.JSON", "notes.txt", "noext", ".hidden.json", "._mac.json", "d.json.bak"] {
            fs::write(dir.join(name), "{}").unwrap();
        }
        fs::create_dir(dir.join("builtin")).unwrap();
        fs::write(dir.join("builtin").join("inside.json"), "{}").unwrap();
        let names: Vec<String> = list_json_files(&dir).unwrap().iter().map(|p| file_label(p)).collect();
        assert_eq!(names, vec!["a.json", "b.json", "C.JSON"], "sorted by name (not by case), `.JSON` counts, no hidden files, no sub-folders");
        assert!(list_json_files(&dir.join("missing")).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_named_like_a_file_is_listed_and_then_refused_when_read() {
        let dir = scratch("dirjson");
        fs::create_dir(dir.join("odd.json")).unwrap();
        let files = list_json_files(&dir).unwrap();
        assert_eq!(files.len(), 1);
        assert!(read_capped(&files[0], 1000).unwrap_err().contains("not a regular file"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_is_read_up_to_the_cap_and_no_further() {
        let dir = scratch("cap");
        let p = dir.join("m.json");
        fs::write(&p, "x".repeat(100)).unwrap();
        assert_eq!(read_capped(&p, 100).unwrap().len(), 100, "exactly the cap is fine");
        let e = read_capped(&p, 99).unwrap_err();
        assert!(e.contains("100 bytes") && e.contains("limited to 99"), "{e}");
        assert_eq!(read_capped(&p, 1_000_000).unwrap().len(), 100);
        assert!(read_capped(&dir.join("gone.json"), 100).unwrap_err().contains("can't read"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_is_not_utf8_is_refused_with_advice() {
        let dir = scratch("utf8");
        let p = dir.join("m.json");
        // what Windows Notepad's "Unicode" option writes: UTF-16 with a byte-order mark
        fs::write(&p, [0xFF, 0xFE, b'{', 0, b'}', 0]).unwrap();
        assert!(read_capped(&p, 100).unwrap_err().contains("UTF-8"));
        // a UTF-8 byte-order mark is fine here (the map reader skips it)
        fs::write(&p, "\u{feff}{}").unwrap();
        assert_eq!(read_capped(&p, 100).unwrap(), "\u{feff}{}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_device_that_never_ends_is_not_read_whole() {
        // /dev/zero is "a file" that is endless; it is not a regular file, so it is refused outright
        assert!(read_capped(Path::new("/dev/zero"), 100).is_err());
    }

    #[test]
    fn file_names_are_shown_and_stemmed_safely() {
        let p = Path::new("maps/My Map.json");
        assert_eq!(file_label(p), "My Map.json");
        assert_eq!(file_stem(p), "My Map");
        assert_eq!(file_label(Path::new("a\nb.json")), "a?b.json");
    }

    #[test]
    fn the_summary_reads_like_the_example() {
        assert_eq!(summary(0, &[]), "Loaded 0 maps");
        assert_eq!(summary(1, &[]), "Loaded 1 map");
        assert_eq!(summary(3, &[]), "Loaded 3 maps");
        let one = [("foo.json".to_string(), "the map has no hexes".to_string())];
        assert_eq!(summary(3, &one), "Loaded 3 maps, skipped 1 (foo.json: the map has no hexes)");
        let two = [("a.json".to_string(), "x".to_string()), ("b.json".to_string(), "y".to_string())];
        assert_eq!(summary(0, &two), "Loaded 0 maps, skipped 2 (a.json: x; b.json: y)");
    }

    #[test]
    fn the_summary_stays_short_however_much_went_wrong() {
        let many: Vec<(String, String)> = (0..9).map(|i| (format!("f{i}.json"), "y".repeat(500))).collect();
        let s = summary(1, &many);
        assert!(s.starts_with("Loaded 1 map, skipped 9 ("), "{s}");
        assert!(s.contains("f4.json") && !s.contains("f5.json"), "{s}");
        assert!(s.ends_with("; and 4 more)"), "{s}");
        assert!(s.contains('…'));
        assert!(s.chars().count() < 1000, "{}", s.chars().count());
    }
}
