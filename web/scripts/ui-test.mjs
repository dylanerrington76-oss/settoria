// Renders real components to HTML in Node (no browser) and checks what the person would read.
//
//   npm test
//
// The components are bundled with Vite's server-side build, so this uses nothing that isn't already a
// dependency of the app. It checks the text and the enabled/disabled state of what is rendered; clicking is
// covered by the server's tests of the messages those clicks send (src/main.rs).
import { build } from 'vite';
import react from '@vitejs/plugin-react';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import assert from 'node:assert/strict';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

// Bundle the entry. Dependencies such as react stay external and are read from node_modules.
const result = await build({
  root,
  configFile: false,
  logLevel: 'silent',
  plugins: [react()],
  build: { ssr: 'scripts/test-entry.tsx', write: false, rollupOptions: { output: { format: 'es' } } },
});
const code = (Array.isArray(result) ? result[0] : result).output[0].code;
// It has to live inside the project for `import 'react'` to be found.
const dir = resolve(root, 'node_modules/.cache/ui-test');
mkdirSync(dir, { recursive: true });
const file = resolve(dir, 'bundle.mjs');
writeFileSync(file, code);

const { createElement } = await import('react');
const { renderToStaticMarkup } = await import('react-dom/server');
const ui = await import(pathToFileURL(file).href);
const { Lobby, GameScreen, Board, Log, settingsDifferences, DEFAULT_SETTINGS, fixture } = ui;

globalThis.location = { host: 'localhost:3000', search: '' }; // the lobby tells friends where to join

// ---------------------------------------------------------------- the lobby

const info = (id, name, min, max, extra = {}) => ({ id, name, author: '', hexes: 19, players_min: min, players_max: max, builtin: true, ...extra });
const quickRules = { ...DEFAULT_SETTINGS, points_to_win: 8, friendly_robber: true };
const MAPS = [
  info('standard', 'Standard', 2, 4),
  info('earth', 'Earth', 2, 4, { hexes: 81 }),
  info('quick', 'Quick Game', 2, 4, { builtin: false, recommended_settings: quickRules }),
  info('trio', 'Trio', 3, 4, { builtin: false }),
  info('duo', 'Duo', 2, 2, { builtin: false }),
];
const people = (n) => ['Ada', 'Bo', 'Cy', 'Di'].slice(0, n).map((name, i) => ({ name, color: ['#FE5100', '#613393', '#FFB81C', '#69BE28'][i], connected: true }));

function lobby(over = {}) {
  const sent = [];
  const html = renderToStaticMarkup(
    createElement(Lobby, {
      you: 0,
      players: people(3),
      timer: 'normal',
      settings: DEFAULT_SETTINGS,
      map: 'standard',
      maps: MAPS,
      send: (m) => sent.push(m),
      onLeave: () => {},
      ...over,
    }),
  );
  const start = /<button[^>]*class="primary"[^>]*>(.*?)<\/button>/.exec(html);
  return { html, sent, startTag: start?.[0] ?? '', startText: start?.[1] ?? '', startDisabled: /<button[^>]*class="primary"[^>]*disabled/.test(html) };
}

test('a map with no suggestion shows no suggestion', () => {
  for (const map of ['standard', 'earth', 'trio', 'duo']) {
    const { html } = lobby({ map, players: people(3) });
    assert.ok(!html.includes('suggests different rules'), map);
    assert.ok(!html.includes('Apply suggested rules'), map);
  }
});

test('the host is offered a map\'s suggested rules, and told what they would change', () => {
  const { html } = lobby({ map: 'quick' });
  assert.match(html, /<b>Quick Game<\/b> suggests different rules: /);
  const changes = settingsDifferences(DEFAULT_SETTINGS, quickRules);
  assert.equal(changes.length, 2);
  for (const c of changes) assert.ok(html.includes(c), `the notice should say "${c}"`);
  assert.match(html, /Points to win 8/i);
  assert.ok(html.includes('Apply suggested rules'));
  assert.ok(!html.includes('The host can apply them'));
});

test('a guest is told the host can apply them, and gets no button', () => {
  const { html } = lobby({ map: 'quick', you: 1 });
  assert.match(html, /suggests different rules/);
  assert.ok(html.includes('The host can apply them.'));
  assert.ok(!html.includes('Apply suggested rules'));
});

test('nothing is offered once the rules already match the map\'s', () => {
  const { html } = lobby({ map: 'quick', settings: quickRules });
  assert.ok(!html.includes('suggests different rules'));
});

test('only the rules that differ are listed', () => {
  const { html } = lobby({ map: 'quick', settings: { ...DEFAULT_SETTINGS, points_to_win: 8 } });
  const changes = settingsDifferences({ ...DEFAULT_SETTINGS, points_to_win: 8 }, quickRules);
  assert.equal(changes.length, 1);
  assert.ok(html.includes(changes[0]));
  assert.ok(!/Points to win 8/i.test(html.replace(/<[^>]*>/g, ' ').split('suggests different rules:')[1].split('.')[0]), 'points to win already matches');
});

test('rendering the lobby sends nothing (suggested rules are never applied on their own)', () => {
  for (const map of ['quick', 'standard']) assert.deepEqual(lobby({ map }).sent, []);
});

test('Start is for the number of players the map is meant for', () => {
  const cases = [
    { map: 'standard', n: 3, ok: true, text: 'Start with 3 players' },
    { map: 'trio', n: 2, ok: false, text: 'Trio is for 3 to 4 players' },
    { map: 'trio', n: 3, ok: true, text: 'Start with 3 players' },
    { map: 'trio', n: 4, ok: true, text: 'Start with 4 players' },
    { map: 'duo', n: 3, ok: false, text: 'Duo is for 2 players' },
    { map: 'duo', n: 2, ok: true, text: 'Start with 2 players' },
    { map: 'standard', n: 1, ok: false, text: 'Need at least 2 players' },
    { map: 'trio', n: 1, ok: false, text: 'Need at least 2 players' }, // too few overall comes first
  ];
  for (const { map, n, ok, text } of cases) {
    const r = lobby({ map, players: people(n) });
    assert.equal(r.startText, text, `${map} with ${n}`);
    assert.equal(r.startDisabled, !ok, `${map} with ${n}: ${r.startTag}`);
  }
});

test('a guest sees no Start button at all', () => {
  const { html } = lobby({ you: 1, map: 'trio', players: people(2) });
  assert.ok(!/class="primary"/.test(html));
  assert.ok(html.includes('Waiting for Ada to start'));
});

test('a map the server did not list does not break the lobby', () => {
  for (const maps of [[], MAPS.slice(0, 2)]) {
    const r = lobby({ map: 'mystery', maps });
    assert.equal(r.startText, 'Start with 3 players');
    assert.ok(!r.startDisabled);
    assert.ok(!r.html.includes('suggests different rules'));
  }
});

test('the picker lists every map the server sent, with its hex count, and marks the chosen one', () => {
  const { html } = lobby({ map: 'earth' });
  // (React writes `checked` before `value`.)
  assert.match(html, /<input type="radio" name="map" checked="" value="earth"/);
  for (const m of MAPS) {
    assert.match(html, new RegExp(`<input type="radio" name="map"( checked="")? value="${m.id}"/>`), `${m.id} is offered`);
    assert.ok(html.includes(`<span class="seg-name">${m.name}</span>`), `${m.id} is shown by name`);
  }
  assert.match(html, /<span class="seg-name">Earth<\/span><span class="seg-secs">81 hexes<\/span>/);
  assert.match(html, /<span class="seg-name">Standard<\/span><span class="seg-secs">19 hexes<\/span>/);
  assert.equal((html.match(/name="map"/g) ?? []).length, MAPS.length, 'one radio per listed map, no more');
});

test('the picker is built from what the server sent, not from a list of its own', () => {
  const only = [info('standard', 'Standard', 2, 4), info('atlantis', 'Atlantis', 2, 4, { hexes: 37, author: 'Ada' })];
  const { html } = lobby({ maps: only, map: 'atlantis' });
  assert.equal((html.match(/name="map"/g) ?? []).length, 2);
  assert.ok(!html.includes('value="earth"'));
  assert.match(html, /<input type="radio" name="map" checked="" value="atlantis"/);
  assert.ok(html.includes('37 hexes') && html.includes('title="by Ada"'));
  assert.deepEqual(lobby({ maps: [], map: 'standard' }).sent, []);
});

test('maps for a different number of players are greyed out, and say who they are for', () => {
  const grey = (html) => [...html.matchAll(/<label class="segment[^"]*?( unfit)"[^>]*>.*?name="map"[^>]*? value="([^"]+)"/g)].map((m) => m[2]);
  assert.deepEqual(grey(lobby({ players: people(3) }).html), ['duo'], 'three seated: only the 2-player map is out');
  assert.deepEqual(grey(lobby({ players: people(2) }).html), ['trio'], 'two seated: only the 3 to 4 player map is out');
  assert.deepEqual(grey(lobby({ players: people(4) }).html), ['duo']);
  assert.deepEqual(grey(lobby({ players: people(1) }).html), ['trio'], 'a lone host counts as two seats');
  const { html } = lobby({ players: people(2) });
  assert.match(html, /<span class="seg-secs seg-note">3 to 4 players<\/span>/);
  assert.ok(html.includes('title="Trio is for 3 to 4 players"'));
  assert.ok(!html.includes('<span class="seg-secs seg-note">2 players'), 'a map that fits says nothing extra');
  // greyed out is not disabled: the host can still pick it, and Start explains what is wrong
  assert.ok(!/<input type="radio" name="map"[^>]*disabled/.test(lobby({ players: people(2) }).html));
  // the chosen map is never dimmed, even if it doesn't fit
  assert.match(lobby({ map: 'trio', players: people(2) }).html, /class="segment on unfit"/, 'marked, so the CSS can keep it readable');
});

// ---------------------------------------------------------------- the board

test('the board is drawn the same way for every map: the sea grid comes from the map\'s style, not its name', () => {
  const draw = (board) => {
    const v = fixture('main').view;
    return renderToStaticMarkup(createElement(Board, { view: { ...v, board }, vertexTargets: [], edgeTargets: [], hexTargets: [], onVertex() {}, onEdge() {}, onHex() {} }));
  };
  const base = fixture('main').view.board;
  const SEA = 'stroke="#3f86c4"';
  assert.ok(!draw(base).includes(SEA), 'Standard: no sea grid');
  assert.ok(draw({ ...base, style: { sea_grid: true } }).includes(SEA), 'asked for in the style');
  // the map's id means nothing to the renderer
  assert.ok(!draw({ ...base, map: 'earth' }).includes(SEA), 'a board called earth with no style has no grid');
  assert.ok(draw({ ...base, map: 'my-map', style: { sea_grid: true } }).includes(SEA), 'and a board with any name can have one');
});

// ---------------------------------------------------------------- what a change to the rules says

test('every rule can be described, and only that rule is', () => {
  for (const [key, value] of Object.entries(DEFAULT_SETTINGS)) {
    const changed = { ...DEFAULT_SETTINGS, [key]: typeof value === 'boolean' ? !value : value + 1 };
    const said = settingsDifferences(DEFAULT_SETTINGS, changed);
    assert.equal(said.length, 1, `${key}: ${JSON.stringify(said)}`);
  }
  assert.deepEqual(settingsDifferences(DEFAULT_SETTINGS, DEFAULT_SETTINGS), []);
});

test('rules are described in the order the rules panel lists them', () => {
  const said = settingsDifferences(DEFAULT_SETTINGS, { ...DEFAULT_SETTINGS, friendly_robber: true, points_to_win: 9 });
  assert.equal(said.length, 2);
  assert.match(said[0], /Points to win/i);
  assert.match(said[1], /Friendly robber/i);
});

test('bridges are a toggle that is off in the standard rules', () => {
  assert.equal(DEFAULT_SETTINGS.bridges, false);
  assert.match(settingsDifferences(DEFAULT_SETTINGS, { ...DEFAULT_SETTINGS, bridges: true })[0], /Bridges on$/);
});

test('a toggle says on or off, and a number says its value', () => {
  assert.match(settingsDifferences(DEFAULT_SETTINGS, { ...DEFAULT_SETTINGS, friendly_robber: true })[0], /on$/);
  assert.match(settingsDifferences({ ...DEFAULT_SETTINGS, friendly_robber: true }, DEFAULT_SETTINGS)[0], /off$/);
  assert.match(settingsDifferences(DEFAULT_SETTINGS, { ...DEFAULT_SETTINGS, max_roads: 20 })[0], /20/);
});

// ---------------------------------------------------------------- the game log

test('the log says which map the game is on, by the name the server sends', () => {
  const { view } = fixture('roll');
  const text = (event) => renderToStaticMarkup(createElement(Log, { lines: [event], v: view })).replace(/<[^>]*>/g, '');
  assert.match(text({ type: 'map_chosen', map: 'earth', name: 'Earth' }), /Map: Earth/);
  assert.match(text({ type: 'map_chosen', map: 'user:arch', name: 'Archipelago' }), /Map: Archipelago/);
});

// ---------------------------------------------------------------- volcanoes

/** The mid-game fixture with its first hex turned into a volcano numbered 8, and a volcano pick under way. */
function volcanoView(over = {}) {
  const v = fixture('main').view;
  const hexes = v.board.hexes.map((h, i) => (i === 0 ? { ...h, terrain: 'volcano', number: 8 } : h));
  return {
    ...v,
    board: { ...v.board, hexes },
    phase: 'volcano_pick',
    volcano: { hex: 0, player: v.you, left: 2 },
    timer: { kind: 'volcano', player: v.you, remaining_ms: 12_000, total_ms: 15_000 },
    ...over,
  };
}
const drawBoard = (view) =>
  renderToStaticMarkup(createElement(Board, { view, vertexTargets: [], edgeTargets: [], hexTargets: [], onVertex() {}, onEdge() {}, onHex() {} }));
const drawGame = (view) => renderToStaticMarkup(createElement(GameScreen, { v: view, send() {} }));
const plain = (html) => html.replace(/<[^>]*>/g, '');

test('a volcano hex is drawn with its six corners numbered 1 to 6, and only a volcano is', () => {
  const html = drawBoard(volcanoView());
  assert.equal((html.match(/class="volcano-corner"/g) ?? []).length, 6);
  for (const n of [1, 2, 3, 4, 5, 6]) assert.ok(html.includes(`>${n}</text>`), `corner ${n}`);
  assert.equal((drawBoard(fixture('main').view).match(/class="volcano-corner"/g) ?? []).length, 0);
});

test('the player whose pick it is gets the five resources, and the ones the bank has run out of are disabled', () => {
  const v = volcanoView({ bank: [0, 4, 4, 4, 4] });
  const html = drawGame(v);
  assert.match(plain(html), /The volcano erupts! Pick a free resource/);
  assert.match(plain(html), /2 cards left to pick/);
  const bar = html.slice(html.indexOf('class="volcano-bar"'));
  const buttons = bar.match(/<button[^>]*res-btn[^>]*>/g) ?? [];
  assert.equal(buttons.length, 5);
  assert.equal(buttons.filter((b) => b.includes('disabled')).length, 1, 'only brick is gone');
});

test('everyone else just sees who is choosing', () => {
  const v = volcanoView({ volcano: { hex: 0, player: 1, left: 1 } });
  const html = drawGame(v);
  assert.match(plain(html), new RegExp(`${v.players[1].name} is picking a free resource from the volcano`));
  assert.equal(((html.slice(html.indexOf('class="volcano-bar"'))).match(/res-btn/g) ?? []).length, 0, 'no buttons for them');
});

test('the volcano bar is only there while a pick is going on', () => {
  assert.ok(!drawGame(fixture('main').view).includes('class="volcano-bar"'));
});

test('the last lava flow is marked on the board, and a settlement on loan has a crack', () => {
  const v = volcanoView({ phase: 'main', volcano: null, lava: { hex: 0, die: 3, vertex: 4 }, damaged: [5] });
  const html = drawBoard(v);
  assert.ok(html.includes('class="lava"'));
  assert.ok(html.includes('Lava flowed down corner 3'));
  assert.ok(html.includes('class="damaged"'));
  const quiet = drawBoard(volcanoView({ phase: 'main', volcano: null }));
  assert.ok(!quiet.includes('class="lava"') && !quiet.includes('class="damaged"'));
});

test('the log tells what the volcano and the lava did', () => {
  const v = volcanoView({ phase: 'main', volcano: null });
  const text = (event) => plain(renderToStaticMarkup(createElement(Log, { lines: [event], v })));
  assert.match(text({ type: 'volcano_picked', player: 0, resource: 2 }), /took a free .* from the volcano/);
  assert.match(text({ type: 'volcano_blocked', hex: 0 }), /robber is on the volcano/);
  assert.match(text({ type: 'volcano_bank_empty' }), /bank is out of cards/);
  assert.match(text({ type: 'lava_flow', hex: 0, die: 4, effect: 'nothing', owner: null }), /corner 4, but nothing was there/);
  assert.match(text({ type: 'lava_flow', hex: 0, die: 2, effect: 'destroyed', owner: 1 }), /corner 2: .* settlement was destroyed/);
  assert.match(text({ type: 'lava_flow', hex: 0, die: 6, effect: 'demoted', owner: 1 }), /corner 6: .* city was reduced to a settlement/);
  assert.match(text({ type: 'timed_out', player: 1, kind: 'volcano' }), /free resources were picked for them at random/);
});
