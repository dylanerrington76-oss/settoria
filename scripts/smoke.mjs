// End-to-end smoke test: 3 websocket clients play through setup and several turns.
// Usage: node scripts/smoke.mjs [port]   (server must be running)
const port = process.argv[2] ?? '3000';
const url = `ws://localhost:${port}/ws`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const assert = (c, m) => { if (!c) { console.error('FAIL:', m); process.exit(1); } };

class Client {
  constructor(name, color) { this.name = name; this.color = color; this.token = null; this.last = null; this.errors = []; }
  connect() {
    return new Promise((resolve) => {
      this.ws = new WebSocket(url);
      this.ws.onopen = () => {
        this.ws.send(JSON.stringify({ type: 'join', name: this.name, color: this.color, token: this.token }));
        resolve();
      };
      this.ws.onmessage = (e) => {
        const m = JSON.parse(e.data);
        if (m.type === 'welcome') this.token = m.token;
        else if (m.type === 'error') { this.errors.push(m.message); this.lastError = m; }
        else this.last = m;
      };
    });
  }
  send(o) { this.ws.send(JSON.stringify(o)); }
  get view() { return this.last?.type === 'game' ? this.last.view : null; }
}
const pick = (a) => a[Math.floor(Math.random() * a.length)];

// Ann and Ben pick colors; Cy sends none and gets the first free palette color (orange is taken).
const clients = [new Client('Ann', '#000000'), new Client('Ben', '#7bafd4'), new Client('Cy')];
for (const c of clients) { await c.connect(); await sleep(60); }
assert(clients[0].last.type === 'lobby' && clients[0].last.players.length === 3, 'lobby has 3 players');
assert(
  clients[0].last.players.map((p) => p.color).join() === '#000000,#7BAFD4,#FE5100',
  'lobby colors: chosen ones canonicalised, default is first free: ' + clients[0].last.players.map((p) => p.color),
);
const lobbyInfo = await (await fetch(`http://localhost:${port}/api/lobby`)).json();
assert(lobbyInfo.taken.length === 3 && lobbyInfo.taken.includes('#000000'), '/api/lobby lists taken colors');

// a taken or unknown color is refused, and the refused client is not seated
const dup = new Client('Dup', '#000000'); await dup.connect(); await sleep(60);
assert(dup.lastError?.code === 'color_taken', 'duplicate color refused with code color_taken');
const bad = new Client('Bad', '#123456'); await bad.connect(); await sleep(60);
assert(bad.errors.some((m) => /unknown color/i.test(m)), 'color outside the palette refused');
dup.ws.close(); bad.ws.close(); await sleep(100);
assert(clients[0].last.players.length === 3, 'refused clients did not take a seat');

// a 5th... a 4th joins then a 5th is refused
const d = new Client('Dee'); await d.connect(); await sleep(60);
const e = new Client('Eve'); await e.connect(); await sleep(60);
assert(e.errors.some((m) => /full/i.test(m)), 'fifth player refused');
d.ws.close(); e.ws.close(); await sleep(100);
assert(clients[0].last.players.length === 3, 'lobby back to 3 after leaves');

assert(clients[0].last.timer === 'normal', 'lobby defaults to the Normal timer');
clients[1].send({ type: 'set_timer', mode: 'slow' }); await sleep(100);
assert(clients[1].errors.some((m) => /host/i.test(m)), 'non-host cannot change the timer');
assert(clients[0].last.timer === 'normal', 'timer unchanged by a non-host');
clients[0].send({ type: 'set_timer', mode: 'very_fast' }); await sleep(100);
assert(clients.every((c) => c.last.timer === 'very_fast'), 'everyone sees the host\'s timer choice');
clients[0].send({ type: 'set_timer', mode: 'bogus' }); await sleep(100);
assert(clients[0].last.timer === 'very_fast', 'unknown timer mode ignored');

// ---- map -------------------------------------------------------------------------------------
assert(clients[0].last.map === 'standard', 'lobby defaults to the Standard map');
{
  const maps = clients[0].last.maps;
  assert(Array.isArray(maps) && maps.map((m) => m.id).join() === 'standard,earth', 'the lobby lists the maps the server has: ' + JSON.stringify(maps?.map((m) => m.id)));
  const [standard, earth] = maps;
  assert(standard.name === 'Standard' && standard.hexes === 19 && earth.name === 'Earth' && earth.hexes === 81, 'each map says its name and size');
  assert(maps.every((m) => m.builtin === true && m.players_min === 2 && m.players_max === 4 && !('recommended_settings' in m)), 'built-in maps: 2 to 4 players, no suggested rules');
  assert(clients.every((c) => c.last.maps.length === 2), 'everyone is told the same map list');
}
clients[1].send({ type: 'set_map', map: 'earth' }); await sleep(100);
assert(clients[1].errors.some((m) => /host/i.test(m)), 'non-host cannot change the map');
assert(clients[0].last.map === 'standard', 'map unchanged by a non-host');
clients[0].send({ type: 'set_map', map: 'atlantis' }); await sleep(100);
assert(clients[0].last.map === 'standard', 'unknown map ignored');
assert(clients[0].errors.some((m) => /no map called `atlantis`/i.test(m)), 'and the host is told there is no such map: ' + clients[0].errors.slice(-1));
clients[0].send({ type: 'set_map', map: 'Earth' }); await sleep(100);
assert(clients[0].last.map === 'standard', 'map ids are case-sensitive');
clients[0].send({ type: 'set_map', map: 'earth' }); await sleep(100);
assert(clients.every((c) => c.last.map === 'earth'), 'everyone sees the host\'s map choice');

// ---- game settings -------------------------------------------------------------------------
const STD = {
  points_to_win: 15, discard_limit: 7, friendly_robber: false, max_roads: 15, max_cities: 4, max_settlements: 5,
  dev_cards: 25, resource_max: 19, settlement_points: 1, city_points: 2, longest_road_points: 2,
  largest_army_points: 2, vp_card_points: 1, longest_road_min: 5, largest_army_min: 3,
  starting_settlements: 2, bank_ratio: 4,
};
assert(JSON.stringify(clients[0].last.settings) === JSON.stringify(STD), 'lobby defaults to the standard rules (15 points to win)');
const CUSTOM = {
  points_to_win: 12, discard_limit: 9, friendly_robber: true, max_roads: 12, max_cities: 3, max_settlements: 6,
  dev_cards: 20, resource_max: 15, settlement_points: 1, city_points: 3, longest_road_points: 2,
  largest_army_points: 3, vp_card_points: 2, longest_road_min: 4, largest_army_min: 2,
  starting_settlements: 3, bank_ratio: 3,
};
clients[1].send({ type: 'set_settings', settings: CUSTOM }); await sleep(100);
assert(clients[1].errors.some((m) => /host/i.test(m)), 'non-host cannot change the settings');
assert(clients[0].last.settings.points_to_win === 15, 'settings unchanged by a non-host');
clients[0].send({ type: 'set_settings', settings: { ...STD, points_to_win: 2 } }); await sleep(100);
assert(clients[0].errors.some((m) => /points to win/i.test(m)), 'out-of-range points to win refused');
clients[0].send({ type: 'set_settings', settings: { ...STD, starting_settlements: 3, max_roads: 2 } }); await sleep(100);
assert(clients[0].errors.some((m) => /road/i.test(m)), 'starting settlements without enough roads refused');
assert(clients[0].last.settings.starting_settlements === 2 && clients[0].last.settings.max_roads === 15, 'a refused change leaves the settings as they were');
clients[0].send({ type: 'set_settings', settings: CUSTOM }); await sleep(100);
assert(clients.every((c) => JSON.stringify(c.last.settings) === JSON.stringify(CUSTOM)), 'everyone sees the host\'s settings');
clients[0].send({ type: 'set_settings', settings: { ...CUSTOM, points_to_win: 'lots' } }); await sleep(100);
assert(clients[0].last.settings.points_to_win === 12, 'a malformed settings message is ignored');

clients[1].send({ type: 'start' }); await sleep(100);
assert(clients[1].errors.some((m) => /host/i.test(m)), 'non-host cannot start');
clients[0].send({ type: 'start' }); await sleep(150);
assert(clients[0].view, 'game started');
const first = clients[0].view;
assert(first.timer_mode === 'very_fast', 'game uses the chosen timer mode');
assert(first.timer.kind === 'setup_settlement' && first.timer.total_ms === 120000, 'setup starts with a 120s settlement clock');
assert([...first.order].sort().join() === '0,1,2' && first.current === first.order[0], 'order is a permutation and the first in it places first');
assert(first.log.some((e) => e.type === 'turn_order' && e.order.join() === first.order.join()), 'turn order is logged');
{
  // the Earth map: 81 hexes on 7 islands, 27 harbours, a fixed Sahara, two more deserts away from it
  const b = first.board;
  assert(b.map === 'earth' && !('kind' in b) && b.hexes.length === 81 && b.ports.length === 27, 'the game is played on the Earth map');
  assert(b.style && b.style.sea_grid === true, 'Earth tells the browser to draw the sea grid: ' + JSON.stringify(b.style));
  assert(first.log.some((e) => e.type === 'map_chosen' && e.map === 'earth' && e.name === 'Earth'), 'the map is logged, by id and by name');
  const at = (q, r) => b.hexes.find((h) => h.q === q && h.r === r);
  assert([[6, 5], [7, 5], [8, 5]].every(([q, r]) => at(q, r).terrain === 'desert'), 'the Sahara is desert');
  const deserts = b.hexes.filter((h) => h.terrain === 'desert');
  assert(deserts.length === 5 && deserts.every((h) => h.number === null), 'five deserts, none with a number');
  assert(b.hexes[first.robber].terrain === 'desert', 'the robber starts on a desert');
  assert(b.ports.filter((p) => p.resource === null).length === 12, 'twelve 3:1 harbours');
}
clients[0].send({ type: 'set_map', map: 'standard' }); await sleep(100);
assert(clients[0].errors.some((m) => /already started/i.test(m)) && clients[0].view.board.map === 'earth', 'the map is locked once the game starts');
assert(JSON.stringify(first.settings) === JSON.stringify(CUSTOM), 'the game uses the host\'s settings');
assert(first.log.some((e) => e.type === 'custom_rules' && e.changes.includes('points to win 12') && e.changes.includes('Friendly Robber on')), 'custom rules are logged');
assert(first.deck_left === 20 && first.bank.every((b) => b === 15), 'deck size and bank size follow the settings');
assert(first.players.every((p) => p.roads_left === 12 && p.settlements_left === 6 && p.cities_left === 3), 'piece limits follow the settings');
assert(first.ratios.every((r) => r === 3), 'bank trade ratio follows the settings');
clients[0].send({ type: 'set_settings', settings: STD }); await sleep(100);
assert(clients[0].errors.some((m) => /already started/i.test(m)) && clients[0].view.settings.points_to_win === 12, 'settings are locked once the game starts');

clients[0].send({ type: 'set_timer', mode: 'slow' }); await sleep(100);
assert(clients[0].errors.some((m) => /already started/i.test(m)), 'timer is locked once the game starts');
assert(clients[0].view.players.map((p) => p.color).join() === '#000000,#7BAFD4,#FE5100', 'game view carries each player\'s color');

let turns = 0, steps = 0;
while (turns < 25 && steps < 4000) {
  steps++;
  await sleep(8);
  const cur = clients.find((c) => c.view && c.view.current === c.view.you);
  if (!cur) continue; // broadcast still in flight
  const v = cur.view;
  switch (v.phase) {
    case 'setup':
      if (v.setup_needs === 'road') cur.send({ type: 'build_road', edge: pick(v.legal.roads) });
      else cur.send({ type: 'build_settlement', vertex: pick(v.legal.settlements) });
      break;
    case 'roll': cur.send({ type: 'roll' }); break;
    case 'discard':
      for (const c of clients) {
        const need = c.view.discards.find((x) => x[0] === c.view.you);
        if (need) {
          const cards = [0, 0, 0, 0, 0]; let left = need[1];
          while (left > 0) { const r = Math.floor(Math.random() * 5); if (c.view.hand[r] > cards[r]) { cards[r]++; left--; } }
          c.send({ type: 'discard', cards });
        }
      }
      break;
    case 'move_robber': cur.send({ type: 'move_robber', hex: pick(v.legal.hexes) }); break;
    case 'steal': cur.send({ type: 'steal', victim: pick(v.steal_candidates) }); break;
    case 'main': {
      if (v.legal.roads.length && v.hand[0] && v.hand[1]) cur.send({ type: 'build_road', edge: pick(v.legal.roads) });
      if (v.legal.settlements.length) cur.send({ type: 'build_settlement', vertex: pick(v.legal.settlements) });
      cur.send({ type: 'bank_trade', give: 0, get: 2 });
      cur.send({ type: 'end_turn' }); turns++;
      break;
    }
    default: break;
  }
}
await sleep(200);
{
  // 3 starting settlements each means 3 settlements and 3 roads per player were placed in setup
  const w = clients[0].view;
  const settled = (p) => 6 - w.players[p].settlements_left + (3 - w.players[p].cities_left);
  assert(w.players.every((p, i) => settled(i) >= 3 && 12 - p.roads_left >= 3), 'every player placed 3 starting settlements and 3 roads');
}
const v = clients[0].view;
assert(v && ['roll', 'main', 'game_over'].includes(v.phase), `ended in sane phase (${v?.phase})`);
assert(turns >= 25 || v.phase === 'game_over', 'played turns');
console.log('turns played:', turns, '| phase:', v.phase, '| log lines:', v.log.length);

// private info: hands of others are never sent
assert(!JSON.stringify(clients[0].last).includes('"hand":') || Array.isArray(v.hand), 'own hand present');
assert(v.players.every((p) => typeof p.cards === 'number'), 'others only expose card counts');

// reconnect: drop Ben and rejoin with token
const tok = clients[1].token; clients[1].ws.close(); await sleep(150);
assert(clients[0].view.players[1].connected === false, 'disconnect visible to others');
const ben2 = new Client('Ben'); ben2.token = tok; await ben2.connect(); await sleep(150);
assert(ben2.view && ben2.view.you === 1, 'reconnect restores seat');
assert(clients[0].view.players[1].connected === true, 'reconnect visible to others');

// a stranger cannot join a running game
const x = new Client('Mallory'); await x.connect(); await sleep(100);
assert(x.errors.some((m) => /progress/i.test(m)), 'stranger refused mid-game');
console.log('SMOKE OK');
process.exit(0);
