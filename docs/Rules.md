# Rules

This is how Settoria plays, as the server implements it (`src/game/`; the rules engine validates every action, so
what is written here is what is enforced). The rules are Catan's, plus a handful of Settoria's own: special hexes
(volcano and gold), bridges, two trading rules aimed at fairness, and a chosen set of **game settings** the host can
change in the lobby.

- [The game at a glance](#the-game-at-a-glance)
- [Setup](#setup)
- [A turn](#a-turn)
- [Building](#building)
- [Special hexes: volcano and gold](#special-hexes-volcano-and-gold)
- [Bridges](#bridges)
- [Trading](#trading)
- [Development cards](#development-cards)
- [Awards and winning](#awards-and-winning)
- [Five to twelve players](#five-to-twelve-players)
- [Game settings](#game-settings)
- [Timers](#timers)
- [Speed mode](#speed-mode)
- [Statistics shown to players](#statistics-shown-to-players)

## The game at a glance

Two to twelve players take turns on a board of hexes (see [Maps](Maps.md)). Each hex produces a resource when its number
token is rolled. Players spend resources to build roads, settlements and cities, which score points, and race to be the
first to reach the **points target** (15 by default) on their own turn.

Resources are brick, lumber, ore, grain and wool, produced by hills, forest, mountains, fields and pasture. A desert
produces nothing and is where the robber starts (unless the map says otherwise). Volcano and gold hexes produce nothing
by themselves; see [Special hexes](#special-hexes-volcano-and-gold).

| Piece | Cost | Points | Limit (default) |
|---|---|---|---|
| Road | 1 brick, 1 lumber | 0 | 15 roads (bridges count against this) |
| Settlement | 1 brick, 1 lumber, 1 grain, 1 wool | 1 | 5 |
| City (replaces a settlement) | 3 ore, 2 grain | 2 | 4 |
| Bridge | 1 brick, 1 lumber, 1 ore | 0 | uses one of the 15 road pieces |
| Development card | 1 ore, 1 grain, 1 wool | see below | the deck |

Points and limits can be changed per game in the [settings](#game-settings).

## Setup

- The turn order is drawn at random when the game starts. The same order is used for the setup draft (forward, then
  backward, then forward again with three or more starting settlements, and so on) and for every turn afterwards. The
  first player to place also takes the first turn.
- Each placement is a settlement and then a road touching it. Settlements obey the distance rule (no building on a
  neighbouring corner).
- Each player's **last** starting settlement pays out: one card for each producing hex around it. A volcano or a gold
  field next to it gives a **free pick** instead (one card of the player's choice per such hex, and no eruption).
- With many starting settlements the board can fill up before everyone has placed theirs. Setup then ends early and the
  game log says so.
- The number tokens are dealt when the game starts. Unless the map says otherwise, no two touching hexes share a number
  and no 6 or 8 touches another 6 or 8.

## A turn

1. **Roll.** The player has 10 seconds to roll (the dice are rolled for them if they don't). A development card may
   be played before rolling; nothing else can be done until the roll.
2. **Production.** Every hex with the rolled number pays one card per settlement and two per city around it, except
   the hex the robber is on. If the bank can't cover a resource and more than one player is owed it, nobody gets it;
   if only one player is owed it, they get what is left.
3. **A 7** produces nothing. Instead:
   1. Every player holding **more than 7** cards discards half of them, rounded down (the limit is a
      [setting](#game-settings), and can rise during the game: see **Variable discard limit**). Everyone discards at
      once.
   2. The player who rolled moves the robber to a different hex. With **Friendly Robber** on, the robber can't go
      onto a hex that touches a protected player (see below), unless every hex does.
   3. They steal one random card from a player with a building on that hex (they choose who if there are several).
4. **Main phase**, in any order and as often as the player can pay: build, trade with the bank, trade with players,
   play one development card, then end the turn.

A volcano or gold hex whose number is rolled adds a pick step after production, see below. A Knight played
mid-turn runs the same robber steps as a 7 does, without the discard.

**Variable discard limit** (off by default): the discard limit starts at the *Card discard limit* and goes up by 1 after
every *x* rolls of the dice (every roll of the game counts, whoever rolls it, and so does a roll made with an Alchemist),
until it reaches the *cap*. A 7 is held to the limit set by the rolls before it: with a start of 7, *x* of 10 and a cap of
9, rolls 1 to 10 use 7, rolls 11 to 20 use 8, and every roll after that 9. Both *x* and the cap are settings, and the cap
can't be lower than the start. The game screen shows the limit in force.

**Friendly Robber** (off by default) protects every other player who has no more visible points than the number every
player has once setup is over (starting settlements times points per settlement, so 2 with the defaults). Hidden
Victory Point cards don't count towards visible points.

## Building

- **Road**: connects to one of your own roads or buildings. An opponent's settlement or city stops a road from
  continuing through that corner (this matters for [Longest Road](#awards-and-winning)).
- **Settlement**: on a free corner with no building on any neighbouring corner, connected to one of your roads.
- **City**: replaces one of your settlements, and the settlement's piece goes back to your stock to be built again.
  Cities pay double.
- Every piece has a limit (see the table above). Bridge sites are not road sites: roads are never built over water.

## Special hexes: volcano and gold

Neither produces a resource by itself. Instead, when its number is rolled (not on a 7), each player with a building
on it **picks free resources from the bank**. Both are only used on maps that contain them. None of the
built-in maps does, so they appear on custom maps (the [map editor](Maps.md#the-map-editor) has a piece for each).

**Free picks**

- A settlement on the hex is worth one pick, a city two, added up over the player's buildings on that hex.
- Picks are made one at a time, from the player whose turn it is, then in turn order. Each pick is any resource the
  bank still has. If the bank runs out, the picks still owed are dropped and the log says so.
- Each pick has **15 seconds** and does not count against the turn clock. If the time runs out, everything that
  player still owes at that hex is picked at random.
- The **robber** on the hex blocks the picks, as it blocks production anywhere else.
- If several such hexes carry the number rolled, they come up one after another in the order of the map file.
- Picked cards count as dice income for the statistics.

**Volcano**

- A volcano always carries a **6 or an 8**, whatever the map file says, and by default a map isn't allowed to put a
  volcano beside another 6 or 8 (see [Maps](Maps.md#volcanoes-and-gold)).
- After the picks, the volcano **erupts**: a die is rolled, and the lava runs down the volcano's corner with that number
  (the corners are numbered 1 to 6, clockwise from the upper right). What is on that corner is hit:
  - nothing: nothing happens;
  - a **settlement** is destroyed and its piece goes back to its owner's stock;
  - a **city** is reduced to a settlement. The city piece goes back to the owner's stock, and a settlement piece takes
    its place.
- A volcano with **no building** on it doesn't erupt at all. This is checked as each volcano comes up, so lava from one
  volcano can stop the next from erupting if it destroyed the last building there.
- The robber on a volcano stops the picks but **not the lava**: it still erupts.
- Lava that destroys a settlement may break a road that was blocked by it, so Longest Road is worked out again.
- The last lava flow is shown on the board until the next roll.
- Volcanoes next to a last starting settlement give their free pick during setup and **never erupt then**.

**Loans (repairing a city).** If lava demotes a city and its owner has no settlement piece left to put in its place,
the settlement is built anyway, but it is **on loan**. While you have a settlement on loan, the only city you may build
is the upgrade of that settlement (at the normal price): repair it before building a city anywhere else. If lava later
destroys the settlement that is on loan, there is no piece to return and the loan is simply gone; if it destroys another
of your settlements while you have a loan, the piece that comes home clears the loan instead of going to your stock.

**Gold** works like a volcano without the lava: the same picks, the same 15 seconds, the same robber rule. It carries an
ordinary number token from the map's pool, and a building on gold is never at risk.

## Bridges

Bridges are **off by default**; the host turns them on in the lobby's game rules.

- A **bridge site** is a stretch of open water exactly one hex side long, between two land corners (a narrow channel,
  or an inlet). Sites are worked out from the map automatically, so no map file mentions them. The standard
  island has none.
- A bridge costs 1 brick, 1 lumber and 1 ore, can be built in your main phase, must connect to your roads or
  buildings like a road does, uses one of your road pieces and counts as **one road** for Longest Road.
- Bridges can't be built in setup, and Road Building places roads only. The distance rule for settlements ignores
  bridges (it only looks at land).

## Trading

**With the bank.** You give 4 of one resource for 1 of another by default. A 3:1 harbour (any resource) improves that
to 3:1, and a 2:1 harbour improves it to 2:1 for its own resource. The base ratio is a [setting](#game-settings) (1 to
8). A harbour belongs to anyone with a settlement or city on one of its two corners.

**With players.** On your own turn, after rolling, you can have up to **4 offers open at once**. Each offer is shown
to everybody else, with its own give and want.

- An offer needs cards on both sides, may not give and ask for the same resource, and may not repeat an offer that is
  already open.
- Every other player accepts or declines each offer separately. A declined offer disappears from that player's screen
  (the proposer still sees it marked declined).
- The proposer **confirms** an offer with one of the players who accepted it. Completing one offer leaves the others
  open, except that any the proposer can no longer pay for are withdrawn, and a player who accepted but can no
  longer pay is asked again.
- Open offers end with your turn. Playing a development card also withdraws all of them.

**Two fairness rules** (added on 2026-11-18):

1. **A harbour owner can't make bank-rate deals with players.** If an offer is nothing but a trade the harbour would
   give you with the bank (3 for 1 with a 3:1 harbour, 2 for 1 with a matching 2:1 harbour, or several of those at
   once), the harbour's owner can't propose it or accept it: "trade with the bank instead". Whether an offer is a
   bank-rate deal is decided on the harbours of the player *giving* the cards, as follows:
   - 6 lumber for 2 brick, or 3 lumber for 1 brick, are bank deals for someone with a 3:1 harbour;
   - 3 lumber and 1 ore for 1 brick, 3 lumber for 2 brick, or a 4:1 deal are **not**, because they don't match a ratio
     the harbour gives, so they are allowed.
   Such a player is shown the offer marked *blocked*; they can still decline it. An offer that becomes a bank deal
   because its proposer builds on a harbour is withdrawn.
2. **The Monopoly lock.** If you gave away **2 or more** of a resource to other players in completed trades this turn,
   you can't play Monopoly on that resource until your next turn. Bank trades don't count.

## Development cards

The deck has 25 cards by default, in the standard proportions: 14 Knights, 5 Victory Points, and 2 each of Road
Building, Year of Plenty and Monopoly. A different deck size (a [setting](#game-settings)) keeps the same proportions.

- You may play **one development card per turn**, never one you bought this turn, and Victory Point cards are not played
  (they count automatically).
- Any card may be played before the roll or in the main phase (except the Alchemist, which is only played before the roll),
  but playing one withdraws all your open trade offers.
- **Knight**: move the robber and steal, like a 7. Knights count towards Largest Army.
- **Road Building**: place two roads for free (fewer if you have fewer pieces left). You can't play it with nowhere to
  build. Roads you don't place when the turn clock runs out are placed at random.
- **Year of Plenty**: take any two resources from the bank (the bank must have them).
- **Monopoly**: name a resource and take every card of it from every other player (unless
  [locked](#trading)).
- **Victory Point**: worth 1 point (a [setting](#game-settings)), hidden from everyone else until the game is over.
- **Alchemist** (only with the *Alchemist card* [setting](#game-settings), off by default): play it at the start of your
  turn, before you roll, **instead of rolling**. You choose what each of the two dice shows (1 to 6 each) and the roll
  carries out exactly as if the dice had fallen that way: production, or the 7 sequence if you name a 7. It counts as a
  roll (for the variable discard limit and the dice statistics) and as your one development card for the turn. It can't
  be played in Speed mode, which has no turns, so the deck has none there.

With the Alchemist on, the deck swaps two Knights for two Alchemists: in a 25-card deck 12 Knights, 5 Victory Points, 2 each
of Road Building, Year of Plenty, Monopoly and Alchemist. Other deck sizes keep these proportions.

## Awards and winning

- **Longest Road** (2 points): a continuous road (or road with bridges) of at least **5** pieces, the longest on the
  board. The holder keeps the award on a tie. If the holder loses the lead and several players are tied for longest,
  the award is held by nobody until someone has the sole longest road. An opponent's settlement or city on a corner
  breaks a road at that corner.
- **Largest Army** (2 points): at least **3** played Knights, and more than the current holder.
- **Winning**: the first player to have at least the points target (15 by default), counting hidden Victory Point
  cards, **on their own turn**, wins. Two players use the same rules unchanged.

Point values, the minimum lengths and the points target are all [settings](#game-settings).

## Five to twelve players

The same rules apply on a map that allows them (Earth, USA and Europe, and any custom map that says so). So that the shared
bank and the development deck don't run dry, the host's choices are scaled to the table, **but only where they were
left at the standard size**:

| Players | Bank (each resource) | Development deck |
|---|---|---|
| 2 to 4 | 19 | 25 |
| 5 or 6 | 24 | 34 |
| 7 or 8 | 30 | 44 |
| 9 or 10 | 36 | 54 |
| 11 or 12 | 42 | 60 |

A bank or deck the host changed is left as chosen. The number of starting settlements the table can have is limited
by what fits: players times starting settlements may not be more than 60 (twelve players with the most starting settlements, 5, each).

## Game settings

The host can change every rule in this table in the lobby. The server checks each value (`Settings::validate` in
`src/game/settings.rs`) and refuses anything outside its range. A map may also suggest a set of settings
(`recommended_settings`); the lobby shows what it would change and the host decides whether to apply it. The log
opens a game with a line for everything that is not the standard value.

| Setting (`Settings` field) | Default | Allowed | What it does |
|---|---|---|---|
| Points to win (`points_to_win`) | 15 | 3 to 99 | Points needed to win, checked on your own turn. |
| Card discard limit (`discard_limit`) | 7 | 1 to 50 | After a 7, players holding more than this discard half. |
| Variable discard limit (`variable_discard`) | off | on/off | The discard limit rises as the game goes on (see [A turn](#a-turn)). |
| Rolls per increase (`discard_step_rolls`) | 10 | 1 to 99 | With the variable limit: rolls of the dice between two rises of 1. |
| Discard limit cap (`discard_limit_cap`) | 10 | 1 to 50 | With the variable limit: the highest it can reach. Not below the discard limit. |
| Alchemist card (`alchemist`) | off | on/off | Adds the [Alchemist](#development-cards) to the deck (not in Speed mode). |
| Friendly Robber (`friendly_robber`) | off | on/off | The robber can't go onto hexes next to players with few points (see [A turn](#a-turn)). |
| Bridges (`bridges`) | off | on/off | Players may build [bridges](#bridges). |
| Max roads (`max_roads`) | 15 | 1 to 50 | Road pieces per player (bridges use them too). |
| Max cities (`max_cities`) | 4 | 0 to 20 | City pieces per player. |
| Max settlements (`max_settlements`) | 5 | 1 to 20 | Settlement pieces per player. |
| Development cards (`dev_cards`) | 25 | 0 to 60 | Size of the deck, in the standard proportions. |
| Max of each resource (`resource_max`) | 19 | 1 to 99 | Cards of each resource in the bank. |
| Settlement points (`settlement_points`) | 1 | 0 to 10 | Points for each settlement. |
| City points (`city_points`) | 2 | 0 to 10 | Points for each city. |
| Longest Road points (`longest_road_points`) | 2 | 0 to 10 | Points for the award. |
| Largest Army points (`largest_army_points`) | 2 | 0 to 10 | Points for the award. |
| Victory Point card points (`vp_card_points`) | 1 | 0 to 10 | Points for each Victory Point card. |
| Longest Road minimum (`longest_road_min`) | 5 | 2 to 20 | Shortest road that can hold the award. |
| Largest Army minimum (`largest_army_min`) | 3 | 1 to 20 | Fewest Knights that can hold the award. |
| Starting settlements (`starting_settlements`) | 2 | 1 to 5 | Settlements (each with a road) every player places in setup. |
| Bank trade ratio (`bank_ratio`) | 4 | 1 to 8 | Cards given to the bank for one card; harbours can only improve on it. |

Combinations that can't be played are refused too: starting settlements may not exceed max settlements or max roads,
and players times starting settlements may not be more than 60.

## Timers

The server owns every clock, so a closed tab or a dropped connection is auto-played like any other timeout. The clock
beside the status pill counts down whichever timer is running, and turns red under 10 seconds.

| What | Time | If it runs out |
|---|---|---|
| Setup: place a settlement (all modes) | 120 s | placed on a random legal corner |
| Setup: place the road (all modes) | 30 s | placed on a random legal side next to it |
| Roll the dice | 10 s | the dice are rolled for you |
| Discard after a 7 (everyone at once) | 20 s | each player who hasn't discarded loses random cards |
| Move the robber | 20 s | placed on a random 6 or 8 hex (never the one it is on), or any allowed hex if Friendly Robber rules those out |
| Choose who to rob | 15 s | a random candidate |
| Pick free resources at a volcano or gold field | 15 s | everything that player still owes is picked at random from what the bank has |
| Your turn (the **turn clock**) | 30 / 60 / 120 / 240 s by mode | the turn ends (unplaced Road Building roads are placed at random) |

The host chooses the mode: **Very Fast** 30 s, **Fast** 60 s, **Normal** 120 s (the default) or **Slow** 240 s.

The roll window, the whole 7 sequence (discard, robber, steal) and the volcano and gold picks are **not** charged to
the turn clock; it starts when they finish. A Knight played mid-turn pauses the turn clock for its robber move, and a
Knight played before rolling keeps the unused part of the 10 s roll window.

**Time bonuses** are added to the turn clock and can never push it above the mode's starting time (for example +10 s
with 22 s left on Very Fast gives 30 s, not 32 s): **+10 s** for each road, bridge, settlement and city built, each
development card bought, each bank trade, and each player trade that is completed (the proposer confirming with
someone who accepted). Roads from a Road Building card count as roads. Sending, accepting or declining an offer earns
nothing by itself.

## Speed mode

**Settoria Speed** is a game mode the host switches on in the lobby (the `speed` field of the [settings](#game-settings),
shown as *Game mode*). The rules above all apply, except for these:

- **No turns.** Setup is still turn based (same order, same clocks). When it ends, the table is open: every player can
  build, trade with the bank, trade with each other and play development cards at any moment. The turn timer, the
  roll window and the time bonuses do not exist, and `roll` and `end_turn` are refused.
- **The server rolls.** The dice are rolled automatically every **20 seconds**. A countdown shows the time to the next
  roll all the time and starts again at 20 after every roll.
- **Simultaneous requests.** The server handles one request at a time. If two players who can both afford a
  settlement build on the same spot "at once", the first request it processes succeeds and the second is refused like
  any other illegal move (and costs nothing).
- **The robber moves down the line.** Each 7 is placed by the next player in turn order: the first 7 by the first
  player, the second by the second, and so on, wrapping round. A Knight is placed by whoever plays it and does not
  move the rotation. Everyone else can see who is next.
- **A 7 stops the table.** While anyone is discarding, and while the robber is placed and a victim chosen, nobody
  else can build or trade, open offers are withdrawn and the roll countdown is frozen. It is not running during any
  of this: it starts again at a full 20 seconds when the last step is done. (A Knight freezes it too, and the
  countdown carries on with the time that was left.) The 7's own clocks are as in [Timers](#timers).
- **Volcano and gold picks** after a roll stop the table in the same way (15 seconds each, starting with the player
  next in the robber rotation), and the countdown restarts afterwards.
- **A round is the time between two rolls.** A player may play one development card per round, a card bought is
  playable from the next roll, and giving away 2 or more of a resource in trades locks Monopoly on it only until the
  next roll.
- **Road Building** does not stop the table: it gives the player two free roads (fewer with fewer pieces left) to
  place whenever they like.
- **Offers.** Every player may have up to 4 offers open at once. A player can only withdraw their own, and playing a
  development card withdraws only the player's own offers. Offers stay open across rolls (but not across a 7).
- **Winning.** Anyone with the points target wins the moment they have it, not only on their own turn (the player
  whose action it was first, if several have it).

## Statistics shown to players

- **Dice income** is every card a player received from a dice roll: production, plus free picks at volcanoes and gold.
  It is public all game, because it feeds the live win-chance bar on the player panels.
- **Postgame%** (the end-game screen) is how likely each player was to win, judged only by dice income. Each player's
  income is taken relative to the leader's (`x`, zero for the leader and negative for everyone else), turned into a
  probability with the logistic model `1 / (1 + e^-(0.0889 + 0.1795 x))`, and normalised so the column adds up to
  100%. The code is in `web/src/postgame.ts`; the live bar uses the same formula with the income so far.
- The end-game **Stats** tab also shows development cards bought, completed player trades, and where every player's points
  came from (hidden Victory Point cards are revealed there).
