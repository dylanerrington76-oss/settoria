# Maps

A map is a JSON file describing which hexes exist, which terrain and number tokens are fixed and which are dealt at
random, and where the harbours go. Standard, Earth and USA are map files like any other, so a map you write is never
second-class. This page covers the maps that come with the game, how to get yours into the lobby, the map editor, the
file format and the `validate` command.

- [Maps that come with the game](#maps-that-come-with-the-game)
- [Putting your maps in the lobby](#putting-your-maps-in-the-lobby)
- [The map editor](#the-map-editor)
- [The map file](#the-map-file)
- [Volcanoes and gold](#volcanoes-and-gold)
- [Checking a map: `validate`](#checking-a-map-validate)

## Maps that come with the game

The files are in `maps/builtin/`. A map in `BUILTIN_MAPS` (`src/builtin.rs`) is compiled into the program with
`include_str!`, so the binary needs nothing next to it, and it is always in the lobby. Built-in maps are never replaced
by a file of the same name.

| Map (`id`) | Hexes | Players | Harbours | Notes |
|---|---|---|---|---|
| Standard (`standard`) | 19 | 2 to 4 | 9 on the coast ring (4 × 3:1, one 2:1 per resource) | A single island, randomised every game: 4 forest, 4 pasture, 4 fields, 3 hills, 3 mountains, 1 desert, and 18 number tokens. |
| Earth (`earth`) | 81 | 2 to 12 | 27, fixed | Seven islands, with a faint hex grid drawn across the sea (`sea_grid`). |
| USA (`usa`) | 140 | 2 to 12 | 25, fixed | One big island; every terrain, token and harbour kind is dealt at random. |
| Texas | 65 | 2 to 4 | 10, fixed | **File only** (see below). |
| Europe | 106 | 2 to 12 | 26, fixed | **File only** (see below). By Dylan Errington. |

**Texas and Europe are files in `maps/builtin/` but are not built in:** they are not in `BUILTIN_MAPS`, and the maps
folder is not searched below its top level, so a server started as it is does not offer them in the lobby. (Their
board geometry is still checked by the tests.) To play them, copy `texas.json` or `europe.json` into `maps/` (see
below), or upload them. To make one built in, add it to `BUILTIN_MAPS`, whose length is part of its type.

None of these maps contains a [volcano or gold](Rules.md#special-hexes-volcano-and-gold) hex. Make a map with them in the
[editor](#the-map-editor).

The built-in files make good templates. Standard shows a single island with `"ring"` harbours, Earth shows fixed
harbours, a fixed desert and a constraint, and USA shows a large island with fixed harbour spots and everything else
dealt at random. The order of the hexes in a file decides how corners and sides are numbered (it is part of
the [protocol](Protocol.md#the-board-on-the-wire)), so the built-in files keep a fixed order; it doesn't matter for
a map of your own. `examples/maps/archipelago.json` is a small example written the way a person would write one.

## Putting your maps in the lobby

**From a folder.** When the server starts it reads every `*.json` file directly inside `MAPS_DIR` (default `maps/`).
Folders under it are not searched (so `maps/builtin/` is not loaded twice), and files starting with `.` are ignored.
For each file it checks the size (256 KB at most), parses it and validates it exactly as [`validate`](#checking-a-map-validate)
does, then adds it to the lobby. Files are read in name order, up to 100 of them, so the same folder gives the same ids
every time. Restart the server to pick up a new file; with Docker, `./maps` is mounted read-only, so use
`docker compose restart settoria`.

A file that is no good never stops the server. It is logged to stderr with its name and every issue, then skipped,
and the startup banner says what happened:

    maps: skipped foo.json: the map has no hexes
      Maps folder:    maps: Loaded 3 maps, skipped 1 (foo.json: the map has no hexes)

The id of a map is its file name without `.json`, made safe (`My Map.json` is `my-map`). If an id or a name is already
taken (by a built-in map, or by another file) a number is added: `earth.json` holding a map called "Earth" loads as
`earth-2`, "Earth (2)". Names are compared without regard to case.

**By upload.** The host can add a map while the server runs with `POST /api/maps` (see
[Protocol](Protocol.md#adding-a-map-http)). Uploaded maps are kept in memory only, so they are gone when the server
stops, and at most 10 can be uploaded between restarts.

Whichever way a map arrives, its `name` and `author` are at most 32 characters of plain text (no line breaks, control
characters or invisible characters), because the lobby shows them in small buttons.

In the lobby, maps for another number of players than are seated are greyed out, so 5 to 12 players play on Earth,
USA, Europe or a map that says so. A map may also suggest game rules (`recommended_settings`); the lobby offers them to the
host and never applies them by itself.

## The map editor

The **map editor** link at the bottom of the lobby (`/map-editor`) builds a map file without writing JSON. Drag pieces
from the bar onto the blue sea and move them around; drag a piece back onto the bar (or select it and press Delete) to
remove it. Ctrl+Z and Ctrl+Shift+Z undo and redo, the wheel zooms and dragging the water pans.

| Piece | In the file |
|---|---|
| Striped hex (red and black) | a hex whose terrain is dealt at random: any resource, or a desert |
| Desert | a hex with `"terrain": "desert"` |
| Volcano | a numbered hex with `"terrain": "volcano"`: free resources, then lava (see [Volcanoes and gold](#volcanoes-and-gold)) |
| Gold | a numbered hex with `"terrain": "gold"`: free resources, no lava |
| Port | a harbour on the coast of a hex, its kind dealt at random (a port can only sit on a coast edge; a hex placed against it removes it) |

Striped means "dealt at random" everywhere in the editor. The `terrain_pool`, `token_pool` and harbour `pool` are worked
out from what is on the map, so they always add up: resources are shared out evenly (Standard's 4/4/4/3/3 split when the
numbers don't divide), number tokens follow the Standard set, and about a fifth of the harbours are 3:1. **Settings** sets
the author, the player range (by default worked out from how many starting settlements fit) and how many of the striped
hexes become deserts (by default one if you placed no desert yourself). The editor runs the same checks as `validate`
(name, room for the starting settlements, a volcano touching a 6 or 8, and so on) and only offers **Download** when the
map would be accepted; problems and notes are listed from the status button.

**Import** (or dropping a file on the page) opens an existing map file to edit, such as the ones in `maps/builtin`.
Fixed terrain, numbers, harbour kinds, `robber_start`, `recommended_settings` and the pools are kept, and a map that is
opened and saved without changes comes out the same. Maps with `"mode": "ring"` harbours are opened with the harbours at
fixed positions.

The editor's checks are a second copy of the server's (`web/src/mapModel.ts` follows `src/validate.rs`), so the server's
verdict is the one that counts.

## The map file

A map is a `MapDef` (`src/map_def.rs`) written as JSON. Unknown fields are refused, so a misspelt field is an error
rather than silently ignored, and a mistake in the JSON itself is reported with line and column. A value that is dealt at
random is written `"?"` (or left out).

| Field | Meaning |
|---|---|
| `version` | Always `1`. |
| `name`, `author` | Shown in the lobby (at most 32 characters each). |
| `players` | `{ "min": 2, "max": 12 }`, within 2 to 12 (2 to 4 if left out). |
| `hexes` | `{ "q": 1, "r": 0 }` per land hex (axial coordinates in `-100` to `100`, at most 500 hexes). Add `"terrain"` and/or `"number"` to fix them. |
| `terrain_pool` | Terrains dealt to the hexes whose terrain isn't fixed: `{ "forest": 4, "desert": 1, ... }`. Must add up to exactly that many hexes. |
| `token_pool` | Number tokens dealt to the numbered hexes whose number isn't fixed: `{ "6": 2, ... }` (2 to 12, never 7). Must add up to exactly the number of numbered hexes still waiting for a token. |
| `ports` | `{ "mode": "fixed", "spots": [{ "q", "r", "side" }], "pool": { "any": 4, "ore": 1, ... } }`, or `{ "mode": "ring", "spacing": [3,4,...], "pool": {...} }` for a single island (gaps between harbours, in coast edges, adding up to the coast's length). A spot may fix its `"kind"`. |
| `robber_start` | `{ "q", "r" }`. Default: a random desert (a map with no desert must say). |
| `constraints` | `spread_tokens` (default `true`: no equal numbers touching and no 6/8 touching) and `deserts_avoid_fixed_deserts` (default `false`: deserts dealt from the pool never touch a desert written into the map). |
| `recommended_settings` | The rules the map suggests (see [Rules](Rules.md#game-settings)); the validator checks room for the starting settlements against them. |
| `style` | How the browser draws the map; cosmetic only. `{ "sea_grid": true }` draws a faint hex grid across the open sea (Earth does). Unknown keys are refused. |

- **Terrains** are `hills`, `forest`, `mountains`, `fields`, `pasture`, `desert`, `volcano` and `gold`.
- **Harbour kinds** are `any` (3:1) and one for each resource: `brick`, `lumber`, `ore`, `grain` and `wool` (2:1).
- **Harbour sides** run clockwise from the east: 0 east, 1 south-east, 2 south-west, 3 west, 4 north-west, 5 north-east.
  A harbour can only sit on a coast edge.
- **Bridge sites** are not in the file: they are worked out from the geometry when a game with bridges starts (see
  [Rules](Rules.md#bridges)).
- **Number tokens** go to every hex that has one: all the resource hexes, gold and volcanoes. A desert never has one.

An example of the hexes of a map that fixes some things:

    "hexes": [
      { "q": 0, "r": 0, "terrain": "desert" },
      { "q": 1, "r": 0, "number": 6 },
      { "q": 2, "r": 0, "terrain": "gold" },
      { "q": 1, "r": 1, "terrain": "volcano", "number": 8 },
      { "q": 0, "r": 1 }
    ]

## Volcanoes and gold

Both are written like any other terrain, either on a hex (`"terrain": "volcano"`) or in the `terrain_pool`
(`"volcano": 2`), and both take a number token from the pool like a resource hex. The rules for how they play are in
[Rules](Rules.md#special-hexes-volcano-and-gold); this is what a map author needs to know.

- **A volcano is always a 6 or an 8.** A volcano written into the map keeps its number if it is a 6 or 8, and otherwise
  takes a 6 or 8 from the `token_pool` if one is left there. If the pool has none left, a random token is dropped from
  the pool and a 6 or 8 is made up, so the counts still add up. A volcano dealt from the `terrain_pool` goes on a hex
  that has, or can be given, a 6 or 8.
- **A volcano may not touch another 6 or 8** while `spread_tokens` is on (the default). That is an error
  (`VolcanoTouchesHot`) for a volcano you wrote in, and `NotEnoughVolcanoSpots` when the pool holds more volcanoes
  than there are hexes that can take one. Turn `spread_tokens` off to allow it.
- **Gold takes an ordinary number** from the pool, with the usual rules about spreading tokens.
- Neither makes a resource by itself. `validate` counts every hex that is not a desert as "producing", volcanoes and
  gold included, so it won't object to a map with no resource hexes at all; it does warn when one of the five
  resources has no hex (*missing resource*). Give a map with volcanoes ordinary resource hexes as well, because lava
  wrecks what is built beside a volcano.

## Checking a map: `validate`

    cargo run --release -- validate examples/maps/archipelago.json [more.json ...]

(With Docker: `docker compose run --rm settoria validate /app/maps/my-map.json`.) It lists *every* problem in one
pass and starts no server. **Errors** make a map unusable; **warnings** are worth knowing but leave it playable. Exit
status: 0 = playable, 1 = the map has errors, 2 = a file can't be read or parsed.

It works out, besides the usual mistakes, whether the starting settlements really fit under the distance rule (exactly,
not by estimate: the most players the map allows, times the starting settlements of its `recommended_settings` or of the
default rules), and tries dealing the number tokens up to ten times to see whether the map can be dealt reliably. For
each map it prints a summary: hexes, islands, how many are not deserts (shown as "producing"), harbours, settlement
room and the count of every terrain.

| Warnings | Meaning |
|---|---|
| Tight settlement room | Setup would fit if everyone placed perfectly, but careless placement could run out of corners. |
| Missing resource | No hex of that terrain exists, so the resource is only available through the bank. |
| Lopsided resources | One resource is far scarcer than another. |
| Island without resources | A whole island of nothing but desert. |
| No harbours | Players can only trade 4:1 with the bank. |
| Harbours share a corner | Two harbours meet at a corner (one settlement uses both, and their pictures overlap). |
| Deal often fails | Some trial deals of the tokens failed, so starting a game on the map sometimes fails and has to be retried. |

The checks run in layers (see [Architecture](Architecture.md#map-validation)), and a layer only runs if the ones before
it found nothing wrong, so fixing the first errors can reveal the next ones. A report keeps at most 100 errors and 100
warnings.
