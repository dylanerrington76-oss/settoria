// The map editor's data model: what the editor holds while someone builds a map, how it reads a map file,
// and how it writes one. Nothing in here touches React or the page, so it can be tested on its own.
//
// What the editor lets you place (and what the file says about it):
//
//   striped hex   a land hex whose terrain is dealt at random:   { "q": 0, "r": 0 }
//   desert        a land hex that is always a desert:            { "q": 0, "r": 0, "terrain": "desert" }
//   port          a harbour on a coast edge, kind dealt at random: { "q": 0, "r": 0, "side": 4 }
//
// The pools the game deals from (`terrain_pool`, `token_pool`, the harbour `pool`) are worked out from what
// is on the map when the file is written, so they always add up exactly, which is what the server checks.
// A map that was imported keeps its own pools for as long as the number of hexes they are dealt to is the
// same, so opening a map and saving it again changes nothing.
//
// Everything the server checks that can be checked here is checked here as well (`checkMap`, which follows
// src/validate.rs), so a map that is offered for download is one the lobby will accept.
import type { Terrain } from './types';

export type PortKind = 'any' | 'brick' | 'lumber' | 'ore' | 'grain' | 'wool';

/** A land hex. `terrain` null = dealt at random; `number` null = dealt at random (a desert never has one). */
export interface EdHex {
  id: number;
  q: number;
  r: number;
  terrain: Terrain | null;
  number: number | null;
}

/** A harbour on side `side` of the hex at (q, r). `kind` null = dealt at random. */
export interface EdPort {
  id: number;
  q: number;
  r: number;
  side: number;
  kind: PortKind | null;
}

export interface Layout {
  hexes: EdHex[];
  ports: EdPort[];
}

/** What an imported file said that the editor does not show but must give back when the map is saved. */
export interface Imported {
  robberStart: { q: number; r: number } | null;
  constraints: { spread_tokens?: boolean; deserts_avoid_fixed_deserts?: boolean } | null;
  recommendedSettings: Record<string, unknown> | null;
  terrainPool: Partial<Record<Terrain, number>>;
  tokenPool: Record<string, number>;
  portPool: Partial<Record<PortKind, number>>;
}

/** Everything about the map that is not a piece on the board. Not part of undo. */
export interface Meta {
  name: string;
  author: string;
  playersMin: number;
  playersMax: number;
  /** Work the player range out from how much room the map has. */
  playersAuto: boolean;
  /** How many of the striped hexes are dealt as deserts; null = decided automatically. */
  randomDeserts: number | null;
  seaGrid: boolean;
  imported: Imported | null;
}

export const PORT_KINDS: PortKind[] = ['any', 'brick', 'lumber', 'ore', 'grain', 'wool'];
export const TERRAINS: Terrain[] = ['hills', 'forest', 'mountains', 'fields', 'pasture', 'desert'];
/** The order in which leftover hexes go to resources when the pool can't be split evenly (as in Standard). */
const SPARE_ORDER: Terrain[] = ['forest', 'fields', 'pasture', 'hills', 'mountains'];
/** The 18 tokens of the standard board, in the order they are handed out when a map has fewer (or more). */
const TOKEN_ORDER = [6, 5, 9, 4, 10, 8, 3, 11, 5, 9, 6, 4, 10, 8, 3, 11, 2, 12];

// Limits the server enforces (src/map_def.rs).
export const MAX_HEXES = 250;
export const MAX_COORD = 100;
export const MAX_MAP_BYTES = 256 * 1024;
export const MAX_NAME_CHARS = 32;
const DEFAULT_STARTING_SETTLEMENTS = 2;

export const defaultMeta = (): Meta => ({
  name: 'My map',
  author: '',
  playersMin: 2,
  playersMax: 4,
  playersAuto: true,
  randomDeserts: null,
  seaGrid: false,
  imported: null,
});

// ---------------------------------------------------------------------------------------------- geometry

/** Board units per hex radius, the same as the game board uses. */
export const S = 64;
const SQRT3 = Math.sqrt(3);
/** The hex across side i (0 east, 1 south-east, 2 south-west, 3 west, 4 north-west, 5 north-east). */
export const STEPS: ReadonlyArray<readonly [number, number]> = [
  [1, 0],
  [0, 1],
  [-1, 1],
  [-1, 0],
  [0, -1],
  [1, -1],
];
const APOTHEM = (SQRT3 / 2) * S;
/** How far past the coast the ship sits, as in the game (0.85 hex radii off the edge). */
const SHIP_OFFSET = 0.85 * S;

export const cellKey = (q: number, r: number) => `${q},${r}`;
export const inRange = (q: number, r: number) => Math.abs(q) <= MAX_COORD && Math.abs(r) <= MAX_COORD;

export function hexCenter(q: number, r: number) {
  return { x: S * SQRT3 * (q + r / 2), y: S * 1.5 * r };
}

/** The hex whose centre is nearest to a point (pointy-top axial coordinates, cube rounding). */
export function pixelToHex(x: number, y: number): { q: number; r: number } {
  const fq = ((SQRT3 / 3) * x - y / 3) / S;
  const fr = ((2 / 3) * y) / S;
  const fs = -fq - fr;
  let q = Math.round(fq);
  let r = Math.round(fr);
  const s = Math.round(fs);
  const dq = Math.abs(q - fq);
  const dr = Math.abs(r - fr);
  const ds = Math.abs(s - fs);
  if (dq > dr && dq > ds) q = -r - s;
  else if (dr > ds) r = -q - s;
  return { q: q + 0, r: r + 0 }; // "+ 0" turns -0 into 0
}

export function hexDistance(a: { q: number; r: number }, b: { q: number; r: number }) {
  const dq = a.q - b.q;
  const dr = a.r - b.r;
  return (Math.abs(dq) + Math.abs(dr) + Math.abs(dq + dr)) / 2;
}

/** Corner i of a hex centred at (cx, cy): corner 0 is the upper right, going clockwise. */
export function corner(cx: number, cy: number, i: number, radius = S) {
  const a = ((60 * i - 30) * Math.PI) / 180;
  return { x: cx + radius * Math.cos(a), y: cy + radius * Math.sin(a) };
}

/** Where a harbour is drawn: the ship, and the two corners its piers reach. */
export function portLayout(q: number, r: number, side: number) {
  const c = hexCenter(q, r);
  const a = (60 * side * Math.PI) / 180;
  const nx = Math.cos(a);
  const ny = Math.sin(a);
  return {
    sx: c.x + nx * (APOTHEM + SHIP_OFFSET),
    sy: c.y + ny * (APOTHEM + SHIP_OFFSET),
    v1: corner(c.x, c.y, side),
    v2: corner(c.x, c.y, (side + 1) % 6),
  };
}

export const occupied = (hexes: EdHex[]) => new Set(hexes.map((h) => cellKey(h.q, h.r)));

export interface Spot {
  q: number;
  r: number;
  side: number;
}

/** Every coast edge of the map as (hex, side), except ones another harbour (not `ignore`) already has. */
export function freeCoastEdges(layout: Layout, ignorePort: number | null = null): Spot[] {
  const cells = occupied(layout.hexes);
  const taken = new Set(layout.ports.filter((p) => p.id !== ignorePort).map((p) => `${p.q},${p.r},${p.side}`));
  const out: Spot[] = [];
  for (const h of layout.hexes) {
    for (let side = 0; side < 6; side++) {
      const [dq, dr] = STEPS[side];
      if (cells.has(cellKey(h.q + dq, h.r + dr))) continue;
      if (taken.has(`${h.q},${h.r},${side}`)) continue;
      out.push({ q: h.q, r: h.r, side });
    }
  }
  return out;
}

/** The free coast edge whose ship position is nearest to a point, if one is close enough. */
export function nearestCoastEdge(layout: Layout, x: number, y: number, ignorePort: number | null = null, reach = 1.3 * S): Spot | null {
  let best: Spot | null = null;
  let bd = reach;
  for (const e of freeCoastEdges(layout, ignorePort)) {
    const p = portLayout(e.q, e.r, e.side);
    const d = Math.hypot(p.sx - x, p.sy - y);
    if (d < bd) {
      bd = d;
      best = e;
    }
  }
  return best;
}

/** Drop harbours whose hex has gone, whose edge is no longer on the coast, or that share an edge. */
export function normalizePorts(layout: Layout): { layout: Layout; removed: number } {
  const cells = occupied(layout.hexes);
  const seen = new Set<string>();
  const ports = layout.ports.filter((p) => {
    if (!cells.has(cellKey(p.q, p.r))) return false;
    const [dq, dr] = STEPS[p.side];
    if (cells.has(cellKey(p.q + dq, p.r + dr))) return false;
    const k = `${p.q},${p.r},${p.side}`;
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  });
  const removed = layout.ports.length - ports.length;
  return { layout: removed ? { ...layout, ports } : layout, removed };
}

// Corner positions on an integer lattice, so corners shared between hexes get the same key exactly.
// A corner is at hex centre + (DX[i] * sqrt(3)/2, DY[i] / 2); the centre is at ((2q + r) * sqrt(3)/2, 3r / 2).
const DX = [1, 1, 0, -1, -1, 0];
const DY = [-1, 1, 2, 1, -1, -2];

interface Graph {
  /** Per vertex, its neighbours along hex edges. */
  adj: number[][];
  /** For each hex (by index in `hexes`), its six corner ids. */
  corners: number[][];
}

function buildGraph(hexes: { q: number; r: number }[]): Graph {
  const ids = new Map<string, number>();
  const adj: number[][] = [];
  const corners: number[][] = [];
  const link = (a: number, b: number) => {
    if (!adj[a].includes(b)) adj[a].push(b);
    if (!adj[b].includes(a)) adj[b].push(a);
  };
  for (const h of hexes) {
    const ring: number[] = [];
    for (let i = 0; i < 6; i++) {
      const k = `${2 * h.q + h.r + DX[i]},${3 * h.r + DY[i]}`;
      let id = ids.get(k);
      if (id === undefined) {
        id = adj.length;
        ids.set(k, id);
        adj.push([]);
      }
      ring.push(id);
    }
    for (let i = 0; i < 6; i++) link(ring[i], ring[(i + 1) % 6]);
    corners.push(ring);
  }
  return { adj, corners };
}

/**
 * The most settlements that fit under the distance rule (no two on corners joined by an edge). The corners of
 * a hex map are bipartite, so this is exact: all corners less a maximum matching (Konig's theorem), which is
 * how the server counts it too.
 */
export function settlementRoom(hexes: { q: number; r: number }[]): number {
  const { adj } = buildGraph(hexes);
  const n = adj.length;
  const side = new Array<number>(n).fill(-1);
  for (let s = 0; s < n; s++) {
    if (side[s] !== -1) continue;
    side[s] = 0;
    const queue = [s];
    while (queue.length) {
      const v = queue.pop()!;
      for (const w of adj[v]) if (side[w] === -1) {
        side[w] = 1 - side[v];
        queue.push(w);
      }
    }
  }
  const owner = new Array<number>(n).fill(-1);
  const stamp = new Array<number>(n).fill(0);
  const augment = (l: number, round: number): boolean => {
    for (const r of adj[l]) {
      if (stamp[r] === round) continue;
      stamp[r] = round;
      if (owner[r] === -1 || augment(owner[r], round)) {
        owner[r] = l;
        return true;
      }
    }
    return false;
  };
  let matched = 0;
  let round = 0;
  for (let v = 0; v < n; v++) if (side[v] === 0 && augment(v, ++round)) matched++;
  return n - matched;
}

/** The coast, as closed loops of (hex, side) edges, each in order round the loop. One loop per island or lake. */
function coastRings(hexes: EdHex[]): Spot[][] {
  const { adj: _adj, corners } = buildGraph(hexes);
  void _adj;
  const cells = occupied(hexes);
  interface E extends Spot {
    a: number;
    b: number;
  }
  const edges: E[] = [];
  const at = new Map<number, number[]>();
  hexes.forEach((h, hi) => {
    for (let side = 0; side < 6; side++) {
      const [dq, dr] = STEPS[side];
      if (cells.has(cellKey(h.q + dq, h.r + dr))) continue;
      const e: E = { q: h.q, r: h.r, side, a: corners[hi][side], b: corners[hi][(side + 1) % 6] };
      edges.push(e);
      for (const v of [e.a, e.b]) at.set(v, [...(at.get(v) ?? []), edges.length - 1]);
    }
  });
  const seen = new Array<boolean>(edges.length).fill(false);
  const rings: Spot[][] = [];
  for (let s = 0; s < edges.length; s++) {
    if (seen[s]) continue;
    seen[s] = true;
    const ring = [s];
    let here = edges[s].b;
    for (;;) {
      const next = (at.get(here) ?? []).find((e) => !seen[e]);
      if (next === undefined) break;
      seen[next] = true;
      ring.push(next);
      here = edges[next].a === here ? edges[next].b : edges[next].a;
    }
    rings.push(ring.map((i) => ({ q: edges[i].q, r: edges[i].r, side: edges[i].side })));
  }
  return rings;
}

// ----------------------------------------------------------------------------------------------- pools

const sum = (o: Record<string, number>) => Object.values(o).reduce((a, b) => a + b, 0);

/** How many hexes get a random terrain, and how many deserts there can be among them. */
function counts(layout: Layout) {
  const wild = layout.hexes.filter((h) => h.terrain === null);
  const fixedDeserts = layout.hexes.filter((h) => h.terrain === 'desert').length;
  const fixedProducing = layout.hexes.filter((h) => h.terrain !== null && h.terrain !== 'desert');
  /** Hexes a dealt desert may go on: random terrain and no number written on them. */
  const spots = wild.filter((h) => h.number === null).length;
  return {
    wild: wild.length,
    fixedDeserts,
    fixedProducing: fixedProducing.length,
    spots,
    waitingFixed: fixedProducing.filter((h) => h.number === null).length,
    waitingRandom: spots,
  };
}

/** The most deserts the striped hexes can hold while at least one hex still produces something. */
export function maxRandomDeserts(layout: Layout) {
  const c = counts(layout);
  return Math.max(0, Math.min(c.spots, c.fixedProducing > 0 ? c.spots : c.wild - 1));
}

/** One desert for a map without any, a few more for a very big one; none if the player placed their own. */
function autoRandomDeserts(layout: Layout) {
  const c = counts(layout);
  if (c.wild === 0 || c.fixedDeserts > 0) return 0;
  return Math.min(maxRandomDeserts(layout), Math.max(1, Math.round(c.wild / 19)));
}

export interface Pools {
  terrain: Partial<Record<Terrain, number>>;
  tokens: Record<string, number>;
  ports: Partial<Record<PortKind, number>>;
  /** Hexes with a random terrain, producing hexes still needing a token, harbours with a random kind. */
  wildHexes: number;
  tokenHexes: number;
  wildPorts: number;
  deserts: number;
}

function terrainPool(wild: number, deserts: number): Partial<Record<Terrain, number>> {
  const rest = wild - deserts;
  const out: Partial<Record<Terrain, number>> = {};
  const each = Math.floor(rest / 5);
  const spare = rest % 5;
  SPARE_ORDER.forEach((t, i) => {
    out[t] = each + (i < spare ? 1 : 0);
  });
  if (deserts) out.desert = deserts;
  return out;
}

function tokenPool(n: number): Record<string, number> {
  const out: Record<number, number> = {};
  for (let i = 0; i < n; i++) {
    const t = TOKEN_ORDER[i % TOKEN_ORDER.length];
    out[t] = (out[t] ?? 0) + 1;
  }
  return out;
}

function portPool(n: number): Partial<Record<PortKind, number>> {
  const any = Math.ceil((n * 4) / 9);
  const out: Partial<Record<PortKind, number>> = {};
  if (any) out.any = any;
  const res: PortKind[] = ['brick', 'lumber', 'ore', 'grain', 'wool'];
  for (let i = 0; i < n - any; i++) out[res[i % 5]] = (out[res[i % 5]] ?? 0) + 1;
  return out;
}

const compact = <T extends string>(o: Partial<Record<T, number>>): Partial<Record<T, number>> =>
  Object.fromEntries(Object.entries(o).filter(([, v]) => (v as number) > 0)) as Partial<Record<T, number>>;

export function planPools(layout: Layout, meta: Meta): Pools {
  const c = counts(layout);
  const im = meta.imported;
  const keepOwn = meta.randomDeserts === null && im !== null && sum(im.terrainPool) === c.wild;
  let terrain: Partial<Record<Terrain, number>>;
  if (keepOwn) terrain = compact(im!.terrainPool);
  else {
    const want = meta.randomDeserts === null ? autoRandomDeserts(layout) : meta.randomDeserts;
    terrain = terrainPool(c.wild, Math.max(0, Math.min(want, maxRandomDeserts(layout))));
  }
  const deserts = terrain.desert ?? 0;
  const tokenHexes = c.waitingFixed + c.waitingRandom - Math.min(deserts, c.waitingRandom);
  const tokens = im && sum(im.tokenPool) === tokenHexes && (keepOwn || meta.randomDeserts === null) ? { ...im.tokenPool } : tokenPool(tokenHexes);
  const wildPorts = layout.ports.filter((p) => p.kind === null).length;
  const ports = im && sum(im.portPool) === wildPorts ? compact(im.portPool) : portPool(wildPorts);
  return { terrain, tokens, ports, wildHexes: c.wild, tokenHexes, wildPorts, deserts };
}

/** The player range the map is saved with. */
export function resolvePlayers(layout: Layout, meta: Meta): { min: number; max: number } {
  if (!meta.playersAuto) return { min: meta.playersMin, max: meta.playersMax };
  const room = settlementRoom(layout.hexes);
  const per = startingSettlements(meta);
  for (const n of [4, 3, 2]) if (room >= 2 * n * per) return { min: 2, max: n };
  for (const n of [4, 3, 2]) if (room >= n * per) return { min: 2, max: n };
  return { min: 2, max: 2 };
}

function startingSettlements(meta: Meta): number {
  const v = meta.imported?.recommendedSettings?.starting_settlements;
  return typeof v === 'number' && v >= 1 ? v : DEFAULT_STARTING_SETTLEMENTS;
}

// ------------------------------------------------------------------------------------------------ export

type Json = null | boolean | number | string | Json[] | { [k: string]: Json };

/** The map as the object the server reads, with the keys in the order a person would write them. */
export function buildMapFile(layout: Layout, meta: Meta): { [k: string]: Json } {
  const pools = planPools(layout, meta);
  const players = resolvePlayers(layout, meta);
  const file: { [k: string]: Json } = { version: 1, name: meta.name.trim() };
  if (meta.author.trim()) file.author = meta.author.trim();
  file.players = { min: players.min, max: players.max };
  file.hexes = layout.hexes.map((h) => {
    const o: { [k: string]: Json } = { q: h.q, r: h.r };
    if (h.terrain !== null) o.terrain = h.terrain;
    if (h.number !== null && h.terrain !== 'desert') o.number = h.number;
    return o;
  });
  const ordered = <T extends string>(keys: readonly T[], pool: Partial<Record<T, number>>) =>
    Object.fromEntries(keys.filter((k) => (pool[k] ?? 0) > 0).map((k) => [k, pool[k]!]));
  file.terrain_pool = ordered(TERRAINS, pools.terrain);
  file.token_pool = Object.fromEntries(
    Object.keys(pools.tokens)
      .map(Number)
      .sort((a, b) => a - b)
      .filter((k) => pools.tokens[k] > 0)
      .map((k) => [String(k), pools.tokens[k]]),
  );
  file.ports = {
    mode: 'fixed',
    spots: layout.ports.map((p) => {
      const o: { [k: string]: Json } = { q: p.q, r: p.r, side: p.side };
      if (p.kind !== null) o.kind = p.kind;
      return o;
    }),
    pool: ordered(PORT_KINDS, pools.ports),
  };
  const im = meta.imported;
  const here = (q: number, r: number) => layout.hexes.some((h) => h.q === q && h.r === r);
  if (im?.robberStart && here(im.robberStart.q, im.robberStart.r)) {
    file.robber_start = { q: im.robberStart.q, r: im.robberStart.r };
  } else if (layout.hexes.length && counts(layout).fixedDeserts + pools.deserts === 0) {
    // No desert anywhere for the robber to start on, so the file has to say where.
    const mid = centroid(layout.hexes);
    const near = [...layout.hexes].sort((a, b) => hexDistance(a, mid) - hexDistance(b, mid))[0];
    file.robber_start = { q: near.q, r: near.r };
  }
  if (im?.constraints) file.constraints = { ...im.constraints } as Json;
  if (im?.recommendedSettings) file.recommended_settings = im.recommendedSettings as Json;
  if (meta.seaGrid) file.style = { sea_grid: true };
  return file;
}

function centroid(hexes: { q: number; r: number }[]) {
  const n = hexes.length || 1;
  return { q: Math.round(hexes.reduce((a, h) => a + h.q, 0) / n), r: Math.round(hexes.reduce((a, h) => a + h.r, 0) / n) };
}

/** Pretty-print: short objects on one line, one hex or harbour per line, like the files in maps/builtin. */
export function formatMapJson(value: Json, indent = ''): string {
  const inner = indent + '  ';
  if (value === null || typeof value !== 'object') return JSON.stringify(value);
  if (Array.isArray(value)) {
    if (!value.length) return '[]';
    const flat = value.every((v) => v === null || typeof v !== 'object');
    if (flat) return `[${value.map((v) => JSON.stringify(v)).join(', ')}]`;
    return `[\n${value.map((v) => inner + formatMapJson(v, inner)).join(',\n')}\n${indent}]`;
  }
  const entries = Object.entries(value);
  if (!entries.length) return '{}';
  if (entries.every(([, v]) => v === null || typeof v !== 'object')) {
    const line = `{ ${entries.map(([k, v]) => `${JSON.stringify(k)}: ${JSON.stringify(v)}`).join(', ')} }`;
    if (line.length <= 96) return line;
  }
  return `{\n${entries.map(([k, v]) => `${inner}${JSON.stringify(k)}: ${formatMapJson(v, inner)}`).join(',\n')}\n${indent}}`;
}

export const mapFileText = (layout: Layout, meta: Meta) => formatMapJson(buildMapFile(layout, meta)) + '\n';

/** A file name for the download: "My map" -> "my-map.json". */
export function fileNameFor(name: string): string {
  const slug = name
    .trim()
    .toLowerCase()
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
  return `${slug || 'map'}.json`;
}

// ------------------------------------------------------------------------------------------------ checks

export interface Issue {
  level: 'error' | 'warning';
  text: string;
}

const BAD_CHAR = /[\u0000-\u001f\u007f-\u009f​-‏‪-‮⁠-⁤⁦-⁩﻿]/;

function checkLabel(field: string, text: string, issues: Issue[]) {
  const t = text.trim();
  const len = [...t].length;
  if (len > MAX_NAME_CHARS) issues.push({ level: 'error', text: `The ${field} is ${len} characters long, but the most allowed is ${MAX_NAME_CHARS}.` });
  if (BAD_CHAR.test(t)) issues.push({ level: 'error', text: `The ${field} has a line break or an invisible character in it; use plain text.` });
}

const isHot = (n: number) => n === 6 || n === 8;
const clash = (a: number, b: number) => a === b || (isHot(a) && isHot(b));

/** Everything the server would say about this map (errors), and what it would warn about. */
export function checkMap(layout: Layout, meta: Meta): Issue[] {
  const issues: Issue[] = [];
  const { hexes, ports } = layout;
  if (!meta.name.trim()) issues.push({ level: 'error', text: 'The map needs a name.' });
  checkLabel('name', meta.name, issues);
  checkLabel('author', meta.author, issues);
  const players = resolvePlayers(layout, meta);
  if (players.min < 2 || players.max > 4 || players.min > players.max) {
    issues.push({ level: 'error', text: `The player range ${players.min} to ${players.max} is not allowed (it must lie within 2 to 4, with the first at most the second).` });
  }
  if (!hexes.length) {
    issues.push({ level: 'error', text: 'The map has no hexes yet. Drag a striped hex onto the sea.' });
    return issues;
  }
  if (hexes.length > MAX_HEXES) issues.push({ level: 'error', text: `The map has ${hexes.length} hexes, but the most allowed is ${MAX_HEXES}.` });

  const pools = planPools(layout, meta);
  const c = counts(layout);
  const desertTotal = c.fixedDeserts + pools.deserts;
  if (hexes.length - desertTotal <= 0) issues.push({ level: 'error', text: 'Every hex is a desert, so nothing would ever be produced. Add a striped hex.' });

  if ((meta.imported?.constraints?.spread_tokens ?? true) !== false) {
    for (let i = 0; i < hexes.length; i++) {
      for (let j = i + 1; j < hexes.length; j++) {
        const a = hexes[i];
        const b = hexes[j];
        if (a.number !== null && b.number !== null && a.terrain !== 'desert' && b.terrain !== 'desert' && hexDistance(a, b) === 1 && clash(a.number, b.number)) {
          issues.push({ level: 'error', text: `Touching hexes (${a.q}, ${a.r}) and (${b.q}, ${b.r}) have fixed numbers that are not allowed to touch.` });
        }
      }
    }
  }

  const room = settlementRoom(hexes);
  const needed = players.max * startingSettlements(meta);
  if (room < needed) {
    issues.push({
      level: 'error',
      text: `${players.max} players placing ${startingSettlements(meta)} settlements each need room for ${needed}, but under the distance rule the map has room for only ${room}. Add more hexes${meta.playersAuto ? '' : ' or lower the number of players'}.`,
    });
  } else if (room < needed * 2) {
    issues.push({ level: 'warning', text: `The map has room for ${room} settlements and setup needs ${needed}. It fits if everyone places well, but careless placement could run out of room.` });
  }

  // Resources: the hexes written into the map plus what the pool deals.
  const have: Record<string, number> = {};
  for (const h of hexes) if (h.terrain) have[h.terrain] = (have[h.terrain] ?? 0) + 1;
  for (const [t, n] of Object.entries(pools.terrain)) have[t] = (have[t] ?? 0) + (n ?? 0);
  const resources = ['forest', 'hills', 'mountains', 'fields', 'pasture'];
  const missing = resources.filter((t) => !have[t]);
  for (const t of missing) issues.push({ level: 'warning', text: `There are no ${t} hexes, so that resource can only be had by trading with the bank.` });
  if (!missing.length) {
    const scarce = resources.reduce((a, b) => (have[b] < have[a] ? b : a));
    const plenty = resources.reduce((a, b) => (have[b] > have[a] ? b : a));
    if (have[scarce] * 3 < have[plenty]) issues.push({ level: 'warning', text: `${scarce} is scarce (${have[scarce]} hexes) next to ${plenty} (${have[plenty]} hexes).` });
  }

  // An island made only of deserts you placed.
  const index = new Map(hexes.map((h, i) => [cellKey(h.q, h.r), i]));
  const seen = new Set<number>();
  for (let s = 0; s < hexes.length; s++) {
    if (seen.has(s)) continue;
    const island = [s];
    seen.add(s);
    for (let k = 0; k < island.length; k++) {
      const h = hexes[island[k]];
      for (const [dq, dr] of STEPS) {
        const o = index.get(cellKey(h.q + dq, h.r + dr));
        if (o !== undefined && !seen.has(o)) {
          seen.add(o);
          island.push(o);
        }
      }
    }
    if (island.every((i) => hexes[i].terrain === 'desert')) {
      issues.push({ level: 'warning', text: `The island near (${hexes[s].q}, ${hexes[s].r}) has ${island.length} hexes and all of them are desert.` });
    }
  }

  if (!ports.length) issues.push({ level: 'warning', text: 'The map has no ports, so players can only trade 4:1 with the bank.' });
  const graph = buildGraph(hexes);
  const used = new Map<number, number>();
  for (const p of ports) {
    const hi = index.get(cellKey(p.q, p.r));
    if (hi === undefined) continue;
    for (const v of [graph.corners[hi][p.side], graph.corners[hi][(p.side + 1) % 6]]) used.set(v, (used.get(v) ?? 0) + 1);
  }
  if ([...used.values()].some((n) => n >= 2)) issues.push({ level: 'warning', text: 'Two ports meet at a corner, so one settlement there would use both and their pictures overlap.' });
  return issues;
}

// ------------------------------------------------------------------------------------------------ import

export type ParseResult = { ok: true; layout: Layout; meta: Meta; notes: string[] } | { ok: false; errors: string[] };

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const isInt = (v: unknown): v is number => typeof v === 'number' && Number.isInteger(v);

function readPool<T extends string>(raw: unknown, valid: (k: string) => boolean): Partial<Record<T, number>> {
  const out: Partial<Record<T, number>> = {};
  if (!isObj(raw)) return out;
  for (const [k, v] of Object.entries(raw)) if (valid(k) && isInt(v) && v > 0) out[k as T] = v;
  return out;
}

/**
 * Read the text of a map file. Anything that makes the map unusable is an error (and nothing is imported);
 * things that can be fixed by leaving them out (a field the game doesn't know, a harbour that isn't on the
 * coast) are dropped and reported in `notes`, so the saved map is one the game accepts.
 */
export function parseMapJson(text: string, fallbackName = 'My map'): ParseResult {
  if (new TextEncoder().encode(text).length > MAX_MAP_BYTES) return { ok: false, errors: [`The file is bigger than ${MAX_MAP_BYTES / 1024} KB, which is the most a map may be.`] };
  let raw: unknown;
  try {
    raw = JSON.parse(text.replace(/^﻿/, ''));
  } catch (e) {
    return { ok: false, errors: [`This is not a JSON file: ${e instanceof Error ? e.message : String(e)}.`] };
  }
  if (!isObj(raw)) return { ok: false, errors: ['A map file is a JSON object, with "version", "name", "hexes" and so on.'] };
  if (raw.version !== 1) return { ok: false, errors: [`Map format version ${JSON.stringify(raw.version) ?? 'missing'} is not supported (this editor reads version 1).`] };
  if (!Array.isArray(raw.hexes)) return { ok: false, errors: ['The file has no "hexes" list.'] };
  if (raw.hexes.length > MAX_HEXES) return { ok: false, errors: [`The map has ${raw.hexes.length} hexes, but the most allowed is ${MAX_HEXES}.`] };

  const errors: string[] = [];
  const notes: string[] = [];
  const hexes: EdHex[] = [];
  const seen = new Set<string>();
  let id = 1;
  raw.hexes.forEach((h, i) => {
    const where = `Hex ${i + 1}`;
    if (!isObj(h) || !isInt(h.q) || !isInt(h.r)) return errors.push(`${where} needs whole-number "q" and "r".`);
    if (!inRange(h.q, h.r)) return errors.push(`${where} (${h.q}, ${h.r}) is outside the allowed coordinate range of -${MAX_COORD} to ${MAX_COORD}.`);
    if (seen.has(cellKey(h.q, h.r))) return errors.push(`${where} (${h.q}, ${h.r}) is listed more than once.`);
    seen.add(cellKey(h.q, h.r));
    let terrain: Terrain | null = null;
    if (h.terrain !== undefined && h.terrain !== '?') {
      if (typeof h.terrain !== 'string' || !TERRAINS.includes(h.terrain as Terrain)) return errors.push(`${where} (${h.q}, ${h.r}) has an unknown terrain ${JSON.stringify(h.terrain)}.`);
      terrain = h.terrain as Terrain;
    }
    let number: number | null = null;
    if (h.number !== undefined && h.number !== '?') {
      if (!isInt(h.number) || h.number < 2 || h.number > 12 || h.number === 7) return errors.push(`${where} (${h.q}, ${h.r}) has number ${JSON.stringify(h.number)}, but tokens are 2 to 12 except 7.`);
      number = h.number;
    }
    if (terrain === 'desert' && number !== null) {
      notes.push(`The desert at (${h.q}, ${h.r}) had a number on it; the number was dropped.`);
      number = null;
    }
    hexes.push({ id: id++, q: h.q, r: h.r, terrain, number });
  });
  if (!hexes.length && !errors.length) errors.push('The map has no hexes.');

  // name, author, players
  let name = fallbackName;
  if (typeof raw.name === 'string') name = raw.name;
  else notes.push(`The file had no name, so it was called "${fallbackName}".`);
  const author = typeof raw.author === 'string' ? raw.author : '';
  let playersMin = 2;
  let playersMax = 4;
  if (isObj(raw.players) && isInt(raw.players.min) && isInt(raw.players.max)) {
    playersMin = raw.players.min;
    playersMax = raw.players.max;
  }

  // harbours
  const layout: Layout = { hexes, ports: [] };
  const portPool = readPool<PortKind>(isObj(raw.ports) ? raw.ports.pool : undefined, (k) => PORT_KINDS.includes(k as PortKind));
  if (isObj(raw.ports) && !errors.length) {
    let spots: { q: number; r: number; side: number; kind: PortKind | null }[] = [];
    if (raw.ports.mode === 'ring') {
      const spacing = raw.ports.spacing;
      const rings = coastRings(hexes);
      const total = Array.isArray(spacing) ? spacing.reduce((a: number, b: unknown) => a + (isInt(b) ? b : 0), 0) : -1;
      if (Array.isArray(spacing) && spacing.every((s) => isInt(s) && s >= 1) && rings.length === 1 && total === rings[0].length) {
        let pos = 0;
        for (const gap of spacing as number[]) {
          spots.push({ ...rings[0][pos], kind: null });
          pos += gap;
        }
        notes.push('This map placed its ports automatically round the coast. They were turned into fixed ports so you can move them; the game will no longer pick a random starting point.');
      } else {
        notes.push('This map placed its ports automatically round the coast, which the editor could not lay out, so its ports were left out.');
      }
    } else if (Array.isArray(raw.ports.spots)) {
      for (const s of raw.ports.spots) {
        if (!isObj(s) || !isInt(s.q) || !isInt(s.r) || !isInt(s.side)) {
          notes.push('A port without a hex and side was left out.');
          continue;
        }
        let kind: PortKind | null = null;
        if (s.kind !== undefined && s.kind !== '?') {
          if (typeof s.kind !== 'string' || !PORT_KINDS.includes(s.kind as PortKind)) {
            notes.push(`A port of unknown kind ${JSON.stringify(s.kind)} became a random port.`);
          } else kind = s.kind as PortKind;
        }
        spots.push({ q: s.q, r: s.r, side: s.side, kind });
      }
    }
    spots = spots.filter((s) => s.side >= 0 && s.side < 6);
    const imported: Layout = { hexes, ports: spots.map((s, i) => ({ id: 1000 + i, ...s })) };
    const { layout: clean, removed } = normalizePorts(imported);
    if (removed) notes.push(`${removed} port${removed === 1 ? ' was' : 's were'} not on the coast of any hex, or shared an edge, and ${removed === 1 ? 'was' : 'were'} left out.`);
    layout.ports = clean.ports.map((p, i) => ({ ...p, id: i + 1 }));
  }

  const known = new Set(['version', 'name', 'author', 'players', 'hexes', 'terrain_pool', 'token_pool', 'ports', 'robber_start', 'constraints', 'recommended_settings', 'style']);
  const unknown = Object.keys(raw).filter((k) => !known.has(k));
  if (unknown.length) notes.push(`Ignored fields the game doesn't read: ${unknown.join(', ')}.`);

  if (errors.length) return { ok: false, errors: errors.slice(0, 12).concat(errors.length > 12 ? [`…and ${errors.length - 12} more.`] : []) };

  const rs = raw.robber_start;
  const cons = isObj(raw.constraints) ? raw.constraints : null;
  const meta: Meta = {
    name,
    author,
    playersMin,
    playersMax,
    playersAuto: false,
    randomDeserts: null,
    seaGrid: isObj(raw.style) && raw.style.sea_grid === true,
    imported: {
      robberStart: isObj(rs) && isInt(rs.q) && isInt(rs.r) ? { q: rs.q, r: rs.r } : null,
      constraints: cons
        ? {
            ...(typeof cons.spread_tokens === 'boolean' ? { spread_tokens: cons.spread_tokens } : {}),
            ...(typeof cons.deserts_avoid_fixed_deserts === 'boolean' ? { deserts_avoid_fixed_deserts: cons.deserts_avoid_fixed_deserts } : {}),
          }
        : null,
      recommendedSettings: isObj(raw.recommended_settings) ? raw.recommended_settings : null,
      terrainPool: readPool<Terrain>(raw.terrain_pool, (k) => TERRAINS.includes(k as Terrain)),
      tokenPool: readPool<string>(raw.token_pool, (k) => /^\d+$/.test(k) && +k >= 2 && +k <= 12 && +k !== 7) as Record<string, number>,
      portPool,
    },
  };
  return { ok: true, layout, meta, notes };
}
