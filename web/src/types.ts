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
}

/** The standard values; the host's starting point and the "Standard" choice for each setting. */
export const DEFAULT_SETTINGS: Settings = {
  points_to_win: 15,
  discard_limit: 7,
  friendly_robber: false,
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
};

/** Friendly Robber protects players with this many visible points or fewer (FRIENDLY_ROBBER_MAX_VP in src/game.rs). */
export const FRIENDLY_ROBBER_MAX_VP = 2;

/** What the running countdown is for. Only `turn` is the standard per-turn clock. */
export type ClockKind = 'setup_settlement' | 'setup_road' | 'roll' | 'discard' | 'robber' | 'steal' | 'turn';
export const CLOCK_LABELS: Record<ClockKind, string> = {
  setup_settlement: 'Settlement',
  setup_road: 'Road',
  roll: 'Roll',
  discard: 'Discard',
  robber: 'Robber',
  steal: 'Rob',
  turn: '',
};
export const CLOCK_TITLES: Record<ClockKind, string> = {
  setup_settlement: 'Time left to place a settlement',
  setup_road: 'Time left to place a road',
  roll: 'Time left to roll (not counted against turn time)',
  discard: 'Time left to discard (not counted against turn time)',
  robber: 'Time left to move the robber (not counted against turn time)',
  steal: 'Time left to choose who to rob (not counted against turn time)',
  turn: 'Time left on this turn',
};
export interface TimerView {
  kind: ClockKind;
  player: number;
  /** Time left when this view was built; the client counts down from the moment it arrived. */
  remaining_ms: number;
  total_ms: number;
}

export type Terrain = 'hills' | 'forest' | 'mountains' | 'fields' | 'pasture' | 'desert';
export type Phase =
  | 'setup'
  | 'roll'
  | 'discard'
  | 'move_robber'
  | 'steal'
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
  vp_total: number | null;
}
export type Reply = 'proposer' | 'pending' | 'accepted' | 'declined';
/** The most offers one player may have open at once (MAX_OPEN_TRADES in src/game.rs). */
export const MAX_OPEN_TRADES = 6;
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
export interface Legal {
  settlements: number[];
  cities: number[];
  roads: number[];
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
  | { type: 'collected'; player: number; cards: Res }
  | { type: 'bank_short'; resource: number }
  | { type: 'must_discard'; limit: number }
  | { type: 'discarded'; player: number; count: number }
  | { type: 'robber_must_move' }
  | { type: 'robber_moved'; player: number }
  | { type: 'stole'; player: number; victim: number }
  | { type: 'placed_settlement'; player: number }
  | { type: 'built_settlement'; player: number }
  | { type: 'built_road'; player: number }
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
  current: number;
  rolled: boolean;
  dice: [number, number] | null;
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
  setup_needs: 'settlement' | 'road' | null;
  discards: [number, number][];
  steal_candidates: number[];
  road_building_left: number;
  /** Offers currently open; only the player whose turn it is can have any. */
  trades: Trade[];
  legal: Legal;
  longest_road_holder: number | null;
  largest_army_holder: number | null;
  winner: number | null;
  log: LogEvent[];
  /** Seat ids in turn order (randomised at the start of the game). */
  order: number[];
  timer_mode: TimerMode;
  timer: TimerView | null;
  settings: Settings;
}

export interface LobbyPlayer {
  name: string;
  color: string;
  connected: boolean;
}

export type ServerMsg =
  | { type: 'welcome'; token: string }
  | { type: 'lobby'; you: number; players: LobbyPlayer[]; timer: TimerMode; settings: Settings; map: string; maps: MapInfo[] }
  | { type: 'game'; view: GameView }
  | { type: 'error'; message: string; code?: string };

export const covers = (have: Res, need: Res): boolean => need.every((n, i) => have[i] >= n);
export const total = (r: readonly number[]): number => r.reduce((a, b) => a + b, 0);
export const bump = (r: Res, i: number, d: number): Res =>
  r.map((x, j) => (j === i ? x + d : x)) as Res;
