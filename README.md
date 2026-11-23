# Settoria

Catan for 2 to 8 players on a local network, on the standard board or on custom maps. Everyone plays in a browser; one
computer runs the server.

- **2 to 8 players**, with CPU players to fill empty seats.
- **Maps**: Standard (2 to 4 players), Earth and USA (up to 8), plus any map you make with the built-in map editor or write
  as a JSON file. Custom maps can use volcano and gold hexes.
- **Game rules** the host can switch on or tune in the lobby: bridges, Friendly Robber, and sixteen numbers (points to win,
  piece limits, bank ratio, ...).
- **Fair trading**: harbour owners can't make bank-rate deals with players, and a Monopoly is locked after you give away
  cards in trades.
- **Server-run clocks**, so a dropped connection never stalls the table, and a reloaded tab rejoins its seat by itself.

## Run it

Needs Rust (1.75+). Node is only needed to rebuild the frontend; a prebuilt copy is already in `web/dist`.

    cargo run --release          # or ./run.sh to rebuild the frontend first

The server prints two addresses:

    This computer:  http://localhost:3000
    Other devices:  http://192.168.x.x:3000   <- share this one

Everyone opens the address in a browser, enters a name, picks a color and takes a seat. The first player to join is the
host. The host picks the **map** and the **turn timer**, can change any of the **game rules**, adds **bots** if people are
short, and presses **Start**. Maps meant for another number of players than are seated are greyed out, so 5 to 8 players
play on Earth or USA.

### With Docker

No Rust or Node needed, only Docker with Compose. The image builds the client and the server itself.

    docker compose up --build            # the game on http://localhost:3000

Friends on your network open `http://<this computer's address>:3000`. The server can't see the host's address from inside
the container, so to have it print a usable "Other devices" link, copy `.env.example` to `.env` and set `ADVERTISE_HOST`.
Allow the port through the host's firewall if other devices can't connect. All the Docker commands (dev client, tests,
checking a map) are in [Development](docs/Development.md#docker).

The game keeps its state in memory only: stopping the server ends the game.

## Playing against bots

Short of people? The host presses **Add bot** in the lobby. Bots sit at the table like anyone else (people and bots are
at most 8 together, and a game needs at least two seats), play by the same rules and clocks, and stay for the next game.
They use the bank but never trade with players. How they choose their moves is in [Bots](docs/Bots.md).

## Maps

**Standard**, **Earth** (81 hexes on 7 islands) and **USA** (140 hexes) come with the game. You can add your own:

- build one in the **map editor**, linked at the bottom of the lobby, and download the file;
- or write the JSON by hand and check it with `cargo run --release -- validate my-map.json`;
- put the file in `maps/` and restart the server, or upload it from the lobby.

Volcano and gold hexes, harbours, number tokens and recommended rules are all part of a map file. Everything about maps is
in [Maps](docs/Maps.md), including the two extra map files, Texas and Europe, that are in `maps/builtin/` but not yet built in.

## Documentation

| Read | For |
|---|---|
| [Rules](docs/Rules.md) | How the game plays: setup, a turn, volcano and gold, bridges, trading, development cards, every game setting, and the timers. |
| [Maps](docs/Maps.md) | The maps that come with the game, the map editor, the map file format, adding maps, and `validate`. |
| [Bots](docs/Bots.md) | Using bots, and how they decide. |
| [Protocol](docs/Protocol.md) | The WebSocket messages, the game view, patches, the HTTP routes and the board on the wire. |
| [Architecture](docs/Architecture.md) | How the code is organised, how a move travels, and recipes for adding a hex type, a setting or an action. |
| [Development](docs/Development.md) | Building, running, tests, Docker, configuration, and releasing. |
| [Changelog](docs/Changelog.md) | What changed, newest first. This file is also shown in the game, so it is written for players. |

## License

MIT, see [LICENSE.txt](LICENSE.txt).
