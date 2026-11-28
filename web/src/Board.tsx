import { memo, useEffect, useMemo, useRef, useState } from 'react';
import type { MutableRefObject } from 'react';
import { Glyph, PieceShadow, PieceShape, PortLabel, SHADOW_DY, SHADOW_OPACITY, isVeryDark, pieceEdge, shade } from './art';
import { DesertArt, DesertDefs, DesertRobberShade } from './DesertArt';
import { useMapZoom } from './mapZoom';
import type { MapZoomApi, ZoomEdge } from './mapZoom';
import { GameView, Terrain, Vertex } from './types';

const S = 64; // pixels per hex radius (board units are multiples of this)

interface Style {
  center: string;
  edge: string;
  border: string;
  hi: string;
}
export const STYLE: Record<Terrain, Style> = {
  hills: { center: '#f07d38', edge: '#dd5924', border: '#c43e25', hi: '#f6a064' },
  mountains: { center: '#bcc0bd', edge: '#9a9f9b', border: '#80868a', hi: '#d6dad7' },
  pasture: { center: '#a9d02c', edge: '#86b50f', border: '#6c9610', hi: '#d6e440' },
  fields: { center: '#ffd75f', edge: '#f0b412', border: '#bb850b', hi: '#ffe58f' },
  forest: { center: '#17a047', edge: '#08893c', border: '#066a2d', hi: '#69d252' },
  desert: { center: '#F5EDDC', edge: '#E9DAC0', border: '#C2AB84', hi: '#FBF6EA' },
  volcano: { center: '#6b5b55', edge: '#4a3c38', border: '#2f2421', hi: '#9a7f74' },
  gold: { center: '#d9a63c', edge: '#c48a2e', border: '#b46f22', hi: '#f2c747' },
};
export const RES_OF: Record<Terrain, number | null> = { hills: 0, forest: 1, mountains: 2, fields: 3, pasture: 4, desert: null, volcano: null, gold: null };
export const ART: Record<Terrain, { fill: string; fill2: string; stroke?: string }> = {
  hills: { fill: '#f9d3c4', fill2: '#f0b3a0', stroke: '#e48b72' },
  mountains: { fill: '#f6f8f7', fill2: '#dfe4e2', stroke: '#a7aeab' },
  pasture: { fill: '#ffffff', fill2: '#ffffff', stroke: '#c9d79a' },
  fields: { fill: '#cf9021', fill2: '#e8b13a', stroke: '#a26d0c' },
  forest: { fill: '#3aa24a', fill2: '#58bd5c', stroke: '#176d2b' },
  desert: { fill: '#000', fill2: '#000' },
  volcano: { fill: '#000', fill2: '#000' },
  gold: { fill: '#000', fill2: '#000' },
};

/** Where corner `i` (0 to 5, clockwise from the upper right) of a hex of radius `r` is. The server numbers a volcano's corners `i + 1`. */
export function hexCorner(cx: number, cy: number, r: number, i: number): [number, number] {
  const a = ((60 * i - 30) * Math.PI) / 180;
  return [cx + r * Math.cos(a), cy + r * Math.sin(a)];
}

/**
 * The volcano drawn on a hex: a cone with a glowing crater and lava runs, then the numbers 1 to 6 on the
 * corners the lava can flow down to (corner `i` of the hex is number `i + 1`). Centred on the hex at (cx, cy),
 * at the board's scale (`S` per hex radius). `lift` adds a flat, translucent
 * shadow copy of the cone underneath it.
 */
export function VolcanoArt({ cx, cy, cone = true, numbers = true, lift = true, scale = S }: { cx: number; cy: number; cone?: boolean; numbers?: boolean; lift?: boolean; scale?: number }) {
  const u = scale;
  const top = cy - 0.43 * u;
  const base = cy + 0.02 * u;
  const r = (v: number) => v.toFixed(1);
  const coneD = `M${r(cx - 0.56 * u)},${r(base)} L${r(cx - 0.17 * u)},${r(top)} L${r(cx + 0.17 * u)},${r(top)} L${r(cx + 0.56 * u)},${r(base)} Z`;
  return (
    <g>
      {cone && (
        <>
      {lift && <path d={coneD} transform={`translate(0 ${SHADOW_DY})`} fill="#000" fillOpacity={SHADOW_OPACITY} />}
      <path d={coneD} fill="#7a665f" stroke="#2f2421" strokeWidth="1.6" strokeLinejoin="round" />
      <path d={`M${r(cx + 0.17 * u)},${r(top)} L${r(cx + 0.56 * u)},${r(base)} L${r(cx + 0.1 * u)},${r(base)} Z`} fill="#5a4943" opacity=".7" />
      <ellipse cx={cx} cy={top} rx={0.17 * u} ry={0.06 * u} fill="#ffb02e" stroke="#2f2421" strokeWidth="1.4" />
      <ellipse cx={cx} cy={top - 0.004 * u} rx={0.1 * u} ry={0.03 * u} fill="#fff2a8" />
      <path d={`M${r(cx - 0.05 * u)},${r(top + 0.04 * u)} Q${r(cx - 0.1 * u)},${r(cy - 0.3 * u)} ${r(cx - 0.03 * u)},${r(cy - 0.22 * u)} Q${r(cx - 0.1 * u)},${r(cy - 0.1 * u)} ${r(cx - 0.17 * u)},${r(base)}`} fill="none" stroke="#ff6a1a" strokeWidth="3.2" strokeLinecap="round" />
      <path d={`M${r(cx + 0.06 * u)},${r(top + 0.04 * u)} Q${r(cx + 0.12 * u)},${r(cy - 0.27 * u)} ${r(cx + 0.08 * u)},${r(cy - 0.18 * u)} Q${r(cx + 0.14 * u)},${r(cy - 0.08 * u)} ${r(cx + 0.26 * u)},${r(base)}`} fill="none" stroke="#ff9d2e" strokeWidth="2.4" strokeLinecap="round" />
        </>
      )}
      {numbers &&
        [0, 1, 2, 3, 4, 5].map((i) => {
          const [px, py] = hexCorner(cx, cy, u * 0.64, i);
          return (
            <g key={i} className="volcano-corner">
              <circle cx={px} cy={py} r={0.125 * u} fill="#2f2421" stroke="#ff9d2e" strokeWidth="1.5" />
              <text x={px} y={py} dy="0.36em" textAnchor="middle" fontSize={0.18 * u} fontWeight="800" fill="#ffe9c2">
                {i + 1}
              </text>
            </g>
          );
        })}
    </g>
  );
}

/** One gold bar, centred on (0, 0) and 28 wide: a pale top face over an orange front, with a bright edge between. */
function GoldBar({ x, y, rot = 0, k = 1, shadow = false }: { x: number; y: number; rot?: number; k?: number; shadow?: boolean }) {
  if (shadow) {
    // just the outline of the two faces, in black: the bar's shadow
    return (
      <g transform={`translate(${x} ${y}) rotate(${rot}) scale(${k})`}>
        <path d="M-14,1 L14,1 L15.5,9 L-15.5,9 Z M-9.5,-8 L9.5,-8 L14,1 L-14,1 Z" fill="#000" />
      </g>
    );
  }
  return (
    <g transform={`translate(${x} ${y}) rotate(${rot}) scale(${k})`} strokeLinejoin="round" strokeLinecap="round">
      <path d="M-14,1 L14,1 L15.5,9 L-15.5,9 Z" fill="#f09a1a" stroke="#b5681a" strokeWidth="1.5" />
      <path d="M-9.5,-8 L9.5,-8 L14,1 L-14,1 Z" fill="#ffd731" stroke="#b5681a" strokeWidth="1.5" />
      <path d="M-7,-6 L7,-6 L9,-2 L-9,-2 Z" fill="#fff3a6" stroke="none" opacity=".85" />
      <path d="M-14,1 L14,1" stroke="#ffe27a" strokeWidth="1.4" fill="none" />
      <path d="M-13.5,5 L13.5,5" stroke="#ffb52e" strokeWidth="1.3" fill="none" opacity=".8" />
    </g>
  );
}

/**
 * The gold field drawn on a hex: a small pile of gold bars above the number token, with a few orange pebbles
 * either side. Centred on the hex at (cx, cy), at the board's scale (`S` per hex radius). `lift` adds a flat,
 * translucent shadow copy of the bars underneath them.
 */
export function GoldArt({ cx, cy, lift = true, scale = S }: { cx: number; cy: number; lift?: boolean; scale?: number }) {
  return (
    <g transform={`translate(${cx.toFixed(1)} ${cy.toFixed(1)}) scale(${(scale / S).toFixed(4)})`}>
      <g fill="#eaa55a" opacity=".92">
        <ellipse cx={-36} cy={-21} rx="6.5" ry="3.6" />
        <ellipse cx={-31} cy={-16.5} rx="3.8" ry="2.4" />
        <ellipse cx={-41} cy={-17.5} rx="3" ry="2" />
        <ellipse cx={36} cy={-19} rx="6.5" ry="3.6" />
        <ellipse cx={41} cy={-14.5} rx="3.8" ry="2.4" />
        <ellipse cx={31} cy={-14} rx="3" ry="2" />
      </g>
      {lift && (
        <g transform={`translate(0 ${SHADOW_DY})`} fillOpacity={SHADOW_OPACITY}>
          <GoldBar x={-15} y={-22} rot={-7} k={0.84} shadow />
          <GoldBar x={11} y={-26} rot={-30} k={0.96} shadow />
          <GoldBar x={-3} y={-14} rot={-2} k={0.84} shadow />
        </g>
      )}
      <g>
        <GoldBar x={-15} y={-22} rot={-7} k={0.84} />
        <GoldBar x={11} y={-26} rot={-30} k={0.96} />
        <GoldBar x={-3} y={-14} rot={-2} k={0.84} />
      </g>
    </g>
  );
}

export function hexPoints(cx: number, cy: number, r: number): string {
  return [0, 1, 2, 3, 4, 5]
    .map((i) => {
      const a = ((60 * i - 30) * Math.PI) / 180;
      return `${(cx + r * Math.cos(a)).toFixed(1)},${(cy + r * Math.sin(a)).toFixed(1)}`;
    })
    .join(' ');
}

/** Width of a road across, outline included, in board units (`S` is one hex radius). */
const ROAD_W = 11;
/** Thickness of the dark outline that runs round every road piece. */
const ROAD_LINE = 1.4;

interface RoadShape {
  edge: number;
  owner: number;
  /** Corner points of the road, already cut to meet its neighbours. */
  pts: [number, number][];
  /** Start and end of the line across the road that the colour gradient follows. */
  across: [number, number, number, number];
}

/**
 * The outline of every road on the board. A road runs the whole way from vertex to vertex, and where
 * several of one player's roads meet their ends are cut along the line between them (a mitre), so
 * a chain reads as one continuous road with just a thin seam at each joint, as in the board game.
 */
function roadShapes(board: BoardData, roads: GameView['roads']): RoadShape[] {
  const h = (ROAD_W - ROAD_LINE) / 2; // the outline is stroked on the edge of the polygon
  const TAU = Math.PI * 2;
  // per (vertex, owner): the direction in which each of that owner's roads leaves the vertex
  const ends = new Map<string, { edge: number; vertex: number; ang: number }[]>();
  const add = (vertex: number, owner: number, edge: number, ang: number) => {
    const k = `${vertex}|${owner}`;
    const list = ends.get(k) ?? [];
    list.push({ edge, vertex, ang: ((ang % TAU) + TAU) % TAU });
    ends.set(k, list);
  };
  board.edges.forEach((e, i) => {
    const owner = roads[i];
    if (owner === null || owner === undefined) return;
    const a = board.vertices[e.a];
    const b = board.vertices[e.b];
    const ang = Math.atan2(b.y - a.y, b.x - a.x);
    add(e.a, owner, i, ang);
    add(e.b, owner, i, ang + Math.PI);
  });

  // the two corners of each road end: on its "turning-up" side (asc) and its other side (desc)
  const corner = new Map<string, { asc: [number, number]; desc: [number, number] }>();
  for (const list of ends.values()) {
    list.sort((p, q) => p.ang - q.ang);
    const n = list.length;
    list.forEach((it, j) => {
      const v = board.vertices[it.vertex];
      const vx = v.x * S;
      const vy = v.y * S;
      // a corner sits on the bisector of the gap to the neighbouring road, h / sin(half the gap) from the vertex
      const at = (gap: number, dir: number): [number, number] => {
        const d = h / Math.max(0.5, Math.abs(Math.sin(gap / 2)));
        return [vx + Math.cos(dir) * d, vy + Math.sin(dir) * d];
      };
      let asc: [number, number];
      let desc: [number, number];
      if (n === 1) {
        asc = [vx - Math.sin(it.ang) * h, vy + Math.cos(it.ang) * h];
        desc = [vx + Math.sin(it.ang) * h, vy - Math.cos(it.ang) * h];
      } else {
        const next = list[(j + 1) % n];
        const prev = list[(j + n - 1) % n];
        const gUp = (next.ang - it.ang + TAU) % TAU;
        const gDown = (it.ang - prev.ang + TAU) % TAU;
        asc = at(gUp, it.ang + gUp / 2);
        desc = at(gDown, it.ang - gDown / 2);
      }
      corner.set(`${it.edge}|${it.vertex}`, { asc, desc });
    });
  }

  const out: RoadShape[] = [];
  board.edges.forEach((e, i) => {
    const owner = roads[i];
    if (owner === null || owner === undefined) return;
    const A = corner.get(`${i}|${e.a}`);
    const B = corner.get(`${i}|${e.b}`);
    if (!A || !B) return;
    // seen from end A the asc side is the same physical side of the road as B's desc side
    const mid = (p: [number, number], q: [number, number]): [number, number] => [(p[0] + q[0]) / 2, (p[1] + q[1]) / 2];
    const lo = mid(A.desc, B.asc);
    const hi = mid(A.asc, B.desc);
    out.push({ edge: i, owner, pts: [A.asc, B.desc, B.asc, A.desc], across: [lo[0], lo[1], hi[0], hi[1]] });
  });
  return out;
}

interface Props {
  view: GameView;
  vertexTargets: number[];
  edgeTargets: number[];
  hexTargets: number[];
  showPips?: boolean;
  onVertex: (v: number) => void;
  onEdge: (e: number) => void;
  onHex: (h: number) => void;
  /** Filled in with `zoomIn`, `zoomOut` and `reset` for the + and − buttons, which live outside the board. */
  zoomApi?: MutableRefObject<MapZoomApi | null>;
  /** Told when the zoom reaches its smallest or largest, or leaves them, so those buttons can grey out. */
  onZoomEdge?: (edge: ZoomEdge) => void;
}

/** One harbour: the ship's position and the two vertices its piers reach. */
function portLayout(board: GameView['board'], p: GameView['board']['ports'][number]) {
  const a = board.vertices[p.a];
  const b = board.vertices[p.b];
  const mx = (a.x + b.x) / 2;
  const my = (a.y + b.y) / 2;
  // the edge belongs to exactly one hex: the normal points away from that hex's centre
  let best = board.hexes[0];
  let bd = Infinity;
  for (const h of board.hexes) {
    const d = Math.hypot(h.x - mx, h.y - my);
    if (d < bd) {
      bd = d;
      best = h;
    }
  }
  const nx = (mx - best.x) / (bd || 1);
  const ny = (my - best.y) / (bd || 1);
  // the hull is anchored 0.85 units off the coast; the sail sits above it
  return { a, b, sx: (mx + nx * 0.85) * S, sy: (my + ny * 0.85) * S };
}

type BoardData = GameView['board'];

/**
 * Whether two boards are the same board. The server re-sends the whole board in every message and
 * `JSON.parse` makes a fresh object each time, so identity can't be used. Hexes (position, terrain,
 * token) and harbours are all there is to a board: vertices and edges are derived from the hex
 * positions, so comparing these is enough.
 */
function sameBoard(a: BoardData, b: BoardData): boolean {
  if (a === b) return true;
  if (a.map !== b.map || a.style.sea_grid !== b.style.sea_grid || a.hexes.length !== b.hexes.length || a.ports.length !== b.ports.length) return false;
  for (let i = 0; i < a.hexes.length; i++) {
    const x = a.hexes[i];
    const y = b.hexes[i];
    if (x.q !== y.q || x.r !== y.r || x.terrain !== y.terrain || x.number !== y.number) return false;
  }
  for (let i = 0; i < a.ports.length; i++) {
    const x = a.ports[i];
    const y = b.ports[i];
    if (x.a !== y.a || x.b !== y.b || x.resource !== y.resource) return false;
  }
  return true;
}

/** Keeps handing back the first copy of a board for as long as later messages carry an identical one. */
function useStableBoard(next: BoardData): BoardData {
  const ref = useRef(next);
  if (ref.current !== next && !sameBoard(ref.current, next)) ref.current = next;
  return ref.current;
}

/** How the coast wobbles: the noise's wavelength is 1 / `WAVE_FREQ` board units, a point moves by at most `WAVE_AMP`, and a hex side is cut in `WAVE_STEPS` pieces so the wobble is followed. */
const WAVE_FREQ = 0.035;
const WAVE_AMP = 4.5;
const WAVE_STEPS = 8;

/** A pseudo-random number in [0, 1) for a lattice point; the same point always gives the same number. */
function lattice(ix: number, iy: number, seed: number): number {
  let h = Math.imul(ix, 374761393) + Math.imul(iy, 668265263) + Math.imul(seed, 1442695041);
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  h ^= h >>> 16;
  return (h >>> 0) / 4294967296;
}

/** Smooth 2D value noise in [0, 1]. */
function valueNoise(x: number, y: number, seed: number): number {
  const ix = Math.floor(x);
  const iy = Math.floor(y);
  const fx = x - ix;
  const fy = y - iy;
  const u = fx * fx * (3 - 2 * fx);
  const v = fy * fy * (3 - 2 * fy);
  const a = lattice(ix, iy, seed);
  const b = lattice(ix + 1, iy, seed);
  const c = lattice(ix, iy + 1, seed);
  const d = lattice(ix + 1, iy + 1, seed);
  return a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v;
}

/** Two octaves of noise, as the old `feTurbulence` filter had, in [-1, 1]. */
const fractal = (x: number, y: number, seed: number): number =>
  ((valueNoise(x * WAVE_FREQ, y * WAVE_FREQ, seed) + 0.5 * valueNoise(x * WAVE_FREQ * 2, y * WAVE_FREQ * 2, seed + 101)) / 1.5 - 0.5) * 2;

/**
 * The wavy coast, worked out once: every hex side that has a hex on one side only (a side between two hexes is inland, and
 * is hidden under the beach), cut into short pieces whose points are each moved by smooth noise that depends only on where
 * the point is. So two sides meeting at a corner move that corner together and the line stays whole. The two shallow-water
 * layers are strokes along this one path, with no filter to run when the board is drawn.
 */
function wavyCoast(board: BoardData): string {
  const sides = new Map<string, { n: number; a: [number, number]; b: [number, number] }>();
  const k = (p: [number, number]) => `${Math.round(p[0] * 10)},${Math.round(p[1] * 10)}`;
  for (const hx of board.hexes) {
    const c = [0, 1, 2, 3, 4, 5].map((i) => hexCorner(hx.x * S, hx.y * S, S, i));
    for (let i = 0; i < 6; i++) {
      const a = c[i];
      const b = c[(i + 1) % 6];
      const key = k(a) < k(b) ? `${k(a)}|${k(b)}` : `${k(b)}|${k(a)}`;
      const side = sides.get(key);
      if (side) side.n++;
      else sides.set(key, { n: 1, a, b });
    }
  }
  const at = (x: number, y: number) => `${(x + fractal(x, y, 7) * WAVE_AMP).toFixed(1)},${(y + fractal(x, y, 23) * WAVE_AMP).toFixed(1)}`;
  let d = '';
  for (const { n, a, b } of sides.values()) {
    if (n > 1) continue;
    for (let j = 0; j <= WAVE_STEPS; j++) {
      const t = j / WAVE_STEPS;
      d += `${j === 0 ? 'M' : 'L'}${at(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)}`;
    }
  }
  return d;
}

/** Everything derived from the board alone, so it is worked out once per game rather than per message. */
function computeGeometry(board: BoardData) {
  const ports = board.ports.map((p) => ({ p, ...portLayout(board, p) }));

  // The box is fitted to what is there, for every map. (A map centred on x = 0, like Standard, gets a
  // box that is symmetrical about it; one that isn't, like Earth, gets one that hugs its content.)
  const xs = [...board.vertices.map((v) => v.x * S), ...ports.map((p) => p.sx)];
  const ys = [...board.vertices.map((v) => v.y * S), ...ports.map((p) => p.sy)];
  const x0 = Math.min(...xs) - 34;
  const y0 = Math.min(...ys, ...ports.map((p) => p.sy - 56)) - 41;
  const h = Math.max(...ys, ...ports.map((p) => p.sy + 12)) + 47 - y0;
  const box = { x0, y0, w: Math.max(...xs) + 34 - x0, h };

  const land = board.hexes.map((hx) => hexPoints(hx.x * S, hx.y * S, S * 1.0));
  // Every hex outline as one path (one closed sub-path per hex). The four shoreline layers are each a single solid
  // colour, so one <path> per layer looks exactly like one <polygon> per hex per layer, with far fewer elements.
  const shore = land.map((pts) => `M${pts.split(' ').join('L')}Z`).join('');
  // The wavy coast the two shallow-water layers follow, and the shadow under every tile's border: one path each.
  const wave = wavyCoast(board);
  const tileShadow = board.hexes.map((hx) => `M${hexPoints(hx.x * S, hx.y * S + SHADOW_DY, S * 0.935).split(' ').join('L')}Z`).join('');

  // A faint hex grid across the open sea, if the map's file asks for one (`style.sea_grid`).
  const sea: string[] = [];
  if (board.style.sea_grid) {
    const w = Math.sqrt(3) * S;
    const r0 = Math.floor(box.y0 / (1.5 * S)) - 1;
    const r1 = Math.ceil((box.y0 + box.h) / (1.5 * S)) + 1;
    for (let r = r0; r <= r1; r++) {
      const q0 = Math.floor(box.x0 / w - r / 2) - 1;
      const q1 = Math.ceil((box.x0 + box.w) / w - r / 2) + 1;
      for (let q = q0; q <= q1; q++) sea.push(hexPoints(w * (q + r / 2), 1.5 * S * r, S));
    }
  }
  return { ports, box, shore, wave, tileShadow, sea };
}
type Geometry = ReturnType<typeof computeGeometry>;

/** Radius of a vertex click target in board units; it is enlarged when the board is drawn so small that this would be under `MIN_TARGET_PX` on screen. */
const TARGET_R = 19;
const MIN_TARGET_PX = 12;
/** Vertices are one hex radius (`S`) apart, so a target may not grow so far that it covers its neighbour's. */
const MAX_TARGET_GROW = 1.5;

/**
 * How big a hex really is on screen: CSS pixels per board unit, i.e. how far the board's box is scaled down to
 * fit the space it is given. Follows the size of the board's host `<div>`, so it also changes with the window.
 * Until the first measurement (and where there is no `ResizeObserver`) it says 1. This is the scale of the whole
 * board; zooming in is on top of it. `size` is the host's size in pixels (null until measured).
 */
function useScreenScale(box: { w: number; h: number }) {
  const ref = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState<{ w: number; h: number } | null>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const read = () => {
      const r = el.getBoundingClientRect();
      const w = Math.round(r.width);
      const h = Math.round(r.height);
      setSize((prev) => (prev && prev.w === w && prev.h === h ? prev : { w, h }));
    };
    read();
    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(read);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  // `preserveAspectRatio="meet"`: the tighter of the two fits decides the scale
  const scale = size && size.w > 0 && size.h > 0 ? Math.min(size.w / box.w, size.h / box.h) : 1;
  return { ref, scale, size };
}

/** By how much to enlarge the click targets so they stay easy to hit when the board is drawn small (1 = not at all). */
const targetGrow = (scale: number): number => Math.min(MAX_TARGET_GROW, Math.max(1, MIN_TARGET_PX / (TARGET_R * scale)));

/**
 * The part of the board that never changes during a game: sea, shoreline, harbours, tiles and
 * number tokens (about 90% of the board's elements). It is memoized, so a new game message
 * does not re-render it; it only redraws when the board itself or the pips setting changes.
 * It is drawn in an `<svg>` of its own (see `Board`) that the browser keeps as a GPU layer, so what it
 * references (gradients, filters) is defined here; the pieces drawn over it have their own `<defs>`.
 */
const BoardStatic = memo(function BoardStatic({ board, geo, showPips }: { board: BoardData; geo: Geometry; showPips: boolean }) {
  const { ports, shore, wave, tileShadow, sea } = geo;
  return (
    <>
      <defs>
        {(Object.keys(STYLE) as Terrain[]).map((t) => (
          <radialGradient key={t} id={`g-${t}`} cx="50%" cy="46%" r="62%">
            <stop offset="0" stopColor={STYLE[t].center} />
            <stop offset="1" stopColor={STYLE[t].edge} />
          </radialGradient>
        ))}
        <DesertDefs uid="bd" scale={S} />
        {/* black copies of the resource symbols: drawn translucent and a little lower, they are the symbols' shadows */}
        {[0, 1, 2, 3, 4].map((r) => (
          <g key={r} id={`gs-${r}`}>
            <Glyph res={r} fill="#000" fill2="#000" silhouette />
          </g>
        ))}
        <linearGradient id="sail" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#ffffff" />
          <stop offset="0.7" stopColor="#f4f4f2" />
          <stop offset="1" stopColor="#d2d7da" />
        </linearGradient>
        <linearGradient id="hull" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#f0b040" />
          <stop offset="1" stopColor="#d88a1c" />
        </linearGradient>
      </defs>

      {sea.length > 0 && (
        <g fill="none" stroke="#3f86c4" strokeOpacity={0.55} strokeWidth={1.4}>
          {sea.map((pts, i) => (
            <polygon key={i} points={pts} />
          ))}
        </g>
      )}

      {/* shoreline: shallow water, foam, pale beach, sand */}
      <path d={wave} fill="none" stroke="#3a93c8" strokeWidth={47} strokeLinecap="round" strokeLinejoin="round" />
      <path d={wave} fill="none" stroke="#86d4f8" strokeWidth={38} strokeLinecap="round" strokeLinejoin="round" />
      <path d={shore} fill="#f4ebc6" stroke="#f4ebc6" strokeWidth={27} strokeLinejoin="round" />
      <path d={shore} fill="#e3b86a" stroke="#e3b86a" strokeWidth={15} strokeLinejoin="round" />


      {/* harbours */}
      {ports.map(({ p, a, b, sx, sy }, i) => (
        <g key={i} className="port">
          {[a, b].map((v, k) => {
            const vx = v.x * S;
            const vy = v.y * S;
            const dx = vx - sx;
            const dy = vy - sy;
            const len = Math.hypot(dx, dy) || 1;
            const ux = dx / len;
            const uy = dy / len;
            // planks run from under the hull to just short of the vertex
            const x1 = sx + ux * 24;
            const y1 = sy + uy * 24;
            const x2 = vx - ux * 3;
            const y2 = vy - uy * 3;
            return (
              <g key={k}>
                <line x1={x1} y1={y1} x2={x2} y2={y2} stroke="#b36a14" strokeWidth={15} />
                <line x1={x1} y1={y1} x2={x2} y2={y2} stroke="#eba233" strokeWidth={12.5} />
                <line x1={x1} y1={y1} x2={x2} y2={y2} stroke="#c47a18" strokeWidth={12.5} strokeDasharray="1.2 5" />
              </g>
            );
          })}
          <g transform={`translate(${sx} ${sy - 21}) scale(0.92)`}>
            <ellipse cx="2" cy="30" rx="21" ry="4.5" fill="#7ed0f6" />
            <path d="M-20 14 L22 11 Q23 25 5 29 Q-14 29 -20 14Z" fill="url(#hull)" stroke="#a8620f" strokeWidth="1.4" strokeLinejoin="round" />
            <path d="M-17 15 Q0 22 19 13" fill="none" stroke="#fff3c9" strokeWidth="1.4" opacity=".6" />
            <rect x="-2.5" y="-35" width="5" height="12" rx="1.5" fill="#d98a1c" />
            <path d="M2 -33 L12 -33 L10 -31 L24 -31 L24 -28 L2 -28Z" fill="#fff" stroke="#d3d8db" strokeWidth="0.8" />
            <path d="M-20 -18 C-26 -4 -24 12 -10 18 C4 23 17 20 21 12 L21 -18 Z" fill="url(#sail)" stroke="#c3c9cc" strokeWidth="1.2" />
            <path d="M-21 -23 H22 V-18 q-3.6 4.6 -7.2 0 q-3.6 4.6 -7.2 0 q-3.6 4.6 -7.2 0 q-3.6 4.6 -7.2 0 q-3.6 4.6 -7.2 0Z" fill="#eba233" stroke="#b46c12" strokeWidth="1" />
            <PortLabel res={p.resource} />
          </g>
        </g>
      ))}

      {/* the shadow under every tile, as one flat translucent path (so it never doubles up where tiles touch) */}
      <path d={tileShadow} fill="#000" fillOpacity={SHADOW_OPACITY} />

      {/* land tiles */}
      {board.hexes.map((h, i) => {
        const cx = h.x * S;
        const cy = h.y * S;
        const st = STYLE[h.terrain];
        const res = RES_OF[h.terrain];
        const art = ART[h.terrain];
        const hot = h.number === 6 || h.number === 8;
        const pips = h.number ? 6 - Math.abs(7 - h.number) : 0;
        const ty = cy + 0.31 * S;
        return (
          <g key={i}>
            <polygon points={hexPoints(cx, cy, S * 0.935)} fill={st.border} />
            <polygon points={hexPoints(cx, cy, S * 0.885)} fill={`url(#g-${h.terrain})`} />
            {h.terrain === 'desert' && <DesertArt cx={cx} cy={cy} uid="bd" scale={S} />}
            <polygon points={hexPoints(cx, cy, S * 0.845)} fill="none" stroke={st.hi} strokeWidth={1.6} opacity={0.75} />

            {/* decorations */}
            {(h.terrain === 'pasture' || h.terrain === 'fields' || h.terrain === 'forest') &&
              [-1, 1].map((d) => (
                <path
                  key={d}
                  d={`M${cx + d * 0.5 * S - 6},${cy - 0.2 * S} l3,-6 l3,6 M${cx + d * 0.5 * S - 1},${cy - 0.2 * S} l3,-7 l3,7`}
                  fill="none"
                  stroke={shade(st.edge, 0.2)}
                  strokeWidth="1.6"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  opacity="0.85"
                />
              ))}
            {(h.terrain === 'hills' || h.terrain === 'mountains') && (
              <g fill={shade(st.edge, h.terrain === 'hills' ? -0.18 : -0.12)} opacity=".75">
                <ellipse cx={cx - 0.55 * S} cy={cy - 0.28 * S} rx="7" ry="3.2" />
                <ellipse cx={cx + 0.5 * S} cy={cy - 0.3 * S} rx="6" ry="3" />
                <ellipse cx={cx + 0.58 * S} cy={cy - 0.22 * S} rx="4" ry="2.4" />
              </g>
            )}

            {/* terrain symbol */}
            {res !== null && (
              <>
                <use href={`#gs-${res}`} transform={`translate(${cx} ${cy - 0.5 * S + SHADOW_DY}) scale(${[1.0, 1.0, 1.12, 1.08, 1.15][res]}) translate(-20 -18)`} fillOpacity={SHADOW_OPACITY} strokeOpacity={SHADOW_OPACITY} />
                <g transform={`translate(${cx} ${cy - 0.5 * S}) scale(${[1.0, 1.0, 1.12, 1.08, 1.15][res]}) translate(-20 -18)`}>
                  <Glyph res={res} fill={art.fill} fill2={art.fill2} stroke={art.stroke} />
                </g>
              </>
            )}
            {h.terrain === 'volcano' && <VolcanoArt cx={cx} cy={cy} numbers={false} />}
            {h.terrain === 'gold' && <GoldArt cx={cx} cy={cy} />}

            {/* number token */}
            {h.number !== null && (
              <g>
                <rect x={cx - 21} y={ty - 20 + SHADOW_DY} width="42" height="40" rx="6.5" fill="#000" fillOpacity={SHADOW_OPACITY} />
                <rect x={cx - 21} y={ty - 20} width="42" height="40" rx="6.5" fill="#eae9e5" />
                <text
                  x={cx}
                  y={ty - (showPips ? 4 : 0)}
                  dy="0.36em"
                  textAnchor="middle"
                  fontSize="29"
                  fontWeight="800"
                  fill={hot ? '#cc0000' : '#114613'}
                  style={{ letterSpacing: '-0.02em' }}
                >
                  {h.number}
                </text>
                {showPips &&
                  Array.from({ length: pips }).map((_, k) => (
                    <circle key={k} cx={cx + (k - (pips - 1) / 2) * 5.4} cy={ty + 13.5} r={1.55} fill={hot ? '#cc0000' : '#114613'} />
                  ))}
              </g>
            )}

            {/* the volcano's corner numbers go on top of its number token */}
            {h.terrain === 'volcano' && <VolcanoArt cx={cx} cy={cy} cone={false} />}
          </g>
        );
      })}
    </>
  );
});

/** What the pieces drawn over the map reference: the robber's shading, and the desert's shade under the robber. */
function OverlayDefs() {
  return (
    <defs>
      <radialGradient id="rob" cx="38%" cy="32%" r="75%">
        <stop offset="0" stopColor="#ececec" />
        <stop offset="1" stopColor="#a9a9a9" />
      </radialGradient>
      <DesertDefs uid="bo" scale={S} />
    </defs>
  );
}

/** The cross planks that mark a road as a bridge: short lines across it, evenly spaced from end to end. */
function BridgePlanks({ a, b, stroke }: { a: Vertex; b: Vertex; stroke: string }) {
  const [ax, ay, bx, by] = [a.x * S, a.y * S, b.x * S, b.y * S];
  const len = Math.hypot(bx - ax, by - ay) || 1;
  const [nx, ny] = [-(by - ay) / len, (bx - ax) / len];
  const half = ROAD_W / 2 - ROAD_LINE;
  return (
    <g stroke={stroke} strokeWidth={1.3} strokeLinecap="round" opacity={0.75}>
      {[0.2, 0.35, 0.5, 0.65, 0.8].map((t) => {
        const [px, py] = [ax + (bx - ax) * t, ay + (by - ay) * t];
        return <line key={t} x1={px - nx * half} y1={py - ny * half} x2={px + nx * half} y2={py + ny * half} />;
      })}
    </g>
  );
}

/**
 * The board: a memoized static map plus the pieces that change with the game (roads, buildings,
 * robber, click targets), which are cheap to redraw on every message.
 *
 * They are two `<svg>`s stacked in one host `<div>`, with the same `viewBox`. The map is in its own `<div>` that the
 * browser keeps as a GPU layer (`.board-static`, `will-change: transform`), so panning and zooming only move that picture
 * and never re-draw its thousands of elements. (The same hint on a `<g>` inside one `<svg>` does nothing: the browser
 * draws the whole `<svg>` as one.) The overlay is small, so it is simply moved as a `<g>`. See mapZoom.ts.
 */
export default function Board({ view, vertexTargets, edgeTargets, hexTargets, showPips = true, onVertex, onEdge, onHex, zoomApi, onZoomEdge }: Props) {
  const board = useStableBoard(view.board);
  const geo = useMemo(() => computeGeometry(board), [board]);
  const { box } = geo;
  const { ref, scale, size } = useScreenScale(box);
  // Zoom and pan move the two layers inside the host; nothing outside the board is touched (see mapZoom.ts).
  const { layer, staticLayer, zoom, handlers } = useMapZoom({ host: ref, box, size, resetKey: board, api: zoomApi, onEdge: onZoomEdge });
  const viewBox = `${box.x0} ${box.y0} ${box.w} ${box.h}`;
  // A board with many hexes is drawn small, so its click targets get more room, by what's actually on screen (zoomed in, hexes are bigger).
  const grow = targetGrow(scale * zoom);
  // `setZoom` re-renders the board on every wheel tick; the road outlines depend only on the board and the roads
  // (patch.ts keeps an untouched `view.roads` identical), so they are worked out only when one of those changes.
  const roads = useMemo(() => roadShapes(board, view.roads), [board, view.roads]);

  return (
    <div ref={ref} className={'board' + (zoom > 1 ? ' zoomed' : '')} role="img" aria-label="Catan board" {...handlers}>
      <div ref={staticLayer} className="board-static">
        <svg className="board-svg" viewBox={viewBox} preserveAspectRatio="xMidYMid meet" aria-hidden="true" focusable="false">
          <BoardStatic board={board} geo={geo} showPips={showPips} />
        </svg>
      </div>
      <svg className="board-svg board-overlay" viewBox={viewBox} preserveAspectRatio="xMidYMid meet" aria-hidden="true" focusable="false">
        <OverlayDefs />
        <g ref={layer} className="board-layer">
          {/* roads */}
          {roads.map((r) => {
            const col = view.players[r.owner].color;
            const dark = isVeryDark(col);
            const outline = dark ? pieceEdge(col) : shade(col, -0.8);
            return (
              <g key={r.edge}>
                <linearGradient id={`road-${r.edge}`} gradientUnits="userSpaceOnUse" x1={r.across[0]} y1={r.across[1]} x2={r.across[2]} y2={r.across[3]}>
                  <stop offset="0" stopColor={shade(col, 0.45)} />
                  <stop offset="0.28" stopColor={shade(col, 0.12)} />
                  <stop offset="0.55" stopColor={shade(col, dark ? 0.1 : -0.3)} />
                  <stop offset="0.82" stopColor={col} />
                  <stop offset="1" stopColor={shade(col, 0.35)} />
                </linearGradient>
                <polygon
                  points={r.pts.map((p) => `${p[0].toFixed(1)},${p[1].toFixed(1)}`).join(' ')}
                  fill={`url(#road-${r.edge})`}
                  stroke={outline}
                  strokeWidth={ROAD_LINE}
                  strokeLinejoin="miter"
                  strokeMiterlimit={4}
                />
                {board.edges[r.edge].bridge && <BridgePlanks a={board.vertices[board.edges[r.edge].a]} b={board.vertices[board.edges[r.edge].b]} stroke={outline} />}
              </g>
            );
          })}

          {/* buildings */}
          {board.vertices.map((v, i) => {
            const b = view.buildings[i];
            if (!b) return null;
            return (
              <g key={i} transform={`translate(${v.x * S} ${v.y * S - 6}) scale(${b.city ? 1.12 : 1.22})`}>
                <PieceShadow city={b.city} />
                <PieceShape color={view.players[b.owner].color} city={b.city} />
              </g>
            );
          })}

          {/* settlements on loan after lava hit a city: a crack in the base */}
          {view.damaged.map((i) => {
            const v = board.vertices[i];
            return (
              <g key={`dm${i}`} className="damaged" transform={`translate(${v.x * S} ${v.y * S + 9})`}>
                <path d="M-9 0 L-3 -3 L0 2 L4 -2 L9 1" fill="none" stroke="#ff5a1f" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" />
                <title>Damaged by lava: repair this settlement (upgrade it to a city) before building another city</title>
              </g>
            );
          })}

          {/* where the last lava flow ran */}
          {view.lava &&
            (() => {
              const hex = board.hexes[view.lava.hex];
              const v = board.vertices[view.lava.vertex];
              if (!hex || !v) return null;
              const [hx, hy] = [hex.x * S, hex.y * S];
              return (
                <g className="lava" pointerEvents="none">
                  <line x1={hx} y1={hy - 0.43 * S} x2={v.x * S} y2={v.y * S} stroke="#ff5a1f" strokeWidth="7" strokeLinecap="round" opacity=".75" />
                  <line x1={hx} y1={hy - 0.43 * S} x2={v.x * S} y2={v.y * S} stroke="#ffd25e" strokeWidth="2.6" strokeLinecap="round" />
                  <circle cx={v.x * S} cy={v.y * S} r="17" fill="#ff5a1f" opacity=".55" />
                  <circle cx={v.x * S} cy={v.y * S} r="10" fill="#ffd25e" opacity=".9" />
                  <title>{`Lava flowed down corner ${view.lava.die}`}</title>
                </g>
              );
            })()}

          {/* robber */}
          {(() => {
            const h = board.hexes[view.robber];
            return (
              <>
              {h.terrain === 'desert' && <DesertRobberShade x={h.x * S - 0.5 * S} y={h.y * S + 0.06 * S + 20} uid="bo" />}
              <g transform={`translate(${h.x * S - 0.5 * S} ${h.y * S + 0.06 * S}) scale(1.18)`} className="robber">
                <path
                  d="M-14 14 C-16 4 -9 -2 0 -2 C9 -2 16 4 14 14 C10 19 -10 19 -14 14Z M-11 -11 A11 10.5 0 1 1 11 -11 A11 10.5 0 1 1 -11 -11Z"
                  transform={`translate(0 ${SHADOW_DY})`}
                  fill="#000"
                  fillOpacity={SHADOW_OPACITY}
                />
                <ellipse cx="0" cy="17" rx="15" ry="5" fill="rgba(0,0,0,.25)" />
                <path d="M-14 14 C-16 4 -9 -2 0 -2 C9 -2 16 4 14 14 C10 19 -10 19 -14 14Z" fill="url(#rob)" stroke="#7b7b7b" strokeWidth="1.8" strokeLinejoin="round" />
                <ellipse cx="0" cy="-11" rx="11" ry="10.5" fill="url(#rob)" stroke="#7b7b7b" strokeWidth="1.8" />
                <path d="M-6 -16 Q-3 -19.5 1 -19" fill="none" stroke="#fff" strokeWidth="2.2" strokeLinecap="round" opacity=".7" />
                <path d="M-9 6 Q-10 11 -6 14" fill="none" stroke="#fff" strokeWidth="2" strokeLinecap="round" opacity=".45" />
              </g>
              </>
            );
          })()}

          {/* click targets */}
          {hexTargets.map((i) => {
            const h = board.hexes[i];
            return <polygon key={`h${i}`} points={hexPoints(h.x * S, h.y * S, S * 0.9)} className="target-hex" onClick={() => onHex(i)} />;
          })}
          {edgeTargets.map((i) => {
            const e = board.edges[i];
            const a = board.vertices[e.a];
            const b = board.vertices[e.b];
            return (
              <g key={`e${i}`} className="target-edge" onClick={() => onEdge(i)}>
                <line x1={a.x * S} y1={a.y * S} x2={b.x * S} y2={b.y * S} className="target-edge-hit" />
                <line x1={a.x * S} y1={a.y * S} x2={b.x * S} y2={b.y * S} className="target-edge-show" />
              </g>
            );
          })}
          {vertexTargets.map((i) => {
            const v = board.vertices[i];
            return (
              <g key={`v${i}`} className="target-vertex" onClick={() => onVertex(i)}>
                <circle cx={v.x * S} cy={v.y * S} r={TARGET_R * grow} className="target-vertex-hit" />
                <circle cx={v.x * S} cy={v.y * S} r={14 * grow} className="target-vertex-show" />
              </g>
            );
          })}
        </g>
      </svg>
    </div>
  );
}
