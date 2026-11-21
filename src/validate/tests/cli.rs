//! The `validate` command.

use super::*;
use crate::validate::run_cli;

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
