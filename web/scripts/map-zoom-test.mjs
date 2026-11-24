// Tests for the maths in src/mapZoom.ts: zooming about a point, keeping the board in view, and the limits.
// Usage: node scripts/map-zoom-test.mjs        (from web/; needs a Node that runs .ts files, 22.18 or later)
import assert from 'node:assert/strict';
import { HOME, MAX_ZOOM, MIN_ZOOM, clampView, clampZoom, edgeOf, toBoard, viewportOf, zoomAt } from '../src/mapZoom.ts';

let passed = 0;
const test = (name, fn) => {
  fn();
  passed++;
  console.log('ok  ', name);
};
const near = (a, b, msg) => assert.ok(Math.abs(a - b) < 1e-6, `${msg ?? ''} ${a} vs ${b}`);

// a board box 1000 x 800 drawn in a 1000 x 800 px element (1 px per unit), and in a wide 1600 x 800 one (letterboxed)
const box = { x0: -500, y0: -400, w: 1000, h: 800 };
const sq = { w: 1000, h: 800 };
const wide = { w: 1600, h: 800 };

test('the visible rectangle is the box when the element has its shape, and wider when it is wider', () => {
  const a = viewportOf(box, sq);
  near(a.s, 1);
  near(a.x0, -500);
  near(a.x1, 500);
  const b = viewportOf(box, wide);
  near(b.s, 1);
  near(b.x0, -800);
  near(b.x1, 800);
  near(b.y0, -400);
});
test('before it is measured the box is what shows', () => {
  const v = viewportOf(box, null);
  near(v.x0, -500);
  near(v.y1, 400);
});
test('screen points map to board points (centre of the element is the centre of the box)', () => {
  const vp = viewportOf(box, wide);
  const rect = { left: 100, top: 50, width: 1600, height: 800 };
  const c = toBoard(box, vp, rect, 100 + 800, 50 + 400);
  near(c.x, 0);
  near(c.y, 0);
  const e = toBoard(box, vp, rect, 100, 50);
  near(e.x, -800);
  near(e.y, -400);
});
test('zooming keeps the point under the cursor where it is', () => {
  const v = zoomAt(HOME, 2, 200, -100);
  // the board point (200, -100) is drawn at (200, -100) both before and after
  near(v.x + v.k * 200, 200);
  near(v.y + v.k * -100, -100);
  const w = zoomAt(v, 4, -300, 250);
  const px = (-300 - v.x) / v.k;
  near(w.x + w.k * px, -300);
});
test('the zoom stays between its limits', () => {
  assert.equal(clampZoom(0.2), MIN_ZOOM);
  assert.equal(clampZoom(99), MAX_ZOOM);
  assert.equal(zoomAt(HOME, 99, 0, 0).k, MAX_ZOOM);
  assert.equal(zoomAt(HOME, 0.1, 0, 0).k, MIN_ZOOM);
});
test('unzoomed, the board cannot be moved at all', () => {
  const vp = viewportOf(box, sq);
  const v = clampView({ k: 1, x: 300, y: -200 }, box, vp);
  near(v.x, 0);
  near(v.y, 0);
});
test('unzoomed in a wider element it stays centred, and zoomed a little it still does while it fits', () => {
  const vp = viewportOf(box, wide); // 1600 wide: the box at 1.5x is 1500, which fits
  const v = clampView({ k: 1.5, x: 400, y: 0 }, box, vp);
  near(v.x, 0);
});
test('zoomed in, the board can be dragged but not far past its edge', () => {
  const vp = viewportOf(box, sq);
  const pad = 56;
  const far = clampView({ k: 2, x: 5000, y: -5000 }, box, vp);
  // left edge of the board (2 * -500 + x) may be at most pad inside the window's left edge (-500)
  near(far.x, -500 + pad + 1000);
  // dragged up as far as it goes: the board's bottom edge (2 * 400 + y) may be at most pad above the window's bottom edge (400)
  near(far.y, 400 - pad - 2 * 400);
  const mid = clampView({ k: 2, x: 100, y: 100 }, box, vp);
  near(mid.x, 100);
  near(mid.y, 100);
});
test('zooming out always ends back at home', () => {
  const vp = viewportOf(box, sq);
  let v = zoomAt(HOME, 3, 250, 120);
  v = clampView(v, box, vp);
  v = clampView(zoomAt(v, 1, 250, 120), box, vp);
  near(v.k, 1);
  near(v.x, 0);
  near(v.y, 0);
});
test('edges are reported at the limits', () => {
  assert.equal(edgeOf(1), 'min');
  assert.equal(edgeOf(MAX_ZOOM), 'max');
  assert.equal(edgeOf(2), 'mid');
});

console.log(`\n${passed} passed`);
