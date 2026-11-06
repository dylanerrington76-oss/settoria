import { memo, useEffect, useMemo, useRef, useState } from 'react';
import { Glyph, PieceShape, isVeryDark, pieceEdge, shade } from './art';
import { GameView, Terrain } from './types';

const S = 64; // pixels per hex radius (board units are multiples of this)

interface Style {
  center: string;
  edge: string;
  border: string;
  hi: string;
}
const STYLE: Record<Terrain, Style> = {
  hills: { center: '#f07d38', edge: '#dd5924', border: '#c43e25', hi: '#f6a064' },
  mountains: { center: '#bcc0bd', edge: '#9a9f9b', border: '#80868a', hi: '#d6dad7' },
  pasture: { center: '#a9d02c', edge: '#86b50f', border: '#6c9610', hi: '#d6e440' },
  fields: { center: '#ffd75f', edge: '#f0b412', border: '#bb850b', hi: '#ffe58f' },
  forest: { center: '#17a047', edge: '#08893c', border: '#066a2d', hi: '#69d252' },
  desert: { center: '#d9d09a', edge: '#c6bc7d', border: '#a39b63', hi: '#e9e2b8' },
};
const RES_OF: Record<Terrain, number | null> = { hills: 0, forest: 1, mountains: 2, fields: 3, pasture: 4, desert: null };
const ART: Record<Terrain, { fill: string; fill2: string; stroke?: string }> = {
  hills: { fill: '#f9d3c4', fill2: '#f0b3a0', stroke: '#e48b72' },
  mountains: { fill: '#f6f8f7', fill2: '#dfe4e2', stroke: '#a7aeab' },
  pasture: { fill: '#ffffff', fill2: '#ffffff', stroke: '#c9d79a' },
  fields: { fill: '#cf9021', fill2: '#e8b13a', stroke: '#a26d0c' },
  forest: { fill: '#3aa24a', fill2: '#58bd5c', stroke: '#176d2b' },
  desert: { fill: '#000', fill2: '#000' },
};

function hexPoints(cx: number, cy: number, r: number): string {
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
  return { ports, box, land, sea };
}
type Geometry = ReturnType<typeof computeGeometry>;

/** Radius of a vertex click target in board units; it is enlarged when the board is drawn so small that this would be under `MIN_TARGET_PX` on screen. */
const TARGET_R = 19;
const MIN_TARGET_PX = 12;
/** Vertices are one hex radius (`S`) apart, so a target may not grow so far that it covers its neighbour's. */
const MAX_TARGET_GROW = 1.5;

/**
 * How big a hex really is on screen: CSS pixels per board unit, i.e. how far the board's box is scaled down to
 * fit the space it is given. Follows the size of the `<svg>`, so it also changes with the window.
 * Until the first measurement (and where there is no `ResizeObserver`) it says 1.
 */
function useScreenScale(box: { w: number; h: number }) {
  const ref = useRef<SVGSVGElement>(null);
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
  return { ref, scale };
}

/** By how much to enlarge the click targets so they stay easy to hit when the board is drawn small (1 = not at all). */
const targetGrow = (scale: number): number => Math.min(MAX_TARGET_GROW, Math.max(1, MIN_TARGET_PX / (TARGET_R * scale)));

/**
 * The part of the board that never changes during a game: sea, shoreline, harbours, tiles and
 * number tokens (about 90% of the board's elements). It is memoized, so a new game message
 * does not re-render it; it only redraws when the board itself or the pips setting changes.
 */
const BoardStatic = memo(function BoardStatic({ board, geo, showPips }: { board: BoardData; geo: Geometry; showPips: boolean }) {
  const { ports, land, sea } = geo;
  return (
    <>
      <defs>
        {(Object.keys(STYLE) as Terrain[]).map((t) => (
          <radialGradient key={t} id={`g-${t}`} cx="50%" cy="46%" r="62%">
            <stop offset="0" stopColor={STYLE[t].center} />
            <stop offset="1" stopColor={STYLE[t].edge} />
          </radialGradient>
        ))}
        <filter id="wavy" x="-5%" y="-5%" width="110%" height="110%">
          <feTurbulence type="fractalNoise" baseFrequency="0.035" numOctaves="2" seed="7" result="n" />
          <feDisplacementMap in="SourceGraphic" in2="n" scale="9" />
        </filter>
        <filter id="lift" x="-30%" y="-30%" width="160%" height="160%">
          <feDropShadow dx="0" dy="1.6" stdDeviation="1.5" floodColor="#000" floodOpacity="0.35" />
        </filter>
        <filter id="drop" x="-40%" y="-40%" width="180%" height="190%">
          <feDropShadow dx="0" dy="2.5" stdDeviation="1.8" floodColor="#000" floodOpacity="0.45" />
        </filter>
        <linearGradient id="sail" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#ffffff" />
          <stop offset="0.7" stopColor="#f4f4f2" />
          <stop offset="1" stopColor="#d2d7da" />
        </linearGradient>
        <radialGradient id="rob" cx="38%" cy="32%" r="75%">
          <stop offset="0" stopColor="#ececec" />
          <stop offset="1" stopColor="#a9a9a9" />
        </radialGradient>
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
      <g filter="url(#wavy)">
        {land.map((pts, i) => (
          <polygon key={i} points={pts} fill="#3a93c8" stroke="#3a93c8" strokeWidth={47} strokeLinejoin="round" />
        ))}
        {land.map((pts, i) => (
          <polygon key={i} points={pts} fill="#86d4f8" stroke="#86d4f8" strokeWidth={38} strokeLinejoin="round" />
        ))}
      </g>
      {land.map((pts, i) => (
        <polygon key={i} points={pts} fill="#f4ebc6" stroke="#f4ebc6" strokeWidth={27} strokeLinejoin="round" />
      ))}
      {land.map((pts, i) => (
        <polygon key={i} points={pts} fill="#e3b86a" stroke="#e3b86a" strokeWidth={15} strokeLinejoin="round" />
      ))}

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
            {p.resource === null ? (
              <text x="0.5" y="-1" textAnchor="middle" fontSize="26" fontWeight="800" fill="#5c2626">
                ?
              </text>
            ) : (
              <g transform="translate(-10.5 -17) scale(0.55)">
                <Glyph res={p.resource} fill="#5c2626" fill2="#5c2626" />
              </g>
            )}
            <text x="0.5" y="14" textAnchor="middle" fontSize="12.5" fontWeight="600" fill="#7a4a4a">
              {p.resource === null ? '3:1' : '2:1'}
            </text>
          </g>
        </g>
      ))}

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
            <g filter="url(#lift)">
              <polygon points={hexPoints(cx, cy, S * 0.935)} fill={st.border} />
            </g>
            <polygon points={hexPoints(cx, cy, S * 0.885)} fill={`url(#g-${h.terrain})`} />
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
            {(h.terrain === 'hills' || h.terrain === 'mountains' || h.terrain === 'desert') && (
              <g fill={shade(st.edge, h.terrain === 'hills' ? -0.18 : -0.12)} opacity=".75">
                <ellipse cx={cx - 0.55 * S} cy={cy - 0.28 * S} rx="7" ry="3.2" />
                <ellipse cx={cx + 0.5 * S} cy={cy - 0.3 * S} rx="6" ry="3" />
                <ellipse cx={cx + 0.58 * S} cy={cy - 0.22 * S} rx="4" ry="2.4" />
              </g>
            )}

            {/* terrain symbol */}
            {res !== null && (
              <g transform={`translate(${cx} ${cy - 0.5 * S}) scale(${[1.0, 1.0, 1.12, 1.08, 1.15][res]}) translate(-20 -18)`} filter="url(#lift)">
                <Glyph res={res} fill={art.fill} fill2={art.fill2} stroke={art.stroke} />
              </g>
            )}
            {h.terrain === 'desert' && (
              <g transform={`translate(${cx + 0.22 * S} ${cy - 0.3 * S})`} fill="#7ea45a" stroke="#4f7a38" strokeWidth="1.4" strokeLinejoin="round">
                <rect x="-4" y="-16" width="8" height="30" rx="4" />
                <path d="M-4 -2 H-9 Q-12 -2 -12 -6 V-12 Q-12 -15 -9 -15 Q-6 -15 -6 -12 V-7 H-4Z" />
                <path d="M4 3 H9 Q12 3 12 -1 V-8 Q12 -11 9 -11 Q6 -11 6 -8 V-3 H4Z" />
              </g>
            )}

            {/* number token */}
            {h.number !== null && (
              <g filter="url(#lift)">
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
          </g>
        );
      })}
    </>
  );
});

/**
 * The board: a memoized static layer plus the pieces that change with the game (roads, buildings,
 * robber, click targets), which are cheap to redraw on every message.
 */
export default function Board({ view, vertexTargets, edgeTargets, hexTargets, showPips = true, onVertex, onEdge, onHex }: Props) {
  const board = useStableBoard(view.board);
  const geo = useMemo(() => computeGeometry(board), [board]);
  const { box } = geo;
  // A board with many hexes is drawn small, so its click targets get more room, by what's actually on screen.
  const { ref, scale } = useScreenScale(box);
  const grow = targetGrow(scale);

  return (
    <svg ref={ref} className="board" viewBox={`${box.x0} ${box.y0} ${box.w} ${box.h}`} preserveAspectRatio="xMidYMid meet" role="img" aria-label="Catan board">
      <BoardStatic board={board} geo={geo} showPips={showPips} />

      {/* roads */}
      {roadShapes(board, view.roads).map((r) => {
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
          </g>
        );
      })}

      {/* buildings */}
      {board.vertices.map((v, i) => {
        const b = view.buildings[i];
        if (!b) return null;
        return (
          <g key={i} transform={`translate(${v.x * S} ${v.y * S - 6}) scale(${b.city ? 1.12 : 1.22})`} filter="url(#drop)">
            <PieceShape color={view.players[b.owner].color} city={b.city} />
          </g>
        );
      })}

      {/* robber */}
      {(() => {
        const h = board.hexes[view.robber];
        return (
          <g transform={`translate(${h.x * S - 0.5 * S} ${h.y * S + 0.06 * S}) scale(1.18)`} className="robber" filter="url(#drop)">
            <ellipse cx="0" cy="17" rx="15" ry="5" fill="rgba(0,0,0,.25)" />
            <path d="M-14 14 C-16 4 -9 -2 0 -2 C9 -2 16 4 14 14 C10 19 -10 19 -14 14Z" fill="url(#rob)" stroke="#7b7b7b" strokeWidth="1.8" strokeLinejoin="round" />
            <ellipse cx="0" cy="-11" rx="11" ry="10.5" fill="url(#rob)" stroke="#7b7b7b" strokeWidth="1.8" />
            <path d="M-6 -16 Q-3 -19.5 1 -19" fill="none" stroke="#fff" strokeWidth="2.2" strokeLinecap="round" opacity=".7" />
            <path d="M-9 6 Q-10 11 -6 14" fill="none" stroke="#fff" strokeWidth="2" strokeLinecap="round" opacity=".45" />
          </g>
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
    </svg>
  );
}
