import type { ReactNode } from 'react';
import { luminance } from './types';

/**
 * Shared vector art: resource glyphs, cards, building pieces, avatars and the
 * small icons used by the toolbar and action bar. Everything is inline SVG so
 * the client stays a single self-contained bundle.
 */

/** Display order of resources in hands and the bank: lumber, brick, wool, grain, ore. */
export const RES_ORDER = [1, 0, 4, 3, 2] as const;

interface Tone {
  top: string;
  bottom: string;
  edge: string;
  glyph: string;
  glyph2: string;
}
/** Card colors per resource index [brick, lumber, ore, grain, wool]. */
export const TONES: Tone[] = [
  { top: '#ee8c7f', bottom: '#d9584a', edge: '#a53a30', glyph: '#fbd3c6', glyph2: '#f4a898' }, // brick
  { top: '#43d44c', bottom: '#1ea32f', edge: '#14701f', glyph: '#1d7a2b', glyph2: '#7de07f' }, // lumber
  { top: '#a9cfc5', bottom: '#82aea3', edge: '#587d73', glyph: '#f4faf8', glyph2: '#c7ddd7' }, // ore
  { top: '#f9bd4a', bottom: '#ec9a1d', edge: '#a96a0c', glyph: '#8d5a10', glyph2: '#ffe29a' }, // grain
  { top: '#b4e23b', bottom: '#8cc31a', edge: '#5e8710', glyph: '#ffffff', glyph2: '#e8f5c0' }, // wool
];
export const DEV_TONE = { top: '#ad84dc', bottom: '#8a5cc4', edge: '#5d3b8c' };
export const UNKNOWN_TONE = { top: '#2d86e6', bottom: '#1346ad', edge: '#0d2f78' };

/* Resource glyphs, drawn in a 40x40 box                               */
export function Glyph({ res, fill, fill2, stroke }: { res: number; fill: string; fill2?: string; stroke?: string }) {
  const f2 = fill2 ?? fill;
  const s = stroke ? { stroke, strokeWidth: 1.2, strokeLinejoin: 'round' as const } : {};
  switch (res) {
    case 0: // brick: three courses of bricks
      return (
        <g {...s}>
          <rect x="3" y="8" width="16" height="7.5" rx="1.6" fill={fill} />
          <rect x="21" y="8" width="16" height="7.5" rx="1.6" fill={fill} />
          <rect x="-2" y="17.5" width="12" height="7.5" rx="1.6" fill={f2} />
          <rect x="12" y="17.5" width="16" height="7.5" rx="1.6" fill={fill} />
          <rect x="30" y="17.5" width="12" height="7.5" rx="1.6" fill={f2} />
          <rect x="3" y="27" width="16" height="7.5" rx="1.6" fill={fill} />
          <rect x="21" y="27" width="16" height="7.5" rx="1.6" fill={fill} />
        </g>
      );
    case 1: // lumber: pine tree
      return (
        <g {...s}>
          <rect x="17.5" y="32" width="5" height="6" rx="1" fill={f2} />
          <path d="M20 2 L28.5 13 H24.5 L32.5 23.5 H27 L36 34 H4 L13 23.5 H7.5 L15.5 13 H11.5 Z" fill={fill} />
        </g>
      );
    case 2: // ore: four lumpy boulders
      return (
        <g {...s}>
          <path d="M4 25 Q2 18 9 16.5 Q15 14.5 19 19 Q22 25 17 29.5 Q9 32 4 25Z" fill={fill} />
          <path d="M16 16 Q15 8.5 22 7 Q29 6.5 31 12 Q33 18.5 27 21 Q19 22 16 16Z" fill={f2} />
          <path d="M21 26 Q20 20 27 19 Q35 18.5 37 24.5 Q38 31 31 33 Q23 33.5 21 26Z" fill={fill} />
          <path d="M9 33 Q8 28.5 14 28 H24 Q26 32 22 34.5 Q14 36.5 9 33Z" fill={f2} />
        </g>
      );
    case 3: // grain: ear of wheat
      return (
        <g {...s}>
          <path d="M20 38 V14" stroke={fill} strokeWidth="2.4" strokeLinecap="round" fill="none" />
          <ellipse cx="20" cy="7" rx="2.8" ry="5" fill={fill} />
          {[
            [14.2, 14, -38],
            [13.4, 21.5, -38],
            [13, 29, -38],
            [25.8, 14, 38],
            [26.6, 21.5, 38],
            [27, 29, 38],
          ].map(([x, y, r], i) => (
            <ellipse key={i} cx={x} cy={y} rx="2.8" ry="5.2" transform={`rotate(${r} ${x} ${y})`} fill={i % 2 ? f2 : fill} />
          ))}
        </g>
      );
    default: // wool: sheep
      return (
        <g {...s}>
          <rect x="11.5" y="27" width="3" height="8" rx="1.2" fill="#4b4b4b" stroke="none" />
          <rect x="25" y="27" width="3" height="8" rx="1.2" fill="#4b4b4b" stroke="none" />
          <ellipse cx="18" cy="23" rx="13" ry="8" fill={fill} />
          <circle cx="9.5" cy="19" r="5.5" fill={fill} />
          <circle cx="17" cy="15" r="6.5" fill={fill} />
          <circle cx="25.5" cy="18" r="5.5" fill={fill} />
          <ellipse cx="32.5" cy="22" rx="5" ry="4.6" fill="#6a6f73" stroke="none" />
          <ellipse cx="29.6" cy="17.6" rx="2.4" ry="1.6" fill="#6a6f73" stroke="none" />
        </g>
      );
  }
}

/* Cards */
let gid = 0;
const useId = (p: string) => `${p}${++gid}`;

function CardDefs({ id, top, bottom }: { id: string; top: string; bottom: string }) {
  return (
    <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stopColor={top} />
      <stop offset="1" stopColor={bottom} />
    </linearGradient>
  );
}

/** Face of a resource card in a 44x60 box (no count badge). */
export function CardFace({ res }: { res: number | 'dev' | 'unknown' }) {
  const id = useId('cf');
  const tone = res === 'dev' ? DEV_TONE : res === 'unknown' ? UNKNOWN_TONE : TONES[res];
  return (
    <svg viewBox="0 0 44 60" className="card-svg" aria-hidden="true">
      <defs>
        <CardDefs id={id} top={tone.top} bottom={tone.bottom} />
      </defs>
      <rect x="1.1" y="1.1" width="41.8" height="57.8" rx="4" fill={`url(#${id})`} stroke="#3b3b3b" strokeWidth="2.2" />
      <rect x="3.4" y="3.4" width="37.2" height="53.2" rx="2.4" fill="none" stroke={tone.top} strokeWidth="1.2" opacity=".85" />
      {res === 'dev' ? (
        <DevEmblem />
      ) : res === 'unknown' ? (
        <>
          
          <text x="21" y="46" textAnchor="middle" fontSize="35" fontWeight="800" fill="#8fd6ff" stroke="#e8f7ff" strokeWidth="0.8" fontFamily="inherit">
            ?
          </text>
        </>
      ) : (
        <g transform={`translate(22 35) scale(${[0.9, 0.92, 1.12, 1.0, 1.1][res]}) translate(-20 -20)`}>
          <Glyph res={res} fill={TONES[res].glyph} fill2={TONES[res].glyph2} stroke={res === 1 ? '#0f5f1b' : undefined} />
        </g>
      )}
    </svg>
  );
}

/** Round "field and hammer" emblem used on development cards. */
function DevEmblem() {
  return (
    <g>
      <circle cx="22" cy="34" r="15.5" fill="#6ec3ee" stroke="#1d3f7a" strokeWidth="1.6" />
      <path d="M7 36 Q22 28 37 36 A15.5 15.5 0 0 1 7 36Z" fill="#79c23a" />
      <path d="M13 45 L29 25" stroke="#9a5a2c" strokeWidth="3.2" strokeLinecap="round" />
      <path d="M26 21 L35 27 L31 31 L22 25 Z" fill="#dfe6ea" stroke="#51606a" strokeWidth="1.2" strokeLinejoin="round" />
    </g>
  );
}

/** A resource card with the blue count badge in its top-right corner. */
export function CountCard({
  res,
  n,
  stack = 0,
  alert,
  className = '',
}: {
  res: number | 'dev' | 'unknown';
  n: number;
  stack?: number;
  alert?: boolean;
  className?: string;
}) {
  const tone = alert || res === 'unknown' ? UNKNOWN_TONE : res === 'dev' ? DEV_TONE : TONES[res];
  return (
    <span className={'cc ' + className} style={{ ['--tone' as string]: tone.bottom, ['--edge' as string]: tone.edge }}>
      {Array.from({ length: stack }).map((_, i) => (
        <span key={i} className="cc-under" style={{ left: -(stack - i) * 7 }} />
      ))}
      {alert ? <CardFace res="unknown" /> : <CardFace res={res} />}
      {alert && <span className="cc-alert">!</span>}
      <span className="cc-badge">
        <b>{n}</b>
      </span>
    </span>
  );
}

/** Tiny card used inline in the log. */
export function MiniCard({ res }: { res: number | 'dev' }) {
  return (
    <span className="mini-card">
      <CardFace res={res} />
    </span>
  );
}

/* Building pieces */
export function shade(hex: string, amt: number): string {
  const n = parseInt(hex.slice(1), 16);
  const c = (v: number) => Math.max(0, Math.min(255, Math.round(v + (amt < 0 ? v * amt : (255 - v) * amt))));
  const r = c((n >> 16) & 255);
  const g = c((n >> 8) & 255);
  const b = c(n & 255);
  return '#' + ((1 << 24) | (r << 16) | (g << 8) | b).toString(16).slice(1);
}

/** Black pieces get a light outline so they don't vanish into their own dark edge. */
export const isVeryDark = (color: string): boolean => luminance(color) < 0.02;
export const pieceEdge = (color: string): string => (isVeryDark(color) ? '#f4f4f4' : '#0b0b0b');

/** Settlement / city draw */
export function PieceShape({ color, city }: { color: string; city?: boolean }) {
  const roof = shade(color, 0.22);
  const wall = shade(color, -0.12);
  const edge = pieceEdge(color);
  const door = isVeryDark(color) ? '#8c8c8c' : '#101010';
  if (city) {
    return (
      <g strokeLinejoin="round" stroke={edge} strokeWidth="2">
        <path d="M-17 14 V-2 H-3 V14Z" fill={wall} />
        <path d="M-3 14 V2 H17 V14Z" fill={wall} />
        <path d="M-19 -2 L-10 -16 L-1 -2Z" fill={roof} />
        <path d="M-5 2 L4 -6 H12 L19 2Z" fill={roof} />
        <path d="M-10 -16 L-1 -2 H-3 Z" fill={color} stroke="none" opacity=".7" />
        <path d="M-12 14 V8 Q-10 5 -8 8 V14Z" fill={door} stroke="none" />
        <path d="M8 14 V9 Q10 6 12 9 V14Z" fill={door} stroke="none" />
      </g>
    );
  }
  return (
    <g strokeLinejoin="round" stroke={edge} strokeWidth="1.9">
      <path d="M-15 15 V-1 L-4 -14 L15 -3 V15Z" fill={color} />
      <path d="M-15 15 V-1 L-4 -14 L-1 -12 V15Z" fill="#fff" opacity=".3" stroke="none" />
      <path d="M-4 -14 L15 -3 V15 H-1Z" fill="#000" opacity=".14" stroke="none" />
      <path d="M-15 -1 L-1 -12 L15 -3" fill="none" strokeWidth="1.2" opacity=".5" />
      <path d="M-3.6 15 V8.5 Q-0.6 5 2.6 8.5 V15Z" fill={door} stroke="none" />
    </g>
  );
}

/** Inline mini piece for the log / action bar. */
export function PieceIcon({ color, kind }: { color: string; kind: 'settlement' | 'city' | 'road' }) {
  if (kind === 'road') {
    return (
      <span className="road-pip" style={{ background: `linear-gradient(90deg, ${shade(color, 0.3)}, ${color})`, borderColor: shade(color, -0.45) }} />
    );
  }
  return (
    <svg viewBox="-20 -19 40 36" className="piece-pip" aria-hidden="true">
      <PieceShape color={color} city={kind === 'city'} />
    </svg>
  );
}

/* People and stats */
export function PersonGlyph({ fill = '#fff', stroke = '#9a9a9a' }: { fill?: string; stroke?: string }) {
  return (
    <g fill={fill} stroke={stroke} strokeWidth="1.6" strokeLinejoin="round">
      <circle cx="20" cy="13" r="7.6" />
      <path d="M6 35 Q6 22.5 20 22.5 Q34 22.5 34 35 Z" />
    </g>
  );
}

/** Small round log/pill avatar. */
export function PersonIcon({ size = 18, color = '#fff', stroke = '#8b8b8b' }: { size?: number; color?: string; stroke?: string }) {
  return (
    <svg viewBox="0 0 40 40" width={size} height={size} aria-hidden="true" className="person-ico">
      <PersonGlyph fill={color} stroke={stroke} />
    </svg>
  );
}

export function Avatar({ color, self, size = 44 }: { color: string; self?: boolean; size?: number }) {
  const id = useId('av');
  // The silhouette is white on a colored disc; on light colors (white, gold, silver...) it would vanish.
  const light = luminance(color) > 0.3;
  return (
    <svg viewBox="0 0 80 80" width={size} height={size} className="avatar" aria-hidden="true">
      <defs>
        <radialGradient id={id} cx="50%" cy="38%" r="70%">
          <stop offset="0" stopColor={shade(color, 0.35)} />
          <stop offset="1" stopColor={color} />
        </radialGradient>
      </defs>
      {self ? (
        <>
          <circle cx="40" cy="40" r="38" fill={`url(#${id})`} stroke={shade(color, -0.25)} strokeWidth="3" />
          <circle cx="40" cy="40" r="34" fill="none" stroke={light ? '#333' : '#fff'} strokeWidth="3" opacity={light ? 0.6 : 0.95} />
          <g transform="translate(14 12) scale(1.3)">
            <PersonGlyph fill={light ? '#333' : '#fff'} stroke="none" />
          </g>
        </>
      ) : (
        <>
          <circle cx="40" cy="40" r="38" fill="#fff" stroke={color} strokeWidth="6" />
          <circle cx="40" cy="40" r="39.3" fill="none" stroke="#000" strokeWidth="1.4" opacity=".55" />
          <circle cx="40" cy="40" r="33" fill="none" stroke={shade(color, -0.3)} strokeWidth="1.5" opacity=".7" />
          <g transform="translate(14 13) scale(1.3)">
            <PersonGlyph fill="#f4f4f4" stroke="#8a8a8a" />
          </g>
        </>
      )}
    </svg>
  );
}

/** Cream ribbon under an avatar that shows the victory points. */
export function Ribbon({ n }: { n: number }) {
  return (
    <span className="ribbon">
      <svg viewBox="0 0 64 24" preserveAspectRatio="none" aria-hidden="true">
        <path d="M0 6 L9 6 L9 20 L0 20 L5 13Z" fill="#cfc7b4" stroke="#a79f8c" strokeWidth="1" />
        <path d="M64 6 L55 6 L55 20 L64 20 L59 13Z" fill="#cfc7b4" stroke="#a79f8c" strokeWidth="1" />
        <rect x="7" y="1" width="50" height="19" rx="2" fill="#f6f2e8" stroke="#bdb5a1" strokeWidth="1" />
      </svg>
      <b>{n}</b>
    </span>
  );
}

export function KnightsIcon() {
  const pawn = (x: number, y: number, s = 1) => (
    <g transform={`translate(${x} ${y}) scale(${s})`}>
      <circle cx="0" cy="-8" r="5" />
      <path d="M-8 9 Q-8 -1 0 -1 Q8 -1 8 9 Z" />
    </g>
  );
  return (
    <svg viewBox="0 0 40 36" className="stat-ico" aria-hidden="true">
      <g fill="#c9c8c3" stroke="#a9a8a3" strokeWidth="1.3" strokeLinejoin="round">
        {pawn(12, 15, 0.95)}
        {pawn(28, 15, 0.95)}
        {pawn(20, 21, 1.1)}
      </g>
    </svg>
  );
}

export function RoadLenIcon() {
  return (
    <svg viewBox="0 0 40 36" className="stat-ico" aria-hidden="true">
      <path d="M4 28 Q20 -2 36 28" fill="none" stroke="#a9a8a3" strokeWidth="7" strokeDasharray="6.5 2.5" strokeLinecap="butt" />
      <path d="M4 28 Q20 -2 36 28" fill="none" stroke="#d5d4cf" strokeWidth="3.4" strokeDasharray="6.5 2.5" strokeLinecap="butt" />
    </svg>
  );
}

export function BankIcon() {
  return (
    <svg viewBox="0 0 48 48" className="bank-ico" aria-hidden="true">
      <path d="M24 5 L45 15 V19 H3 V15 Z" fill="#d9a074" stroke="#6b3f26" strokeWidth="1.8" strokeLinejoin="round" />
      <circle cx="24" cy="13" r="3.3" fill="#f7e2b4" stroke="#8a5a2c" strokeWidth="1.2" />
      {[8, 17.5, 27, 36.5].map((x) => (
        <rect key={x} x={x} y="21" width="4.6" height="17" rx="1" fill="#f0dcc0" stroke="#6b3f26" strokeWidth="1.4" />
      ))}
      <rect x="4" y="39" width="40" height="4.5" rx="1" fill="#d9a074" stroke="#6b3f26" strokeWidth="1.6" />
    </svg>
  );
}

/* Dice */
const PIPS: Record<number, [number, number][]> = {
  1: [[50, 50]],
  2: [[26, 26], [74, 74]],
  3: [[26, 26], [50, 50], [74, 74]],
  4: [[26, 26], [74, 26], [26, 74], [74, 74]],
  5: [[26, 26], [74, 26], [50, 50], [26, 74], [74, 74]],
  6: [[26, 24], [74, 24], [26, 50], [74, 50], [26, 76], [74, 76]],
};

export function DieFace({ n, r = 13.5 }: { n: number; r?: number }) {
  return (
    <svg viewBox="0 0 100 100" className="die-svg" aria-hidden="true">
      {PIPS[n].map(([x, y], i) => (
        <circle key={i} cx={x} cy={y} r={r} fill="#111" />
      ))}
    </svg>
  );
}

/* Toolbar + action bar icons */
export const Chevron = ({ dir = 'up' }: { dir?: 'up' | 'down' | 'left' }) => (
  <svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true" style={{ transform: `rotate(${dir === 'up' ? 0 : dir === 'down' ? 180 : -90}deg)` }}>
    <path d="M3.5 13 L10 6.5 L16.5 13" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);

export function ToolIcon({ kind }: { kind: 'gear' | 'book' | 'expand' | 'info' }) {
  const common = { fill: '#fff', stroke: '#0a4f80', strokeWidth: 1.6, strokeLinejoin: 'round' as const };
  let body: ReactNode;
  if (kind === 'gear') {
    const teeth = Array.from({ length: 8 }).map((_, i) => (
      <rect key={i} x="-3.4" y="-17" width="6.8" height="7" rx="1.4" transform={`rotate(${i * 45})`} {...common} />
    ));
    body = (
      <g transform="translate(20 20)">
        {teeth}
        <circle r="11.5" {...common} />
        <circle r="5.2" fill="#0a5b92" stroke="#0a4f80" strokeWidth="1.4" />
      </g>
    );
  } else if (kind === 'book') {
    body = (
      <g {...common}>
        <path d="M3 9 Q11 5 19.5 9.5 V33 Q11 29 3 32.5Z" />
        <path d="M37 9 Q29 5 20.5 9.5 V33 Q29 29 37 32.5Z" />
        <path d="M7 14 Q11 12.5 15.5 14.5 M7 19 Q11 17.5 15.5 19.5 M7 24 Q11 22.5 15.5 24.5 M33 14 Q29 12.5 24.5 14.5 M33 19 Q29 17.5 24.5 19.5 M33 24 Q29 22.5 24.5 24.5" fill="none" stroke="#7fb5d6" strokeWidth="1.4" />
      </g>
    );
  } else if (kind === 'expand') {
    body = (
      <g fill="none" stroke="#fff" strokeWidth="3.4" strokeLinecap="round" strokeLinejoin="round">
        <path d="M5 14 V5 H14 M26 5 H35 V14 M35 26 V35 H26 M14 35 H5 V26" />
      </g>
    );
  } else {
    body = (
      <g>
        <circle cx="20" cy="20" r="16.5" {...common} />
        <circle cx="20" cy="11.8" r="2.6" fill="#0a5b92" />
        <path d="M17 17 H22 V28 H24 V30.5 H16 V28 H18 V19.5 H17Z" fill="#0a5b92" />
      </g>
    );
  }
  return (
    <svg viewBox="0 0 40 40" width="34" height="34" aria-hidden="true">
      {body}
    </svg>
  );
}

const BR = '#8a3b2e';
const BR2 = '#b9604e';

export function ActionIcon({ kind }: { kind: 'trade' | 'dev' | 'road' | 'settlement' | 'city' | 'turn' }) {
  let body: ReactNode;
  switch (kind) {
    case 'trade':
      body = (
        <>
          <rect x="8" y="7" width="19" height="25" rx="2.5" fill="#6fa06f" stroke="#3f6b45" strokeWidth="1.8" />
          <path d="M17.5 14 C13 15 12 22 17.5 26 C22 22 22 15 17.5 14Z" fill="#d6ead6" />
          <rect x="22" y="22" width="19" height="25" rx="2.5" fill="#8c9aa5" stroke="#4e5b66" strokeWidth="1.8" />
          <path d="M26 30 H37 M26 35 H37 M26 40 H34" stroke="#e8eef2" strokeWidth="2" strokeLinecap="round" />
          <path d="M30 10 Q43 11 42 22" fill="none" stroke="#fff" strokeWidth="3" strokeLinecap="round" />
          <path d="M39 20 L42 25 L45 20Z" fill="#fff" stroke="#fff" strokeWidth="1.4" strokeLinejoin="round" />
          <path d="M18 44 Q5 43 6 33" fill="none" stroke="#fff" strokeWidth="3" strokeLinecap="round" />
          <path d="M3 35 L6 30 L9 35Z" fill="#fff" stroke="#fff" strokeWidth="1.4" strokeLinejoin="round" />
        </>
      );
      break;
    case 'dev':
      body = (
        <>
          <rect x="9" y="3" width="30" height="42" rx="3" fill="#2b4d8f" stroke="#17306a" strokeWidth="2" />
          <circle cx="24" cy="24" r="11" fill="#7fd0ea" stroke="#ecfbff" strokeWidth="1.8" />
          <path d="M13.5 26 Q24 20 34.5 26 A11 11 0 0 1 13.5 26Z" fill="#79c23a" />
          <path d="M18 33 L30 17" stroke="#8d4f28" strokeWidth="2.6" strokeLinecap="round" />
          <path d="M28 13.5 L35 18.5 L32 21.5 L25 16.5Z" fill="#dfe6ea" stroke="#51606a" strokeWidth="1" strokeLinejoin="round" />
        </>
      );
      break;
    case 'road':
      body = (
        <>
          <rect x="19" y="3" width="10" height="42" rx="3" fill={BR} stroke="#5a2118" strokeWidth="2" />
          <rect x="21" y="5" width="3" height="38" rx="1.5" fill={BR2} opacity=".8" />
        </>
      );
      break;
    case 'settlement':
      body = (
        <g strokeLinejoin="round" stroke="#5a2118" strokeWidth="2">
          <path d="M9 44 V23 H39 V44Z" fill={BR} />
          <path d="M5 24 L15 5 H33 L43 24Z" fill={BR2} />
          <path d="M5 24 L15 5 H24 V24Z" fill="#cf7b68" stroke="none" />
          <path d="M5 24 L15 5 H33 L43 24Z" fill="none" />
          <path d="M20.5 44 V35 Q24 31 27.5 35 V44Z" fill="#3a130d" stroke="none" />
        </g>
      );
      break;
    case 'city':
      body = (
        <g strokeLinejoin="round" stroke="#5a2118" strokeWidth="2">
          <path d="M4 44 V19 H20 V44Z" fill={BR} />
          <path d="M20 44 V28 H45 V44Z" fill={BR} />
          <path d="M1 20 L12 3 L23 20Z" fill={BR2} />
          <path d="M18 29 L28 19 H37 L47 29Z" fill={BR2} />
          <path d="M8 44 V34 Q12 30 16 34 V44Z" fill="#3a130d" stroke="none" />
          <path d="M30 44 V36 Q34 32 38 36 V44Z" fill="#3a130d" stroke="none" />
        </g>
      );
      break;
    default:
      body = (
        <>
          <rect x="11" y="3" width="26" height="4.5" rx="2" fill="#3d4b57" />
          <rect x="11" y="40.5" width="26" height="4.5" rx="2" fill="#3d4b57" />
          <path d="M14 7.5 H34 Q34 20 25.5 24 Q34 28 34 40.5 H14 Q14 28 22.5 24 Q14 20 14 7.5Z" fill="#d6eefa" stroke="#3d4b57" strokeWidth="2.2" strokeLinejoin="round" />
          <path d="M18 36 H30 L26.5 30 H21.5Z" fill="#6f7f8c" />
          <path d="M18.5 11 H29.5 Q28.5 17 25 20 H23 Q19.5 17 18.5 11Z" fill="#6f7f8c" opacity=".55" />
        </>
      );
  }
  return (
    <svg viewBox="0 0 48 48" width="50" height="50" aria-hidden="true">
      {body}
    </svg>
  );
}

export function CheckIcon() {
  return (
    <svg viewBox="0 0 40 40" width="30" height="30" aria-hidden="true">
      <path d="M7 21 L16 30 L33 10" fill="none" stroke="#fff" strokeWidth="5.2" strokeLinecap="round" strokeLinejoin="round" />
      <path d="M7 21 L16 30 L33 10" fill="none" stroke="#7a9aa4" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" opacity=".55" transform="translate(0 1.5)" />
    </svg>
  );
}

export function DiscardArrow() {
  return (
    <svg viewBox="0 0 30 24" width="30" height="24" aria-hidden="true">
      <path d="M15 1 L28 13 H20 V23 H10 V13 H2Z" fill="#e8333a" stroke="#9c1b20" strokeWidth="1.6" strokeLinejoin="round" />
    </svg>
  );
}