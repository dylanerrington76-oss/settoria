// Dev-only harness: renders the game screen from fixture states, no server needed.
// Open /mock.html?s=setup|roll|discard|main|steal  (used by scripts/shoot.mjs).
// ?s=lobby shows the lobby with the game-rules panel (?host=0 to view it as a guest;
// edits work locally, as the server would echo them back).
import '@fontsource/open-sans/latin-400.css';
import '@fontsource/open-sans/latin-600.css';
import '@fontsource/open-sans/latin-700.css';
import '@fontsource/open-sans/latin-800.css';
import '../style.css';
import { createRoot } from 'react-dom/client';
import { GameScreen, Lobby } from '../App';
import { DEFAULT_SETTINGS, PALETTE, type MapInfo, type Settings } from '../types';
import { useState } from 'react';
import { fixture } from './fixtures';

const q = new URLSearchParams(location.search);
const name = q.get('s') ?? 'roll';
const clock = q.get('t');

// What the server's lobby message lists; the picker is built from it. Any of these can be chosen to start with via
// ?map=<id> (?map=earth, say): `quick` suggests its own rules (?map=quick), `trio` is only for 3 or 4 players
// (?map=trio&n=2 seats two, which greys it out). An id that isn't listed falls back to `standard`.
const MAPS_LISTED: MapInfo[] = [
  { id: 'standard', name: 'Standard', author: '', hexes: 19, players_min: 2, players_max: 4, builtin: true },
  { id: 'earth', name: 'Earth', author: '', hexes: 81, players_min: 2, players_max: 4, builtin: true },
  {
    id: 'quick',
    name: 'Quick Game',
    author: 'Ada',
    hexes: 19,
    players_min: 2,
    players_max: 4,
    builtin: false,
    recommended_settings: { ...DEFAULT_SETTINGS, points_to_win: 8, friendly_robber: true },
  },
  { id: 'trio', name: 'Trio', author: '', hexes: 12, players_min: 3, players_max: 4, builtin: false },
];

function MockLobby({ host }: { host: boolean }) {
  const [settings, setSettings] = useState<Settings>({ ...DEFAULT_SETTINGS, ...(q.get('custom') ? { points_to_win: 12, friendly_robber: true, max_roads: 20 } : {}) });
  const [map, setMap] = useState<string>(MAPS_LISTED.some((m) => m.id === q.get('map')) ? q.get('map')! : 'standard');
  const seated = Math.min(4, Math.max(1, Number(q.get('n')) || 3));
  const players = [0, 1, 2, 3].slice(0, seated).map((i) => ({ name: ['Ada', 'Bo', 'Cy', 'Di'][i], color: PALETTE[[5, 4, 3, 2][i]].hex, connected: true }));
  return (
    <Lobby
      you={host ? 0 : 1}
      players={players}
      timer="normal"
      map={map}
      maps={MAPS_LISTED}
      settings={settings}
      onLeave={() => {}}
      send={(m) => {
        console.log('send', JSON.stringify(m));
        const msg = m as { type: string; settings?: Settings; map?: string };
        if (msg.type === 'set_settings' && msg.settings) setSettings(msg.settings);
        if (msg.type === 'set_map' && msg.map) setMap(msg.map);
      }}
    />
  );
}

if (name === 'lobby') {
  createRoot(document.getElementById('root')!).render(<MockLobby host={q.get('host') !== '0'} />);
} else {
  const { view, clock: c } = fixture(name);
  // ?c=7,9,8,2 gives the four players those palette colors (to eyeball light/dark ones)
  q.get('c')?.split(',').forEach((idx, i) => {
    if (view.players[i] && PALETTE[+idx]) view.players[i].color = PALETTE[+idx].hex;
  });
  createRoot(document.getElementById('root')!).render(
    <GameScreen v={view} send={(m) => console.log('send', JSON.stringify(m))} clockOverride={clock ? +clock : c} />,
  );
}
