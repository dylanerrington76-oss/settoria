import type { BoardData, Edge, Hex, Vertex, WireBoard } from './types';

/**
 * Works out the corners and sides of a board from its hexes, so the server doesn't have to send them.
 *
 * The server sends each hex as `[q, r, terrain, number]`. Where a hex is drawn, its six corners ("vertices") and
 * its six sides ("edges") all follow from `q` and `r`. The numbering has to be the one the server's rules use
 * (a building is placed by vertex number, a road by edge number), so this repeats `Geometry::build` in
 * src/board.rs step for step:
 *
 *  - go through the hexes in the order they are sent, and through each hex's corners clockwise from the upper
 *    right; a corner not seen before takes the next number;
 *  - then go round the hex's six sides (corner i to corner i + 1); a side not seen before takes the next number,
 *    and is stored with its smaller corner first;
 *  - the bridge sites come last, numbered on from the land sides in the order the server lists them (they are
 *    sent rather than worked out, because they are few and finding them here would be a second copy of the
 *    server's search).
 *
 * `web/scripts/board-geometry-test.mjs` checks this against what the server builds, for every built-in map
 * (the server's own test, `board_wire::golden`, keeps the file it checks against up to date).
 *
 * This file imports only types, so Node can run it directly, as the test does.
 */

const SQRT3 = Math.sqrt(3);

// Corner positions on an integer lattice, so corners shared between hexes get the same key exactly. A corner is
// at hex centre + (DX[i] * sqrt(3)/2, DY[i] / 2); the centre is at ((2q + r) * sqrt(3)/2, 3r / 2).
const DX = [1, 1, 0, -1, -1, 0];
const DY = [-1, 1, 2, 1, -1, -2];

/** A pair of corner ids as one number, smaller first. Corner ids stay far below 2^20 (a map has at most 500 hexes). */
const pairKey = (lo: number, hi: number): number => lo * 1048576 + hi;

/** Turns a board as it comes over the socket into the one the page draws from. Throws if the board is malformed. */
export function expandBoard(wire: WireBoard): BoardData {
  const corners = new Map<string, number>();
  const sides = new Map<number, number>();
  const hexes: Hex[] = [];
  const vertices: Vertex[] = [];
  const edges: Edge[] = [];

  for (const [q, r, terrain, number] of wire.hexes) {
    const ring: number[] = [];
    for (let i = 0; i < 6; i++) {
      const lx = 2 * q + r + DX[i];
      const ly = 3 * r + DY[i];
      const key = `${lx},${ly}`;
      let id = corners.get(key);
      if (id === undefined) {
        id = vertices.length;
        corners.set(key, id);
        vertices.push({ x: (lx * SQRT3) / 2, y: ly / 2 });
      }
      ring.push(id);
    }
    for (let i = 0; i < 6; i++) {
      const a = ring[i];
      const b = ring[(i + 1) % 6];
      const lo = Math.min(a, b);
      const hi = Math.max(a, b);
      const key = pairKey(lo, hi);
      if (!sides.has(key)) {
        sides.set(key, edges.length);
        edges.push({ a: lo, b: hi });
      }
    }
    hexes.push({ q, r, x: SQRT3 * (q + r / 2), y: 1.5 * r, terrain, number });
  }

  for (const pair of wire.bridges ?? []) {
    const [a, b] = pair;
    if (!(a >= 0 && b >= 0 && a < vertices.length && b < vertices.length)) throw new Error(`bridge site ${a}-${b} is off the board`);
    edges.push({ a, b, bridge: true });
  }

  for (const p of wire.ports) {
    if (!(p.a >= 0 && p.b >= 0 && p.a < vertices.length && p.b < vertices.length)) throw new Error(`harbour ${p.a}-${p.b} is off the board`);
  }
  if (!(wire.desert >= 0 && wire.desert < hexes.length)) throw new Error(`the robber's start, hex ${wire.desert}, is off the board`);

  return { map: wire.map, style: wire.style, hexes, vertices, edges, ports: wire.ports, desert: wire.desert };
}
