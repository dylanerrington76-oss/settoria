// Tests for src/boardGeometry.ts: the browser works out a board's corners and sides from its hexes, and has to
// number them exactly as the server does (a building is placed by corner number, a road by side number).
// Usage: node scripts/board-geometry-test.mjs        (from web/; needs a Node that runs .ts files, 22.18 or later)
//
// There is no server in this test. board-golden.json is what the server builds for every map (written by the
// Rust test `board_wire::golden`, which fails if the file goes out of date), so this checks the browser's
// numbering against the server's.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { expandBoard } from '../src/boardGeometry.ts';

const golden = JSON.parse(readFileSync(new URL('./board-golden.json', import.meta.url), 'utf8'));

let passed = 0;
const test = (name, fn) => {
  fn();
  passed++;
  console.log('ok  ', name);
};

/** A board as the server sends it, for a map's hexes: every hex a forest, no harbours. */
const wireOf = (map) => ({
  map: 'test',
  style: { sea_grid: false },
  hexes: map.hexes.map(([q, r]) => [q, r, 'forest', null]),
  ports: [],
  ...(map.bridges.length ? { bridges: map.bridges } : {}),
  desert: 0,
});

const ids = Object.keys(golden);
test('the golden file has the maps it should', () => {
  for (const id of ['standard', 'earth', 'usa']) assert.ok(ids.includes(id), `${id} is in the golden file`);
});

for (const id of ids) {
  const map = golden[id];
  const board = expandBoard(wireOf(map));

  test(`${id}: the hexes are where the server draws them`, () => {
    assert.equal(board.hexes.length, map.hexes.length);
    board.hexes.forEach((h, i) => {
      assert.deepEqual([h.q, h.r], map.hexes[i]);
      assert.ok(Math.abs(h.x * 1e6 - map.centres[i][0]) <= 1 && Math.abs(h.y * 1e6 - map.centres[i][1]) <= 1, `hex ${i} is drawn at the server's centre`);
    });
  });

  test(`${id}: the corners are numbered and placed as the server's are`, () => {
    assert.equal(board.vertices.length, map.vertices.length);
    board.vertices.forEach((v, i) => {
      assert.ok(Math.abs(v.x * 1e6 - map.vertices[i][0]) <= 1 && Math.abs(v.y * 1e6 - map.vertices[i][1]) <= 1, `corner ${i} is where the server has it`);
    });
  });

  test(`${id}: the sides are numbered as the server's are, bridge sites after them`, () => {
    const land = board.edges.filter((e) => !e.bridge);
    assert.deepEqual(
      land.map((e) => [e.a, e.b]),
      map.edges,
    );
    const bridges = board.edges.filter((e) => e.bridge);
    assert.deepEqual(
      bridges.map((e) => [e.a, e.b]),
      map.bridges,
    );
    assert.deepEqual(board.edges.slice(0, land.length), land, 'the land sides come first, so their numbers never move');
    assert.ok(land.every((e) => !('bridge' in e)), 'a land side carries no bridge flag, as before');
  });
}

test('what the server deals passes through: terrain, tokens, harbours, the map, the style and the robber', () => {
  const ports = [{ a: 0, b: 1, resource: 2 }];
  const b = expandBoard({
    map: 'earth',
    style: { sea_grid: true },
    hexes: [
      [0, 0, 'desert', null],
      [1, 0, 'mountains', 10],
    ],
    ports,
    desert: 0,
  });
  assert.deepEqual(b.hexes.map((h) => [h.terrain, h.number]), [['desert', null], ['mountains', 10]]);
  assert.equal(b.ports, ports);
  assert.deepEqual([b.map, b.style, b.desert], ['earth', { sea_grid: true }, 0]);
  assert.equal(b.vertices.length, 10, 'two touching hexes have ten corners');
  assert.equal(b.edges.length, 11, 'and eleven sides');
});

test('a board with no bridges field has no bridge sites', () => {
  const b = expandBoard(wireOf({ hexes: [[0, 0]], bridges: [] }));
  assert.equal(b.edges.length, 6);
  assert.ok(b.edges.every((e) => !e.bridge));
});

test('a malformed board is refused rather than drawn wrongly', () => {
  const base = { map: 'x', style: { sea_grid: false }, hexes: [[0, 0, 'forest', 5]], ports: [], desert: 0 };
  assert.throws(() => expandBoard({ ...base, bridges: [[0, 99]] }), /bridge/);
  assert.throws(() => expandBoard({ ...base, ports: [{ a: 0, b: 99, resource: null }] }), /harbour/);
  assert.throws(() => expandBoard({ ...base, desert: 3 }), /robber/);
  assert.doesNotThrow(() => expandBoard(base));
});

console.log(`\n${passed} passed`);
