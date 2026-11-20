import { ReactNode, useEffect, useState } from 'react';
import { Chevron, CountCard, DieFace, KnightsIcon, PersonIcon, PieceShape, RoadLenIcon, ToolIcon, isVeryDark, pieceEdge, shade } from './art';
import { BankRow, Log, WinBar } from './ui';
import { GameView, luminance, total } from './types';

/**
 * The phone layout of the game screen, used at 600px wide and under (see `useIsPhone`).
 *
 * It is a separate tree of elements, all with `m-` class names and styled in mobile.css under `.m-table`, so
 * the desktop screen in App.tsx is never touched by it. What it shows is the same game: App.tsx builds the
 * board, the hand, the action buttons and the overlays once and passes them in; this file arranges them.
 */

const PHONE_QUERY = '(max-width: 600px)';

/** Whether the window is phone-sized. Follows the window, so rotating or resizing switches layouts live. */
export function useIsPhone(): boolean {
  const query = () => (typeof window !== 'undefined' && typeof window.matchMedia === 'function' ? window.matchMedia(PHONE_QUERY) : null);
  const [phone, setPhone] = useState(() => !!query()?.matches);
  useEffect(() => {
    const mq = query();
    if (!mq) return;
    const sync = () => setPhone(mq.matches);
    sync();
    // Safari before 14 only knows the old addListener.
    if (mq.addEventListener) mq.addEventListener('change', sync);
    else mq.addListener(sync);
    return () => {
      if (mq.removeEventListener) mq.removeEventListener('change', sync);
      else mq.removeListener(sync);
    };
  }, []);
  return phone;
}

/** Short words for the tab under the player whose turn it is (their name is already on the card above it). */
function shortStatus(v: GameView): string {
  const mine = v.current === v.you;
  switch (v.phase) {
    case 'setup':
      return v.setup_needs === 'road' ? 'Place Road' : 'Settlement';
    case 'roll':
      return mine ? 'Roll Dice' : 'Rolling';
    case 'discard':
      return v.discards.some((d) => d[0] === v.you) ? 'Discard' : 'Discarding';
    case 'move_robber':
      return mine ? 'Move Robber' : 'Robber';
    case 'steal':
      return mine ? 'Rob a Player' : 'Robbing';
    case 'volcano_pick':
      return 'Volcano';
    case 'main':
      return mine ? 'Your Turn' : 'Their Turn';
    case 'road_building':
      return mine ? `Place ${v.road_building_left} Road${v.road_building_left === 1 ? '' : 's'}` : 'Roads';
    case 'game_over':
      return 'Game Over';
  }
}

/** Seat ids in turn order, starting with you, so the row reads left to right the way the turns pass. */
function seatOrder(v: GameView): number[] {
  const ids = v.order.length === v.players.length ? v.order : v.players.map((p) => p.id);
  const at = ids.indexOf(v.you);
  return at < 0 ? ids : [...ids.slice(at), ...ids.slice(0, at)];
}

/* ------------------------------------------------------------------ */
/* Small icons                                                          */
/* ------------------------------------------------------------------ */

/** A road piece: an upright bar in the player's color. */
function RoadPip({ color }: { color: string }) {
  return (
    <svg viewBox="0 0 10 24" className="m-roadpip" aria-hidden="true">
      <rect x="1.5" y="1.5" width="7" height="21" rx="2" fill={color} stroke={pieceEdge(color)} strokeWidth="1.4" />
      <rect x="3" y="3.5" width="2" height="17" rx="1" fill="#fff" opacity={isVeryDark(color) ? 0.25 : 0.35} />
    </svg>
  );
}

function PiecePip({ color, city }: { color: string; city?: boolean }) {
  return (
    <svg viewBox="-20 -19 40 36" className="m-piecepip" aria-hidden="true">
      <PieceShape color={color} city={city} />
    </svg>
  );
}

/** The button that shows how many pieces everyone has left: a road, a settlement and a city, in your color. */
function PiecesGlyph({ color }: { color: string }) {
  return (
    <svg viewBox="0 0 78 26" className="m-pglyph" aria-hidden="true">
      <g transform="translate(4 1)">
        <rect x="1.5" y="1.5" width="6" height="21" rx="1.8" fill={color} stroke={pieceEdge(color)} strokeWidth="1.4" />
      </g>
      <g transform="translate(30 13) scale(0.78)">
        <PieceShape color={color} />
      </g>
      <g transform="translate(58 13) scale(0.74)">
        <PieceShape color={color} city />
      </g>
    </svg>
  );
}

/** Two sheets of paper: the log button. */
function LogIcon() {
  return (
    <svg viewBox="0 0 36 40" aria-hidden="true">
      <g fill="#d9f0ff" stroke="#1f3a52" strokeWidth="2.4" strokeLinejoin="round">
        <rect x="2" y="2" width="21" height="28" rx="3.5" />
        <rect x="11" y="9" width="23" height="29" rx="3.5" />
      </g>
      <path d="M16.5 20 H28.5 M16.5 27 H28.5" stroke="#1f3a52" strokeWidth="3" strokeLinecap="round" />
    </svg>
  );
}

/* ------------------------------------------------------------------ */
/* Player card                                                          */
/* ------------------------------------------------------------------ */

function Stat({ icon, n, held, title }: { icon: ReactNode; n: number; held: boolean; title: string }) {
  return (
    <span className={'m-stat' + (held ? ' held' : '')} title={title}>
      {icon}
      <b>{n}</b>
    </span>
  );
}

function MobilePlayer({ v, id, pieces, below }: { v: GameView; id: number; pieces: boolean; below: ReactNode }) {
  const p = v.players[id];
  const me = id === v.you;
  const ink = luminance(p.color) > 0.35 ? '#1c1c1c' : '#fff';
  const cards = me ? total(v.hand) : p.cards;
  const devs = me ? total(v.dev) + total(v.dev_new) : p.dev_cards;
  // Your own points include the victory point cards nobody else can see; the circle shows what they can.
  const hidden = me && v.vp_you !== p.vp ? v.vp_you : null;
  const army = v.largest_army_holder === id;
  const road = v.longest_road_holder === id;
  return (
    <li className={'m-player' + (me ? ' me' : '') + (p.connected ? '' : ' away')}>
      <div className="m-pcard">
        <div className="m-pband" style={{ background: p.color, color: ink }}>
          <span
            className="m-vp"
            style={{
              background: `radial-gradient(circle at 50% 30%, ${shade(p.color, 0.3)}, ${p.color})`,
              borderColor: isVeryDark(p.color) ? '#d8d8d8' : shade(p.color, -0.35),
              color: ink,
            }}
            title="Victory points"
          >
            {p.vp}
          </span>
          {hidden !== null && (
            <span className="m-vphid" title="Your points, including victory point cards only you can see">
              {hidden}
            </span>
          )}
          <PersonIcon size={14} color={ink} stroke="none" />
          <span className="m-pname" title={p.connected ? p.name : `${p.name} (offline)`}>
            {p.name}
          </span>
        </div>
        <div className="m-pbody">
          <div className="m-pmain">
            <CountCard res="unknown" n={cards} alert={me && cards > v.settings.discard_limit} className="m-pc" />
            <CountCard res="dev" n={devs} className="m-pc" />
            <div className="m-stats">
              <Stat icon={<KnightsIcon />} n={p.knights} held={army} title={army ? 'Largest Army: knights played' : 'Knights played'} />
              <Stat icon={<RoadLenIcon />} n={p.longest_road} held={road} title={road ? 'Longest Road: roads in a row' : 'Longest connected road'} />
            </div>
          </div>
          <WinBar v={v} id={id} className="m-win" />
          {pieces && (
            <div className="m-ppieces" aria-label="Pieces left">
              <span title="Roads left">
                <RoadPip color={p.color} />
                <b>{p.roads_left}</b>
              </span>
              <span title="Settlements left">
                <PiecePip color={p.color} />
                <b>{p.settlements_left}</b>
              </span>
              <span title="Cities left">
                <PiecePip color={p.color} city />
                <b>{p.cities_left}</b>
              </span>
            </div>
          )}
        </div>
      </div>
      {below}
    </li>
  );
}

/* ------------------------------------------------------------------ */
/* The screen                                                           */
/* ------------------------------------------------------------------ */

export interface MobileTableProps {
  v: GameView;
  /** The full sentence for screen readers, e.g. "Angle is rolling". */
  status: string;
  board: ReactNode;
  /** Trade offers, the steal bar and the volcano bar: drawn over the board. */
  overlays: ReactNode;
  /** The turn countdown. */
  clock: ReactNode;
  /** The buttons of the action row (`ActBtn`s). */
  actions: ReactNode;
  hand: ReactNode;
  /** The discard panel, when you have to discard. */
  discard: ReactNode;
  dice: [number, number];
  showDice: boolean;
  canRoll: boolean;
  onRoll: () => void;
  onSettings: () => void;
  onInfo: () => void;
}

export function MobileTable({ v, status, board, overlays, clock, actions, hand, discard, dice, showDice, canRoll, onRoll, onSettings, onInfo }: MobileTableProps) {
  const [piecesOpen, setPiecesOpen] = useState(false);
  const [logOpen, setLogOpen] = useState(false);
  // The page itself must not scroll or bounce under the game, and the sea should show where the page ends.
  useEffect(() => {
    const root = document.documentElement;
    root.classList.add('m-lock');
    return () => root.classList.remove('m-lock');
  }, []);

  const me = v.players[v.you];
  const order = seatOrder(v);
  const over = v.phase === 'game_over';
  const mineTurn = v.current === v.you;
  // The dice are yours to roll on your turn. On anyone else's turn, what they rolled is shown small under their card.
  const bigDice = showDice && mineTurn && !over;
  const smallDice = showDice && !mineTurn && !over && v.phase !== 'roll' && v.dice !== null;

  return (
    <div className="m-table">
      <header className="m-top">
        <nav className="m-tools" aria-label="Game menu">
          <button className="tool" aria-label="Settings" title="Settings" onClick={onSettings}>
            <ToolIcon kind="gear" />
          </button>
          <button className="tool" aria-label="Game info" title="Game info" onClick={onInfo}>
            <ToolIcon kind="info" />
          </button>
        </nav>
        <BankRow v={v} />
        <button
          className={'m-pieces' + (piecesOpen ? ' on' : '')}
          aria-expanded={piecesOpen}
          aria-label={piecesOpen ? 'Hide pieces left' : 'Show pieces left'}
          title="Pieces left"
          onClick={() => setPiecesOpen(!piecesOpen)}
        >
          <PiecesGlyph color={me.color} />
          <Chevron dir={piecesOpen ? 'up' : 'down'} />
        </button>
      </header>

      <ul className={'m-players' + (order.length > 4 ? ' wrap' : '')} aria-label="Players">
        {order.map((id) => {
          const turn = !over && v.current === id;
          return (
            <MobilePlayer
              key={id}
              v={v}
              id={id}
              pieces={piecesOpen}
              below={
                turn && (
                  <>
                    <div className="m-tab" aria-live="polite" title={status}>
                      <span className="sr-only">{status}</span>
                      <span className="m-tabtxt" aria-hidden="true">
                        {shortStatus(v)}
                      </span>
                      {clock}
                    </div>
                    {smallDice && v.dice && (
                      <div className="m-dice-s" aria-label={`${v.players[id].name} rolled ${v.dice[0] + v.dice[1]}`}>
                        {v.dice.map((n, i) => (
                          <span key={i} className="m-mdie">
                            <DieFace n={n} r={15} />
                          </span>
                        ))}
                      </div>
                    )}
                  </>
                )
              }
            />
          );
        })}
      </ul>

      <main className="m-stage">
        {board}
        {overlays}
        {bigDice && (
          <button className={'dice' + (canRoll ? ' ready' : '')} disabled={!canRoll} onClick={onRoll} aria-label={canRoll ? 'Roll dice' : 'Last roll'}>
            <span className="die">
              <DieFace n={dice[0]} />
            </span>
            <span className="die">
              <DieFace n={dice[1]} />
            </span>
          </button>
        )}
        <button className={'m-logbtn' + (logOpen ? ' on' : '')} aria-label="Game log" aria-expanded={logOpen} title="Game log" onClick={() => setLogOpen(!logOpen)}>
          <LogIcon />
        </button>
      </main>

      {!over && <div className="m-actions">{actions}</div>}

      <div className="m-handwrap">
        {discard}
        <div className="m-hand">{hand}</div>
      </div>

      {logOpen && (
        <section className="m-log" aria-label="Game log">
          <Log lines={v.log} v={v} dark />
        </section>
      )}
    </div>
  );
}
