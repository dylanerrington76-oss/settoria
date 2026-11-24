/** Resource bundle indexed [brick, lumber, ore, grain, wool]. */
export type Res = [number, number, number, number, number];

export const RES_NAMES = ['brick', 'lumber', 'ore', 'grain', 'wool'] as const;
export const RES_ICONS = ['🧱', '🪵', '⛰️', '🌾', '🐑'] as const;
export const RES_IDX = [0, 1, 2, 3, 4] as const;

export const DEV_NAMES = ['Knight', 'Road Building', 'Year of Plenty', 'Monopoly', 'Victory Point'] as const;
export const devHelp = (kind: number, vpCardPoints: number): string =>
  [
    'Move the robber and steal a card',
    'Build 2 roads for free',
    'Take any 2 resources from the bank',
    'Take every card of one resource from the other players',
    `Worth ${vpCardPoints} point${vpCardPoints === 1 ? '' : 's'}. Stays hidden until you win`,
  ][kind];

export const COST_ROAD: Res = [1, 1, 0, 0, 0];
export const COST_SETTLEMENT: Res = [1, 1, 0, 1, 1];
export const COST_CITY: Res = [0, 0, 3, 2, 0];
export const COST_DEV: Res = [0, 0, 1, 1, 1];
/** A bridge: 1 brick, 1 lumber and 1 ore (COST_BRIDGE in src/game.rs). */
export const COST_BRIDGE: Res = [1, 1, 1, 0, 0];

/** How many players one table seats (MIN_PLAYERS and MAX_PLAYERS in src/map_def.rs). */
export const MIN_PLAYERS = 2;
export const MAX_PLAYERS = 12;

/** Colours a player can pick when joining. Hex values must match PALETTE in src/game.rs. */
export const PALETTE = [
  { name: 'Orange', hex: '#FE5100' },
  { name: 'Purple', hex: '#613393' },
  { name: 'Gold', hex: '#FFB81C' },
  { name: 'Green', hex: '#69BE28' },
  { name: 'Blue', hex: '#0033A0' },
  { name: 'Red', hex: '#CC0000' },
  { name: 'Mystic Blue', hex: '#7BAFD4' },
  { name: 'White', hex: '#FFFFFF' },
  { name: 'Silver', hex: '#8A8D8F' },
  { name: 'Black', hex: '#000000' },
  { name: 'Maroon', hex: '#500000' },
  { name: 'Khaki', hex: '#C8C372' },
] as const;

export const colorName = (hex: string): string =>
  PALETTE.find((c) => c.hex.toLowerCase() === hex.toLowerCase())?.name ?? hex;

const channel = (v: number): number => {
  const c = v / 255;
  return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
};
const rgb = (hex: string): [number, number, number] => {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};
/** WCAG relative luminance, 0 (black) to 1 (white). */
export const luminance = (hex: string): number => {
  const [r, g, b] = rgb(hex);
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
};
const contrast = (a: number, b: number): number => (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);

const CREAM_LUM = luminance('#efe9dd');
const textCache = new Map<string, string>();
/**
 * A darker version of a player color for colored text on the cream panels:
 * the color itself if it is already readable, otherwise darkened until it has
 * a 4.5:1 contrast ratio (so gold, white, silver and light blue stay legible).
 */
export function textColor(hex: string): string {
  const hit = textCache.get(hex);
  if (hit) return hit;
  let [r, g, b] = rgb(hex);
  const to = (v: number) => v.toString(16).padStart(2, '0');
  let out = hex;
  for (let i = 0; i < 40 && contrast(luminance(out), CREAM_LUM) < 4.5; i++) {
    r *= 0.93;
    g *= 0.93;
    b *= 0.93;
    out = '#' + to(Math.round(r)) + to(Math.round(g)) + to(Math.round(b));
  }
  textCache.set(hex, out);
  return out;
}

const NAVY_LUM = luminance('#143153');
const lightCache = new Map<string, string>();
/**
 * The counterpart of `textColor` for the dark panels of the phone layout (the log): a lighter version of a
 * player color, lightened until it has a 4.5:1 contrast ratio against the panel's navy.
 */
export function lightTextColor(hex: string): string {
  const hit = lightCache.get(hex);
  if (hit) return hit;
  const [r, g, b] = rgb(hex);
  const mix = (v: number, t: number) => Math.round(v + (255 - v) * t).toString(16).padStart(2, '0');
  let out = hex;
  for (let i = 1; i <= 40 && contrast(luminance(out), NAVY_LUM) < 4.5; i++) out = '#' + mix(r, i * 0.05) + mix(g, i * 0.05) + mix(b, i * 0.05);
  lightCache.set(hex, out);
  return out;
}

/** Turn timer modes the host can choose in the lobby (seconds must match `TimerMode::secs` in src/game.rs). */
export type TimerMode = 'very_fast' | 'fast' | 'normal' | 'slow';
export const TIMER_MODES: { id: TimerMode; label: string; secs: number }[] = [
  { id: 'very_fast', label: 'Very Fast', secs: 30 },
  { id: 'fast', label: 'Fast', secs: 60 },
  { id: 'normal', label: 'Normal', secs: 120 },
  { id: 'slow', label: 'Slow', secs: 240 },
];

/** What the server says about one map it can play (see `MapInfo` in src/maps.rs). The lobby's picker is built from these. */
export interface MapInfo {
  id: string;
  name: string;
  /** Empty if the map doesn't say. */
  author: string;
  hexes: number;
  players_min: number;
  players_max: number;
  /** One of the maps compiled into the server, as opposed to one someone added. */
  builtin: boolean;
  /** The rules the map suggests. Offered to the host, never applied automatically. */
  recommended_settings?: Settings;
}

/**
 * The rules of one game, chosen by the host in the lobby. Field names and
 * limits mirror `Settings` in src/game.rs, which is what actually enforces them.
 */
export interface Settings {
  points_to_win: number;
  discard_limit: number;
  friendly_robber: boolean;
  bridges: boolean;
  max_roads: number;
  max_cities: number;
  max_settlements: number;
  dev_cards: number;
  resource_max: number;
  settlement_points: number;
  city_points: number;
  longest_road_points: number;
  largest_army_points: number;
  vp_card_points: number;
  longest_road_min: number;
  largest_army_min: number;
  starting_settlements: number;
  bank_ratio: number;
  /** Settoria Speed: no turns, the server rolls every `SPEED_ROLL_SECS` seconds, everyone builds and trades at any time. */
  speed: boolean;
}

/** Seconds between the automatic rolls in Speed mode (SPEED_ROLL_SECS in src/game.rs). */
export const SPEED_ROLL_SECS = 20;

/** The standard values; the host's starting point and the "Standard" choice for each setting. */
export const DEFAULT_SETTINGS: Settings = {
  points_to_win: 15,
  discard_limit: 7,
  friendly_robber: false,
  bridges: false,
  max_roads: 15,
  max_cities: 4,
  max_settlements: 5,
  dev_cards: 25,
  resource_max: 19,
  settlement_points: 1,
  city_points: 2,
  longest_road_points: 2,
  largest_army_points: 2,
  vp_card_points: 1,
  longest_road_min: 5,
  largest_army_min: 3,
  starting_settlements: 2,
  bank_ratio: 4,
  speed: false,
};

/** Friendly Robber protects players with this many visible points or fewer in a standard game (2 starting settlements worth 1 point each). */
export const FRIENDLY_ROBBER_MAX_VP = 2;

/**
 * Friendly Robber protects players with this many visible points or fewer: the points every player has once
 * setup is over, i.e. the starting settlements at the settlement value (`Settings::friendly_robber_max_vp` in src/game.rs).
 */
export function friendlyRobberMaxVp(s: Pick<Settings, 'starting_settlements' | 'settlement_points'>): number {
  return s.starting_settlements * s.settlement_points;
}

/** What the running countdown is for. Only `turn` is the standard per-turn clock. */
export type ClockKind = 'setup_settlement' | 'setup_road' | 'roll' | 'discard' | 'robber' | 'steal' | 'volcano' | 'turn' | 'auto_roll';
export const CLOCK_LABELS: Record<ClockKind, string> = {
  setup_settlement: 'Settlement',
  setup_road: 'Road',
  roll: 'Roll',
  discard: 'Discard',
  robber: 'Robber',
  steal: 'Rob',
  volcano: 'Volcano',
  turn: '',
  auto_roll: 'Next roll',
};
export const CLOCK_TITLES: Record<ClockKind, string> = {
  setup_settlement: 'Time left to place a settlement',
  setup_road: 'Time left to place a road',
  roll: 'Time left to roll (not counted against turn time)',
  discard: 'Time left to discard (not counted against turn time)',
  robber: 'Time left to move the robber (not counted against turn time)',
  steal: 'Time left to choose who to rob (not counted against turn time)',
  volcano: 'Time left to pick a free resource from the volcano (not counted against turn time)',
  turn: 'Time left on this turn',
  auto_roll: 'Time until the server rolls the dice',
};
export interface TimerView {
  kind: ClockKind;
  player: number;
  remaining_ms: number;
  total_ms: number;
}

export type Terrain = 'hills' | 'forest' | 'mountains' | 'fields' | 'pasture' | 'desert' | 'volcano' | 'gold';
export type Phase =
  | 'setup'
  | 'roll'
  | 'discard'
  | 'move_robber'
  | 'steal'
  | 'volcano_pick'
  | 'main'
  | 'road_building'
  | 'game_over';

export interface Hex {
  q: number;
  r: number;
  x: number;
  y: number;
  terrain: Terrain;
  number: number | null;
}
export interface Vertex {
  x: number;
  y: number;
}
export interface Edge {
  a: number;
  b: number;
  bridge?: boolean;
}
export interface Port {
  a: number;
  b: number;
  resource: number | null;
}
/** How a map asks to be drawn (`MapStyle` in src/map_def.rs). Cosmetic only; the board is never drawn differently by map id. */
export interface MapStyle {
  /** Draw a faint hex grid across the open sea. */
  sea_grid: boolean;
}
/**
 * A hex as the server sends it: `[q, r, terrain, number]`. Everything else about a hex (where it is drawn, its
 * corners and sides) follows from `q` and `r`, so the server leaves it out; see `expandBoard` in boardGeometry.ts.
 */
export type WireHex = [q: number, r: number, terrain: Terrain, number: number | null];
/**
 * The board as it comes over the socket (`impl Serialize for Board` in src/board_wire.rs): the hexes, the harbours
 * and the few things that can't be worked out from the hexes. `expandBoard` turns it into a `BoardData`.
 */
export interface WireBoard {
  map: string;
  style: MapStyle;
  hexes: WireHex[];
  ports: Port[];
  /** The bridge sites as pairs of corner ids, in edge order (left out when there are none). */
  bridges?: [a: number, b: number][];
  desert: number;
}
/** What the page works with: the board with its corners and sides worked out (see `expandBoard`). */
export interface BoardData {
  /** Which map this is a board of: its id in the server's map list (`MapInfo.id`). */
  map: string;
  /** The map's drawing hints, from its file. */
  style: MapStyle;
  hexes: Hex[];
  vertices: Vertex[];
  edges: Edge[];
  ports: Port[];
  /** The hex the robber starts on. */
  desert: number;
}
export interface Building {
  owner: number;
  city: boolean;
}
/** Where a player's points came from, in points (`VpBreakdown` in src/game.rs). The parts add up to `vp_total`. */
export interface VpBreakdown {
  settlements: number;
  cities: number;
  dev_cards: number;
  largest_army: number;
  longest_road: number;
}
/** Whole-game totals per player, sent once the game is over (`PlayerStats` in src/game.rs). */
export interface PlayerStats {
  /** Resource cards received because of dice rolls, volcano picks included. */
  dice_income: number;
  /** Development cards bought from the deck. */
  dev_cards_bought: number;
  /** Completed player-to-player trades (bank trades are not counted). */
  trades: number;
}
export interface PlayerView {
  id: number;
  name: string;
  color: string;
  connected: boolean;
  cards: number;
  dev_cards: number;
  knights: number;
  roads_left: number;
  settlements_left: number;
  cities_left: number;
  vp: number;
  longest_road: number;
  /** Dice income so far, public all game: the input of the live win% bar (see `postgame.ts`). */
  dice_income: number;
  vp_total: number | null;
  /** Set together with `vp_total`, once the game is over. */
  vp_breakdown: VpBreakdown | null;
  /** Set together with `vp_total`, once the game is over. */
  stats: PlayerStats | null;
}
/** `blocked`: the offer is a bank-rate deal for this player's own harbour, so they can't accept it (only decline). */
export type Reply = 'proposer' | 'pending' | 'accepted' | 'blocked' | 'declined';
/** The most offers one player may have open at once (MAX_OPEN_TRADES in src/game.rs). */
export const MAX_OPEN_TRADES = 4;
/** Seconds a bank trade or a completed player trade adds to the turn clock (BONUS_SECS in src/game.rs). */
export const TRADE_BONUS_SECS = 10;
export interface Trade {
  /** Unique per game; replies, confirmations and withdrawals name an offer by it. */
  id: number;
  from: number;
  give: Res;
  want: Res;
  replies: Reply[];
}
/** One lava flow: the volcano's hex, the die (1 to 6) and the corner it ran down (`Lava` in src/game.rs). */
export interface Lava {
  hex: number;
  /** The volcano's corners are numbered 1 to 6 clockwise from the upper right: corner `i` of the hex is `i + 1`. */
  die: number;
  vertex: number;
}
/** Who is picking a free resource from an erupting volcano, and how many cards they still take (`VolcanoView` in src/game.rs). */
export interface VolcanoView {
  hex: number;
  player: number;
  left: number;
  /** True while a last starting settlement is picking (no eruption follows). */
  setup: boolean;
}
/** What a lava flow did to the corner it reached (`LavaEffect` in src/log_event.rs). */
export type LavaEffect = 'nothing' | 'destroyed' | 'demoted';
export interface Legal {
  settlements: number[];
  cities: number[];
  roads: number[];
  /** Where a bridge could go (empty unless the game has bridges on). */
  bridges: number[];
  hexes: number[];
}
/**
 * One entry of the game log, as sent by the server (`LogEvent` in src/log_event.rs).
 * The server says what happened; `Log` in ui.tsx decides how to show it. `player` is the
 * seat id of whoever the entry is about; resources are indices in RES_NAMES order.
 */
export type LogEvent =
  | { type: 'game_started' }
  | { type: 'map_chosen'; map: string; name: string }
  | { type: 'custom_rules'; changes: string[] }
  | { type: 'turn_order'; order: number[] }
  | { type: 'setup_ended_early' }
  | { type: 'setup_complete' }
  | { type: 'rolled'; player: number; dice: [number, number] }
  /** Speed mode: the server rolled, so nobody is the roller. */
  | { type: 'dice_rolled'; dice: [number, number] }
  | { type: 'collected'; player: number; cards: Res }
  | { type: 'bank_short'; resource: number }
  | { type: 'must_discard'; limit: number }
  | { type: 'discarded'; player: number; count: number }
  | { type: 'robber_must_move' }
  | { type: 'robber_moved'; player: number }
  | { type: 'stole'; player: number; victim: number }
  | { type: 'volcano_picked'; player: number; resource: number }
  | { type: 'volcano_blocked'; hex: number }
  | { type: 'gold_picked'; player: number; resource: number }
  | { type: 'gold_blocked'; hex: number }
  | { type: 'volcano_bank_empty' }
  | { type: 'lava_flow'; hex: number; die: number; effect: LavaEffect; owner: number | null }
  | { type: 'placed_settlement'; player: number }
  | { type: 'built_settlement'; player: number }
  | { type: 'built_road'; player: number }
  | { type: 'built_bridge'; player: number }
  | { type: 'built_city'; player: number }
  | { type: 'bought_dev_card'; player: number }
  | { type: 'played_knight'; player: number }
  | { type: 'played_road_building'; player: number }
  | { type: 'played_year_of_plenty'; player: number; first: number; second: number }
  | { type: 'played_monopoly'; player: number; resource: number; taken: number }
  | { type: 'bank_trade'; player: number; give: number; get: number; ratio: number }
  | { type: 'trade_proposed'; player: number }
  | { type: 'trade_completed'; player: number; with: number }
  | { type: 'longest_road_taken'; player: number; roads: number }
  | { type: 'longest_road_lost' }
  | { type: 'largest_army_taken'; player: number; knights: number }
  | { type: 'won'; player: number; points: number }
  /** A countdown ran out and the server acted. For `discard`, `player` is whoever's turn it is. */
  | { type: 'timed_out'; player: number; kind: ClockKind };

export interface GameView {
  you: number;
  board: BoardData;
  phase: Phase;
  /**
   * Whose turn it is. In Speed mode nobody has one: in setup this is who places, and after it, whoever places the
   * robber on the next 7 (or is placing it now: the player of a 7 in the rotation, or of a Knight).
   */
  current: number;
  rolled: boolean;
  dice: [number, number] | null;
  /** How many times each dice total was rolled this game: index 0 is 2, index 10 is 12 (`Game::roll_counts` in src/game.rs). */
  roll_counts: number[];
  robber: number;
  buildings: (Building | null)[];
  roads: (number | null)[];
  players: PlayerView[];
  hand: Res;
  dev: [number, number, number, number, number];
  dev_new: [number, number, number, number, number];
  vp_you: number;
  bank: Res;
  deck_left: number;
  dev_played: boolean;
  ratios: [number, number, number, number, number];
  /** The harbours you own: players can't be offered the bank-rate deals these give (see `bankRateDeal`). */
  harbours: Harbours;
  /** Resources you can't play Monopoly on until your turn ends (Speed mode: until the next roll); none unless it is your turn (MONOPOLY_LOCK_CARDS in src/game.rs). */
  monopoly_locked: boolean[];
  setup_needs: 'settlement' | 'road' | null;
  discards: [number, number][];
  steal_candidates: number[];
  /** Set while players pick free resources from an erupting volcano (one player at a time). */
  volcano: VolcanoView | null;
  /** Where the last lava flow went; cleared by the next roll. */
  lava: Lava | null;
  /** Corners holding a settlement that is on loan after lava hit a city and its owner had no settlement piece left. */
  damaged: number[];
  /** Free roads still to place: from the phase after a Road Building card, or (Speed mode) your own credit of free roads. */
  road_building_left: number;
  /** Offers currently open; in a classic game only the player whose turn it is can have any. */
  trades: Trade[];
  legal: Legal;
  longest_road_holder: number | null;
  largest_army_holder: number | null;
  winner: number | null;
  log: LogEvent[];
  /** Number of the first line in `log` (lines are numbered from 0 for the whole game). */
  log_start: number;
  /** Seat ids in turn order (randomised at the start of the game). */
  order: number[];
  timer_mode: TimerMode;
  timer: TimerView | null;
  settings: Settings;
}
/** The view as it comes over the socket: the same, but with the board in its short form. */
export type WireGameView = Omit<GameView, 'board'> & { board: WireBoard };

export interface LobbyPlayer {
  name: string;
  color: string;
  connected: boolean;
  /** A computer player (see `src/bot.rs`): the host can remove it again before the game starts. */
  bot?: boolean;
}

/** One step of a patch's path: a key of an object, or an index into an array. */
export type PatchStep = string | number;
/** `[path, value]` sets what is at `path` to `value`; `[path]` removes the key there (see `src/patch.rs`). */
export type PatchOp = [path: PatchStep[], value?: unknown];
/**
 * What changed in the view since the last message: `ops` for everything but the board and the log, and the log
 * events the client doesn't have yet, numbered from `log_from`. `seq` is one more than the last message's.
 */
export interface GamePatch {
  type: 'patch';
  seq: number;
  ops: PatchOp[];
  log_from?: number;
  log?: LogEvent[];
}

export type ServerMsg =
  | { type: 'welcome'; token: string }
  | { type: 'lobby'; you: number; players: LobbyPlayer[]; timer: TimerMode; settings: Settings; map: string; maps: MapInfo[] }
  /** The whole view, with the board. `seq` (0) is there when the server is sending patches, which count on from it. */
  | { type: 'game'; seq?: number; view: WireGameView }
  | GamePatch
  | { type: 'error'; message: string; code?: string };

export const covers = (have: Res, need: Res): boolean => need.every((n, i) => have[i] >= n);

/** Giving this many cards of one resource to other players in one turn locks it out of Monopoly until the turn ends (src/game.rs). */
export const MONOPOLY_LOCK_CARDS = 2;
/** Which harbours a player owns: a 3:1 for anything, and a 2:1 for each resource (`HarbourView` in src/game.rs). */
export interface Harbours {
  any: boolean;
  resource: boolean[];
}
/**
 * Whether handing over `give` for `getTotal` cards from another player is nothing but bank trades that these harbours
 * allow: every resource given is a whole number of 3s (3:1 harbour) or 2s (its own 2:1 harbour), and the groups add up to
 * `getTotal`. Harbour owners must use the bank for those (`bank_rate_deal` in src/game.rs, which this mirrors).
 */
export function bankRateDeal(give: Res, getTotal: number, h: Harbours): boolean {
  if (getTotal <= 0 || !(h.any || h.resource.some(Boolean))) return false;
  const size = Math.floor(total(give) / 2) + 1;
  let reach: boolean[] = new Array<boolean>(size + 1).fill(false);
  reach[0] = true;
  for (let r = 0; r < give.length; r++) {
    const n = give[r];
    if (n === 0) continue;
    const options: number[] = [];
    if (h.resource[r] && n % 2 === 0) options.push(n / 2);
    if (h.any && n % 3 === 0) options.push(n / 3);
    if (options.length === 0) return false; // a resource that doesn't fit a harbour ratio is not a bank deal
    const next: boolean[] = new Array<boolean>(size + 1).fill(false);
    reach.forEach((ok, have) => {
      if (!ok) return;
      for (const add of options) if (have + add <= size) next[have + add] = true;
    });
    reach = next;
  }
  return getTotal <= size && reach[getTotal];
}
export const total = (r: readonly number[]): number => r.reduce((a, b) => a + b, 0);
export const bump = (r: Res, i: number, d: number): Res =>
  r.map((x, j) => (j === i ? x + d : x)) as Res;
