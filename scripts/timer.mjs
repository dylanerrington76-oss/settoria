// End-to-end timer check: the server must auto-roll after 10 seconds and the turn clock
// must not be charged for them. Takes ~12 seconds.
// Usage: node scripts/timer.mjs [port]   (server must be running with no game in progress)
const port = process.argv[2] ?? '3000';
const url = `ws://localhost:${port}/ws`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const assert = (c, m) => { if (!c) { console.error('FAIL:', m); process.exit(1); } };
const pick = (a) => a[Math.floor(Math.random() * a.length)];

class Client {
  constructor(name) { this.name = name; this.token = null; this.last = null; }
  connect() {
    return new Promise((resolve) => {
      this.ws = new WebSocket(url);
      this.ws.onopen = () => { this.ws.send(JSON.stringify({ type: 'join', name: this.name, token: this.token })); resolve(); };
      this.ws.onmessage = (e) => {
        const m = JSON.parse(e.data);
        if (m.type === 'welcome') this.token = m.token; else if (m.type !== 'error') this.last = m;
      };
    });
  }
  send(o) { this.ws.send(JSON.stringify(o)); }
  get view() { return this.last?.type === 'game' ? this.last.view : null; }
}

const clients = [new Client('Ann'), new Client('Ben')];
for (const c of clients) { await c.connect(); await sleep(60); }
clients[0].send({ type: 'set_timer', mode: 'very_fast' }); await sleep(80);
clients[0].send({ type: 'start' }); await sleep(120);
assert(clients[0].view, 'game started');

// play the setup quickly
while (clients[0].view.phase === 'setup') {
  await sleep(10);
  const cur = clients.find((c) => c.view && c.view.current === c.view.you);
  if (!cur) continue;
  const v = cur.view;
  if (v.setup_needs === 'road') cur.send({ type: 'build_road', edge: pick(v.legal.roads) });
  else cur.send({ type: 'build_settlement', vertex: pick(v.legal.settlements) });
}
await sleep(150);
let v = clients[0].view;
assert(v.phase === 'roll', 'setup finished, waiting for the roll: ' + v.phase);
assert(v.timer.kind === 'roll' && v.timer.total_ms === 10000, 'roll clock is 10s');
assert(v.timer.remaining_ms > 8500, 'roll clock just started');
assert(v.dice === null, 'not rolled yet');

// nobody rolls. After ~10s the server rolls for them.
const t = Date.now();
while (clients[0].view.phase === 'roll' && Date.now() - t < 14000) await sleep(100);
const waited = (Date.now() - t) / 1000;
v = clients[0].view;
assert(v.dice !== null && v.rolled, 'dice were rolled automatically');
assert(waited > 8.5 && waited < 12.5, `auto-roll came after ~10s (waited ${waited.toFixed(1)}s)`);
assert(v.log.some((e) => e.type === 'timed_out' && e.kind === 'roll'), 'log explains the auto-roll');
console.log(`auto-roll after ${waited.toFixed(1)}s -> phase ${v.phase}, clock ${v.timer?.kind} ${v.timer?.remaining_ms}ms of ${v.timer?.total_ms}ms`);

if (v.phase === 'main') {
  assert(v.timer.kind === 'turn' && v.timer.total_ms === 30000, 'turn clock is the Very Fast 30s');
  assert(v.timer.remaining_ms > 29000, 'the 10s roll window was not charged to the turn clock');
} else {
  assert(['discard', 'robber'].includes(v.timer.kind), 'after a 7 the discard/robber clocks run, not the turn clock');
}
console.log('TIMER OK');
process.exit(0);
