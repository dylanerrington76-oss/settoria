import { useEffect, useState } from 'react';
import { DEFAULT_SETTINGS, FRIENDLY_ROBBER_MAX_VP, type Settings } from './types';

type NumKey = { [K in keyof Settings]: Settings[K] extends number ? K : never }[keyof Settings];

interface NumSpec {
  kind: 'num';
  key: NumKey;
  label: string;
  hint?: string;
  /** Printed after the custom value ("points", ":1"...). */
  unit?: string;
  /** Limits; must match `Settings::validate` in src/game.rs. */
  min: number;
  max: number;
  /** Text of the "standard" button and of the value elsewhere. Defaults to the number. */
  show?: (n: number) => string;
  /** Text of the standard button when it isn't just the number. */
  standardLabel?: string;
}

interface ToggleSpec {
  kind: 'toggle';
  key: 'friendly_robber';
  label: string;
  hint?: string;
}

type Spec = NumSpec | ToggleSpec;

const plural = (n: number, one: string, many = one + 's') => `${n} ${n === 1 ? one : many}`;

/** The settings, grouped by what they affect. Within each group the order follows the host's checklist. */
const GROUPS: { title: string; items: Spec[] }[] = [
  {
    title: 'Winning',
    items: [
      { kind: 'num', key: 'points_to_win', label: 'Points to win', unit: 'points', min: 3, max: 99, hint: 'Checked on your own turn.' },
    ],
  },
  {
    title: 'Points',
    items: [
      { kind: 'num', key: 'settlement_points', label: 'Settlement points', unit: 'points', min: 0, max: 10 },
      { kind: 'num', key: 'city_points', label: 'City points', unit: 'points', min: 0, max: 10, hint: 'What a city is worth in total, not on top of the settlement.' },
      { kind: 'num', key: 'longest_road_points', label: 'Longest Road points', unit: 'points', min: 0, max: 10 },
      { kind: 'num', key: 'largest_army_points', label: 'Largest Army points', unit: 'points', min: 0, max: 10 },
      {
        kind: 'num',
        key: 'vp_card_points',
        label: 'Victory Point cards',
        unit: 'points each',
        min: 0,
        max: 10,
        standardLabel: 'Standard',
        hint: 'Standard: 1 point per card. Custom: points each card is worth.',
      },
    ],
  },
  {
    title: 'Awards',
    items: [
      { kind: 'num', key: 'longest_road_min', label: 'Longest Road minimum', unit: 'roads', min: 2, max: 20, hint: 'Fewest roads in a row that can hold it.' },
      { kind: 'num', key: 'largest_army_min', label: 'Largest Army minimum', unit: 'knights', min: 1, max: 20, hint: 'Fewest knights played that can hold it.' },
    ],
  },
  {
    title: 'Pieces and supply',
    items: [
      { kind: 'num', key: 'max_roads', label: 'Max roads', unit: 'roads', min: 1, max: 50, hint: 'Per player.' },
      { kind: 'num', key: 'max_settlements', label: 'Max settlements', unit: 'settlements', min: 1, max: 20, hint: 'Per player.' },
      { kind: 'num', key: 'max_cities', label: 'Max cities', unit: 'cities', min: 0, max: 20, hint: 'Per player.' },
      { kind: 'num', key: 'dev_cards', label: 'Total development cards', unit: 'cards', min: 0, max: 60, hint: 'Same mix of cards as standard, scaled to this size. Left at 25, it grows with 5 or more players (34 for 5-6, 44 for 7-8).' },
      { kind: 'num', key: 'resource_max', label: 'Max of each resource', unit: 'cards', min: 1, max: 99, hint: 'How many of each resource the bank holds. Left at 19, it grows with 5 or more players (24 for 5-6, 30 for 7-8).' },
    ],
  },
  {
    title: 'Setup',
    items: [
      {
        kind: 'num',
        key: 'starting_settlements',
        label: 'Starting settlements',
        unit: 'settlements',
        min: 1,
        max: 5,
        hint: 'Number of settlements players begin with. Always have one road per starting settlement.',
      },
    ],
  },
  {
    title: 'Robber and trading',
    items: [
      { kind: 'num', key: 'discard_limit', label: 'Card discard limit', unit: 'cards', min: 1, max: 50, hint: 'On a 7, anyone holding more than this discards half.' },
      {
        kind: 'toggle',
        key: 'friendly_robber',
        label: 'Friendly Robber',
        hint: `The robber can't go on a hex next to a player with ${FRIENDLY_ROBBER_MAX_VP} points or fewer.`,
      },
      {
        kind: 'num',
        key: 'bank_ratio',
        label: 'Bank trade ratio',
        unit: ': 1',
        min: 1,
        max: 8,
        show: (n) => `${n}:1`,
        hint: 'Standard resource-to-bank trade ratio. Harbours still give 3:1 and 2:1 where that is better.',
      },
    ],
  },
];

const ALL: Spec[] = GROUPS.flatMap((g) => g.items);

/** Allowed range for a number setting, given the others (e.g. every starting settlement needs a road). */
function limits(spec: NumSpec, s: Settings): [number, number] {
  switch (spec.key) {
    case 'starting_settlements':
      return [spec.min, Math.min(spec.max, s.max_roads, s.max_settlements)];
    case 'max_roads':
    case 'max_settlements':
      return [Math.max(spec.min, s.starting_settlements), spec.max];
    default:
      return [spec.min, spec.max];
  }
}

const shown = (spec: NumSpec, n: number): string => (spec.show ? spec.show(n) : String(n));

/** What changes if the rules go from `from` to `to`, as one short phrase per rule, in the order the rules
 *  panel lists them ("Points to win 12", "Friendly robber on"). Empty if they are the same. */
export function settingsDifferences(from: Settings, to: Settings): string[] {
  return ALL.filter((sp) => from[sp.key] !== to[sp.key]).map((sp) =>
    sp.kind === 'toggle' ? `${sp.label} ${to[sp.key] ? 'on' : 'off'}` : `${sp.label} ${shown(sp, to[sp.key])}`,
  );
}

/** How many settings differ from the standard. */
export function changedCount(s: Settings): number {
  return ALL.filter((sp) => s[sp.key] !== DEFAULT_SETTINGS[sp.key]).length;
}

/** Label and value for every setting, for showing the rules of a running game. */
export function settingsRows(s: Settings): { label: string; value: string; changed: boolean }[] {
  return ALL.map((sp) => ({
    label: sp.label,
    value: sp.kind === 'toggle' ? (s[sp.key] ? 'On' : 'Off') : shown(sp, s[sp.key]),
    changed: s[sp.key] !== DEFAULT_SETTINGS[sp.key],
  }));
}

/* ------------------------------------------------------------------ */

function Stepper({
  label,
  value,
  min,
  max,
  unit,
  onCommit,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  unit?: string;
  onCommit: (n: number) => void;
}) {
  const [draft, setDraft] = useState(String(value));
  // Follow the server: after a change (or a refusal) show the real value again.
  useEffect(() => setDraft(String(value)), [value]);
  const commit = () => {
    if (draft.trim() === '') return setDraft(String(value));
    const n = Math.min(max, Math.max(min, Math.round(Number(draft))));
    setDraft(String(n));
    if (n !== value) onCommit(n);
  };
  return (
    <div className="stepper">
      <button type="button" aria-label={`Decrease ${label}`} disabled={value <= min} onClick={() => onCommit(value - 1)}>
        −
      </button>
      <input
        type="text"
        inputMode="numeric"
        autoComplete="off"
        aria-label={`${label}, custom value (${min} to ${max})`}
        value={draft}
        onChange={(e) => setDraft(e.target.value.replace(/\D/g, '').slice(0, 2))}
        onBlur={commit}
        onKeyDown={(e) => e.key === 'Enter' && (e.currentTarget.blur(), e.preventDefault())}
      />
      <button type="button" aria-label={`Increase ${label}`} disabled={value >= max} onClick={() => onCommit(value + 1)}>
        +
      </button>
      {unit && <span className="unit">{unit}</span>}
    </div>
  );
}

function Choice({ name, label, on, onPick, children }: { name: string; label: string; on: boolean; onPick: () => void; children: string }) {
  return (
    <label className={'segment' + (on ? ' on' : '')}>
      <input type="radio" name={name} value={label} checked={on} onChange={onPick} />
      <span className="seg-name">{children}</span>
    </label>
  );
}

function NumberRow({ spec, s, onChange }: { spec: NumSpec; s: Settings; onChange: (patch: Partial<Settings>) => void }) {
  const value = s[spec.key];
  const std = DEFAULT_SETTINGS[spec.key];
  const [min, max] = limits(spec, s);
  // "Custom" is chosen by clicking it, or implied by a non-standard value. Once on, it stays
  // on while stepping (even back through the standard value) until Standard is clicked.
  const [forced, setForced] = useState(false);
  useEffect(() => {
    if (value !== std) setForced(true);
  }, [value, std]);
  const custom = forced || value !== std;
  const name = `setting-${spec.key}`;
  return (
    <div className={'srow' + (value !== std ? ' changed' : '')}>
      <div className="srow-text">
        <span className="srow-label">{spec.label}</span>
        {spec.hint && <small className="muted">{spec.hint}</small>}
      </div>
      <div className="srow-ctl">
        <div className="segments two" role="radiogroup" aria-label={spec.label}>
          <Choice
            name={name}
            label="standard"
            on={!custom}
            onPick={() => {
              setForced(false);
              if (value !== std) onChange({ [spec.key]: std });
            }}
          >
            {spec.standardLabel ?? shown(spec, std)}
          </Choice>
          <Choice name={name} label="custom" on={custom} onPick={() => setForced(true)}>
            Custom
          </Choice>
        </div>
        {custom && <Stepper label={spec.label} value={value} min={min} max={max} unit={spec.unit} onCommit={(n) => onChange({ [spec.key]: n })} />}
      </div>
    </div>
  );
}

function ToggleRow({ spec, s, onChange }: { spec: ToggleSpec; s: Settings; onChange: (patch: Partial<Settings>) => void }) {
  const on = s[spec.key];
  const name = `setting-${spec.key}`;
  return (
    <div className={'srow' + (on ? ' changed' : '')}>
      <div className="srow-text">
        <span className="srow-label">{spec.label}</span>
        {spec.hint && <small className="muted">{spec.hint}</small>}
      </div>
      <div className="srow-ctl">
        <div className="segments two" role="radiogroup" aria-label={spec.label}>
          <Choice name={name} label="off" on={!on} onPick={() => onChange({ [spec.key]: false })}>
            Off
          </Choice>
          <Choice name={name} label="on" on={on} onPick={() => onChange({ [spec.key]: true })}>
            On
          </Choice>
        </div>
      </div>
    </div>
  );
}

/**
 * Every game rule the host can set. Everyone sees the current values; only the host
 * can change them. Each change is sent to the server, which checks it and sends the
 * result back to the whole lobby.
 */
export function GameSettingsPanel({
  settings,
  host,
  onChange,
  onReset,
}: {
  settings: Settings;
  host: boolean;
  onChange: (patch: Partial<Settings>) => void;
  onReset: () => void;
}) {
  const n = changedCount(settings);
  return (
    <fieldset className="game-settings" disabled={!host}>
      <legend>Game rules</legend>
      <div className="gs-head">
        <span className="muted">
          {n === 0 ? 'Standard rules' : plural(n, 'rule') + ' changed from standard'}
          {!host && ' · only the host can change these'}
        </span>
        {host && (
          <button type="button" className="small" disabled={n === 0} onClick={onReset}>
            Reset to standard
          </button>
        )}
      </div>
      {GROUPS.map((g) => (
        <section key={g.title} className="gs-group" aria-label={g.title}>
          <h3>{g.title}</h3>
          {g.items.map((sp) =>
            sp.kind === 'num' ? (
              <NumberRow key={sp.key} spec={sp} s={settings} onChange={onChange} />
            ) : (
              <ToggleRow key={sp.key} spec={sp} s={settings} onChange={onChange} />
            ),
          )}
        </section>
      ))}
    </fieldset>
  );
}
