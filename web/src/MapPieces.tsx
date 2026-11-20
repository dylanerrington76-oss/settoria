// How the map editor draws its pieces. The terrain colours, glyphs and the harbour ship are the game board's own
// (see Board.tsx), so a map looks in the editor the way it will in a game. What is new is the red and black
// stripe: a hex or harbour drawn in stripes is one the game deals at random.
import { memo } from 'react';
import { Glyph, PortLabel, shade } from './art';
import { ART, GoldArt, RES_OF, STYLE, VolcanoArt, hexPoints } from './Board';
import { DesertArt, DesertDefs } from './DesertArt';
import { EdHex, PortKind, S } from './mapModel';
import type { Terrain } from './types';

export const STRIPE_RED = '#d8322b';
export const STRIPE_BLACK = '#1b1b1f';

const TERRAIN_LIST = Object.keys(STYLE) as Terrain[];

/**
 * The gradients and the stripe pattern the pieces use. Ids are shared by everything in one document, so each
 * <svg> that draws pieces gets its own `uid` and refers to its own copies.
 */
export function PieceDefs({ uid }: { uid: string }) {
  return (
    <defs>
      <DesertDefs uid={uid} scale={S} />
      {TERRAIN_LIST.map((t) => (
        <radialGradient key={t} id={`${uid}-g-${t}`} cx="50%" cy="46%" r="62%">
          <stop offset="0" stopColor={STYLE[t].center} />
          <stop offset="1" stopColor={STYLE[t].edge} />
        </radialGradient>
      ))}
      <pattern id={`${uid}-stripes`} patternUnits="userSpaceOnUse" width="26" height="26" patternTransform="rotate(45)">
        <rect width="26" height="13" fill={STRIPE_RED} />
        <rect y="13" width="26" height="13" fill={STRIPE_BLACK} />
      </pattern>
      <linearGradient id={`${uid}-sail`} x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" stopColor="#ffffff" />
        <stop offset="0.7" stopColor="#f4f4f2" />
        <stop offset="1" stopColor="#d2d7da" />
      </linearGradient>
      <linearGradient id={`${uid}-hull`} x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" stopColor="#f0b040" />
        <stop offset="1" stopColor="#d88a1c" />
      </linearGradient>
    </defs>
  );
}

/** One land hex centred on (0, 0), `S` units from corner to centre, as the game board draws it. */
export const HexShape = memo(function HexShape({ hex, uid }: { hex: Pick<EdHex, 'terrain' | 'number'>; uid: string }) {
  const t = hex.terrain;
  const st = t ? STYLE[t] : null;
  const res = t ? RES_OF[t] : null;
  const art = t ? ART[t] : null;
  const hot = hex.number === 6 || hex.number === 8;
  return (
    <g>
      <polygon points={hexPoints(0, 3, S)} fill="#000" opacity={0.22} />
      <polygon points={hexPoints(0, 0, S)} fill="#e3b86a" />
      <polygon points={hexPoints(0, 0, S * 0.935)} fill={st ? st.border : STRIPE_BLACK} />
      <polygon points={hexPoints(0, 0, S * 0.885)} fill={t ? `url(#${uid}-g-${t})` : `url(#${uid}-stripes)`} />
      {t === 'desert' && <DesertArt cx={0} cy={0} uid={uid} scale={S} />}
      <polygon points={hexPoints(0, 0, S * 0.845)} fill="none" stroke={st ? st.hi : '#ffffff'} strokeWidth={1.6} opacity={st ? 0.75 : 0.35} />
      {t && (t === 'pasture' || t === 'fields' || t === 'forest') &&
        [-1, 1].map((d) => (
          <path
            key={d}
            d={`M${d * 0.5 * S - 6},${-0.2 * S} l3,-6 l3,6 M${d * 0.5 * S - 1},${-0.2 * S} l3,-7 l3,7`}
            fill="none"
            stroke={shade(st!.edge, 0.2)}
            strokeWidth="1.6"
            strokeLinecap="round"
            strokeLinejoin="round"
            opacity="0.85"
          />
        ))}
      {t && (t === 'hills' || t === 'mountains') && (
        <g fill={shade(st!.edge, t === 'hills' ? -0.18 : -0.12)} opacity=".75">
          <ellipse cx={-0.55 * S} cy={-0.28 * S} rx="7" ry="3.2" />
          <ellipse cx={0.5 * S} cy={-0.3 * S} rx="6" ry="3" />
          <ellipse cx={0.58 * S} cy={-0.22 * S} rx="4" ry="2.4" />
        </g>
      )}
      {res !== null && art && (
        <g transform={`translate(0 ${-0.5 * S}) scale(${[1.0, 1.0, 1.12, 1.08, 1.15][res]}) translate(-20 -18)`}>
          <Glyph res={res} fill={art.fill} fill2={art.fill2} stroke={art.stroke} />
        </g>
      )}
      {t === 'volcano' && <VolcanoArt cx={0} cy={0} scale={S} lift={false} />}
      {t === 'gold' && <GoldArt cx={0} cy={0} scale={S} lift={false} />}
      {hex.number !== null && t !== 'desert' && (
        <g>
          <rect x={-21} y={0.31 * S - 20} width="42" height="40" rx="6.5" fill="#eae9e5" stroke="#00000033" />
          <text x={0} y={0.31 * S} dy="0.36em" textAnchor="middle" fontSize="29" fontWeight="800" fill={hot ? '#cc0000' : '#114613'}>
            {hex.number}
          </text>
        </g>
      )}
    </g>
  );
});

/** The planks from a ship to the two corners of its coast edge, in the same frame as the ship's position. */
export function Piers({ sx, sy, v1, v2 }: { sx: number; sy: number; v1: { x: number; y: number }; v2: { x: number; y: number } }) {
  return (
    <>
      {[v1, v2].map((v, k) => {
        const dx = v.x - sx;
        const dy = v.y - sy;
        const len = Math.hypot(dx, dy) || 1;
        const ux = dx / len;
        const uy = dy / len;
        const x1 = sx + ux * 24;
        const y1 = sy + uy * 24;
        const x2 = v.x - ux * 3;
        const y2 = v.y - uy * 3;
        return (
          <g key={k}>
            <line x1={x1} y1={y1} x2={x2} y2={y2} stroke="#b36a14" strokeWidth={15} />
            <line x1={x1} y1={y1} x2={x2} y2={y2} stroke="#eba233" strokeWidth={12.5} />
            <line x1={x1} y1={y1} x2={x2} y2={y2} stroke="#c47a18" strokeWidth={12.5} strokeDasharray="1.2 5" />
          </g>
        );
      })}
    </>
  );
}

const RES_OF_KIND: Record<Exclude<PortKind, 'any'>, number> = { brick: 0, lumber: 1, ore: 2, grain: 3, wool: 4 };

/** The harbour ship, centred where the game puts it. A random harbour has a striped sail with a "?". */
export const Ship = memo(function Ship({ kind, uid }: { kind: PortKind | null; uid: string }) {
  return (
    <g transform="translate(0 -21) scale(0.92)">
      <ellipse cx="2" cy="30" rx="21" ry="4.5" fill="#7ed0f6" />
      <path d="M-20 14 L22 11 Q23 25 5 29 Q-14 29 -20 14Z" fill={`url(#${uid}-hull)`} stroke="#a8620f" strokeWidth="1.4" strokeLinejoin="round" />
      <path d="M-17 15 Q0 22 19 13" fill="none" stroke="#fff3c9" strokeWidth="1.4" opacity=".6" />
      <rect x="-2.5" y="-35" width="5" height="12" rx="1.5" fill="#d98a1c" />
      <path d="M2 -33 L12 -33 L10 -31 L24 -31 L24 -28 L2 -28Z" fill="#fff" stroke="#d3d8db" strokeWidth="0.8" />
      <path d="M-20 -18 C-26 -4 -24 12 -10 18 C4 23 17 20 21 12 L21 -18 Z" fill={kind === null ? `url(#${uid}-stripes)` : `url(#${uid}-sail)`} stroke="#c3c9cc" strokeWidth="1.2" />
      <path d="M-21 -23 H22 V-18 q-3.6 4.6 -7.2 0 q-3.6 4.6 -7.2 0 q-3.6 4.6 -7.2 0 q-3.6 4.6 -7.2 0 q-3.6 4.6 -7.2 0Z" fill="#eba233" stroke="#b46c12" strokeWidth="1" />
      {kind === null ? (
        <text x="0.5" y="6" textAnchor="middle" fontSize="30" fontWeight="800" fill="#fff" stroke="#111" strokeWidth="3.2" paintOrder="stroke" strokeLinejoin="round">
          ?
        </text>
      ) : (
        <PortLabel res={kind === 'any' ? null : RES_OF_KIND[kind]} />
      )}
    </g>
  );
});
