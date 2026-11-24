//! How a board goes over the wire.
//!
//! A board used to be sent whole: every hex with its drawing position as two 17-digit floats, every corner
//! with its position again, every side as a pair of corner numbers. All of that follows from where the hexes
//! are, so the browser is now sent only what it can't work out:
//!
//! ```json
//! { "map": "earth", "style": { "sea_grid": true },
//!   "hexes": [[5, 0, "forest", 8], [6, 0, "desert", null], ...],
//!   "ports": [{ "a": 12, "b": 13, "resource": null }, ...],
//!   "bridges": [[40, 41], ...],
//!   "desert": 17 }
//! ```
//!
//! - `hexes` are `[q, r, terrain, number]`, in the order of the map's file. That order is what numbers the
//!   corners and sides, so it must not change.
//! - `bridges` are the bridge sites as pairs of corner numbers, in edge order (left out when the map has
//!   none). The browser numbers them on from the last land side. They are sent rather than worked out because
//!   there are few of them and the search for them (`add_bridge_sites`) would otherwise exist twice.
//! - `ports` and `desert` are as they were.
//!
//! The browser (web/src/boardGeometry.ts) numbers the corners and sides exactly as `Geometry::build` does.
//! The test `golden` writes down what this side builds for every map into `web/scripts/board-golden.json`,
//! fails if the file is out of date, and `web/scripts/board-geometry-test.mjs` checks the browser against it,
//! so the two can't drift apart unnoticed.

use crate::board::Board;
use serde::ser::{Serialize, SerializeMap, Serializer};

/// The hexes as `[q, r, terrain, number]`.
struct Hexes<'a>(&'a Board);

impl Serialize for Hexes<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.hexes.iter().map(|h| (h.q, h.r, h.terrain, h.number)))
    }
}

/// The bridge sites as `[a, b]`, in edge order. They are the last edges (`add_bridge_sites` appends them).
struct Bridges<'a>(&'a Board);

impl Serialize for Bridges<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.edges.iter().filter(|e| e.bridge).map(|e| (e.a, e.b)))
    }
}

impl Serialize for Board {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let has_bridges = self.edges.iter().any(|e| e.bridge);
        let mut board = serializer.serialize_map(Some(5 + usize::from(has_bridges)))?;
        board.serialize_entry("map", &self.map)?;
        board.serialize_entry("style", &self.style)?;
        board.serialize_entry("hexes", &Hexes(self))?;
        board.serialize_entry("ports", &self.ports)?;
        if has_bridges {
            board.serialize_entry("bridges", &Bridges(self))?;
        }
        board.serialize_entry("desert", &self.desert)?;
        board.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::{archipelago, builtin};
    use crate::map_def::MapDef;
    use rand::rngs::StdRng;
    use rand::SeedableRng;
    use serde_json::{json, Value};
    use std::path::Path;

    /// Every map whose geometry the browser is checked against: the built-in ones, and the map files that sit
    /// beside them in `maps/builtin` without being built in.
    fn maps() -> Vec<(&'static str, MapDef)> {
        let mut out: Vec<(&'static str, MapDef)> = ["standard", "earth", "usa"].iter().map(|&id| (id, builtin(id).expect("built in").clone())).collect();
        for (id, text) in [
            ("europe", include_str!("../maps/builtin/europe.json")),
            ("texas", include_str!("../maps/builtin/texas.json")),
            ("maritime southeast asia", include_str!("../maps/builtin/maritime_southeast_asia.json")),
        ] {
            out.push((id, MapDef::from_json(text).unwrap_or_else(|e| panic!("`{id}` is not a map: {e}"))));
        }
        out.push(("archipelago", archipelago()));
        out
    }

    fn board_of(id: &str, def: &MapDef) -> Board {
        Board::from_def(def, id, &mut StdRng::seed_from_u64(7)).unwrap_or_else(|e| panic!("`{id}` does not build: {e}"))
    }

    fn json_of(b: &Board) -> Value {
        serde_json::to_value(b).unwrap()
    }

    #[test]
    fn a_board_is_sent_in_its_short_form() {
        let b = board_of("standard", builtin("standard").unwrap());
        let v = json_of(&b);
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["desert", "hexes", "map", "ports", "style"], "no corners, no sides, and no bridges on an island");
        assert_eq!(v["map"], "standard");
        assert_eq!(v["desert"], b.desert);
        let hexes = v["hexes"].as_array().unwrap();
        assert_eq!(hexes.len(), 19);
        for (sent, h) in hexes.iter().zip(&b.hexes) {
            assert_eq!(sent, &json!([h.q, h.r, h.terrain, h.number]));
        }
        assert!(hexes.iter().any(|h| h[3].is_null()), "the desert has no number: null, not left out");
        assert_eq!(v["ports"].as_array().unwrap().len(), 9);
        assert!(v["ports"][0].get("a").is_some() && v["ports"][0].get("resource").is_some());
    }

    #[test]
    fn bridge_sites_are_sent_as_pairs_in_edge_order() {
        let b = board_of("earth", builtin("earth").unwrap());
        let land = b.edges.iter().filter(|e| !e.bridge).count();
        assert!(b.edges[..land].iter().all(|e| !e.bridge) && b.edges[land..].iter().all(|e| e.bridge), "the bridge sites are the last edges");
        let v = json_of(&b);
        let sent = v["bridges"].as_array().expect("Earth has bridge sites");
        assert_eq!(sent.len(), b.edges.len() - land);
        for (pair, e) in sent.iter().zip(&b.edges[land..]) {
            assert_eq!(pair, &json!([e.a, e.b]));
        }
    }

    /// The point of it all: how many bytes a whole-board message costs. A ceiling per map, with some room, so
    /// that putting the corners back (or anything as large) fails here rather than going unnoticed.
    #[test]
    fn boards_stay_small() {
        // (map, ceiling in bytes). They come to about 0.7, 2.6 and 3.5 KB; before the short form they were about 4.9, 21.5 and 31.6 KB.
        for (id, ceiling) in [("standard", 800), ("earth", 3_000), ("usa", 4_000)] {
            let b = board_of(id, builtin(id).unwrap());
            let size = serde_json::to_string(&b).unwrap().len();
            assert!(size <= ceiling, "the {id} board is {size} bytes, over the {ceiling} it is allowed");
        }
    }

    // ------------------------------------------------------------------
    // The golden file
    // ------------------------------------------------------------------

    /// Where the file the browser's test reads lives.
    fn golden_path() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("web/scripts/board-golden.json")
    }

    /// What this side builds for every map, as the browser has to rebuild it: where the hexes are (in order) and
    /// where each is drawn, each corner's position (both in millionths), each land side as a pair of corners, each bridge site likewise.
    /// One map to a line, so a change shows up as the line it is.
    fn golden_text() -> String {
        let mut lines = vec![];
        for (id, def) in maps() {
            let b = board_of(id, &def);
            let land = b.edges.iter().filter(|e| !e.bridge).count();
            let entry = json!({
                "hexes": b.hexes.iter().map(|h| json!([h.q, h.r])).collect::<Vec<_>>(),
                "centres": b.hexes.iter().map(|h| json!([(h.x * 1e6).round() as i64, (h.y * 1e6).round() as i64])).collect::<Vec<_>>(),
                "vertices": b.vertices.iter().map(|v| json!([(v.x * 1e6).round() as i64, (v.y * 1e6).round() as i64])).collect::<Vec<_>>(),
                "edges": b.edges[..land].iter().map(|e| json!([e.a, e.b])).collect::<Vec<_>>(),
                "bridges": b.edges[land..].iter().map(|e| json!([e.a, e.b])).collect::<Vec<_>>(),
            });
            lines.push(format!("  {}: {}", serde_json::to_string(id).unwrap(), serde_json::to_string(&entry).unwrap()));
        }
        format!("{{\n{}\n}}\n", lines.join(",\n"))
    }

    /// `web/scripts/board-golden.json` is what `web/scripts/board-geometry-test.mjs` holds the browser's
    /// corner and side numbering to. This keeps it true to the server. After a change to the geometry on
    /// purpose, run the test with `UPDATE_GOLDEN=1` to write the file again (and the browser's code must then
    /// be changed to match, which the other test checks).
    #[test]
    fn golden() {
        let now = golden_text();
        let path = golden_path();
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::write(&path, &now).expect("write the golden file");
            return;
        }
        let was = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            was == now,
            "{} is out of date with the board geometry. If the geometry was changed on purpose, run \
             `UPDATE_GOLDEN=1 cargo test --release golden` and change web/src/boardGeometry.ts to match.",
            path.display()
        );
    }
}
