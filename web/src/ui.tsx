import { Fragment, ReactNode, useEffect, useRef, useState } from 'react';
import {
  Avatar,
  BankIcon,
  Chevron,
  CheckIcon,
  CountCard,
  DiscardArrow,
  DieFace,
  KnightsIcon,
  MiniCard,
  PersonIcon,
  PieceIcon,
  Ribbon,
  RoadLenIcon,
  RES_ORDER,
} from './art';
import {
  bump,
  COST_CITY,
  COST_DEV,
  COST_ROAD,
  COST_SETTLEMENT,
  covers,
  devHelp,
  DEV_NAMES,
  FRIENDLY_ROBBER_MAX_VP,
  ClockKind,
  GameView,
  LogEvent,
  MAX_OPEN_TRADES,
  textColor,
  Res,
  RES_IDX,
  RES_NAMES,
  Settings,
  TIMER_MODES,
  total,
  Trade,
  TRADE_BONUS_SECS,
} from './types';
import { changedCount, settingsRows } from './GameSettings';

/** Small color swatch; with no color it is an empty outline (an open seat). */
export function Chip({ color, name }: { color?: string; name?: string }) {
  return <span className={'chip' + (color ? '' : ' empty')} style={color ? { background: color } : undefined} title={name} aria-hidden="true" />;
}

/** A compact bundle of cards, e.g. [brick] [lumber] x2 */
export function Bundle({ r }: { r: Res }) {
  const parts = RES_ORDER.filter((i) => r[i] > 0);
  if (parts.length === 0) return <span className="muted">nothing</span>;
  return (
    <span className="bundle">
      {parts.map((i) => (
        <span key={i} className="bundle-item" title={RES_NAMES[i]}>
          <MiniCard res={i} />
          {r[i] > 1 && <b>×{r[i]}</b>}
        </span>
      ))}
    </span>
  );
}

export function Modal({ title, children, onClose }: { title: string; children: ReactNode; onClose?: () => void }) {
  useEffect(() => {
    if (!onClose) return;
    const h = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', h);
    return () => window.removeEventListener('keydown', h);
  }, [onClose]);
  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose?.()}>
      <div className="modal" role="dialog" aria-modal="true" aria-label={title}>
        <h2>{title}</h2>
        {children}
      </div>
    </div>
  );
}

/** +/- counters for choosing a bundle of cards. */
export function CardPicker({ value, onChange, max }: { value: Res; onChange: (r: Res) => void; max?: Res }) {
  return (
    <div className="picker">
      {RES_ORDER.map((i) => {
        const cap = max ? max[i] : 9;
        return (
          <div key={i} className="picker-col">
            <MiniCard res={i} />
            <div className="picker-name">{RES_NAMES[i]}</div>
            <button className="step" aria-label={`more ${RES_NAMES[i]}`} disabled={value[i] >= cap} onClick={() => onChange(bump(value, i, 1))}>
              +
            </button>
            <div className="picker-n">{value[i]}</div>
            <button className="step" aria-label={`fewer ${RES_NAMES[i]}`} disabled={value[i] <= 0} onClick={() => onChange(bump(value, i, -1))}>
              −
            </button>
          </div>
        );
      })}
    </div>
  );
}

export function ResourceButtons({
  selected,
  onPick,
  disabled,
  label,
}: {
  selected: number | null;
  onPick: (i: number) => void;
  disabled?: (i: number) => boolean;
  label?: (i: number) => string;
}) {
  return (
    <div className="res-buttons">
      {RES_ORDER.map((i) => (
        <button key={i} className={'res-btn' + (selected === i ? ' on' : '')} disabled={disabled?.(i)} onClick={() => onPick(i)}>
          <MiniCard res={i} />
          <span>{RES_NAMES[i]}</span>
          {label && <small>{label(i)}</small>}
        </button>
      ))}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Discard panel (docked above the hand)                               */
/* ------------------------------------------------------------------ */

export function DiscardPanel({
  need,
  limit,
  pick,
  onUnpick,
  onConfirm,
  rolled,
}: {
  need: number;
  limit: number;
  pick: Res;
  onUnpick: (i: number) => void;
  onConfirm: () => void;
  rolled: number | null;
}) {
  const n = total(pick);
  return (
    <section className="discard" aria-label="Discard cards">
      <header>
        <span className="discard-ico">
          <DiscardArrow />
          <CountCardPlain />
        </span>
        <div>
          <h2>
            Discard Cards ({n}/{need})
          </h2>
          <p>
            {rolled === 7 ? '7 was rolled! ' : ''}You have more than {limit} cards. You need to discard {need} of them. Click cards in your hand to select them.
          </p>
        </div>
      </header>
      <div className="discard-zone">
        {RES_ORDER.map((i) =>
          Array.from({ length: pick[i] }).map((_, k) => (
            <button key={`${i}-${k}`} className="zone-card" onClick={() => onUnpick(i)} aria-label={`Put back ${RES_NAMES[i]}`}>
              <CountCard res={i} n={1} className="plain" />
            </button>
          )),
        )}
      </div>
      <footer>
        <button className={'check' + (n === need ? ' ready' : '')} disabled={n !== need} onClick={onConfirm} aria-label="Confirm discard">
          <CheckIcon />
        </button>
      </footer>
    </section>
  );
}

function CountCardPlain() {
  return <CountCard res="unknown" n={0} className="plain nobadge" />;
}

/* ------------------------------------------------------------------ */
/* Trading                                                              */
/* ------------------------------------------------------------------ */

export function TradeModal({
  v,
  onClose,
  onOffer,
  onBank,
}: {
  v: GameView;
  onClose: () => void;
  onOffer: (give: Res, want: Res) => void;
  onBank: (give: number, get: number) => void;
}) {
  const [tab, setTab] = useState<'players' | 'bank'>('players');
  const [give, setGive] = useState<Res>([0, 0, 0, 0, 0]);
  const [want, setWant] = useState<Res>([0, 0, 0, 0, 0]);
  const [bGive, setBGive] = useState<number | null>(null);
  const [bGet, setBGet] = useState<number | null>(null);
  const [sent, setSent] = useState(false);
  const clash = RES_IDX.some((i) => give[i] > 0 && want[i] > 0);
  const open = v.trades.filter((t) => t.from === v.you);
  const duplicate = open.some((t) => t.give.every((n, i) => n === give[i]) && t.want.every((n, i) => n === want[i]));
  const full = open.length >= MAX_OPEN_TRADES;
  const ok = total(give) > 0 && total(want) > 0 && !clash && !duplicate && !full;
  const ratio = bGive === null ? 0 : v.ratios[bGive];
  const bankOk = bGive !== null && bGet !== null && bGive !== bGet && v.hand[bGive] >= ratio && v.bank[bGet] > 0;

  return (
    <Modal title="Trade" onClose={onClose}>
      <div className="tabs" role="tablist">
        <button role="tab" aria-selected={tab === 'players'} className={tab === 'players' ? 'on' : ''} onClick={() => setTab('players')}>
          With players
        </button>
        <button role="tab" aria-selected={tab === 'bank'} className={tab === 'bank' ? 'on' : ''} onClick={() => setTab('bank')}>
          With the bank
        </button>
      </div>
      {tab === 'players' ? (
        <>
          <h3>You give</h3>
          <CardPicker value={give} onChange={(r) => { setSent(false); setGive(r); }} max={v.hand} />
          <h3>You want</h3>
          <CardPicker value={want} onChange={(r) => { setSent(false); setWant(r); }} />
          {clash && <p className="warn">You can't offer and ask for the same resource.</p>}
          {duplicate && !clash && <p className="warn">You already have that offer open.</p>}
          {full && <p className="warn">You can have at most {MAX_OPEN_TRADES} offers open at once.</p>}
          <p className="muted">
            {open.length > 0 && ` Open offers: ${open.length}/${MAX_OPEN_TRADES}.`}
            {sent && ' Offer sent'}
          </p>
          <div className="modal-actions">
            <button onClick={onClose}>{open.length > 0 || sent ? 'Done' : 'Cancel'}</button>
            <button
              className="primary"
              disabled={!ok}
              onClick={() => {
                onOffer(give, want);
                setGive([0, 0, 0, 0, 0]);
                setWant([0, 0, 0, 0, 0]);
                setSent(true);
              }}
            >
              Send offer
            </button>
          </div>
        </>
      ) : (
        <>
          <h3>Give</h3>
          <ResourceButtons selected={bGive} onPick={setBGive} disabled={(i) => v.hand[i] < v.ratios[i]} label={(i) => `${v.ratios[i]}:1`} />
          <h3>Get 1</h3>
          <ResourceButtons selected={bGet} onPick={setBGet} disabled={(i) => i === bGive || v.bank[i] === 0} label={(i) => `bank ${v.bank[i]}`} />
          <div className="modal-actions">
            <button onClick={onClose}>Close</button>
            <button
              className="primary"
              disabled={!bankOk}
              onClick={() => {
                onBank(bGive!, bGet!);
                setBGive(null);
                setBGet(null);
              }}
            >
              {bankOk ? `Trade ${ratio} ${RES_NAMES[bGive!]} for 1 ${RES_NAMES[bGet!]}` : 'Trade'}
            </button>
          </div>
        </>
      )}
    </Modal>
  );
}

export function PickResourceDialog({
  title,
  help,
  count,
  bank,
  onClose,
  onPick,
}: {
  title: string;
  help: string;
  count: 1 | 2;
  bank?: Res;
  onClose: () => void;
  onPick: (picks: number[]) => void;
}) {
  const [picks, setPicks] = useState<number[]>([]);
  const add = (i: number) => setPicks((p) => (count === 1 ? [i] : p.length >= 2 ? [p[1], i] : [...p, i]));
  const short = (i: number) => {
    if (!bank) return false;
    const need = picks.filter((p) => p === i).length + 1;
    return bank[i] < need;
  };
  return (
    <Modal title={title} onClose={onClose}>
      <p className="muted">{help}</p>
      <ResourceButtons selected={null} onPick={add} disabled={bank ? short : undefined} />
      <p className="picked">
        {picks.length === 0
          ? 'Nothing chosen yet'
          : picks.map((p, k) => (
              <Fragment key={k}>
                {k > 0 && ' + '}
                <MiniCard res={p} /> {RES_NAMES[p]}
              </Fragment>
            ))}
      </p>
      <div className="modal-actions">
        <button onClick={onClose}>Cancel</button>
        <button className="primary" disabled={picks.length !== count} onClick={() => onPick(picks)}>
          Confirm
        </button>
      </div>
    </Modal>
  );
}

/** "1 point", "2 points". */
const pts = (n: number): string => `${n} point${n === 1 ? '' : 's'}`;

export function RulesDialog({ settings, onClose }: { settings: Settings; onClose: () => void }) {
  const row = (label: ReactNode, cost: Res) => (
    <div className="cost-row">
      <span>{label}</span>
      <Bundle r={cost} />
    </div>
  );
  return (
    <Modal title="Building costs" onClose={onClose}>
      {row('Road', COST_ROAD)}
      {row('Settlement', COST_SETTLEMENT)}
      {row('City', COST_CITY)}
      {row('Development card', COST_DEV)}
      <h3>Reminders</h3>
      <ul className="tips">
        <li>
          A settlement is worth {pts(settings.settlement_points)}, a city {pts(settings.city_points)}.
          {settings.longest_road_points === settings.largest_army_points
            ? ` Longest Road and Largest Army are worth ${pts(settings.longest_road_points)} each.`
            : ` Longest Road is worth ${pts(settings.longest_road_points)} and Largest Army ${pts(settings.largest_army_points)}.`}
        </li>
        <li>
          Longest Road needs at least {settings.longest_road_min} roads in a row; Largest Army needs at least{' '}
          {settings.largest_army_min} knight{settings.largest_army_min === 1 ? '' : 's'}.
        </li>
        <li>
          Each Victory Point card is worth {pts(settings.vp_card_points)}.
        </li>
        <li>
          Roll a 7 and anyone holding more than {settings.discard_limit} cards discards half, then the robber moves.
          {settings.friendly_robber &&
            ` Friendly Robber is on: it can't go next to a player with ${FRIENDLY_ROBBER_MAX_VP} points or fewer.`}
        </li>
        <li>
          Trade with the bank at {settings.bank_ratio}:1, or better with a harbour.
        </li>
        <li>The first player to reach {settings.points_to_win} points on their own turn wins.</li>
      </ul>
      <div className="modal-actions">
        <button className="primary" onClick={onClose}>
          Close
        </button>
      </div>
    </Modal>
  );
}

export function InfoDialog({ v, onClose }: { v: GameView; onClose: () => void }) {
  const holder = (id: number | null) => (id === null ? 'Nobody' : v.players[id].name);
  const timerMode = TIMER_MODES.find((m) => m.id === v.timer_mode);
  return (
    <Modal title="Game info" onClose={onClose}>
      <div className="cost-row">
        <span>Turn order</span>
        <b>{v.order.map((id) => v.players[id].name).join(' → ')}</b>
      </div>
      <div className="cost-row">
        <span>Turn timer</span>
        <b>{timerMode ? `${timerMode.label} (${timerMode.secs}s)` : v.timer_mode}</b>
      </div>
      <div className="cost-row">
        <span>Longest Road</span>
        <b>{holder(v.longest_road_holder)}</b>
      </div>
      <div className="cost-row">
        <span>Largest Army</span>
        <b>{holder(v.largest_army_holder)}</b>
      </div>
      <div className="cost-row">
        <span>Development cards left</span>
        <b>{v.deck_left}</b>
      </div>
      <div className="cost-row">
        <span>Bank</span>
        <Bundle r={v.bank} />
      </div>
      <details className="rules-list">
        <summary>
          Game rules{changedCount(v.settings) > 0 ? ` (${changedCount(v.settings)} changed from standard)` : ' (standard)'}
        </summary>
        {settingsRows(v.settings).map((r) => (
          <div key={r.label} className={'cost-row' + (r.changed ? ' changed' : '')}>
            <span>{r.label}</span>
            <b>{r.value}</b>
          </div>
        ))}
      </details>
      <div className="cost-row">
        <span>Your trade ratios</span>
        <span className="ratios">
          {RES_ORDER.map((i) => (
            <span key={i} className="ratio">
              <MiniCard res={i} />
              {v.ratios[i]}:1
            </span>
          ))}
        </span>
      </div>
      <div className="modal-actions">
        <button className="primary" onClick={onClose}>
          Close
        </button>
      </div>
    </Modal>
  );
}

export function SettingsDialog({
  pips,
  onPips,
  onFullscreen,
  onClose,
}: {
  pips: boolean;
  onPips: (b: boolean) => void;
  onFullscreen: () => void;
  onClose: () => void;
}) {
  return (
    <Modal title="Settings" onClose={onClose}>
      <label className="check-row">
        <input type="checkbox" checked={pips} onChange={(e) => onPips(e.target.checked)} />
        <span>Show probability dots on number tokens</span>
      </label>
      <div className="modal-actions" style={{ justifyContent: 'space-between' }}>
        <button onClick={onFullscreen}>Toggle full screen</button>
        <button className="primary" onClick={onClose}>
          Done
        </button>
      </div>
    </Modal>
  );
}

/* ------------------------------------------------------------------ */
/* Hand                                                                 */
/* ------------------------------------------------------------------ */

export function Hand({
  v,
  pick,
  onPick,
  canPlayDev,
  onPlayDev,
}: {
  v: GameView;
  /** While discarding: cards already moved to the discard zone, and the click handler. */
  pick?: Res;
  onPick?: (i: number) => void;
  canPlayDev: boolean;
  onPlayDev: (kind: number) => void;
}) {
  const devKinds = [0, 1, 2, 3, 4].filter((k) => v.dev[k] + v.dev_new[k] > 0);
  return (
    <div className="hand" aria-label="Your cards">
      {RES_ORDER.map((i) => {
        const n = v.hand[i] - (pick ? pick[i] : 0);
        if (n <= 0) return null;
        const stack = Math.min(n - 1, 3);
        const body = <CountCard res={i} n={n} stack={stack} />;
        return onPick ? (
          <button key={i} className="stack pickable" style={{ marginLeft: stack * 7 }} title={`Discard a ${RES_NAMES[i]} card`} onClick={() => onPick(i)}>
            {body}
          </button>
        ) : (
          <div key={i} className="stack" style={{ marginLeft: stack * 7 }} title={RES_NAMES[i]}>
            {body}
          </div>
        );
      })}
      {devKinds.length > 0 && <span className="hand-sep" />}
      {devKinds.map((k) => {
        const n = v.dev[k] + v.dev_new[k];
        const ready = v.dev[k] > 0;
        const playable = ready && k !== 4 && canPlayDev;
        return (
          <button
            key={k}
            className={'stack dev' + (playable ? ' playable' : '') + (ready ? '' : ' fresh')}
            title={`${DEV_NAMES[k]}: ${devHelp(k, v.settings.vp_card_points)}${ready ? '' : ' (playable next turn)'}`}
            disabled={!playable}
            onClick={() => onPlayDev(k)}
          >
            <CountCard res="dev" n={n} />
            <span className="dev-tag">{['Knight', 'Roads', 'Plenty', 'Monop.', 'VP'][k]}</span>
          </button>
        );
      })}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Right rail                                                           */
/* ------------------------------------------------------------------ */

export function BankRow({ v }: { v: GameView }) {
  return (
    <div className="bank" title="Cards left in the bank">
      <BankIcon />
      <div className="bank-cards">
        {RES_ORDER.map((i) => (
          <CountCard key={i} res={i} n={v.bank[i]} stack={0} className="bankcard" />
        ))}
        <CountCard res="dev" n={v.deck_left} className="bankcard" />
      </div>
    </div>
  );
}

function Stat({ icon, n, held, title, slot }: { icon: ReactNode; n: number; held?: boolean; title: string; slot: 'a' | 'b' }) {
  return (
    <span className={'stat stat-' + slot + (held ? ' held' : '')} title={title}>
      {icon}
      <b>{n}</b>
    </span>
  );
}

export function PlayerRow({ v, id }: { v: GameView; id: number }) {
  const p = v.players[id];
  const me = id === v.you;
  const turn = v.current === id && v.phase !== 'game_over';
  const [open, setOpen] = useState(false);
  const mine = v.dev.reduce((a, b) => a + b, 0) + v.dev_new.reduce((a, b) => a + b, 0);
  const cards = me ? total(v.hand) : p.cards;
  const devs = me ? mine : p.dev_cards;
  const pts = me ? v.vp_you : p.vp;
  return (
    <li className={'prow' + (me ? ' me' : '') + (turn ? ' turn' : '') + (p.connected ? '' : ' away')}>
      <div className="prow-main">
        <div className="prow-id">
          <span className="prow-name" title={p.name}>
            {p.name}
          </span>
          <span className="avatar-wrap">
            <Avatar color={p.color} self={me} size={me ? 70 : 44} />
            <Ribbon n={pts} />
          </span>
        </div>
        <CountCard res="unknown" n={cards} alert={me && cards > v.settings.discard_limit} className="pcard pcard-a" />
        <CountCard res="dev" n={devs} className="pcard pcard-b" />
        <Stat slot="a" icon={<KnightsIcon />} n={p.knights} held={v.largest_army_holder === id} title={v.largest_army_holder === id ? 'Largest Army: knights played' : 'Knights played'} />
        <Stat slot="b" icon={<RoadLenIcon />} n={p.longest_road} held={v.longest_road_holder === id} title={v.longest_road_holder === id ? 'Longest Road: roads in a row' : 'Longest connected road'} />
        <button className="chev" aria-expanded={open} aria-label={open ? 'Hide details' : 'Show details'} onClick={() => setOpen(!open)}>
          <Chevron dir={open ? 'down' : 'left'} />
        </button>
      </div>
      {!p.connected && <div className="prow-note warn">offline</div>}
      {open && (
        <dl className="prow-more">
          <div>
            <dt>Roads left</dt>
            <dd>{p.roads_left}</dd>
          </div>
          <div>
            <dt>Settlements left</dt>
            <dd>{p.settlements_left}</dd>
          </div>
          <div>
            <dt>Cities left</dt>
            <dd>{p.cities_left}</dd>
          </div>
          {v.longest_road_holder === id && (
            <div>
              <dt>Award</dt>
              <dd>Longest Road</dd>
            </div>
          )}
          {v.largest_army_holder === id && (
            <div>
              <dt>Award</dt>
              <dd>Largest Army</dd>
            </div>
          )}
        </dl>
      )}
    </li>
  );
}

/* ------------------------------------------------------------------ */
/* Log                                                                  */
/* ------------------------------------------------------------------ */

/** A player's name in their colour. */
function Name({ v, id }: { v: GameView; id: number }) {
  return (
    <span className="who" style={{ color: textColor(v.players[id].color) }}>
      {v.players[id].name}
    </span>
  );
}

/** The player a line starts with: a person icon and their name. */
function Who({ v, id }: { v: GameView; id: number }) {
  return (
    <>
      <PersonIcon size={24} />
      <Name v={v} id={id} />
    </>
  );
}

/** What the server says when it acts for a player whose clock ran out (after the player's name, except `discard`). */
const TIMEOUT_TEXT: Record<ClockKind, string> = {
  setup_settlement: ' ran out of time; a settlement was placed for them',
  setup_road: ' ran out of time; a road was placed for them',
  roll: ' ran out of time; the dice were rolled for them',
  // Everyone discards at once, so this line has no single player to name.
  discard: 'Time is up; remaining cards were discarded at random',
  robber: ' ran out of time; the robber was placed on a 6 or 8',
  steal: ' ran out of time; a victim was chosen at random',
  turn: ' ran out of time; their turn was ended',
};

/** The player a log entry is about, if it has one: they get the leading icon and drive the turn separators. */
function actorOf(e: LogEvent): number | null {
  if (!('player' in e)) return null;
  return e.type === 'timed_out' && e.kind === 'discard' ? null : e.player;
}

/** Compile-time check that `describe` handles every event; at run time an unknown one shows its name. */
function unhandled(e: never): ReactNode {
  return String((e as { type?: string }).type ?? 'event');
}

/** The words and icons for one entry, without the leading player (see `actorOf`). */
function describe(e: LogEvent, v: GameView): ReactNode {
  const color = (id: number) => v.players[id].color;
  switch (e.type) {
    case 'game_started':
      return 'Game started. Place your first settlement.';
    case 'map_chosen':
      return `Map: ${e.name}`;
    case 'custom_rules':
      return `Custom rules: ${e.changes.join(', ')}`;
    case 'turn_order':
      return (
        <>
          Turn order:{' '}
          {e.order.map((id, i) => (
            <Fragment key={id}>
              {i > 0 && ', '}
              <Name v={v} id={id} />
            </Fragment>
          ))}
        </>
      );
    case 'setup_ended_early':
      return 'There is no room left for more settlements; setup ends early';
    case 'setup_complete':
      return 'Setup complete. The game begins!';
    case 'rolled':
      return (
        <>
          {' rolled '}
          {e.dice.map((n, i) => (
            <span key={i} className="mini-die">
              <DieFace n={n} r={13} />
            </span>
          ))}
        </>
      );
    case 'collected':
      return (
        <>
          {' got '}
          {e.cards.flatMap((n, res) => Array.from({ length: n }, (_, k) => <MiniCard key={`${res}-${k}`} res={res} />))}
        </>
      );
    case 'bank_short':
      return (
        <>
          The bank is short of <MiniCard res={e.resource} />; nobody collects it
        </>
      );
    case 'must_discard':
      return `Players with more than ${e.limit} cards must discard half`;
    case 'discarded':
      return ` discarded ${e.count} cards`;
    case 'robber_must_move':
      return 'The robber must move';
    case 'robber_moved':
      return ' moved the robber';
    case 'stole':
      return (
        <>
          {' stole a card from '}
          <Name v={v} id={e.victim} />
        </>
      );
    case 'placed_settlement':
    case 'built_settlement':
      return (
        <>
          {e.type === 'placed_settlement' ? ' placed a Settlement ' : ' built a Settlement '}
          <PieceIcon color={color(e.player)} kind="settlement" />
        </>
      );
    case 'built_road':
      return (
        <>
          {' built a Road '}
          <PieceIcon color={color(e.player)} kind="road" />
        </>
      );
    case 'built_city':
      return (
        <>
          {' upgraded a Settlement to a City '}
          <PieceIcon color={color(e.player)} kind="city" />
        </>
      );
    case 'bought_dev_card':
      return (
        <>
          {' bought a development card '}
          <MiniCard res="dev" />
        </>
      );
    case 'played_knight':
      return ' played a Knight';
    case 'played_road_building':
      return ' played Road Building';
    case 'played_year_of_plenty':
      return (
        <>
          {' played Year of Plenty and took 1 '}
          <MiniCard res={e.first} />
          {' and 1 '}
          <MiniCard res={e.second} />
        </>
      );
    case 'played_monopoly':
      return (
        <>
          {' played Monopoly on '}
          <MiniCard res={e.resource} />
          {` and took ${e.taken}`}
        </>
      );
    case 'bank_trade':
      return (
        <>
          {` traded ${e.ratio} `}
          <MiniCard res={e.give} />
          {' for 1 '}
          <MiniCard res={e.get} />
          {' with the bank'}
        </>
      );
    case 'trade_proposed':
      return ' proposed a trade';
    case 'trade_completed':
      return (
        <>
          {' traded with '}
          <Name v={v} id={e.with} />
        </>
      );
    case 'longest_road_taken':
      return ` takes Longest Road`;
    case 'longest_road_lost':
      return 'Nobody holds Longest Road now';
    case 'largest_army_taken':
      return ` takes Largest Army`;
    case 'won':
      return ` wins with ${e.points} victory points!`;
    case 'timed_out':
      return TIMEOUT_TEXT[e.kind];
    default:
      return unhandled(e);
  }
}

function renderLine(e: LogEvent, v: GameView): ReactNode {
  const actor = actorOf(e);
  return (
    <>
      {actor !== null && <Who v={v} id={actor} />}
      {describe(e, v)}
    </>
  );
}

export function Log({ lines, v }: { lines: LogEvent[]; v: GameView }) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = box.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [lines.length]);
  // A rule is drawn between turns: before every roll, and (before the first roll,
  // i.e. during setup) whenever the acting player changes.
  const firstRoll = lines.findIndex((e) => e.type === 'rolled');
  let last: number | null = null;
  return (
    <div className="log" ref={box} aria-live="polite">
      {lines.map((e, i) => {
        const a = actorOf(e);
        const inSetup = firstRoll === -1 || i < firstRoll;
        const split = i > 0 && (e.type === 'rolled' || (inSetup && a !== null && last !== null && a !== last));
        if (a !== null) last = a;
        return (
          <Fragment key={i}>
            {split && <hr />}
            <p>{renderLine(e, v)}</p>
          </Fragment>
        );
      })}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Trade banner                                                         */
/* ------------------------------------------------------------------ */

/** Every open offer as its own box, stacked. Offers you have declined are not shown to you. */
export function TradeBanners({
  v,
  onRespond,
  onConfirm,
  onCancel,
}: {
  v: GameView;
  onRespond: (id: number, accept: boolean) => void;
  onConfirm: (id: number, with_: number) => void;
  onCancel: (id: number) => void;
}) {
  const visible = v.trades.filter((t) => t.from === v.you || t.replies[v.you] !== 'declined');
  if (visible.length === 0) return null;
  return (
    <div className="trade-stack">
      {visible.map((t) => (
        <TradeBox key={t.id} v={v} t={t} onRespond={onRespond} onConfirm={onConfirm} onCancel={onCancel} />
      ))}
    </div>
  );
}

function TradeBox({
  v,
  t,
  onRespond,
  onConfirm,
  onCancel,
}: {
  v: GameView;
  t: Trade;
  onRespond: (id: number, accept: boolean) => void;
  onConfirm: (id: number, with_: number) => void;
  onCancel: (id: number) => void;
}) {
  const mine = t.from === v.you;
  const from = v.players[t.from];
  const myReply = t.replies[v.you];
  const canAccept = covers(v.hand, t.want);
  return (
    <div className="trade-banner" role="status">
      <div className="trade-line">
        <Avatar color={from.color} size={26} />
        <b style={{ color: textColor(from.color) }}>{mine ? 'Your offer' : `${from.name} offers`}</b>
        <Bundle r={t.give} />
        <span className="muted">for</span>
        <Bundle r={t.want} />
      </div>
      {mine ? (
        <div className="trade-replies">
          {v.players
            .filter((p) => p.id !== v.you)
            .map((p) => {
              const r = t.replies[p.id];
              return (
                <span key={p.id} className={'reply ' + r}>
                  <span className="who" style={{ color: textColor(p.color) }}>
                    {p.name}
                  </span>
                  :{' '}
                  {r === 'accepted' ? (
                    <button className="primary small" onClick={() => onConfirm(t.id, p.id)}>
                      Trade with {p.name}
                    </button>
                  ) : r === 'declined' ? (
                    'declined'
                  ) : (
                    'thinking…'
                  )}
                </span>
              );
            })}
          <button className="small" onClick={() => onCancel(t.id)}>
            Withdraw offer
          </button>
        </div>
      ) : (
        <div className="trade-replies">
          {myReply === 'pending' && (
            <>
              <button className="primary small" disabled={!canAccept} onClick={() => onRespond(t.id, true)}>
                {canAccept ? 'Accept' : "Can't afford"}
              </button>
              <button className="small" onClick={() => onRespond(t.id, false)}>
                Decline
              </button>
            </>
          )}
          {myReply === 'accepted' && <span className="muted">You accepted. Waiting for {from.name} to confirm.</span>}
        </div>
      )}
    </div>
  );
}
