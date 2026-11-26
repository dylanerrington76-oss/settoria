# Architecture

How Settoria is put together, for someone who is about to change it. For what the game does, see [Rules](Rules.md); for
the messages, [Protocol](Protocol.md); for how to build and test it, [Development](Development.md).

- [The big picture](#the-big-picture)
- [The server](#the-server)
- [A move, start to finish](#a-move-start-to-finish)
- [The game's states and clocks](#the-games-states-and-clocks)
- [Map validation](#map-validation)
- [The client](#the-client)
- [Design decisions worth knowing](#design-decisions-worth-knowing)
- [Recipes: adding things](#recipes-adding-things)
- [Limits](#limits)

## The big picture

One Rust program serves everything: the React client (static files), a WebSocket for the lobby and the game, and two
small HTTP routes. There is one table per server, held in memory.

    browser (React, SVG board)  <-- WebSocket /ws -->  axum server (src/main.rs)
        web/src                    JSON messages            |
                                                            +-- Room: seats, lobby choices, bots      (main.rs, bot.rs)
        also fetches:                                       +-- Game: the rules and the clocks        (game/)
          /api/lobby, POST /api/maps                        |     +-- Board: hexes, corners, sides    (board.rs)
          static files from web/dist                        +-- Map registry: built-ins, folder, uploads
                                                            |        (maps.rs, map_loader.rs, map_files.rs)
                                                            +-- Validator: is a map any good?        (validate/)

The server is **authoritative**: a client sends what it wants to do, the server checks it against the rules and sends
back the new state. Nothing the client computes is trusted, and nothing a player shouldn't see is sent to them (the view
is built per seat).

## The server

| File | What it holds |
|---|---|
| `src/main.rs` | The `Room` (seats, host choices, the game, the bots) behind a single `Mutex`; the WebSocket handler; the lobby and game messages; `update_seat` (whole views or patches); `GET /api/lobby`, `POST /api/maps`; static file serving (the `.br` and `.gz` copies the build writes, and cache headers: `immutable` for `/assets`, `no-cache` for `index.html`); the 200 ms timer loop; startup and the `validate` subcommand. |
| `src/game.rs` | **The rules and clocks, start here.** The `Game` struct (the state of one game), `Phase`, and the module list for `src/game/`, which holds everything else: see below. Re-exports what the rest of the program uses, so it says `game::Action`, not `game::action::Action`. |
| `src/game/` | One file per part of the rules, each an `impl Game` block: `settings.rs` (`Settings`), `setup.rs` (constructors, the setup phase), `action.rs` (`Action`, `Game::apply`: validates and carries out every action), `turn.rs` (dice, production, ending the turn), `legal.rs` (where things may go), `build.rs`, `dev_cards.rs`, `robber.rs`, `volcano.rs` (volcanoes and gold), `trade.rs` (harbours, bank trades, offers), `scoring.rs` (points, awards, winning), `clock.rs` and `timeout.rs` (the countdowns, and `Game::tick` firing one), `player.rs`, `view.rs` (`Game::view`: what one seat sees). The unit and random-play tests are in `src/game/tests/`, in groups by topic. |
| `src/board.rs` | `Board::from_def` deals a map's random parts (terrain, tokens, harbours, volcano spots) and builds the hex, corner and side geometry (`Geometry::build`) and the harbours and bridge sites. A bad map is a `MapError`, never a panic or a hang. |
| `src/board_wire.rs` | The short form a board goes over the wire in, and the test that keeps the browser's corner and side numbering true to the server's. |
| `src/types.rs` | The small types the rules are written in: `Resource`, `Res` (a bundle of cards), `DevCard`, `PlayerId`, `PerPlayer<T>`. They exist so a resource can't be mistaken for a hex or a seat number. |
| `src/log_event.rs` | The structured game log (`LogEvent`) the browser turns into sentences. |
| `src/patch.rs` | The difference between two JSON values, which is what a client that asks for patches is sent. |
| `src/bot.rs` | Computer players: `choose` scores every legal move and picks the best; `Driver` runs the bots' turns at a human pace. See [Bots](Bots.md). |
| `src/map_def.rs` | What a map is: `MapDef` (hexes, terrain and token pools, harbours, constraints, style), read from JSON, and `MapError`. Written for untrusted input: unknown fields are refused and sizes are capped before anything is allocated. |
| `src/builtin.rs` | The built-in maps: `BUILTIN_MAPS`, files in `maps/builtin/` compiled in with `include_str!`. |
| `src/maps.rs` | The map registry: every map the server can play, by id (built-ins, the maps folder, uploads). It has a lock of its own, so looking up a map never waits for the room. |
| `src/map_loader.rs`, `src/map_files.rs` | Loading maps: parse, validate, register; the maps folder at startup; what an upload uses; reading files within a size cap; making ids and names safe and unique. |
| `src/validate.rs`, `src/validate/` | `validate`: every problem with a map in one pass, and the `validate` command. See [below](#map-validation). |
| `maps/builtin`, `examples/maps` | The map files, and an example map. |

Everything outside `game/` is small by comparison: the rules live in one place so that every action goes through one
check (`Game::apply`), and the lobby code only decides who may ask for what.

## A move, start to finish

1. The browser sends an action, for example `{"type":"build_road","edge":17}` (`useGame.ts`).
2. `handle_socket` parses it, takes the room's lock and calls `Room::handle`. A room message (lobby, bots) is dealt with
   there; anything else is a game `Action` for the seat whose token the connection joined with.
3. `Game::apply` runs the rules. Only the current player may act, except to discard, respond to a trade or pick a free
   resource. If it is allowed, the state changes; if not it returns the reason in words, and `Room` sends it as an `error`.
4. After a successful action the game checks for a win, withdraws trade offers that no longer make sense, moves the
   clock to the next countdown and adds any time bonus.
5. `Room::broadcast` builds the view for every seat (`Game::view`; the longest road is computed once and shared) and
   `update_seat` sends each seat either the whole view or a patch (see [Protocol](Protocol.md#patches)).
6. The browser applies it (`patch.ts`) and renders.

**Time** works the same way, driven by the server: a task wakes every 200 ms and calls `Room::tick`, which asks the game
whether a countdown has run out (`Game::tick`: the server plays the move for the player who timed out) and lets a bot
move if one has something to do (`Driver::step`). If anything changed, it broadcasts. The browser only counts down for
show, from the `remaining_ms` in the last message.

## The game's states and clocks

`Game.phase` says what the game is waiting for:

| Phase | Waiting for | Clock |
|---|---|---|
| `setup` | the current player's settlement, then road | `setup_settlement`, `setup_road` |
| `roll` | the current player to roll (a development card may be played first) | `roll` |
| `discard` | every player over the limit to discard | `discard` |
| `move_robber`, `steal` | the robber's owner to move it and choose a victim | `robber`, `steal` |
| `volcano_pick` | the player whose free pick it is | `volcano` |
| `main` | the current player to build, trade, play cards or end the turn | `turn` |
| `road_building` | the two free roads of a Road Building card | `turn` |
| `game_over` | `reset` | none |

The usual path is `setup` → (`roll` → `main`)\*, where `roll` may detour through `discard` → `move_robber` → `steal` after a
7, and `main` through `volcano_pick` right after production when a volcano or gold number came up. A Knight sends either
`roll` or `main` through `move_robber` and back to wherever it came from (`resume_phase`).

Exactly one countdown runs at a time (`Game::clock`, kept in step with the phase by `sync_clock`). The turn clock's
remaining time is saved when another clock takes over and restored after, which is how the 7 sequence and the volcano
picks are "not charged to the turn clock". Time bonuses are banked per action and added in `apply_bonus`, capped at the
mode's starting time.

## Map validation

A map file is checked in layers, and a layer runs only if the ones before it found nothing wrong:

1. **The hex list** (`validate/hexes.rs`): format version, size, coordinates, duplicates.
2. **The rules** (`validate/rules.rs`): name and author, the player range, the recommended settings.
3. **The content** (`validate/content.rs`): pools against hex counts, tokens (including a volcano beside a 6 or 8),
   harbours and the robber start.
4. **Playability**, only once nothing above found a problem (`validate/playability.rs`): does anything produce, is there
   room for the starting settlements (exactly, by a bipartite matching in `validate/bipartite.rs`), and can the tokens be
   dealt? `validate/quality.rs` adds the advice (the warnings), and `validate/stats.rs` the facts for the summary line.

**Layers 1 to 3 are the only place these rules live.** `Board::from_def` calls the same functions and refuses a map with
the first problem they find, so the generator and the validator can't drift apart. Layer 4 is advice about whether a map
is worth playing, so it isn't needed to build a board. The same checks are repeated in TypeScript for the map editor
(`web/src/mapModel.ts`), which has to be kept in step by hand.

## The client

A React single-page app built with Vite (`web/`), drawn with inline SVG, with no runtime dependencies besides React and
the Open Sans font.

| File | What it holds |
|---|---|
| `main.tsx`, `App.tsx`, `route.ts` | The screens (join, lobby, game) and the extra pages (`/changelog`, `/map-editor`). Moving between them doesn't reload the page, so the socket stays connected. The two extra pages are loaded on demand (`React.lazy`), so the map editor is not part of the download every player makes. |
| `useGame.ts` | The connection: joins, keeps the seat's token, applies whole views and patches, asks for a resync when a patch doesn't fit. |
| `types.ts` | The message and view types, mirroring the server (`PALETTE`, `DEFAULT_SETTINGS`, `Terrain`, the log events). Keep in step with `src/game/` (`view.rs`, `settings.rs`, `player.rs`). |
| `boardGeometry.ts`, `patch.ts` | No React: the corner and side numbering, worked out from a board's hexes, and the patch applier (`LOG_WINDOW` must equal the server's). |
| `Board.tsx`, `art.tsx`, `DesertArt.tsx` | The SVG board (hexes, tokens, harbours, pieces, robber, lava) and the shared vector art. |
| `ui.tsx`, `Mobile.tsx` | The panels, dialogs, trade and log views; the phone layout (600 px and under). |
| `GameSettings.tsx` | The host's game-rules panel in the lobby. |
| `MapEditor.tsx`, `mapModel.ts`, `MapPieces.tsx`, `mapUpload.ts` | The map editor page; its data model (reads, checks and writes map files, no React); the pieces it draws; uploading a map file to the server. |
| `postgame.ts` | The Postgame% and live win-chance formula (no React). |
| `Changelog.tsx` | The changelog page, which reads `docs/Changelog.md` and shows it newest first. The text is written for players. |
| `mock/`, `mock.html` | A dev-only harness that renders the game screen from fixtures, with no server. |
| `scripts/` | The client's tests and the screenshot tool; see [Development](Development.md). |

## Design decisions worth knowing

- **The hex order in a map file is part of the protocol.** It numbers every corner and side, on the server and in the
  browser, so the built-in files keep a fixed order, and a golden file (`web/scripts/board-golden.json`) pins the
  numbering for both sides. Change the geometry on purpose, and regenerate it.
- **The server owns every clock**, so a closed tab or a dropped connection is auto-played like any other timeout, and no
  client needs a synchronised clock.
- **Hidden information stays on the server.** A view holds your own hand and cards, and others as counts. Hidden
  Victory Point cards, each player's point breakdown and their whole-game stats are revealed only once the game is over.
- **Whole views are the default, patches an option.** A client that doesn't ask for `delta` is sent the whole view after
  every action, so simple clients keep working.
- **Map input is untrusted.** Every file or upload is size-capped, parsed with unknown fields refused, validated by
  the same layers the generator uses, and checked off the async threads (it deals trial boards). A bad file is logged and
  skipped; it never stops the server.
- **Every action is validated in one place** (`Game::apply`), and refusals are written for players to read. The bots only
  ever pick moves the rules list as legal, so a bot refused by the rules is a bug and is logged.
- **Everything is in memory.** Stopping the server ends the game. Uploaded maps are gone too; built-in maps and the maps
  folder are read again at startup.

## Recipes: adding things

**A new hex type** (as volcano and gold were added). Search for `gold` to find every place a terrain is handled:
- `src/board.rs`: the `Terrain` enum, its name and `resource()`, any predicate such as `gives_free_pick`, and how the
  deal treats its number (see `deal_layout`).
- `src/game/`: what the hex does when its number is rolled (`volcano.rs`), and its clock and phase if it needs one
  (`clock.rs`; `Phase` is in `game.rs`).
- `src/log_event.rs` and `src/bot.rs` (the log event; `hex_value` and any pick the bot must make).
- `src/validate/` if a map needs a new rule about it.
- The client: `types.ts` (`Terrain`, the log event), `Board.tsx` (colour, art, `RES_OF`), `art.tsx`/`style.css`, `ui.tsx`
  (the log line), `mapModel.ts`, `MapEditor.tsx` and `MapPieces.tsx` (the editor piece and its checks), and
  `boardGeometry.ts`.
- The golden board test if geometry changed, then [Rules](Rules.md), [Maps](Maps.md), [Protocol](Protocol.md) and
  [Bots](Bots.md).

**A new game setting.** Add the field to `Settings` and its default in `src/game/settings.rs`, its range in `Settings::validate`
and its label in `Settings::changes`; mirror it in `DEFAULT_SETTINGS` (`types.ts`) and in `GameSettings.tsx`; use it in
the rules; add it to the tables in [Rules](Rules.md#game-settings) and the `set_settings` list in
[Protocol](Protocol.md#room-messages).

**A new action.** Add a variant to `Action`, handle it in `apply_at`/`do_current`, add what it needs to `Legal` and the
view if the client must know, teach `bot.rs` about it if bots should use it, then add the type to `types.ts` and the
client, and list it in [Protocol](Protocol.md#game-actions).

**A new log event.** Add it to `LogEvent` (`src/log_event.rs`), to the log events in `web/src/types.ts` and to the log
view in `ui.tsx`, and to the table in [Protocol](Protocol.md#the-game-log).

**A new map field.** Add it to `MapDef` in `src/map_def.rs` (`deny_unknown_fields` is on, so old servers refuse it), check
it in `src/validate/`, mirror it in `mapModel.ts`, and document it in [Maps](Maps.md#the-map-file).

## Limits

- One table, one game at a time, held in memory; there is no database or saved game.
- No accounts and no encryption: a seat is held by a random token, and the server is meant for a trusted local network.
- Up to 12 seats (`MAX_PLAYERS`), and no spectators: a new player is refused once a game has started.
- At most 100 maps are read from the maps folder and 10 uploaded between restarts, each at most 256 KB, 500 hexes.
- Bots never propose trades and decline every offer.
