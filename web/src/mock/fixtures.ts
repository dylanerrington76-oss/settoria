import { DEFAULT_SETTINGS, PALETTE } from '../types';
import type { BoardData, Building, GameView, Hex, Res, Terrain } from '../types';

/** Same construction as src/board.rs, so vertex and edge ids have the real shape. */
const SQ3 = Math.sqrt(3);
const key = (x: number, y: number) => `${Math.round(x * 100)},${Math.round(y * 100)}`;

const LAYOUT: Record<string, [Terrain, number | null]> = {
  '0,-2': ['hills', 4], '1,-2': ['hills', 8], '2,-2': ['mountains', 5],
  '-1,-1': ['desert', null], '0,-1': ['mountains', 3], '1,-1': ['pasture', 10], '2,-1': ['mountains', 2],
  '-2,0': ['fields', 11], '-1,0': ['fields', 6], '0,0': ['fields', 11], '1,0': ['forest', 9], '2,0': ['forest', 6],
  '-2,1': ['forest', 12], '-1,1': ['forest', 5], '0,1': ['pasture', 4], '1,1': ['pasture', 3],
  '-2,2': ['hills', 9], '-1,2': ['fields', 10], '0,2': ['pasture', 8],
};

// Harbour ships as they sit in the reference screenshot (board units), with resource
// index [brick, lumber, ore, grain, wool] or null for 3:1.
const SHIPS: [number, number, number | null][] = [
  [-2.7, -4.77, 2], [0.77, -4.77, 4], [3.375, -3.36, 0], [5.03, -0.28, null], [3.39, 2.73, null],
  [0.77, 4.22, 1], [-2.7, 4.22, null], [-4.42, 1.22, 3], [-4.42, -1.83, null],
];

export function makeBoard(): BoardData {
  const hexes: Hex[] = [];
  const vertices: { x: number; y: number }[] = [];
  const vid = new Map<string, number>();
  const eid = new Map<string, number>();
  const edges: { a: number; b: number }[] = [];
  const edgeCount: number[] = [];
  for (let q = -2; q <= 2; q++) {
    for (let r = -2; r <= 2; r++) {
      if (Math.abs(q + r) > 2) continue;
      const cx = SQ3 * (q + r / 2);
      const cy = 1.5 * r;
      const ids: number[] = [];
      for (let i = 0; i < 6; i++) {
        const a = ((60 * i - 30) * Math.PI) / 180;
        const x = cx + Math.cos(a);
        const y = cy + Math.sin(a);
        const k = key(x, y);
        if (!vid.has(k)) {
          vid.set(k, vertices.length);
          vertices.push({ x, y });
        }
        ids.push(vid.get(k)!);
      }
      for (let i = 0; i < 6; i++) {
        const a = ids[i];
        const b = ids[(i + 1) % 6];
        const k = `${Math.min(a, b)}-${Math.max(a, b)}`;
        if (!eid.has(k)) {
          eid.set(k, edges.length);
          edges.push({ a: Math.min(a, b), b: Math.max(a, b) });
          edgeCount.push(0);
        }
        edgeCount[eid.get(k)!]++;
      }
      const [terrain, number] = LAYOUT[`${q},${r}`];
      hexes.push({ q, r, x: cx, y: cy, terrain, number });
    }
  }
  // harbours: for each ship, the coastal edge whose offshore point is nearest
  const used = new Set<number>();
  const ports = SHIPS.map(([sx, sy, resource]) => {
    let best = -1;
    let bd = Infinity;
    edges.forEach((e, i) => {
      if (edgeCount[i] !== 1 || used.has(i)) return;
      const a = vertices[e.a];
      const b = vertices[e.b];
      const mx = (a.x + b.x) / 2;
      const my = (a.y + b.y) / 2;
      const h = hexes.reduce((p, c) => (Math.hypot(c.x - mx, c.y - my) < Math.hypot(p.x - mx, p.y - my) ? c : p));
      const d0 = Math.hypot(h.x - mx, h.y - my);
      const px = mx + ((mx - h.x) / d0) * 1.42;
      const py = my + ((my - h.y) / d0) * 1.42;
      const d = Math.hypot(px - sx, py - sy);
      if (d < bd) {
        bd = d;
        best = i;
      }
    });
    used.add(best);
    return { a: edges[best].a, b: edges[best].b, resource };
  });
  const desert = hexes.findIndex((h) => h.terrain === 'desert');
  return { map: 'standard', style: { sea_grid: false }, hexes, vertices, edges, ports, desert };
}

const board = makeBoard();
const V = (x: number, y: number) =>
  board.vertices.reduce((best, v, i) => (Math.hypot(v.x - x, v.y - y) < Math.hypot(board.vertices[best].x - x, board.vertices[best].y - y) ? i : best), 0);
const E = (a: [number, number], b: [number, number]) => {
  const [va, vb] = [V(...a), V(...b)];
  return board.edges.findIndex((e) => (e.a === va && e.b === vb) || (e.a === vb && e.b === va));
};
const hexAt = (q: number, r: number) => board.hexes.findIndex((h) => h.q === q && h.r === r);

const NAMES = ['Onida#7249', 'Kieran', 'Dust', 'Claman'];
const RED = 0, BLUE = 1, GREEN = 2, ORANGE = 3;
// seat order in the screenshots: red = you, then blue, green, orange
const SEAT_OF: Record<number, number> = { [RED]: 0, [BLUE]: 1, [GREEN]: 2, [ORANGE]: 3 };

type P = [number, number];
const HOUSES: [number, P][] = [
  [ORANGE, [-0.866, -3.5]], [ORANGE, [-2.598, 0.5]],
  [RED, [0.866, -2.5]], [RED, [-0.866, 0.5]],
  [GREEN, [-1.732, -1.0]], [GREEN, [1.732, 1.0]],
  [BLUE, [-0.866, 2.5]], [BLUE, [0.866, 2.5]],
];
const ROADS: [number, P, P][] = [
  [ORANGE, [-0.866, -3.5], [0, -4]], [ORANGE, [-2.598, 0.5], [-2.598, -0.5]],
  [RED, [0.866, -2.5], [1.732, -2.0]], [RED, [-0.866, 0.5], [0, 1.0]],
  [GREEN, [-1.732, -1.0], [-0.866, -0.5]], [GREEN, [1.732, 1.0], [2.598, 0.5]],
  [BLUE, [-0.866, 2.5], [0, 2.0]], [BLUE, [0.866, 2.5], [1.732, 2.0]],
];
const MORE_HOUSES: [number, P][] = [[BLUE, [2.598, 2.5]]];
const MORE_ROADS: [number, P, P][] = [
  [RED, [1.732, -2.0], [1.732, -1.0]], [RED, [1.732, -1.0], [2.598, -0.5]],
  [ORANGE, [-0.866, -3.5], [-0.866, -2.5]], [BLUE, [1.732, 2.0], [2.598, 2.5]],
];

function pieces(extra: boolean) {
  const buildings: (Building | null)[] = board.vertices.map(() => null);
  const roads: (number | null)[] = board.edges.map(() => null);
  for (const [o, p] of [...HOUSES, ...(extra ? MORE_HOUSES : [])]) buildings[V(...p)] = { owner: SEAT_OF[o], city: false };
  for (const [o, a, b] of [...ROADS, ...(extra ? MORE_ROADS : [])]) roads[E(a, b)] = SEAT_OF[o];
  return { buildings, roads };
}

const pv = (id: number, cards: number, vp: number, road: number, left: [number, number, number], knights = 0) => ({
  id, name: NAMES[id], color: PALETTE[[5, 4, 3, 0][id]].hex, connected: true, cards, dev_cards: 0, knights,
  roads_left: left[0], settlements_left: left[1], cities_left: left[2], vp, longest_road: road, vp_total: null, vp_breakdown: null,
});

function base(): GameView {
  return {
    you: 0, board, phase: 'setup', current: 0, rolled: false, dice: null, roll_counts: Array(11).fill(0), robber: board.desert,
    buildings: board.vertices.map(() => null), roads: board.edges.map(() => null),
    players: [pv(0, 0, 0, 0, [15, 5, 4]), pv(1, 0, 0, 0, [15, 5, 4]), pv(2, 0, 0, 0, [15, 5, 4]), pv(3, 0, 0, 0, [15, 5, 4])],
    hand: [0, 0, 0, 0, 0], dev: [0, 0, 0, 0, 0], dev_new: [0, 0, 0, 0, 0], vp_you: 0,
    bank: [19, 19, 19, 19, 19], deck_left: 25, dev_played: false, ratios: [4, 4, 4, 4, 4],
    setup_needs: 'settlement', discards: [], steal_candidates: [], road_building_left: 0, trades: [],
    legal: { settlements: [], cities: [], roads: [], bridges: [], hexes: [] },
    longest_road_holder: null, largest_army_holder: null, winner: null, log: [],
    order: [0, 1, 2, 3], timer_mode: 'normal', settings: DEFAULT_SETTINGS,
    timer: { kind: 'setup_settlement', player: 0, remaining_ms: 94_000, total_ms: 120_000 },
  };
}

export function fixture(name: string): { view: GameView; clock?: number } {
  const v = base();
  if (name === 'setup') {
    v.legal.settlements = board.vertices.map((_, i) => i);
    v.log = [{ type: 'game_started' }];
    return { view: v, clock: 94 };
  }
  // shared mid-game bits
  const first = pieces(name === 'discard');
  v.buildings = first.buildings;
  v.roads = first.roads;
  v.setup_needs = null;
  if (name === 'roll') {
    v.phase = 'roll';
    v.hand = [0, 1, 0, 2, 0];
    v.vp_you = 2;
    v.bank = [19, 17, 18, 13, 17];
    v.players = [pv(0, 3, 2, 1, [13, 3, 4]), pv(1, 3, 2, 1, [13, 3, 4]), pv(2, 2, 2, 1, [13, 3, 4]), pv(3, 3, 2, 1, [13, 3, 4])];
    v.players[0].roads_left = 13;
    v.log = [
      { type: 'collected', player: 1, cards: [0, 0, 0, 1, 2] }, { type: 'built_road', player: 1 },
      { type: 'placed_settlement', player: 0 }, { type: 'collected', player: 0, cards: [0, 1, 0, 2, 0] },
      { type: 'built_road', player: 0 },
    ];
    v.timer = { kind: 'roll', player: 0, remaining_ms: 7_000, total_ms: 10_000 };
    return { view: v, clock: 7 };
  }
  if (name === 'discard') {
    v.phase = 'discard';
    v.rolled = true;
    v.dice = [3, 4];
    v.robber = hexAt(-1, 0);
    v.hand = [1, 3, 3, 4, 2];
    v.vp_you = 2;
    v.bank = [17, 15, 15, 9, 12];
    v.discards = [[0, 6]];
    v.players = [pv(0, 13, 2, 3, [11, 3, 4]), pv(1, 5, 3, 2, [13, 3, 4]), pv(2, 6, 2, 1, [14, 3, 4]), pv(3, 3, 2, 2, [13, 3, 4])];
    v.log = [
      { type: 'rolled', player: 2, dice: [4, 1] }, { type: 'rolled', player: 3, dice: [2, 3] },
      { type: 'collected', player: 0, cards: [0, 1, 1, 0, 0] }, { type: 'collected', player: 1, cards: [0, 1, 0, 0, 0] },
      { type: 'trade_proposed', player: 3 }, { type: 'rolled', player: 0, dice: [3, 4] },
    ];
    v.timer = { kind: 'discard', player: 0, remaining_ms: 14_000, total_ms: 20_000 };
    return { view: v, clock: 14 };
  }
  if (name === 'main' || name === 'steal') {
    v.phase = name === 'steal' ? 'steal' : 'main';
    v.rolled = true;
    v.dice = [6, 4];
    v.hand = [3, 4, 1, 2, 2];
    v.dev = [1, 0, 0, 0, 1];
    v.dev_new = [0, 1, 0, 0, 0];
    v.vp_you = 3;
    v.legal.roads = [E([-0.866, -3.5], [-0.866, -2.5])];
    v.legal.settlements = [V(0, -4), V(0, 1)];
    v.steal_candidates = [1, 3];
    v.players = [pv(0, 12, 2, 3, [11, 3, 4], 1), pv(1, 5, 3, 2, [13, 3, 4]), pv(2, 6, 2, 1, [14, 3, 4]), pv(3, 3, 2, 2, [13, 3, 4], 2)];
    v.longest_road_holder = null;
    v.largest_army_holder = 3;
    v.log = [
      { type: 'bought_dev_card', player: 3 }, { type: 'rolled', player: 0, dice: [6, 4] },
      { type: 'collected', player: 0, cards: [2, 0, 0, 1, 0] }, { type: 'built_city', player: 2 },
    ];
    v.trades = [
      { id: 0, from: 2, give: [0, 0, 0, 0, 1] as Res, want: [0, 1, 0, 0, 0] as Res, replies: ['pending', 'pending', 'proposer', 'pending'] },
      { id: 1, from: 2, give: [0, 0, 1, 0, 0] as Res, want: [2, 0, 0, 0, 0] as Res, replies: ['pending', 'declined', 'proposer', 'pending'] },
    ];
    v.timer =
      name === 'steal'
        ? { kind: 'steal', player: 0, remaining_ms: 11_000, total_ms: 15_000 }
        : { kind: 'turn', player: 0, remaining_ms: 83_000, total_ms: 120_000 };
    return { view: v, clock: name === 'steal' ? 11 : 83 };
  }
  if (name === 'over') {
    // A finished game: the end screen with the dice-roll chart (?s=over).
    v.phase = 'game_over';
    v.rolled = true;
    v.dice = [4, 3];
    v.winner = 0;
    v.roll_counts = [2, 5, 8, 11, 14, 17, 15, 12, 9, 4, 3];
    const parts: [number, number, number, number, number][] = [
      [3, 4, 1, 0, 2], [2, 6, 0, 0, 0], [3, 4, 0, 0, 0], [2, 2, 0, 2, 0], // settlements, cities, dev cards, army, road
    ];
    v.players = [pv(0, 5, 10, 4, [9, 1, 2], 3), pv(1, 5, 8, 2, [11, 2, 3]), pv(2, 6, 7, 1, [12, 2, 3]), pv(3, 3, 6, 2, [12, 3, 3])].map((p, i) => {
      const [settlements, cities, dev_cards, largest_army, longest_road] = parts[i];
      return { ...p, vp_total: p.vp, vp_breakdown: { settlements, cities, dev_cards, largest_army, longest_road } };
    });
    return { view: v };
  }
  throw new Error('unknown fixture ' + name);
}
