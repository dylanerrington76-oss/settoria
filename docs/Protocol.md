# Protocol

Everything the browser and the server say to each other: one WebSocket at `/ws` for the lobby and the game, and two
small HTTP routes. The code is `src/main.rs` (messages and the room), `src/game.rs` (actions and the view),
`src/patch.rs` and `src/board_wire.rs`; the browser's half is in `web/src/useGame.ts`, `types.ts`, `patch.ts` and
`boardGeometry.ts`.

- [Conventions](#conventions)
- [Client to server](#client-to-server)
- [Server to client](#server-to-client)
- [The lobby](#the-lobby)
- [The game view](#the-game-view)
- [Open trade offers](#open-trade-offers)
- [The timer](#the-timer)
- [The game log](#the-game-log)
- [HTTP](#http)
- [Patches](#patches)
- [The board on the wire](#the-board-on-the-wire)

## Conventions

- Every message is a JSON object in a WebSocket text frame, tagged by `"type"` in `snake_case`.
- **Resources** are numbers: 0 brick, 1 lumber, 2 ore, 3 grain, 4 wool. A **bundle** of resources (a hand, a cost, an
  offer) is a 5-element array in that order, e.g. `[1, 0, 2, 0, 0]`.
- **Development cards** are indexed 0 Knight, 1 Road Building, 2 Year of Plenty, 3 Monopoly, 4 Victory Point; a hand of
  them is a 5-element array.
- **Seats and players** are numbers: a seat's index in the lobby's `players`. The host is always seat 0.
- **Corners** ("vertices"), **sides** ("edges") and **hexes** are numbers too, numbered as described in
  [The board on the wire](#the-board-on-the-wire).
- The server decides everything. A client message that isn't allowed is answered with an `error` and changes nothing; an
  action that is allowed is answered by the new state, sent to every seat.
- `color` is one of the palette strings in `PALETTE` (`src/game.rs`, mirrored in `web/src/types.ts`).

## Client to server

There are two kinds of message: **room messages** (joining and the lobby) and **game actions**.

### Room messages

| Message | Who, when | What it does |
|---|---|---|
| `{"type":"join","name","color"?,"token"?,"delta"?}` | anyone, first message | Takes a seat (the first person to join is the host), or, with the `token` from an earlier `welcome`, takes the same seat back. `name` is cut to 16 characters (an empty one becomes "Player N"). A `color` that is missing is the first free one; one that is taken is refused with `code: "color_taken"`. `delta: true` asks for [patches](#patches). A new player is refused once a game is under way or 8 seats are taken. |
| `{"type":"resync"}` | a seat that is sent patches | Asks for the whole view again (see [Patches](#patches)). |
| `{"type":"set_timer","mode"}` | host, lobby | `mode` is `very_fast`, `fast`, `normal` or `slow`. |
| `{"type":"set_map","map"}` | host, lobby | `map` is the `id` of a map in the lobby's `maps`. An id the server doesn't have is refused and nothing changes. |
| `{"type":"set_settings","settings"}` | host, lobby | `settings` is the whole object listed in [Rules](Rules.md#game-settings), using the field names of `Settings` in `src/game.rs`: `points_to_win`, `discard_limit`, `friendly_robber`, `bridges`, `max_roads`, `max_cities`, `max_settlements`, `dev_cards`, `resource_max`, `settlement_points`, `city_points`, `longest_road_points`, `largest_army_points`, `vp_card_points`, `longest_road_min`, `largest_army_min`, `starting_settlements`, `bank_ratio`. An invalid object is refused with an `error`, and the lobby is sent again with the previous values. |
| `{"type":"add_bot"}` | host, lobby | Seats a computer player, named at random from Bender, C-3PO, Multivac, Skynet, AM, Hal 9000 and Data, never one a person or another bot at the table already has. Refused when 8 seats are taken, bots included. |
| `{"type":"remove_bot","seat"}` | host, lobby | `seat` is the bot's index in the lobby's `players`; refused if that seat is not a bot. |
| `{"type":"start"}` | host, lobby | Starts the game. Refused with fewer than 2 seats, when the map doesn't take that many players, when the starting settlements won't fit, or when the map can't be dealt. |
| `{"type":"reset"}` | anyone, game over | Returns everyone to the lobby for a new game. People who left during the game lose their seats; the bots stay. |

Every host-only message is refused with an `error` for anyone else, and the lobby messages are refused once the game has
started.

### Game actions

All of these are sent after joining, in a game, and are checked against the rules (see [Rules](Rules.md)).

| Action | Meaning |
|---|---|
| `roll` | Roll the dice. |
| `build_road{edge}`, `build_bridge{edge}`, `build_settlement{vertex}`, `build_city{vertex}` | Build. In setup, a settlement and then a road. `build_bridge` needs a game with bridges and an `edge` that is a bridge site. |
| `buy_dev` | Buy a development card. |
| `play_knight`, `play_road_building`, `play_year_of_plenty{a,b}`, `play_monopoly{resource}` | Play a card (`a` and `b` are resources). |
| `move_robber{hex}`, `steal{victim}` | The robber steps after a 7 or a Knight. |
| `discard{cards}` | Discard a bundle of exactly the required number of cards. |
| `volcano_pick{resource}` | One free resource, by the player whose pick it is at a volcano or gold field. |
| `bank_trade{give,get}` | Trade at the best ratio you have; `give` and `get` are single resources. |
| `offer_trade{give,want}` | Open another player offer (bundles). Refused for an exact duplicate or beyond 4 open offers. |
| `respond_trade{id,accept}` | Accept or decline one offer. |
| `confirm_trade{id,with}` | The proposer completes an offer with a player who accepted it. |
| `cancel_trade{id}` | Withdraw one of your own offers. |
| `end_turn` | End the turn. |

Offers are named by the `id` in the view's `trades`.

## Server to client

| Message | Meaning |
|---|---|
| `welcome{token}` | Sent after a `join`: the seat's token, which the browser keeps so that a reloaded tab or a dropped connection rejoins the same seat. Anyone with the token has the seat. |
| `lobby{you,players,timer,settings,map,maps}` | The lobby (see below). Sent to everyone on every change, and again after a game. |
| `game{seq?,view}` | The whole [game view](#the-game-view) for one seat. |
| `patch{seq,ops,log_from?,log?}` | The difference from the last message to this seat. See [Patches](#patches). |
| `error{message,code?}` | A refusal, in words fit to show to the player. The only `code` is `color_taken`, for a `join` whose color was just taken. |

An unparseable message is answered with `error` "Unrecognised message".

## The lobby

`lobby{you, players, timer, settings, map, maps}`:

- `you` is the receiving seat.
- `players` lists every seat: `{name, color, connected, bot}`. A bot is always `connected`. The host is always a person:
  if the host leaves and a bot would be first, the first person becomes host.
- `timer` is the [timer mode](Rules.md#timers) and `settings` the game rules, as the host last set them.
- `map` is the id of the chosen map, and `maps` lists every map the host can choose from, in the order to show them:
  `{id, name, author, hexes, players_min, players_max, builtin, recommended_settings?}`.
  `players_min` and `players_max` are who the map is meant for (`start` is refused with an `error` outside that range,
  and the lobby stays as it is). `recommended_settings` is the rule set the map suggests, present only if it suggests
  one. It is only ever *offered*: the lobby shows what it would change, and the host applies it by sending it as
  `set_settings`. The server never applies it itself.

If everyone leaves, the server forgets the game, the timer mode, the settings and the map choice.

## The game view

`game{view}` is **personalised**: you see your own hand and development cards, others only as counts. The view holds:

| Field | Meaning |
|---|---|
| `you`, `players`, `order` | Your seat; one entry per player (name, color, `connected`, card counts, knights, pieces left, visible points `vp`, `longest_road`, `dice_income`); the seats in turn order. `vp_total`, `vp_breakdown` and `stats` are filled in only when the game is over. |
| `settings`, `timer_mode` | The rules of this game (with the bank and deck scaled for 5 to 8 players, see [Rules](Rules.md#five-to-eight-players)) and the timer mode. |
| `board` | The board, in the short form below. **Only in a whole view**: a client that is sent patches gets it once. |
| `phase` | `setup`, `roll`, `discard`, `move_robber`, `steal`, `volcano_pick`, `main`, `road_building` or `game_over`. |
| `current`, `rolled`, `dice`, `roll_counts` | Whose turn it is; whether they rolled; the dice (`[a, b]` or `null`); how many times each total 2 to 12 was rolled this game. |
| `robber`, `buildings`, `roads` | The robber's hex; for each corner `null` or `{owner, city}`; for each side `null` or the owner. Bridges are roads on the sides numbered after the land sides. |
| `hand`, `dev`, `dev_new`, `vp_you` | Your cards: resources, development cards you can play, development cards bought this turn (not yet playable), and your points including hidden ones. |
| `bank`, `deck_left`, `dev_played` | What the bank holds; cards left in the deck; whether you played a development card this turn. |
| `ratios`, `harbours` | Your bank ratio for each resource; the harbours you own (`any`, and `resource` for each), which the trade dialog needs, because you can't be offered bank-rate deals those give. |
| `monopoly_locked` | For each resource, whether the Monopoly lock applies to you now (all false when it isn't your turn). |
| `setup_needs` | In setup, `"settlement"` or `"road"`: what the current player must place. |
| `discards`, `steal_candidates` | Who must discard, and how many; who the robber's owner may steal from. |
| `volcano` | While free picks are owed: `{hex, player, left, setup}`, the hex, the player whose pick it is, how many cards they still pick, and whether this is the free card for a last starting settlement rather than an eruption. |
| `lava`, `damaged` | Where the last lava flow went, `{hex, die, vertex}` (cleared by the next roll); the corners holding a settlement that is on loan after lava hit a city. |
| `road_building_left` | Free roads still to place. |
| `trades` | The [open offers](#open-trade-offers). |
| `legal` | What you may do now: `{settlements, cities, roads, bridges, hexes}` as lists of corner, side or hex numbers. `bridges` is empty unless the game has bridges on. |
| `longest_road_holder`, `largest_army_holder`, `winner` | Seats, or `null`. |
| `log`, `log_start` | The newest 25 [log events](#the-game-log) and the number of the first of them. |
| `timer` | The running [countdown](#the-timer), or `null`. |

The game's phase also says which action is next: for example in `volcano_pick`, only the player in `volcano.player` can
`volcano_pick`.

## Open trade offers

`trades` is a list of `{id, from, give, want, replies}`. `replies` is indexed by seat, each `proposer`, `pending`,
`accepted`, `declined` or `blocked`. `blocked` marks a player who owns the harbour that makes the offer a bank-rate deal
for them, so they can't accept it (they can still decline it); see [Rules](Rules.md#trading). The browser hides offers you
have declined. Up to 4 offers can be open at once, each with its own `id`, unique for the whole game.

## The timer

`timer` is `null` or `{kind, player, remaining_ms, total_ms}`:

- `kind` is `setup_settlement`, `setup_road`, `roll`, `discard`, `robber`, `steal`, `volcano` or `turn`.
- `player` is whose action the clock waits for (during a volcano pick, the player whose pick it is).
- `remaining_ms` is measured when the view was built, so clients count down from the moment the message arrives, and need
  no synchronised clock. Every action (and every timeout) sends a fresh view, which also reflects bonuses.
  (A client that is sent patches is told the time again only when it isn't where that countdown has got to: see
  [Patches](#patches).)

## The game log

The log holds structured events, `{"type": ..., ...}`, which the browser turns into sentences. Events are numbered from 0
for the whole game, and a view carries the newest 25. They are defined in `src/log_event.rs`:

| Group | Events |
|---|---|
| Game and setup | `game_started`, `map_chosen{map,name}` (not for Standard), `custom_rules{changes}`, `turn_order{order}`, `setup_ended_early`, `setup_complete` |
| Dice and the robber | `rolled{player,dice}`, `collected{player,cards}`, `bank_short{resource}`, `must_discard{limit}`, `discarded{player,count}`, `robber_must_move`, `robber_moved{player}`, `stole{player,victim}` |
| Volcano and gold | `volcano_picked{player,resource}`, `volcano_blocked{hex}`, `volcano_bank_empty`, `lava_flow{hex,die,effect,owner}` with `effect` of `nothing`, `destroyed` or `demoted`, `gold_picked{player,resource}`, `gold_blocked{hex}` |
| Building and cards | `placed_settlement`, `built_settlement`, `built_road`, `built_bridge`, `built_city`, `bought_dev_card`, `played_knight`, `played_road_building`, `played_year_of_plenty{player,first,second}`, `played_monopoly{player,resource,taken}` |
| Trading | `bank_trade{player,give,get,ratio}`, `trade_proposed{player}`, `trade_completed{player,with}` |
| Awards and the end | `longest_road_taken{player,roads}`, `longest_road_lost`, `largest_army_taken{player,knights}`, `won{player,points}` |
| Timers | `timed_out{player,kind}` (`kind` as in the [timer](#the-timer)) |

A game opens with `map_chosen`, unless it is the Standard map.

## HTTP

**`GET /api/lobby`** returns `{"taken":[...]}`, the colors already taken, so the join screen can grey them out before the
socket is open.

### Adding a map (HTTP)

`POST /api/maps` with the map's JSON as the body (at most 256 KB) and the host's seat token, the one from `welcome`, as
`Authorization: Bearer <token>`:

    curl -X POST http://localhost:3000/api/maps -H "Authorization: Bearer $TOKEN" --data-binary @examples/maps/archipelago.json

A good map is answered with `201` and `{"id","name","warnings"}` (`id` and `name` are what the map was added as, which
differ from the file's if they were taken; `warnings` are the things `validate` calls warnings), and the lobby is sent to
everyone again with the map in its list. Every failure is answered with `{"issues":[...]}` (a list of sentences) and a
status:

| Status | Meaning |
|---|---|
| `401` | no known token |
| `403` | the token isn't the host's |
| `413` | over 256 KB |
| `422` | not a good map (every issue is listed, as `validate` lists them), or not UTF-8 |
| `409` | the 10-upload limit is used up |
| `500` | something went wrong inside the server while checking the map |

Uploaded maps are kept in memory only. See [Maps](Maps.md#putting-your-maps-in-the-lobby).

Anything else is served from `STATIC_DIR` (default `web/dist`), and unknown paths get `index.html`, so the browser's
routes (`/map-editor`, the changelog) work when a page is reloaded.

## Patches

By default every action sends every seat the whole `game{view}`. The board is a fifth to a third of it (before the board
was sent in its short form: half to three quarters), though it never changes during a game. A client can ask for less
with `"delta": true` in its `join`, which the browser client does. Such a seat is then sent:

- `game{seq: 0, view}` first: the whole view, board and log included. It is sent again, and `seq` starts from 0 again,
  when the seat reconnects, when it sends `resync`, and when a new game starts after a lobby.
- `patch{seq, ops, log_from?, log?}` after every change, and nothing at all when nothing the seat can see has changed.
  `seq` is one more than the last message's (a client that sees a gap, or a patch that doesn't fit, sends `resync` and
  ignores patches until the next `game`).

`ops` change the view as the client holds it, in order, and say nothing of the board (which never changes) or of the
`log` and `log_start` (see below). An op is `[path, value]`, which sets what is at `path` to `value`, or `[path]`, which
removes the key there. A `path` is an array of steps from the top of the view: a string is a key of an object, a number is
an index into an array. For example `[["buildings", 17], {"owner": 1, "city": false}]` and
`[["players", 2, "cards"], 5]`. Arrays are patched entry by entry only when that is shorter than sending them (a long array
that is mostly `null`, like `buildings` and `roads`, or one of objects, like `players`); a short list of numbers, such as
`hand`, comes whole, and so does an array whose length has changed (`trades`). `remaining_ms` of the `timer` is left out
while the same countdown runs where the client has counted it to (within half a second), so it is sent when the clock
starts, changes hands, or is given a bonus.

The log is numbered, not patched: `log` holds the events the client doesn't have, the first of which is number
`log_from`. The client appends them and keeps the newest 25 (`LOG_WINDOW`, the same number in `src/game.rs` and
`web/src/patch.ts`, which must agree), and `log_start` is the number of the first it keeps. A client that finds
`log_from` isn't where its own log ends asks for a `resync`.

A client that doesn't send `delta` is sent a whole `game{view}` after every action, exactly as before, with no `seq`.
`src/patch.rs` makes patches and `web/src/patch.ts` applies them.

## The board on the wire

Where a hex is drawn, and what its corners ("vertices") and sides ("edges") are numbered, all follow from the hexes' `q`
and `r`, so the server does not send them. `board` is:

    { "map": "earth", "style": { "sea_grid": true },
      "hexes": [[5, 0, "forest", 8], [6, 0, "desert", null], ...],
      "ports": [{ "a": 12, "b": 13, "resource": null }, ...],
      "bridges": [[40, 41], ...],
      "desert": 17 }

- `map` is the id of the map the game is on, and `style` is the map's `style` (`{sea_grid}`; the browser draws from this,
  never from the map's name).
- `hexes` are `[q, r, terrain, number]` in the order of the map file. That order numbers the corners and sides, so it is
  part of the protocol. `terrain` includes `volcano` and `gold`.
- `ports` name their two corners by number, and `desert` is the hex the robber starts on.
- `bridges` are the bridge sites as pairs of corner numbers, in side order; it is left out when there are none.

A client numbers the corners and sides as `Geometry::build` (`src/board.rs`) does: go through the hexes in order and
through each hex's six corners clockwise from the upper right, a corner not seen before takes the next number; then go
round each hex's six sides (corner `i` to corner `i + 1`), a side not seen before takes the next number, stored smaller
corner first; the bridge sites follow, numbered on from the last land side. The browser does this in
`web/src/boardGeometry.ts`, once, when a whole view arrives. `cargo test` keeps `web/scripts/board-golden.json` true to the
server, and `web/scripts/board-geometry-test.mjs` holds the browser to it (see [Development](Development.md#tests)). A board
is about 0.7 KB (Standard), 2.6 KB (Earth) or 3.5 KB (USA); it was 4.9, 21.8 and 31.4 KB when it carried every position and
side.
