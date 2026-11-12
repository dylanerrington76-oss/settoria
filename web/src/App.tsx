import { DragEvent, FormEvent, ReactNode, useEffect, useMemo, useRef, useState } from 'react';
import { ActionIcon, Avatar, Chevron, DieFace, ToolIcon } from './art';
import Board from './Board';
import Changelog from './Changelog';
import Footer from './Footer';
import MapEditor from './MapEditor';
import { MAX_FILES_AT_ONCE, MAX_MAP_BYTES, UploadResult, uploadMapFile } from './mapUpload';
import { GameSettingsPanel, settingsDifferences } from './GameSettings';
import {
  BankRow,
  Chip,
  Hand,
  InfoDialog,
  Log,
  Modal,
  PickResourceDialog,
  PlayerRow,
  DiscardPanel,
  RulesDialog,
  SettingsDialog,
  TradeBanners,
  TradeModal,
} from './ui';
import { useRoute } from './route';
import { useGame } from './useGame';
import {
  colorName,
  COST_BRIDGE,
  COST_CITY,
  COST_DEV,
  COST_ROAD,
  COST_SETTLEMENT,
  covers,
  DEFAULT_SETTINGS,
  GameView,
  LobbyPlayer,
  CLOCK_LABELS,
  CLOCK_TITLES,
  luminance,
  MapInfo,
  MAX_PLAYERS,
  MIN_PLAYERS,
  PALETTE,
  Res,
  Settings,
  TIMER_MODES,
  TimerMode,
  TimerView,
  total,
} from './types';

type Send = (msg: object) => void;
type BuildMode = 'road' | 'bridge' | 'settlement' | 'city' | null;
type Dialog = null | 'trade' | 'plenty' | 'monopoly' | 'rules' | 'info' | 'settings';

export default function App() {
  const { screen, error, online, name, send, join, leave } = useGame();
  const { changelog, editor, go } = useRoute();

  // The game connection above stays open while the changelog is showing, so going back finds the lobby as it was.
  if (changelog) return <Changelog onBack={() => go('app')} />;
  if (editor) return <MapEditor onBack={() => go('app')} />;

  let body;
  if (screen.kind === 'game') {
    body = <GameScreen v={screen.view} send={send} />;
  } else if (screen.kind === 'lobby') {
    body = <Lobby you={screen.you} players={screen.players} timer={screen.timer} settings={screen.settings} map={screen.map} maps={screen.maps} send={send} onLeave={leave} />;
  } else if (!name) {
    body = <JoinScreen onJoin={join} />;
  } else {
    body = (
      <div className="centered">
        <div className="panel narrow">
          <h1>Settoria</h1>
          <p className="muted">{online ? 'Joining the table…' : 'Connecting to the server…'}</p>
          <button onClick={leave}>Back</button>
        </div>
      </div>
    );
  }

  return (
    <>
      {screen.kind === 'game' ? (
        body
      ) : (
        <div className="entry">
          {body}
          <Footer onChangelog={() => go('changelog')} onMapEditor={() => go('editor')} />
        </div>
      )}
      {error && (
        <div className="toast" role="alert">
          {error}
        </div>
      )}
      {name && !online && screen.kind !== 'idle' && (
        <div className="offline" role="alert">
          Connection lost. Reconnecting…
        </div>
      )}
    </>
  );
}

/** Colors already taken at the table, polled while the join screen is open (the socket isn't open yet). */
function useTakenColors(): Set<string> {
  const [taken, setTaken] = useState<Set<string>>(new Set());
  useEffect(() => {
    let alive = true;
    const poll = () =>
      fetch('/api/lobby', { cache: 'no-store' })
        .then((r) => r.json())
        .then((j: { taken: string[] }) => alive && setTaken(new Set(j.taken.map((c) => c.toLowerCase()))))
        .catch(() => {});
    poll();
    const t = window.setInterval(poll, 2000);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, []);
  return taken;
}

function JoinScreen({ onJoin }: { onJoin: (n: string, color: string) => void }) {
  const [n, setN] = useState(() => sessionStorage.getItem('catan-prefill') ?? '');
  const [pick, setPick] = useState(() => sessionStorage.getItem('catan-color') ?? PALETTE[0].hex);
  const taken = useTakenColors();
  // If the picked color is (or becomes) taken, fall back to the first free one.
  const chosen = taken.has(pick.toLowerCase()) ? PALETTE.find((c) => !taken.has(c.hex.toLowerCase()))?.hex : pick;
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (chosen) onJoin(n.trim() || 'Player', chosen);
  };
  return (
    <div className="centered">
      <form className="panel narrow" onSubmit={submit}>
        <h1>Settoria</h1>
        <label htmlFor="name">Your name</label>
        <input id="name" autoFocus maxLength={16} value={n} onChange={(e) => setN(e.target.value)} placeholder="" />
        <fieldset className="colors">
          <legend>Your color</legend>
          <div className="swatches">
            {PALETTE.map((c) => {
              const isTaken = taken.has(c.hex.toLowerCase());
              return (
                <label key={c.hex} className={'swatch' + (isTaken ? ' taken' : '')} title={isTaken ? `${c.name} (taken)` : c.name}>
                  <input
                    type="radio"
                    name="color"
                    value={c.hex}
                    checked={chosen === c.hex}
                    disabled={isTaken}
                    aria-label={isTaken ? `${c.name}, taken` : c.name}
                    onChange={() => setPick(c.hex)}
                  />
                  <span className="swatch-dot" style={{ background: c.hex, color: luminance(c.hex) > 0.35 ? '#222' : '#fff' }}>
                    <svg viewBox="0 0 20 20" aria-hidden="true">
                      <path d="M4.5 10.5 8.5 14.5 15.5 6" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" />
                    </svg>
                  </span>
                </label>
              );
            })}
          </div>
          <div className="color-name" aria-live="polite">
            {chosen ? colorName(chosen) : 'No colors left'}
          </div>
        </fieldset>
        <button className="primary" type="submit" disabled={!chosen}>
          Take a seat
        </button>
      </form>
    </div>
  );
}

/** Who a map is meant for, as the Start button words it: "2", or "3 to 4". */
const playersText = (m: MapInfo): string => (m.players_min === m.players_max ? `${m.players_min}` : `${m.players_min} to ${m.players_max}`);

/** How many of a map's problems are written out before "and N more". */
const SHOWN_ISSUES = 6;

/** What came of the files the host just chose: each one is added (with any warnings) or refused (with every problem). */
function UploadReport({ results, onClose }: { results: UploadResult[]; onClose: () => void }) {
  return (
    <div className="upload-report" role="status">
      <ul>
        {results.map((r, i) =>
          r.ok ? (
            <li key={i}>
              <span aria-hidden="true">✓ </span>
              <b>{r.name}</b> added.
              {r.warnings.length > 0 && (
                <ul className="note">
                  {r.warnings.map((w, j) => (
                    <li key={j}>{w}</li>
                  ))}
                </ul>
              )}
            </li>
          ) : (
            <li key={i} className="bad">
              <span aria-hidden="true">✗ </span>
              <b>{r.file}</b> was not added:
              <ul className="note">
                {r.issues.slice(0, SHOWN_ISSUES).map((w, j) => (
                  <li key={j}>{w}</li>
                ))}
                {r.issues.length > SHOWN_ISSUES && <li>…and {r.issues.length - SHOWN_ISSUES} more.</li>}
              </ul>
            </li>
          ),
        )}
      </ul>
      <button type="button" className="small" onClick={onClose}>
        Dismiss
      </button>
    </div>
  );
}

export function Lobby({
  you,
  players,
  timer,
  settings,
  map,
  maps,
  send,
  onLeave,
}: {
  you: number;
  players: LobbyPlayer[];
  timer: TimerMode;
  settings: Settings;
  map: string;
  maps: MapInfo[];
  send: Send;
  onLeave: () => void;
}) {
  const host = you === 0;
  // Changes are sent as the whole settings object. Keep the latest one in a ref so two quick
  // changes made before the server answers don't overwrite each other.
  const latest = useRef(settings);
  useEffect(() => {
    latest.current = settings;
  }, [settings]);
  const change = (patch: Partial<Settings>) => {
    latest.current = { ...latest.current, ...patch };
    send({ type: 'set_settings', settings: latest.current });
  };
  const reset = () => {
    latest.current = DEFAULT_SETTINGS;
    send({ type: 'set_settings', settings: DEFAULT_SETTINGS });
  };
  // What the server says about the chosen map: the players it is meant for, and the rules it suggests.
  const info = maps.find((m) => m.id === map);
  const suggested = info?.recommended_settings;
  const suggestion = suggested ? settingsDifferences(settings, suggested) : [];
  // Suggested rules are only ever offered. Applying them is the host's choice, made the ordinary way.
  const applySuggested = () => {
    if (!suggested) return;
    latest.current = suggested;
    send({ type: 'set_settings', settings: suggested });
  };
  const wrongCount = !!info && (players.length < info.players_min || players.length > info.players_max);
  const range = info ? playersText(info) : '';
  // Maps for another number of players are greyed out, not disabled: the host may want to pick one while the
  // table fills up, and Start already says what is wrong. A lone host counts as two (the least a game takes),
  // so the picker isn't grey from end to end before the first guest arrives.
  const seated = Math.max(players.length, MIN_PLAYERS);
  const fits = (m: MapInfo) => seated >= m.players_min && seated <= m.players_max;

  // Maps from the host's own files. The server checks them and adds the good ones to everyone's list; the host
  // then has the last one added selected, so a map they upload is one click from being played.
  const [uploading, setUploading] = useState(false);
  const [report, setReport] = useState<UploadResult[]>([]);
  const [dragging, setDragging] = useState(false);
  const picker = useRef<HTMLInputElement>(null);
  const upload = async (picked: File[]) => {
    if (!host || uploading || picked.length === 0) return;
    setUploading(true);
    setReport([]);
    try {
      const token = sessionStorage.getItem('catan-token');
      const results: UploadResult[] = [];
      for (const file of picked.slice(0, MAX_FILES_AT_ONCE)) results.push(await uploadMapFile(file, token));
      if (picked.length > MAX_FILES_AT_ONCE) {
        results.push({ ok: false, file: `${picked.length - MAX_FILES_AT_ONCE} more files`, issues: [`Choose at most ${MAX_FILES_AT_ONCE} files at a time.`] });
      }
      const added = results.filter((r): r is Extract<UploadResult, { ok: true }> => r.ok);
      if (added.length > 0) send({ type: 'set_map', map: added[added.length - 1].id });
      setReport(results);
    } finally {
      setUploading(false);
    }
  };
  // Only a drag that carries files is taken, so dragging text around the page does nothing here.
  const carriesFiles = (e: DragEvent) => Array.from(e.dataTransfer.types).includes('Files');
  return (
    <div className="centered">
      <div className="panel lobby">
        <div className="lobby-main">
          <h1>Waiting for players</h1>
          <ul className="seats">
            {Array.from({ length: MAX_PLAYERS }, (_, i) => i).map((i) => {
              const p = players[i];
              return (
                <li key={i} className={p ? 'seat taken' : 'seat'}>
                  <Chip color={p?.color} name={p ? colorName(p.color) : undefined} />
                  {p ? (
                    <span>
                      {p.name}
                      {i === you && <em> (you)</em>}
                      {i === 0 && <small className="muted"> host</small>}
                    </span>
                  ) : (
                    <span className="muted">Open seat</span>
                  )}
                </li>
              );
            })}
          </ul>
          <p className="muted">
            Friends join by opening <b>{location.host}</b> in their browser.
          </p>
          <fieldset
            className={'timer-modes' + (dragging ? ' dropping' : '')}
            disabled={!host}
            onDragOver={
              host
                ? (e) => {
                    if (!carriesFiles(e)) return;
                    e.preventDefault();
                    setDragging(true);
                  }
                : undefined
            }
            onDragLeave={host ? (e) => !e.currentTarget.contains(e.relatedTarget as Node | null) && setDragging(false) : undefined}
            onDrop={
              host
                ? (e) => {
                    if (!carriesFiles(e)) return;
                    e.preventDefault();
                    setDragging(false);
                    void upload(Array.from(e.dataTransfer.files));
                  }
                : undefined
            }
          >
            <legend>Map</legend>
            <div className="segments maps" role="radiogroup" aria-label="Map">
              {maps.map((m) => {
                const unfit = !fits(m);
                const title = [m.author && `by ${m.author}`, unfit && `${m.name} is for ${playersText(m)} players`].filter(Boolean).join(', ');
                return (
                  <label key={m.id} className={'segment' + (map === m.id ? ' on' : '') + (unfit ? ' unfit' : '')} title={title || undefined}>
                    <input type="radio" name="map" value={m.id} checked={map === m.id} onChange={() => send({ type: 'set_map', map: m.id })} />
                    <span className="seg-name">{m.name}</span>
                    <span className="seg-secs">{m.hexes === 1 ? '1 hex' : `${m.hexes} hexes`}</span>
                    {unfit && <span className="seg-secs seg-note">{playersText(m)} players</span>}
                  </label>
                );
              })}
            </div>
            {host && (
              <div className="map-upload">
                <input
                  ref={picker}
                  type="file"
                  accept=".json,application/json"
                  multiple
                  hidden
                  aria-label="Map file"
                  onChange={(e) => {
                    const files = Array.from(e.target.files ?? []);
                    e.target.value = ''; // so choosing the same file again (after fixing it) still counts
                    void upload(files);
                  }}
                />
                <button type="button" className="small" disabled={uploading} onClick={() => picker.current?.click()}>
                  {uploading ? 'Uploading…' : 'Upload map…'}
                </button>
                <span className="muted note">
                  Choose a map .json file, or drop it here (up to {MAX_MAP_BYTES / 1024} KB). It stays until the server restarts.
                </span>
              </div>
            )}
            {report.length > 0 && <UploadReport results={report} onClose={() => setReport([])} />}
            {suggestion.length > 0 && (
              <div className="map-suggest" role="note">
                <p>
                  <b>{info?.name}</b> suggests different rules: {suggestion.join(', ')}.
                </p>
                {host ? (
                  <button type="button" className="small" onClick={applySuggested}>
                    Apply suggested rules
                  </button>
                ) : (
                  <p className="muted note">The host can apply them.</p>
                )}
              </div>
            )}
            {!host && <p className="muted note">Only the host can change this.</p>}
          </fieldset>
          <fieldset className="timer-modes" disabled={!host}>
            <legend>Turn timer</legend>
            <div className="segments" role="radiogroup" aria-label="Turn timer">
              {TIMER_MODES.map((m) => (
                <label key={m.id} className={'segment' + (timer === m.id ? ' on' : '')}>
                  <input
                    type="radio"
                    name="timer"
                    value={m.id}
                    checked={timer === m.id}
                    onChange={() => send({ type: 'set_timer', mode: m.id })}
                  />
                  <span className="seg-name">{m.label}</span>
                  <span className="seg-secs">{m.secs}s</span>
                </label>
              ))}
            </div>
            {!host && <p className="muted note">Only the host can change this.</p>}
          </fieldset>
        </div>
        <div className="lobby-rules">
          <GameSettingsPanel settings={settings} host={host} onChange={change} onReset={reset} />
        </div>
        <div className="lobby-actions">
          {host ? (
            <button className="primary" disabled={players.length < MIN_PLAYERS || wrongCount} onClick={() => send({ type: 'start' })}>
              {players.length < MIN_PLAYERS
                ? `Need at least ${MIN_PLAYERS} players`
                : wrongCount
                  ? `${info?.name} is for ${range} players`
                  : `Start with ${players.length} players`}
            </button>
          ) : (
            <p>Waiting for {players[0]?.name ?? 'the host'} to start…</p>
          )}
          <button onClick={onLeave}>Leave table</button>
        </div>
      </div>
    </div>
  );
}

/** Short text for the status pill above the action bar. */
function statusFor(v: GameView): string {
  const cur = v.players[v.current]?.name;
  const mine = v.current === v.you;
  switch (v.phase) {
    case 'setup':
      return mine ? (v.setup_needs === 'road' ? 'Place Road' : 'Place Settlement') : `${cur}: Place ${v.setup_needs === 'road' ? 'Road' : 'Settlement'}`;
    case 'roll':
      return mine ? 'Roll Dice' : `${cur} is rolling`;
    case 'discard': {
      const need = v.discards.find((d) => d[0] === v.you);
      return need ? 'Select Cards to Discard' : 'Waiting for discards';
    }
    case 'move_robber':
      return mine ? 'Move the Robber' : `${cur} is moving the robber`;
    case 'steal':
      return mine ? 'Select Player to Rob' : `${cur} is choosing who to rob`;
    case 'main':
      return mine ? 'Your Turn' : `${cur}'s Turn`;
    case 'road_building':
      return mine ? `Place ${v.road_building_left} Free Road${v.road_building_left === 1 ? '' : 's'}` : `${cur} is placing roads`;
    case 'game_over':
      return 'Game Over';
  }
}

/** Whole seconds shown on the clock (rounded up, so 00:00 only appears when time is really up). */
const shownSecs = (ms: number): number => Math.ceil(Math.max(0, ms) / 1000);
const fmt = (s: number): string => `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;

/**
 * The countdown beside the status pill. The server owns the clock: each game view says how
 * much time was left when it was built, and this counts down from the moment the view arrived.
 * Turns red under 10 seconds. It lives in its own component so the 10 Hz repaint doesn't
 * re-render the board.
 */
function TurnClock({ timer, override }: { timer: TimerView | null; override?: number }) {
  const [, tick] = useState(0);
  const anchor = useMemo(() => ({ at: performance.now(), ms: timer?.remaining_ms ?? 0 }), [timer]);
  useEffect(() => {
    const t = window.setInterval(() => tick((n) => n + 1), 100);
    return () => window.clearInterval(t);
  }, []);
  if (!timer && override === undefined) {
    return (
      <div className="pill timer" title="No timer running">
        --:--
      </div>
    );
  }
  const ms = override !== undefined ? override * 1000 : Math.max(0, anchor.ms - (performance.now() - anchor.at));
  const label = timer ? CLOCK_LABELS[timer.kind] : '';
  const secs = shownSecs(ms);
  return (
    <div
      className={'pill timer' + (secs < 10 ? ' urgent' : '') + (label ? ' labelled' : '')}
      title={timer ? CLOCK_TITLES[timer.kind] : 'Time left on this turn'}
      role="timer"
    >
      {label && <span className="timer-label">{label}</span>}
      <span className="timer-value">{fmt(secs)}</span>
    </div>
  );
}

function ActBtn({
  kind,
  badge,
  label,
  title,
  disabled,
  on,
  onClick,
}: {
  kind: 'trade' | 'dev' | 'road' | 'bridge' | 'settlement' | 'city' | 'turn';
  badge?: number;
  label: string;
  title: string;
  disabled: boolean;
  on?: boolean;
  onClick: () => void;
}) {
  return (
    <button className={'act' + (on ? ' on' : '')} disabled={disabled} onClick={onClick} aria-label={label} title={title}>
      <ActionIcon kind={kind} />
      {badge !== undefined && <span className="act-badge">{badge}</span>}
    </button>
  );
}

const costText = (name: string, c: Res) => {
  const n = ['brick', 'lumber', 'ore', 'grain', 'wool'];
  return `${name}: ` + c.map((x, i) => (x ? `${x} ${n[i]}` : '')).filter(Boolean).join(', ');
};

export function GameScreen({ v, send, clockOverride }: { v: GameView; send: Send; clockOverride?: number }) {
  const [mode, setMode] = useState<BuildMode>(null);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [pips, setPips] = useState(true);
  const [logOpen, setLogOpen] = useState(true);
  const [pick, setPick] = useState<Res>([0, 0, 0, 0, 0]);
  /** After the game: the stats screen (true) or the board (false). */
  const [statsOpen, setStatsOpen] = useState(true);

  const mine = v.current === v.you;
  const main = mine && v.phase === 'main';
  const me = v.players[v.you];
  const act = (a: object) => {
    send(a);
  };

  // What the board should highlight right now.
  const placing = v.phase === 'setup' || v.phase === 'road_building';
  const vertexTargets =
    mine && (v.phase === 'setup' || (main && mode === 'settlement')) ? v.legal.settlements : main && mode === 'city' ? v.legal.cities : [];
  const edgeTargets = !mine ? [] : placing || (main && mode === 'road') ? v.legal.roads : main && mode === 'bridge' ? v.legal.bridges : [];
  const hexTargets = mine && v.phase === 'move_robber' ? v.legal.hexes : [];

  const canDev = mine && (v.phase === 'roll' || v.phase === 'main') && !v.dev_played;
  const discardNeed = v.phase === 'discard' ? v.discards.find((d) => d[0] === v.you) : undefined;
  useEffect(() => {
    if (!discardNeed) setPick([0, 0, 0, 0, 0]);
  }, [discardNeed]);

  const pickMode = (m: Exclude<BuildMode, null>) => setMode((cur) => (cur === m ? null : m));
  const afford = (c: Res) => main && covers(v.hand, c);

  const playDev = (k: number) => {
    if (k === 0) act({ type: 'play_knight' });
    else if (k === 1) act({ type: 'play_road_building' });
    else if (k === 2) setDialog('plenty');
    else if (k === 3) setDialog('monopoly');
  };

  const canRoll = mine && v.phase === 'roll';
  const showDice = v.phase !== 'setup';
  const die = v.dice ?? [1, 1];
  const toggleFull = () => {
    if (document.fullscreenElement) void document.exitFullscreen();
    else void document.documentElement.requestFullscreen?.();
  };
  const tool = (kind: 'gear' | 'book' | 'expand' | 'info', label: string, fn: () => void): ReactNode => (
    <button className="tool" aria-label={label} title={label} onClick={fn}>
      <ToolIcon kind={kind} />
    </button>
  );

  return (
    <div className="table">
      <div className="left">
        <main className="stage">
          <nav className="tools" aria-label="Game menu">
            {tool('gear', 'Settings', () => setDialog('settings'))}
            {tool('book', 'Building costs', () => setDialog('rules'))}
            {tool('expand', 'Full screen', toggleFull)}
            {tool('info', 'Game info', () => setDialog('info'))}
          </nav>

          <Board
            view={v}
            showPips={pips}
            vertexTargets={vertexTargets}
            edgeTargets={edgeTargets}
            hexTargets={hexTargets}
            onVertex={(i) => {
              act(mode === 'city' && main ? { type: 'build_city', vertex: i } : { type: 'build_settlement', vertex: i });
              setMode(null);
            }}
            onEdge={(i) => {
              act({ type: main && mode === 'bridge' ? 'build_bridge' : 'build_road', edge: i });
              if (v.phase === 'main') setMode(null);
            }}
            onHex={(i) => act({ type: 'move_robber', hex: i })}
          />

          <TradeBanners
            v={v}
            onRespond={(id, accept) => act({ type: 'respond_trade', id, accept })}
            onConfirm={(id, w) => act({ type: 'confirm_trade', id, with: w })}
            onCancel={(id) => act({ type: 'cancel_trade', id })}
          />

          {mine && v.phase === 'steal' && (
            <div className="steal-bar">
              <span>Steal a random card from</span>
              {v.steal_candidates.map((id) => (
                <button key={id} onClick={() => act({ type: 'steal', victim: id })}>
                  <Avatar color={v.players[id].color} size={22} /> {v.players[id].name}
                </button>
              ))}
            </div>
          )}

          {showDice && (
            <button className={'dice' + (canRoll ? ' ready' : '')} disabled={!canRoll} onClick={() => act({ type: 'roll' })} aria-label={canRoll ? 'Roll dice' : 'Last roll'}>
              <span className="die">
                <DieFace n={die[0]} />
              </span>
              <span className="die">
                <DieFace n={die[1]} />
              </span>
            </button>
          )}
        </main>

        <div className={'bottom' + (v.settings.bridges ? ' with-bridge' : '')}>
          <div className="status">
            {canRoll ? (
              <button className="pill" onClick={() => act({ type: 'roll' })} aria-live="polite">
                <Avatar color={v.players[v.current].color} self size={34} />
                <span>{statusFor(v)}</span>
              </button>
            ) : (
              <div className="pill" aria-live="polite">
                {v.phase !== 'game_over' && <Avatar color={v.players[v.current].color} self size={34} />}
                <span>{statusFor(v)}</span>
              </div>
            )}
            <TurnClock timer={v.timer} override={clockOverride} />
          </div>

          <div className="handwrap">
            {discardNeed && (
              <DiscardPanel
                need={discardNeed[1]}
                limit={v.settings.discard_limit}
                pick={pick}
                rolled={v.dice ? v.dice[0] + v.dice[1] : null}
                onUnpick={(i) => setPick((p) => p.map((x, j) => (j === i ? x - 1 : x)) as Res)}
                onConfirm={() => act({ type: 'discard', cards: pick })}
              />
            )}
            <div className="handbar">
              <Hand
                v={v}
                canPlayDev={canDev}
                onPlayDev={playDev}
                pick={discardNeed ? pick : undefined}
                onPick={
                  discardNeed
                    ? (i) => setPick((p) => (total(p) < discardNeed[1] && p[i] < v.hand[i] ? (p.map((x, j) => (j === i ? x + 1 : x)) as Res) : p))
                    : undefined
                }
              />
            </div>
          </div>

          <div className={'actions' + (v.settings.bridges ? ' with-bridge' : '')}>
            <ActBtn kind="trade" label="Trade" title="Trade with players or the bank" disabled={!main} onClick={() => setDialog('trade')} />
            <ActBtn
              kind="dev"
              label="Buy development card"
              title={costText('Development card', COST_DEV) + ` (${v.deck_left} left)`}
              disabled={!afford(COST_DEV) || v.deck_left === 0}
              onClick={() => act({ type: 'buy_dev' })}
            />
            <ActBtn
              kind="road"
              label="Build road"
              title={costText('Road', COST_ROAD)}
              badge={me.roads_left}
              on={mode === 'road'}
              disabled={!afford(COST_ROAD) || v.legal.roads.length === 0}
              onClick={() => pickMode('road')}
            />
            {v.settings.bridges && (
              <ActBtn
                kind="bridge"
                label="Build bridge"
                title={costText('Bridge', COST_BRIDGE) + ' (a road over water; uses a road piece)'}
                badge={me.roads_left}
                on={mode === 'bridge'}
                disabled={!afford(COST_BRIDGE) || v.legal.bridges.length === 0}
                onClick={() => pickMode('bridge')}
              />
            )}
            <ActBtn
              kind="settlement"
              label="Build settlement"
              title={costText('Settlement', COST_SETTLEMENT)}
              badge={me.settlements_left}
              on={mode === 'settlement'}
              disabled={!afford(COST_SETTLEMENT) || v.legal.settlements.length === 0}
              onClick={() => pickMode('settlement')}
            />
            <ActBtn
              kind="city"
              label="Build city"
              title={costText('City', COST_CITY)}
              badge={me.cities_left}
              on={mode === 'city'}
              disabled={!afford(COST_CITY) || v.legal.cities.length === 0}
              onClick={() => pickMode('city')}
            />
            <ActBtn
              kind="turn"
              label="End turn"
              title="End your turn"
              disabled={!main}
              onClick={() => {
                setMode(null);
                act({ type: 'end_turn' });
              }}
            />
          </div>
        </div>
      </div>

      <aside className={'rail' + (v.players.length > 5 ? ' crowded' : '')}>
        <section className={'logpanel' + (logOpen ? '' : ' closed')}>
          <Log lines={v.log} v={v} />
        </section>
        <button className="logbar" aria-expanded={logOpen} onClick={() => setLogOpen(!logOpen)}>
          <span>Log</span>
          <Chevron dir={logOpen ? 'up' : 'down'} />
        </button>
        <BankRow v={v} />
        <ul className="plist">
          {v.players
            .filter((p) => p.id !== v.you)
            .map((p) => (
              <PlayerRow key={p.id} v={v} id={p.id} />
            ))}
          <PlayerRow v={v} id={v.you} />
        </ul>
      </aside>

      {dialog === 'trade' && (
        <TradeModal
          v={v}
          onClose={() => setDialog(null)}
          onOffer={(give, want) => act({ type: 'offer_trade', give, want })}
          onBank={(give, get) => act({ type: 'bank_trade', give, get })}
        />
      )}
      {dialog === 'plenty' && (
        <PickResourceDialog
          title="Year of Plenty"
          help="Take any 2 resources from the bank."
          count={2}
          bank={v.bank}
          onClose={() => setDialog(null)}
          onPick={(p) => {
            act({ type: 'play_year_of_plenty', a: p[0], b: p[1] });
            setDialog(null);
          }}
        />
      )}
      {dialog === 'monopoly' && (
        <PickResourceDialog
          title="Monopoly"
          help="Every other player hands you all of the resource you name."
          count={1}
          onClose={() => setDialog(null)}
          onPick={(p) => {
            act({ type: 'play_monopoly', resource: p[0] });
            setDialog(null);
          }}
        />
      )}
      {dialog === 'rules' && <RulesDialog settings={v.settings} onClose={() => setDialog(null)} />}
      {dialog === 'info' && <InfoDialog v={v} onClose={() => setDialog(null)} />}
      {dialog === 'settings' && <SettingsDialog pips={pips} onPips={setPips} onFullscreen={toggleFull} onClose={() => setDialog(null)} />}
      {v.phase === 'game_over' && (
        <>
          {statsOpen && <GameOver v={v} send={send} />}
          <button className="stats-toggle" onClick={() => setStatsOpen(!statsOpen)}>
            {statsOpen ? 'Map' : 'Stats'}
          </button>
        </>
      )}
    </div>
  );
}

/** Bar chart of how often each dice total (2 to 12) came up this game, under the total number of rolls. */
function RollChart({ counts }: { counts: number[] }) {
  const total = counts.reduce((a, b) => a + b, 0);
  const max = Math.max(1, ...counts);
  const summary = counts.map((n, i) => `${i + 2}: ${n}`).join(', ');
  return (
    <section className="rolls" aria-label="Dice rolls">
      <div className="rolls-total">
        <b>{total}</b> {total === 1 ? 'dice roll' : 'dice rolls'}
      </div>
      {total > 0 ? (
        <>
          <div className="rolls-chart" role="img" aria-label={`Number of times each dice total was rolled. ${summary}.`}>
            {counts.map((n, i) => (
              <div key={i} className="rolls-col">
                <div className="rolls-plot">
                  <span className="rolls-n">{n}</span>
                  {n > 0 && <div className="rolls-bar" style={{ height: `max(3px, calc((100% - 18px) * ${n / max}))` }} />}
                </div>
                <span className="rolls-x">{i + 2}</span>
              </div>
            ))}
          </div>
        </>
      ) : (
        <p className="muted">No dice were rolled.</p>
      )}
    </section>
  );
}

function GameOver({ v, send }: { v: GameView; send: Send }) {
  const winner = v.winner !== null ? v.players[v.winner] : null;
  const ranked = [...v.players].sort((a, b) => (b.vp_total ?? 0) - (a.vp_total ?? 0));
  return (
    <Modal title={winner ? `${winner.name} wins!` : 'Game over'}>
      <div className="final-scroll">
        <table className="final-table" style={{ borderColor: winner ? winner.color : undefined }}>
          <thead>
            <tr>
              <th scope="col">Player</th>
              <th scope="col">Total points</th>
              <th scope="col">Settlements</th>
              <th scope="col">Cities</th>
              <th scope="col">Dev cards</th>
              <th scope="col">Largest army</th>
              <th scope="col">Longest road</th>
            </tr>
          </thead>
          <tbody>
            {ranked.map((p) => {
              const b = p.vp_breakdown;
              return (
                <tr key={p.id}>
                  <th scope="row">
                    <div className="who">
                      <Chip color={p.color} />
                      <span>{p.name}</span>
                    </div>
                  </th>
                  <td className="total">{p.vp_total ?? p.vp}</td>
                  <td>{b?.settlements ?? '–'}</td>
                  <td>{b?.cities ?? '–'}</td>
                  <td>{b?.dev_cards ?? '–'}</td>
                  <td>{b?.largest_army ?? '–'}</td>
                  <td>{b?.longest_road ?? '–'}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <RollChart counts={v.roll_counts} />
      <div className="modal-actions">
        <button className="primary" onClick={() => send({ type: 'reset' })}>
          Back to lobby
        </button>
      </div>
    </Modal>
  );
}
