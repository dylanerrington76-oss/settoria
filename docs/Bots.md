# Bots

A bot is a computer player that sits at the table like anyone else, for a group that is short of people. This page says
how to use one and how one decides what to do. The code is `src/bot.rs`: the numbers are constants at the top, and
`choose` is the one function that decides a move.

- [Using bots](#using-bots)
- [How a bot decides](#how-a-bot-decides)
- [Phase by phase](#phase-by-phase)
- [Special hexes and bridges](#special-hexes-and-bridges)
- [How bots are run](#how-bots-are-run)
- [Tuning and testing](#tuning-and-testing)
- [What bots don't do](#what-bots-dont-do)

## Using bots

- The host presses **Add bot** in the lobby to seat a computer player; the ✕ on a bot's row takes it away again. Only the
  host can add or remove bots, and only in the lobby (`add_bot` and `remove_bot` in [Protocol](Protocol.md#room-messages)).
- Bots count towards the table limit: people and bots together are at most 12, and a game needs at least two seats, which
  can be one person and one bot.
- Each bot is named after a famous robot, drawn at random from Bender, C-3PO, Multivac, Skynet, AM, Hal 9000, Data, GLaDOS, Marvin, WOPR, Johnny 5 and Bishop. A
  name a person or another bot at the table already has is never used (and a person called one of those names keeps it).
  If every name is taken, the bot is "Bot 1", "Bot 2" and so on.
- A bot is always "connected", plays whenever it is its turn (about half a second to a second per move, so you can follow
  it), and stays at the table for the next game. The host is always a person: if the host leaves and a bot would be
  first, the first person takes over.
- Bots play by the same rules, and obey the same [timers](Rules.md#timers), as people.

## How a bot decides

A bot sees the game as a player at its seat would: its own hand and development cards, and for everyone else only what is
public (card counts, points, buildings). Each time it has something to do it lists the moves that are legal right now,
gives each one a number, and plays the highest, picking at random among ties. When nothing scores above ending the turn
(which is worth 0), it ends the turn, and it never takes more than **15 actions in one turn** (the roll counts).

Points and production dominate:

    score = points_gained * 100 + production_gained * 10 + small bonuses

Production is counted in **pips**: a 6 or 8 is five pips, a 5 or 9 four, a 4 or 10 three, a 3 or 11 two, a 2 or 12 one. A
pip is worth 10, so a spot's production is in the same units as a point (worth 100).

| Move | Worth |
|---|---|
| Settlement | its point(s), plus the pips of the hexes around it, a bonus for each resource the bot doesn't produce yet (+8) and for a harbour it can use (+12 for a harbour matching something it produces, +5 for a 3:1) |
| City | the extra point(s), plus the spot's pips again (a city doubles production) |
| Road, bridge | the best corner it newly opens up: half that corner's score if a settlement could go there as soon as the road is built, a quarter if one more road is needed, an eighth for two more; a quarter of that again while a settlement can already be built |
| Development card | a middling baseline (30), more (+30) when ore, grain and wool are spare after saving for its best build, and more again (+30) when nothing else is affordable |
| Bank trade | 6/10 of the build it makes affordable this turn, and only for a build worth at least 30; never otherwise |
| Robber | the production of the other players' buildings on a hex, doubled for cities and weighted toward whoever is ahead; a heavy penalty (1000 per building) for its own |
| Steal | the player with the most cards, then the most public points |
| Discard, volcano or gold pick | keeps (or takes) what completes its best build, affordable or not |
| End turn | 0 |

The weighting toward the leader (`pressure`) is 100%, plus the player's share of the points target, plus another 50% for
whoever has the most public points among the others.

## Phase by phase

- **Setup.** Settlements are scored as above; roads by the best corner they open up (free roads from a Road Building card
  are scored the same way).
- **Roll.** Rolling scores 50. A Knight is played first only if it scores more than that: it must free the bot's own blocked
  production or win Largest Army.
- **Main.** All the builds (settlement, city, road, bridge, development card) are listed with their value. An affordable
  one is scored at its value; one that is worth at least 30 and not yet affordable may be reached by a bank trade, scored
  as above. Development cards in hand are scored in the same units, below.
- **Robber** after a 7 or a Knight: every legal hex is scored (see the table) and the best is taken. With Friendly Robber
  on, only the hexes the rules allow are considered.
- **Discard.** One card at a time from the biggest pile beyond what the best build needs.
- **Development cards.** Knight: a little even with nothing to rob (15), plus the bot's own production it frees, plus Largest
  Army if it takes it, plus half the best robbing. Road Building: twice the best road it can place, if that is worth at
  least 15. Year of Plenty: nine tenths of a build it completes. Monopoly: what it expects to take (it can't see hands, so
  a fifth of the cards the others hold) plus a bonus if the resource is one it is short of, if that reaches 30 and the resource
  is not [locked](Rules.md#trading). Alchemist: before the roll, a bot names the best total from 2 to 12 other than 7 (the one that pays it most, and the
  others least, per player) when that beats an average roll by at least `ALCHEMIST_MIN_GAIN` half cards; otherwise it
  rolls. It never names a 7. Bots play only one card per turn, like anyone.
- **Trading with players.** A bot never proposes a trade, and it declines any offer a player makes it, so bots don't trade
  with people. (They use the bank.)

## Special hexes and bridges

- A **volcano's** production counts for **half**, because its lava can wreck the building, and a **gold** field counts in
  full, because gold gives the same free pick with no lava. Both are valued by their number's pips. See
  [Rules](Rules.md#special-hexes-volcano-and-gold).
- A **volcano or gold pick** takes the resource that is furthest from completing the bot's current target (any, if it is
  already covered), from what the bank still has, with ties broken at random. The bot answers within a second, so its picks
  never run into the 15-second limit.
- **Bridges** are listed like roads, with the same value, and cost 1 brick, 1 lumber and 1 ore (only in games with bridges on).
- **Repairing a city.** If lava left the bot a settlement on loan, the only city it considers is the upgrade of that settlement.

## How bots are run

`Driver` (in `src/bot.rs`) is what the server runs each time its timer ticks (every 200 ms; see `Room::tick_at` in
`src/main.rs`):

- It lets **one bot make one move** at a time, with a random pause of 450 to 900 ms after each, so people can follow what
  happens. The first move comes a moment after the game appears on everyone's screen.
- It counts each seat's actions in its current turn, and ends the turn after 15.
- A bot is asked about moves even when it isn't its turn: to discard, to pick at a volcano or gold field, and to decline
  trade offers.
- The bot only ever picks moves the rules list as legal. If the rules refuse one anyway, that is a bug: it is logged to
  stderr, the bots stay still for 2 seconds, and the game's own clocks play the seat if it keeps happening.

## Tuning and testing

The weights are named constants at the top of `src/bot.rs`: `POINT`, `PIP`, `NEW_RESOURCE_BONUS`, `HARBOUR_MATCH_BONUS`,
`HARBOUR_ANY_BONUS`, `ROAD_LOOKAHEAD`, `ROAD_WHEN_SETTLING_DIVISOR`, `DEV_BASE`, `DEV_SPARE_BONUS`, `DEV_IDLE_BONUS`,
`ROLL_SCORE`, `KNIGHT_BASE`, `ALCHEMIST_MIN_GAIN`, `ALCHEMIST_GAIN_SCORE`, `TRADE_SHARE_TENTHS`, `TRADE_MIN_TARGET`, `PLENTY_SHARE_TENTHS`, `MONOPOLY_MIN`,
`ROAD_BUILDING_MIN`, `OWN_HEX_PENALTY`, `THINK_MS_MIN` and `THINK_MS_MAX`, and `MAX_ACTIONS_PER_TURN`. Change a number and
`cargo test --release`: the bot tests (in `src/bot.rs`) check individual choices, and whole games are played to the
end on the standard map, Earth with bridges, and a map with volcanoes, to make sure bots never get stuck or refused.

To teach a bot a new kind of move, add a variant to `Kind` (if it is a build) or a case to `choose`, score it in the same
units as the rest, and add a test. If the rules gain a new action, see the recipe in
[Architecture](Architecture.md#recipes-adding-things).

## What bots don't do

- Propose or accept trades with players.
- Plan for Longest Road: roads are valued only by the settlement spots they reach.
- Remember anything between moves: each choice is made from the game as it stands.
- Read other players' hands. They count cards, as everyone can.
