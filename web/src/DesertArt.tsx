// The desert hex: one big dune crest running through the middle, gentle low swells around it, fine wind ripples,
// and long shadows falling to the lower left (the sun is in the upper right). Pale, bleached sand, painted with
// soft gradients. It is drawn once in a 260 x 300 box (a hex 150 units from corner to centre) and scaled down to
// the hex, so the shapes are easy to tweak in one place.
import { useMemo } from 'react';

const L = '#F5EDDC'; // sunlit sand
const M = '#E9DAC0'; // mid sand
const SH = '#CBB38E'; // shadow side of a dune
const BOX_R = 150; // corner-to-centre of the box the dunes are drawn in
const INNER = 0.885; // the face of the hex, as a share of `scale` (same as the board's gradient polygon)

// [start y, bend, drop]: each crest runs from upper left to lower right, so shadows fall to its lower left.
const CRESTS: [number, number, number][] = [
  [10, 6, 60],
  [60, 34, 140],
  [190, 8, 60],
  [232, 6, 55],
];

const curve = (y: number, a: number, s: number) =>
  `M-20,${y.toFixed(1)} C60,${(y + a).toFixed(1)} 150,${(y + s / 2 - a * 0.6).toFixed(1)} 280,${(y + s).toFixed(1)}`;

function hexPts(r: number): string {
  return [0, 1, 2, 3, 4, 5]
    .map((i) => {
      const a = ((60 * i - 30) * Math.PI) / 180;
      return `${(r * Math.cos(a)).toFixed(1)},${(r * Math.sin(a)).toFixed(1)}`;
    })
    .join(' ');
}

// The ripples never change, so they are worked out once. Spacing and width are coarser than the mock-up
// because the hex is much smaller on the board.
const RIPPLES: { d: string; d2: string }[] = (() => {
  const out: { d: string; d2: string }[] = [];
  for (let k = 0, y = -30; y < 310; k++, y += 11) {
    const a = 5 * Math.sin(k * 1.7) + 3;
    out.push({ d: curve(y, a, 54), d2: curve(y + 3, a, 54) });
  }
  return out;
})();

/** Gradients and the clip for the desert. Ids are shared by everything in one document, so each <svg> passes its own `uid`. */
export function DesertDefs({ uid, scale = 64 }: { uid: string; scale?: number }) {
  return (
    <>
      <clipPath id={`${uid}-desert-clip`}>
        <polygon points={hexPts(scale * INNER)} />
      </clipPath>
      <radialGradient id={`${uid}-desert-bg`} cx="0.8" cy="0.1" r="1">
        <stop offset="0" stopColor="#ffffff" />
        <stop offset="1" stopColor={M} />
      </radialGradient>
      {CRESTS.map((_, i) => (
        <linearGradient key={i} id={`${uid}-desert-dune${i}`} x1="0.25" y1="0" x2="0.45" y2="1">
          <stop offset="0" stopColor={SH} />
          <stop offset="0.1" stopColor={SH} />
          <stop offset="0.35" stopColor={M} />
          <stop offset="1" stopColor={L} />
        </linearGradient>
      ))}
      {/* the soft patch of shade under the robber */}
      <radialGradient id={`${uid}-desert-shade`}>
        <stop offset="0" stopColor="#4A3B2C" stopOpacity="0.4" />
        <stop offset="0.6" stopColor="#4A3B2C" stopOpacity="0.2" />
        <stop offset="1" stopColor="#4A3B2C" stopOpacity="0" />
      </radialGradient>
    </>
  );
}

/** The dunes, centred on (cx, cy), clipped to the face of the hex. Needs `DesertDefs` with the same `uid` in the same <svg>. */
export function DesertArt({ cx, cy, uid, scale = 64 }: { cx: number; cy: number; uid: string; scale?: number }) {
  const k = (scale * INNER) / BOX_R;
  const ripples = useMemo(
    () =>
      RIPPLES.map((r, i) => (
        <g key={i} fill="none">
          <path d={r.d} stroke={SH} strokeOpacity="0.28" strokeWidth="1.6" />
          <path d={r.d2} stroke="#fff" strokeOpacity="0.45" strokeWidth="1.2" />
        </g>
      )),
    [],
  );
  return (
    <g transform={`translate(${cx} ${cy})`} pointerEvents="none">
      <g clipPath={`url(#${uid}-desert-clip)`}>
        <g transform={`scale(${k}) translate(-130 -150)`}>
          <rect x="-20" y="-20" width="300" height="340" fill={`url(#${uid}-desert-bg)`} />
          {CRESTS.map(([y, a, s], i) => (
            <path key={i} d={`${curve(y, a, s)} L280,340 L-20,340 Z`} fill={`url(#${uid}-desert-dune${i})`} />
          ))}
          {ripples}
        </g>
      </g>
    </g>
  );
}

/** A soft patch of shade on the sand under the robber, falling to the lower left like the dunes' shadows. `x`, `y` is the robber's base. */
export function DesertRobberShade({ x, y, uid }: { x: number; y: number; uid: string }) {
  return <ellipse cx={x - 9} cy={y + 3} rx="30" ry="10" fill={`url(#${uid}-desert-shade)`} pointerEvents="none" />;
}
