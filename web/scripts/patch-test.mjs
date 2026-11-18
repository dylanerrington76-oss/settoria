// Tests for src/patch.ts, the client's half of the patch protocol.
// Usage: node scripts/patch-test.mjs        (from web/; needs a Node that runs .ts files, 22.18 or later)
//
// There is no server in this test. `diff` below is a port of `diff` in ../src/patch.rs, used to make patches
// to apply, so that what the client does with them is checked against what the server makes of them.
import assert from 'node:assert/strict';
import { applyPatch, LOG_WINDOW, PatchError } from '../src/patch.ts';

// ---- a port of src/patch.rs ---------------------------------------------------------------------
const SMALL_ARRAY = 12;
const isObject = (v) => v !== null && typeof v === 'object' && !Array.isArray(v);
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

function diff(prev, next, ops = [], path = []) {
  if (same(prev, next)) return ops;
  if (isObject(prev) && isObject(next)) {
    for (const key of Object.keys(next)) {
      if (key in prev) diff(prev[key], next[key], ops, [...path, key]);
      else ops.push([[...path, key], next[key]]);
    }
    for (const key of Object.keys(prev)) if (!(key in next)) ops.push([[...path, key]]);
  } else if (Array.isArray(prev) && Array.isArray(next) && prev.length === next.length) {
    const changed = next.filter((v, i) => !same(prev[i], v)).length;
    const byEntry = next.length <= SMALL_ARRAY ? next.some(isObject) : changed * 4 <= next.length;
    if (byEntry) next.forEach((v, i) => diff(prev[i], v, ops, [...path, i]));
    else ops.push([path, next]);
  } else {
    ops.push([path, next]);
  }
  return ops;
}

const deepFreeze = (v) => {
  if (v && typeof v === 'object') {
    Object.values(v).forEach(deepFreeze);
    Object.freeze(v);
  }
  return v;
};
const clone = (v) => JSON.parse(JSON.stringify(v));
const patchOf = (ops, extra = {}) => ({ type: 'patch', seq: 1, ops, ...extra });

let passed = 0;
const test = (name, fn) => {
  fn();
  passed++;
  console.log('ok  ', name);
};

// ---- a view the shape of the real one -------------------------------------------------------------
const blank = (n, v) => Array.from({ length: n }, () => v);
const event = (i) => ({ type: 'built_road', player: i % 3 });
const view = () => ({
  you: 0,
  board: { map: 'standard', hexes: [{ q: 0, r: 0, x: 0, y: 0, terrain: 'forest', number: 5 }], vertices: [], edges: [], ports: [], desert: 0 },
  phase: 'main',
  current: 1,
  dice: null,
  buildings: blank(54, null),
  roads: blank(72, null),
  players: [0, 1, 2].map((id) => ({ id, name: 'P' + id, cards: 4, vp: 2, connected: true })),
  hand: [1, 0, 2, 0, 1],
  legal: { settlements: [], cities: [], roads: [], bridges: [], hexes: [] },
  trades: [],
  timer: { kind: 'turn', player: 1, remaining_ms: 60000, total_ms: 90000 },
  log: Array.from({ length: 10 }, (_, i) => event(i)),
  log_start: 0,
});

// ---- tests ------------------------------------------------------------------------------------------
test('sets a field, and what it does not touch stays the same object', () => {
  const v = deepFreeze(view());
  const next = applyPatch(v, patchOf([[['phase'], 'discard'], [['dice'], [3, 4]]]));
  assert.equal(next.phase, 'discard');
  assert.deepEqual(next.dice, [3, 4]);
  assert.equal(next.board, v.board, 'the board is the same object');
  assert.equal(next.players, v.players);
  assert.equal(next.buildings, v.buildings);
  assert.equal(v.phase, 'main', 'the old view is not changed');
});

test('patches an array entry and an object inside one, copying only along the path', () => {
  const v = deepFreeze(view());
  const next = applyPatch(
    v,
    patchOf([
      [['buildings', 5], { owner: 1, city: false }],
      [['players', 1, 'cards'], 3],
      [['roads', 17], 2],
    ]),
  );
  assert.deepEqual(next.buildings[5], { owner: 1, city: false });
  assert.equal(next.players[1].cards, 3);
  assert.equal(next.roads[17], 2);
  assert.notEqual(next.players, v.players);
  assert.equal(next.players[0], v.players[0], 'the other players are the same objects');
  assert.equal(next.players[2], v.players[2]);
  assert.equal(next.buildings[6], v.buildings[6]);
  assert.equal(v.players[1].cards, 4);
  assert.equal(v.buildings[5], null);
});

test('an op that changes nothing copies nothing', () => {
  const v = deepFreeze(view());
  assert.equal(applyPatch(v, patchOf([[['phase'], 'main'], [['players', 0, 'vp'], 2]])), v);
});

test('removes a key', () => {
  const v = deepFreeze({ a: 1, b: { c: 2, d: 3 } });
  const next = applyPatch(v, patchOf([[['b', 'c']]]));
  assert.deepEqual(next, { a: 1, b: { d: 3 } });
  assert.deepEqual(v.b, { c: 2, d: 3 });
});

test('replaces a whole array, and sets null and a nested value', () => {
  const v = deepFreeze(view());
  const next = applyPatch(v, patchOf([[['hand'], [0, 0, 0, 0, 0]], [['timer'], null], [['legal', 'roads'], [4, 9]]]));
  assert.deepEqual(next.hand, [0, 0, 0, 0, 0]);
  assert.equal(next.timer, null);
  assert.deepEqual(next.legal.roads, [4, 9]);
  assert.equal(next.legal.settlements, v.legal.settlements);
});

test('log events are appended, numbered from where the held log ends', () => {
  const v = deepFreeze(view());
  const next = applyPatch(v, patchOf([], { log_from: 10, log: [event(10), event(11)] }));
  assert.equal(next.log.length, 12);
  assert.equal(next.log_start, 0);
  assert.deepEqual(next.log[11], event(11));
  assert.equal(v.log.length, 10);
});

test('the log keeps its window, and log_start moves on with it', () => {
  let v = deepFreeze(view());
  let total = v.log.length;
  for (let round = 0; round < 60; round++) {
    const fresh = Array.from({ length: 1 + (round % 4) }, (_, i) => event(total + i));
    v = deepFreeze(applyPatch(v, patchOf([], { log_from: total, log: fresh })));
    total += fresh.length;
    assert.equal(v.log_start + v.log.length, total, 'the window ends at the newest event');
    assert.ok(v.log.length <= LOG_WINDOW);
  }
  assert.equal(v.log.length, LOG_WINDOW);
  assert.ok(v.log_start > 0);
  assert.deepEqual(v.log[LOG_WINDOW - 1], event(total - 1));
});

test('a patch with no log events leaves the log alone', () => {
  const v = deepFreeze(view());
  assert.equal(applyPatch(v, patchOf([[['phase'], 'roll']])).log, v.log);
});

test('what does not fit is refused with a PatchError', () => {
  const v = deepFreeze(view());
  const bad = [
    patchOf([[['nothing', 'here'], 1]]),
    patchOf([[['phase', 'deeper'], 1]]),
    patchOf([[['players', 'x'], 1]]),
    patchOf([[['board', 0], 1]]),
    patchOf([[[], 1]]),
    patchOf([[['hand', 'a'], 1]]),
    patchOf([], { log_from: 7, log: [event(7)] }),
    patchOf([], { log_from: 12, log: [event(12)] }),
  ];
  for (const p of bad) assert.throws(() => applyPatch(v, p), PatchError, JSON.stringify(p));
});

test('random patches made by the server\'s diff always give the new view', () => {
  let seed = 12345;
  const rnd = (n) => ((seed = (seed * 1103515245 + 12345) % 2147483648), seed % n);
  let ops = 0;
  for (let game = 0; game < 200; game++) {
    let v = deepFreeze(view());
    for (let turn = 0; turn < 30; turn++) {
      const next = clone(v);
      for (let k = rnd(7); k > 0; k--) {
        switch (rnd(8)) {
          case 0: next.buildings[rnd(54)] = rnd(3) === 0 ? null : { owner: rnd(3), city: rnd(2) === 0 }; break;
          case 1: next.roads[rnd(72)] = rnd(4) === 0 ? null : rnd(3); break;
          case 2: next.players[rnd(3)].cards = rnd(20); break;
          case 3: next.players[rnd(3)].connected = rnd(2) === 0; break;
          case 4: next.hand[rnd(5)] = rnd(6); break;
          case 5: next.legal.settlements = Array.from({ length: rnd(30) }, (_, i) => i * 2); break;
          case 6: next.trades = Array.from({ length: rnd(3) }, (_, i) => ({ id: i, from: 1, replies: ['pending', 'declined'] })); break;
          default: next.timer = rnd(3) === 0 ? null : { kind: 'turn', player: rnd(3), remaining_ms: rnd(90000), total_ms: 90000 };
        }
      }
      const patchOps = diff(v, next);
      ops += patchOps.length;
      const got = applyPatch(v, patchOf(patchOps));
      assert.deepEqual(got, next);
      assert.equal(got.board, v.board, 'the board is never copied');
      v = deepFreeze(got);
    }
  }
  assert.ok(ops > 1000, 'the test made patches');
});

console.log(`\n${passed} tests passed`);
