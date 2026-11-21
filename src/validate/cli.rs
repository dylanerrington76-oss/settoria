//! The command line: `catan-lan validate map.json ...`

use super::{validate, Report};
use crate::map_def::{MapDef, MAX_MAP_BYTES};
use std::io::Write;

const USAGE: &str = "usage: catan-lan validate <map.json> [more.json ...]

Checks map files and says everything that is wrong with them (errors) or worth knowing (warnings).
Exit status: 0 if every map is playable, 1 if any has errors, 2 if a file can't be read or parsed.";

const EXIT_PLAYABLE: i32 = 0;
const EXIT_HAS_ERRORS: i32 = 1;
const EXIT_UNUSABLE: i32 = 2;

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

/// Read and parse one map file. The error says what is wrong with the file, without its name.
fn load_map(path: &str) -> Result<MapDef, String> {
    let size = std::fs::metadata(path).map_err(|e| format!("can't read the file: {e}"))?.len();
    if size > MAX_MAP_BYTES as u64 {
        return Err(format!("the file is {size} bytes, but maps are limited to {MAX_MAP_BYTES} bytes"));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("can't read the file: {e}"))?;
    MapDef::from_json(&text).map_err(|e| format!("not a valid map file: {e}"))
}

/// Run the `validate` command. Reports go to `out`; problems with the files themselves and usage
/// go to `err`. Returns the exit status.
pub fn run_cli(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        let _ = writeln!(err, "{USAGE}");
        return EXIT_UNUSABLE;
    }
    let mut status = EXIT_PLAYABLE;
    for path in args {
        let def = match load_map(path) {
            Ok(def) => def,
            Err(why) => {
                let _ = writeln!(err, "{path}: {why}");
                status = status.max(EXIT_UNUSABLE);
                continue;
            }
        };
        let settings = def.recommended_settings.unwrap_or_default();
        let report = validate(&def, &settings);
        let _ = print_report(out, path, &def, &report);
        if !report.is_ok() {
            status = status.max(EXIT_HAS_ERRORS);
        }
    }
    status
}
