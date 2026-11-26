# Development

How to build, run, test and release Settoria. For how the code is organised, see [Architecture](Architecture.md).

- [What you need](#what-you-need)
- [Running it](#running-it)
- [Working on the client](#working-on-the-client)
- [Tests](#tests)
- [Docker](#docker)
- [Configuration](#configuration)
- [The changelog](#the-changelog)
- [Releasing](#releasing)

## What you need

| For | You need |
|---|---|
| The server | Rust 1.75 or later (`cargo`). |
| Rebuilding the client, and the client's tests | Node. The scripts that run `.ts` files directly (`patch-test`, `board-geometry-test`) need Node **22.18 or later**; the Docker image uses Node 22. |
| Docker instead of both | Docker with Compose. The image builds the client and the server itself. |

The crate and the binary are called `catan-lan` (the project's earlier name): `cargo run` runs `target/release/catan-lan`,
and the web package is `catan-lan-web`.

A prebuilt client is kept in `web/dist`, so the server runs without Node. Rebuild it after changing `web/src` (see
[Releasing](#releasing)).

## Running it

    cargo run --release          # the server on http://localhost:3000, serving web/dist
    ./run.sh                     # builds the client first (if Node is installed), then the same

The server prints the address for the computer it runs on and, for other devices, its address on the network. It listens on
all interfaces, so people on the same network can join. Check a map without starting a server:

    cargo run --release -- validate examples/maps/archipelago.json [more.json ...]

## Working on the client

    cd web
    npm install                  # once
    npm run dev                  # hot-reloading client on http://localhost:5173
    npm run typecheck            # tsc --noEmit
    npm run build                # typecheck, build into web/dist, then write a .br and .gz copy of each text file
    npm run precompress          # only that last step, on the web/dist that is already there

`npm run dev` proxies `/ws` and `/api` to the Rust server, so start that first (`cargo run --release`). It looks for the
server on `localhost:3000`; set `API_TARGET` (for example `API_TARGET=other-host:3000`) to point it elsewhere. The
changelog page imports `../docs/Changelog.md`, which is outside `web/`, so the dev server is allowed to read the parent
folder (`web/vite.config.ts`).

**The fixtures harness** renders the game screen from made-up game states, with no server (`web/src/mock/`). Open
`/mock.html?s=setup|roll|discard|main|steal` while `npm run dev` runs, or `?s=lobby` for the lobby and the game-rules panel
(`&host=0` to see it as a guest). `npm run shots` screenshots every state with Puppeteer into `web/shots/` (or one state:
`npm run shots -- discard 1280 720`); set `CHROME_PATH` if Chrome or Chromium isn't in a standard place.

## Tests

| Command | What it covers | Needs |
|---|---|---|
| `cargo test --release` | Over 300 Rust tests: the rules (including random-play fuzzing), maps and their validation, map loading and uploads, the lobby protocol, patches, the bots, and the board geometry | Rust |
| `cd web && npm test` | Renders the lobby, the game screen and the log in Node (no browser) and checks what they say and what is enabled | Node |
| `cd web && node scripts/patch-test.mjs` | The client's half of the [patch protocol](Protocol.md#patches) | Node 22.18+ |
| `cd web && node scripts/board-geometry-test.mjs` | The browser's corner and side numbering against the server's | Node 22.18+ |
| `UPDATE_GOLDEN=1 cargo test --release golden` | Not a check: rewrites `web/scripts/board-golden.json` after you changed the board geometry **on purpose**; change `web/src/boardGeometry.ts` to match, and the test above shows whether you did | Rust |
| `node scripts/smoke.mjs [port]` | End to end: three WebSocket clients play through setup and several turns | a running server |
| `node scripts/timer.mjs [port]` | End to end, about 12 seconds: the 10 s auto-roll and the clock accounting | a **fresh** server (an empty table) |
| `docker compose --profile test run --rm tests` | `cargo test --release` in the image the server is built in | Docker |

Notes:

- The Rust tests are mostly next to the code they test (`#[cfg(test)]` modules); the validator's are in `src/validate/tests/`.
- The board geometry test covers a fixed list of maps in `src/board_wire.rs`: Standard, Earth, USA, Europe and Texas (the
  last two are files in `maps/builtin/` that are not built in) and the example map. Add a map to that list if its
  geometry should be pinned for the browser too.
- Clicking in the browser is not tested directly: the Node tests check what a screen shows, and the server's tests cover
  the messages those clicks send (`src/main.rs`).
- The patch test has its own port of `diff` from `src/patch.rs`; if you change one, change the other.

## Docker

    docker compose up --build            # the game on http://localhost:3000 (restarts itself unless stopped with `down`)
    docker compose down                  # stop it
    docker compose --profile dev up      # also the hot-reloading client on http://localhost:5173, proxied to the game
    docker compose --profile test run --rm tests
    docker compose run --rm settoria validate /app/maps/my-map.json

The `Dockerfile` has four stages: the client (Node, `npm ci`, `npm run build`), the server (Rust; the dependencies are
built first on their own so editing code doesn't rebuild them), an optional `tests` stage, and a small Debian `runtime`
image. The image runs as a non-root user and has a health check that asks the server for `/api/lobby`. It **builds its own
client**, so it doesn't use `web/dist` (it is in `.dockerignore`).

- `./maps` is mounted into the container read-only. A `*.json` file put there is loaded after
  `docker compose restart settoria`. See [Maps](Maps.md#putting-your-maps-in-the-lobby).
- The built-in maps and the example map are compiled into the program (`include_str!`), so `maps/` and `examples/` are
  copied into the build; `docs/` is copied into the client stage because the changelog page imports
  `docs/Changelog.md`.
- The game keeps its state in memory only: stopping the container ends the game.
- The server's start-up log can't see the host's address from inside the container. To have it print a usable "Other
  devices" link, copy `.env.example` to `.env` and set `ADVERTISE_HOST`. Allow the port through the host's firewall if other
  devices can't connect.
- The `web-dev` service mounts `./web` and `./docs`, so saving a file updates the browser; the Rust server is rebuilt with
  `up --build`.

## Configuration

| Variable | Used by | Default | What it does |
|---|---|---|---|
| `PORT` | server, Compose | `3000` | The port the game is served on. |
| `STATIC_DIR` | server | `web/dist` (in Docker `/app/web/dist`) | Where the built client is served from. |
| `MAPS_DIR` | server | `maps` (in Docker `/app/maps`) | The folder custom maps are read from at start-up. |
| `ADVERTISE_HOST` | server, Compose | the computer's detected address | The address printed as "Other devices" (needed in Docker). |
| `DEV_PORT` | Compose | `5173` | The port of the hot-reloading client (`--profile dev`). |
| `API_TARGET` | `npm run dev` | `localhost:3000` | Where the dev client proxies `/ws` and `/api`. |
| `CHROME_PATH` | `npm run shots` | auto | Chrome or Chromium for screenshots. |
| `UPDATE_GOLDEN` | `cargo test` | unset | When set, the `golden` test rewrites the golden file. |

Put the Compose ones in a `.env` file next to `docker-compose.yml` (see `.env.example`; `.env` is ignored by git).

## The changelog

`docs/Changelog.md` is **for players**: the game shows it on its changelog page, newest first, so write entries as you
would tell a player. The page reads the file at build time, so a new entry shows after the client is rebuilt.

    ### [v2026.11.22]
    - boards are sent in a compact form

- A heading `### [vYYYY.MM.DD]` starts an entry (the `v` and the brackets are optional; entries are sorted by that text, so
  keep it to the date).
- Lines starting with `- ` or `* ` become a bullet list. Other lines are joined into one sentence.
- Any other heading, such as the title, ends the previous entry and is not shown.

Developer notes belong in these docs, not in the changelog.

## Releasing

1. Update the docs that the change touched (see the recipes in [Architecture](Architecture.md#recipes-adding-things)).
2. Add an entry to `docs/Changelog.md`.
3. `cargo test --release`, `cd web && npm test`, and the two Node tests above.
4. `cd web && npm run build` to refresh `web/dist`, because the prebuilt client is what `cargo run` serves. (Docker builds
   its own.) The build leaves a `.br` and a `.gz` beside the `.js`, `.css` and `.html` files, and those belong in the
   commit with the rest of `web/dist`: the server sends them to browsers that accept them. A `web/dist` without them
   still works; everything is just sent uncompressed. To check, `curl -sI -H 'Accept-Encoding: br' localhost:3000/assets/<the .js file>`
   should show `content-encoding: br` and `cache-control: public, max-age=31536000, immutable`.
5. Optionally, run the server and `node scripts/smoke.mjs`, and then `docker compose up --build` to check the image.
