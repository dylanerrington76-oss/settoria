# Settoria

Catan for 2 to 4 players on a local network, on either the standard board or costum maps.

## Run it

build the frontend


Needs Rust (1.75+). Node is only needed to rebuild the frontend; a prebuilt
copy is already in `web/dist`.

    cargo run --release          # or ./run.sh to rebuild the frontend first

The server prints two addresses:

    This computer:  http://localhost:3000
    Other devices:  http://192.168.x.x:3000   <- share this one

Everyone opens the address in a browser, enters a name, picks one of 10 colors (orange, purple, gold, green,
blue, red, mystic blue, white, silver, black; colors already taken are greyed out) and takes a seat.
The first player to join is the host. The host picks the **map** (any map in the lobby's list: Standard, Earth and USA to begin with), the **turn timer** (Very Fast 30s, Fast 60s,
Normal 120s, Slow 240s; Normal is the default), can change any of the **game rules** (see below), and presses
**Start**. The draft order is shuffled when the
game starts, so the host does not necessarily go first.

Options (environment variables): `PORT` (default 3000), `STATIC_DIR` (default `web/dist`), `MAPS_DIR`
(default `maps`: the folder custom maps are read from, see *Maps*).
If other devices can't connect, allow port 3000 through the host's firewall.

Reloading a tab, or a dropped connection, rejoins the same seat automatically.

## Maps

The host chooses the map in the lobby; it is locked once the game starts.

**Standard**: the base-game board, randomised every game (described under *Rules implemented*).

**Earth**: 81 hexes on 7 islands with 27 harbours. The land is a fixed shape.

**USA**: 144 hexes in the shape of the United States, as one big island with 25 harbours.

### Map editor

The **map editor** link at the bottom of the lobby (`/map-editor`) builds a map file without writing JSON. Drag pieces from the
bar onto the blue sea and move them around; drag a piece back onto the bar (or select it and press Delete) to remove it.
Ctrl+Z / Ctrl+Shift+Z undo and redo, the wheel zooms and dragging the water pans.

| Piece | In the file |
|---|---|
| Striped hex (red and black) | a hex whose terrain is dealt at random: any resource, or a desert |
| Desert | a hex with `"terrain": "desert"` |
| Port | a harbour on the coast of a hex, its kind dealt at random (a port can only sit on a coast edge; a hex placed against it removes it) |

Striped means "dealt at random" everywhere in the editor. The `terrain_pool`, `token_pool` and harbour `pool` are worked out
from what is on the map, so they always add up: resources are shared out evenly (Standard's 4/4/4/3/3 split when the
numbers don't divide), number tokens follow the Standard set, and about a fifth of the harbours are 3:1. **Settings** sets the
author, the player range (by default worked out from how many starting settlements fit) and how many of the striped hexes
become deserts (by default one if you placed no desert yourself). The editor runs the same checks as `validate` (name, room for
the starting settlements, and so on) and only offers **Download** when the map would be accepted; problems and notes are listed
from the status button.

**Import** (or dropping a file on the page) opens an existing map file to edit, such as the ones in `maps/builtin`. Fixed
terrain, numbers, harbour kinds, `robber_start`, `recommended_settings` and the pools are kept, and a map that is opened and saved
without changes comes out the same. Maps with `"mode": "ring"` harbours are opened with the harbours at fixed positions.

### Map files (work in progress)

A map is a `MapDef` (`src/map_def.rs`) written as JSON. Standard, Earth and USA are map files like any other:
`maps/builtin/standard.json`, `maps/builtin/earth.json` and `maps/builtin/usa.json`, compiled into the program (so the binary needs
nothing next to it) and read by the same code as a map you write. They make good templates: Standard
shows a single island with `"ring"` harbours, Earth shows fixed harbours, a fixed desert and a constraint, USA shows a
large single island with fixed harbour spots and every terrain, token and harbour kind dealt at random.
(The order of the hexes in a file decides how corners and sides are numbered, so the built-in files keep a
fixed order; it doesn't matter for a map of your own.)

Write one and check it:

    cargo run --release -- validate examples/maps/archipelago.json [more.json ...]

#### Putting your maps in the lobby

**From a folder.** When the server starts it reads every `*.json` file directly inside `MAPS_DIR` (default
`maps/`; folders under it are not searched, so `maps/builtin/`, which holds the source of the two built-in
maps, is not loaded twice; files starting with `.` are ignored). For each file it checks the size (256 KB at
most), parses it and validates it exactly as `validate` does, then adds it to the lobby. Files are read in
name order, up to 100 of them, so the same folder gives the same ids every time.

A file that is no good never stops the server. It is logged to stderr with its name and every issue, then
skipped, and the startup banner says what happened:

    maps: skipped foo.json: the map has no hexes
      Maps folder:    maps: Loaded 3 maps, skipped 1 (foo.json: the map has no hexes)

The id of a map is its file name without `.json`, made safe (`My Map.json` is `my-map`). If an id or a name is
already taken (by a built-in map, or by another file) a number is added: `earth.json` holding a map called
"Earth" loads as `earth-2`, "Earth (2)". Names are compared without regard to case. Built-in maps are never
replaced.

**By upload.** The host can add a map while the server runs with `POST /api/maps`; see *Protocol*. Uploaded
maps are kept in memory only (they are gone when the server stops), and at most 10 can be uploaded between
restarts.

Whichever way a map arrives, its `name` and `author` are at most 32 characters of plain text (no line breaks,
control characters or invisible characters), because the lobby shows them in small buttons.

`validate` lists *every* problem in one pass. **Errors** make a map unusable; **warnings** are worth knowing
but leave it playable. Exit status: 0 = playable, 1 = the map has errors, 2 = a file can't be read or parsed.
Mistakes in the JSON itself (a misspelt field, an unknown terrain) are reported with line and column.

| Field | Meaning |
|---|---|
| `version` | Always `1`. |
| `name`, `author` | Shown in the lobby. |
| `players` | `{ "min": 2, "max": 4 }`, within 2 to 4. |
| `hexes` | `{ "q": 1, "r": 0 }` per land hex (axial coordinates, at most 250). Add `"terrain"` and/or `"number"` to fix them. |
| `terrain_pool` | Terrains dealt to the hexes whose terrain isn't fixed: `{ "forest": 4, "desert": 1, ... }`. Must add up to exactly that many hexes. |
| `token_pool` | Number tokens dealt to the producing hexes whose number isn't fixed: `{ "6": 2, ... }` (2 to 12, never 7). |
| `ports` | `{ "mode": "fixed", "spots": [{ "q", "r", "side" }], "pool": { "any": 4, "ore": 1, ... } }`, or `{ "mode": "ring", "spacing": [3,4,...], "pool": {...} }` for a single island (gaps between harbours, in coast edges, adding up to the coast's length). A spot may fix its `"kind"`. Sides run clockwise from the east: 0 east, 1 south-east, 2 south-west, 3 west, 4 north-west, 5 north-east. |
| `robber_start` | `{ "q", "r" }`. Default: a random desert (a map with no desert must say). |
| `constraints` | `spread_tokens` (default `true`: no equal numbers and no 6/8 touching), `deserts_avoid_fixed_deserts`. |
| `recommended_settings` | The rules the map suggests (see *Rules*); the validator checks room for the starting settlements against them. |
| `style` | How the browser draws the map; cosmetic only. `{ "sea_grid": true }` draws a faint hex grid across the open sea (Earth does). Unknown keys are refused. |

A value that is dealt at random is written `"?"` (or left out). Besides the usual mistakes, `validate`
works out whether the starting settlements really fit under the distance rule (exactly, not by estimate),
and tries dealing the number tokens a few times to see whether the map can be dealt reliably.

## Rules

- 19 hexes, randomised every game (4 forest, 4 pasture, 4 fields, 3 hills, 3 mountains, 1 desert),
  18 number tokens placed randomly with no 6/8 touching, 9 harbours (4 x 3:1, one 2:1 per resource).
- Setup: settlement + road forward, then backward (then forward again with 3 or more starting settlements);
  each player's last settlement pays out its adjacent hexes.
  The order is randomised at the start of the game; the same order is used for the draft (forward, then
  backward) and for every turn afterwards. The first player to place also takes the first turn.
- Dice, production (cities pay double), bank shortage rule (if the bank can't cover a resource and more
  than one player is owed it, nobody gets it; if only one is owed, they get what's left).
- Rolling 7: players with more than 7 cards discard half (rounded down), move the robber, steal a random card.
- Building: road, settlement (distance rule, must connect to your road), city; piece limits 15/5/4.
  Opponent settlements block roads from continuing through them.
- Bank trades at 4:1 / 3:1 / 2:1 according to your harbours.
- Player trades: on your turn you can have several offers open at once (up to 6, each with its own give/want,
  all shown to every other player). Each player accepts or declines each offer separately; an offer a player
  declines disappears from their screen (you still see it marked declined). You confirm an offer with one of
  its accepters. Completing one offer leaves the others open, except that any you can no longer pay for are
  withdrawn, and a player who accepted but can no longer pay is asked again. Offers end with your turn.
- Development cards (25-card deck): Knight, Road Building, Year of Plenty, Monopoly, Victory Point.
  One per turn, not on the turn bought, may be played before rolling. Victory Point cards stay hidden.
- Longest Road (5+, longest-path with ties and breaks handled) and Largest Army (3+ knights).
- First to reach the points target (15 by default) on their own turn wins. 2 players use the same rules unchanged.

## Timers

The server owns every clock so a closed tab or dropped connection
is auto-played like any other timeout. The clock beside the status pill shows the countdown that is
running and turns red under 10 seconds.

| What                                   | Time | If it runs out |
| -------------------------------------- | ---- | -------------- |
| Setup: place a settlement (all modes)  | 120s | placed on a random legal vertex |
| Setup: place the road (all modes)      | 30s  | placed on a random legal edge next to it |
| Roll the dice                          | 10s  | the dice are rolled for you |
| Discard after a 7 (everyone at once)   | 20s  | each player who hasn't discarded loses random cards |
| Move the robber                        | 20s  | placed on a random 6 or 8 hex (never the one it is on) |
| Choose who to rob                      | 15s  | a random candidate |
| Your turn (the **turn clock**)         | 30 / 60 / 120 / 240s by mode | the turn ends (unplaced Road Building roads are placed at random) |

The roll window and the whole 7 sequence (discard, robber, steal) are **not** charged to the turn clock;
it starts when they finish. A Knight played mid-turn pauses the turn clock for its robber move, and a
Knight played before rolling keeps the unused part of the 10s roll window.

Time bonuses are added to the turn clock and can never push it above the mode's starting time
(e.g. +10s with 22s left on Very Fast gives 30s, not 32s): **+10s** for each road, settlement and city built,
each development card bought, each bank trade, and each player trade that is completed (the proposer confirming
with someone who accepted). Roads from a Road Building card count as roads. Sending, accepting or declining an
offer earns nothing by itself.

## Layout

    src/map_def.rs  what a map is: `MapDef` (hexes, terrain/number pools, harbours; read from JSON) and `MapError`
    src/builtin.rs  the built-in maps: `maps/builtin/*.json` compiled into the program
    src/maps.rs     the map registry: every map the server can play, by id (built-ins, the maps folder, uploads)
    src/map_loader.rs  loading maps: parse, validate, register; the maps folder at startup; what an upload uses
    src/map_files.rs   finding and reading map files within a size cap; making ids and names safe and unique
    src/validate.rs `validate`: every problem with a map in one pass (errors and warnings), and the `validate` command
    src/board.rs    `Board::from_def`: deals a map's random parts, builds hex/vertex/edge geometry and harbours;
                    a bad map is a `MapError`, never a panic or a hang (it uses the same checks as `validate`)
    maps/builtin    the Standard, Earth and USA map files
    examples/maps   an example map file
    src/game.rs    all rules and timers (Game::apply validates every action, Game::tick fires timeouts) + unit and fuzz tests
    src/main.rs    lobby, WebSocket protocol, static file serving
    web/src        React client (Board.tsx = SVG board, ui.tsx = panels/dialogs, App.tsx = screens)
    web/src/MapEditor.tsx  the map editor page; mapModel.ts reads, checks and writes map files (no React), MapPieces.tsx draws the pieces
    scripts/smoke.mjs   end-to-end test with 3 websocket clients (server must be running)
    scripts/timer.mjs   end-to-end check of the 10s auto-roll and clock accounting (~12s, needs a fresh server)

## Develop

    cargo test --release                 # rules, maps, map loading and uploads, the lobby protocol, random-play fuzzing
    (cd web && npm test)                 # renders the lobby and log in Node and checks what they say
    cargo run --release &                # server on :3000
    cargo run --release -- validate examples/maps/archipelago.json   # check a map file (no server started)
    node scripts/smoke.mjs               # end-to-end check
    node scripts/timer.mjs               # timer check (restart the server first: it needs an empty table)
    cd web && npm run dev                # hot-reloading client on :5173, proxies /ws to :3000
   cd web && npm run build

## Protocol (JSON over `/ws`)

Client to server: `{"type":"join","name","color"?,"token"?}`, `set_timer{mode}` (host only, before the game starts;
`mode` is `very_fast`, `fast`, `normal` or `slow`), `set_map{map}` (host only, before the game starts; `map` is the
`id` of one of the maps in the lobby's `maps`, such as `standard` or `earth`; an id the server doesn't have is refused
with an `error` and nothing changes), `set_settings{settings}` (host only, before the game starts;
`settings` is the whole object listed under *Game rules*, with the field names of `Settings` in `src/game.rs`:
`points_to_win`, `discard_limit`, `friendly_robber`, `max_roads`, `max_cities`, `max_settlements`, `dev_cards`,
`resource_max`, `settlement_points`, `city_points`, `longest_road_points`, `largest_army_points`,
`vp_card_points`, `longest_road_min`, `largest_army_min`, `starting_settlements`, `bank_ratio`; an invalid object
is refused with an `error` and the lobby is re-sent with the previous values), `start`, `reset`, then game actions:
`roll`, `build_road{edge}`, `build_settlement{vertex}`, `build_city{vertex}`, `buy_dev`, `play_knight`,
`play_road_building`, `play_year_of_plenty{a,b}`, `play_monopoly{resource}`, `move_robber{hex}`,
`steal{victim}`, `discard{cards}`, `bank_trade{give,get}`, `offer_trade{give,want}` (opens another offer; refused for
an exact duplicate or beyond 6 open offers), `respond_trade{id,accept}`, `confirm_trade{id,with}`,
`cancel_trade{id}` (withdraws one offer), `end_turn`. Offers are named by the `id` in the view's `trades`.
Resources are indexed 0 brick, 1 lumber, 2 ore, 3 grain, 4 wool; bundles are 5-element arrays.

Add a map (HTTP, not the socket): `POST /api/maps` with the map's JSON as the body (at most 256 KB) and the
host's seat token, the one from `welcome`, as `Authorization: Bearer <token>`:

    curl -X POST http://localhost:3000/api/maps -H "Authorization: Bearer $TOKEN" --data-binary @examples/maps/archipelago.json

A good map is answered with `201` and `{"id","name","warnings"}` (`id` and `name` are what the map was added as,
which differ from the file's if they were taken; `warnings` are the things `validate` calls warnings), and the
lobby is sent to everyone again with the map in its list. Every failure is answered with `{"issues":[...]}` (a
list of sentences) and a status: `401` no known token, `403` the token isn't the host's, `413` over 256 KB, `422`
not a good map (every issue is listed, as `validate` lists them), `409` the 10-upload limit is used up.

Server to client: `welcome{token}`, `lobby{you,players,timer,settings,map,maps}`, `game{view}`, `error{message,code?}`.
In the lobby, `map` is the id of the chosen map and `maps` lists every map the host can choose from, in the order to
show them: `{id, name, author, hexes, players_min, players_max, builtin, recommended_settings?}`. `players_min` and
`players_max` are who the map is meant for (`start` is refused with an `error` outside that range, and the lobby stays
as it is). `recommended_settings` is the rule set the map suggests, present only if it suggests one; it is only ever
*offered*: the lobby shows what it would change and the host applies it, if they want to, by sending it as
`set_settings`. The server never applies it itself. The game view's `board.map` is the id of the map the game is
on, `board.style` is the map's `style` (`{sea_grid}`; the browser draws from this, never from the map's name), and the game log opens with `{type:"map_chosen", map, name}` unless it is the Standard map.
`color` is one of the palette hex strings in `PALETTE` (`src/game.rs`, mirrored in `web/src/types.ts`). If it is
missing the server picks the first free color; if it is taken the join is refused with `code: "color_taken"`.
Lobby players and game-view players carry their `color`. `GET /api/lobby` returns `{"taken":[...]}` so the join
screen can grey out taken colors before the socket is open.
The view is personalised: you see your own hand and development cards, others only as counts.
The view also carries `trades` (the open offers: `{id, from, give, want, replies}`, `replies` indexed by seat as
`proposer`, `pending`, `accepted` or `declined`; the client hides offers you have declined), `order` (seat ids in turn order), `settings` (the game's rules), `timer_mode`, and `timer`: `null` or
`{kind, player, remaining_ms, total_ms}` where `kind` is `setup_settlement`, `setup_road`, `roll`, `discard`,
`robber`, `steal` or `turn`. `remaining_ms` is measured when the view was built, so clients count down from the
moment the message arrives; every action (and every timeout) sends a fresh view, which also reflects bonuses.