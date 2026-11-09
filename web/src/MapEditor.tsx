// The map editor page (/map-editor). Drag striped hexes, deserts and ports from the bar at the bottom onto the
// sea, move them around, and download the result as a map file the lobby accepts. An existing map file can be
// opened first. What the pieces mean and how the file is written is in mapModel.ts.
import { ChangeEvent, DragEvent, PointerEvent as ReactPointerEvent, memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { hexPoints } from './Board';
import {
  EdHex,
  EdPort,
  Layout,
  MAX_HEXES,
  MAX_MAP_BYTES,
  MAX_NAME_CHARS,
  Meta,
  S,
  STEPS,
  cellKey,
  checkMap,
  defaultMeta,
  fileNameFor,
  freeCoastEdges,
  hexCenter,
  inRange,
  mapFileText,
  maxRandomDeserts,
  nearestCoastEdge,
  normalizePorts,
  parseMapJson,
  pixelToHex,
  planPools,
  portLayout,
  resolvePlayers,
} from './mapModel';
import { HexShape, PieceDefs, Piers, Ship } from './MapPieces';
import { linkClick } from './route';

const MIN_K = 0.12;
const MAX_K = 2.2;
const START_K = 0.8;
const HISTORY = 100;

interface View {
  x: number;
  y: number;
  k: number;
}
type Piece = 'wild' | 'desert' | 'port';
type Target = { t: 'cell'; q: number; r: number; ok: boolean } | { t: 'edge'; q: number; r: number; side: number } | { t: 'bar' } | { t: 'none' };

/** A piece being dragged: new from the bar (`id` null) or one already on the map. Positions are screen pixels. */
interface Drag {
  kind: 'hex' | 'port';
  id: number | null;
  hex: EdHex;
  port: EdPort;
  startX: number;
  startY: number;
  x: number;
  y: number;
  moved: boolean;
  target: Target;
}
interface Notice {
  tone: 'error' | 'info';
  title: string;
  lines: string[];
}

const clampK = (k: number) => Math.min(MAX_K, Math.max(MIN_K, k));
const plural = (n: number, one: string, many = one + 's') => `${n} ${n === 1 ? one : many}`;

// ------------------------------------------------------------------------------------------ world pieces

const HexTile = memo(function HexTile({ hex, selected, onDown }: { hex: EdHex; selected: boolean; onDown: (e: ReactPointerEvent, hex: EdHex) => void }) {
  const c = hexCenter(hex.q, hex.r);
  return (
    <g transform={`translate(${c.x} ${c.y})`} className="me-piece" onPointerDown={(e) => onDown(e, hex)}>
      <HexShape hex={hex} uid="w" />
      {selected && <polygon points={hexPoints(0, 0, S * 0.975)} fill="none" stroke="#fff" strokeWidth={5} strokeDasharray="12 7" strokeLinejoin="round" pointerEvents="none" />}
    </g>
  );
});

const PortTile = memo(function PortTile({ port, selected, onDown }: { port: EdPort; selected: boolean; onDown: (e: ReactPointerEvent, port: EdPort) => void }) {
  const p = portLayout(port.q, port.r, port.side);
  return (
    <g className="me-piece" onPointerDown={(e) => onDown(e, port)}>
      <g pointerEvents="none">
        <Piers sx={p.sx} sy={p.sy} v1={p.v1} v2={p.v2} />
      </g>
      <g transform={`translate(${p.sx} ${p.sy})`}>
        <circle r={38} fill="transparent" />
        <Ship kind={port.kind} uid="w" />
        {selected && <circle r={40} cy={-6} fill="none" stroke="#fff" strokeWidth={4} strokeDasharray="10 6" pointerEvents="none" />}
      </g>
    </g>
  );
});

/** The pale shallows round every hex, drawn in two passes so neighbours merge instead of piling up. */
const Shore = memo(function Shore({ hexes }: { hexes: EdHex[] }) {
  return (
    <g pointerEvents="none">
      {hexes.map((h) => {
        const c = hexCenter(h.q, h.r);
        return <polygon key={h.id} points={hexPoints(c.x, c.y, S * 1.34)} fill="#2a85bf" />;
      })}
      {hexes.map((h) => {
        const c = hexCenter(h.q, h.r);
        return <polygon key={h.id} points={hexPoints(c.x, c.y, S * 1.14)} fill="#63bde6" />;
      })}
    </g>
  );
});

// ------------------------------------------------------------------------------------------- tiles

/** What the bar offers. Each is drawn from the same shapes the map uses. */
const TILES: { piece: Piece; label: string; hint: string }[] = [
  { piece: 'wild', label: 'Hex', hint: 'any resource or desert' },
  { piece: 'port', label: 'Port', hint: 'on the coast' },
  { piece: 'desert', label: 'Desert', hint: 'always a desert' },
];

function TileArt({ piece }: { piece: Piece }) {
  const uid = `t-${piece}`;
  return (
    <svg viewBox="-70 -76 140 152" width="64" height="70" aria-hidden="true">
      <PieceDefs uid={uid} />
      {piece === 'port' ? (
        <g transform="translate(0 36) scale(1.7)">
          <Ship kind={null} uid={uid} />
        </g>
      ) : (
        <HexShape hex={{ terrain: piece === 'desert' ? 'desert' : null, number: null }} uid={uid} />
      )}
    </svg>
  );
}

const newHex = (terrain: EdHex['terrain']): EdHex => ({ id: 0, q: 0, r: 0, terrain, number: null });
const newPort = (): EdPort => ({ id: 0, q: 0, r: 0, side: 0, kind: null });

// ------------------------------------------------------------------------------------------- the page

export default function MapEditor({ onBack }: { onBack: () => void }) {
  const [layout, setLayout] = useState<Layout>({ hexes: [], ports: [] });
  const [past, setPast] = useState<Layout[]>([]);
  const [future, setFuture] = useState<Layout[]>([]);
  const [meta, setMeta] = useState<Meta>(defaultMeta);
  const [dirty, setDirty] = useState(false);
  const [view, setView] = useState<View>({ x: 0, y: 0, k: START_K });
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [drag, setDrag] = useState<Drag | null>(null);
  const [sel, setSel] = useState<{ kind: 'hex' | 'port'; id: number } | null>(null);
  const [panel, setPanel] = useState<null | 'settings' | 'issues'>(null);
  const [toast, setToast] = useState('');
  const [notice, setNotice] = useState<Notice | null>(null);

  // The latest values for handlers that live on `window` (they must not close over old ones).
  const layoutRef = useRef(layout);
  layoutRef.current = layout;
  const viewRef = useRef(view);
  viewRef.current = view;
  const sizeRef = useRef(size);
  sizeRef.current = size;
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  const dragRef = useRef<Drag | null>(null);
  const nextId = useRef(1);
  const canvasRef = useRef<HTMLDivElement>(null);
  const barRef = useRef<HTMLElement>(null);
  const topRef = useRef<HTMLElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const toastTimer = useRef(0);

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(''), 3800);
  }, []);
  useEffect(() => () => window.clearTimeout(toastTimer.current), []);

  const setViewBoth = useCallback((v: View) => {
    viewRef.current = v;
    setView(v);
  }, []);

  // ---- the size of the sea, and where the view starts
  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    let first = true;
    const read = () => {
      const r = el.getBoundingClientRect();
      const s = { w: Math.round(r.width), h: Math.round(r.height) };
      sizeRef.current = s;
      setSize(s);
      if (first && s.w > 0) {
        first = false;
        setViewBoth({ x: s.w / 2, y: s.h / 2, k: START_K });
      }
    };
    read();
    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(read);
    ro.observe(el);
    return () => ro.disconnect();
  }, [setViewBoth]);

  const toWorld = (cx: number, cy: number, v: View = viewRef.current) => {
    const r = canvasRef.current!.getBoundingClientRect();
    return { x: (cx - r.left - v.x) / v.k, y: (cy - r.top - v.y) / v.k };
  };

  const zoomAt = useCallback(
    (cx: number, cy: number, k: number) => {
      const el = canvasRef.current;
      if (!el) return;
      const v = viewRef.current;
      const nk = clampK(k);
      const r = el.getBoundingClientRect();
      const wx = (cx - r.left - v.x) / v.k;
      const wy = (cy - r.top - v.y) / v.k;
      setViewBoth({ k: nk, x: cx - r.left - wx * nk, y: cy - r.top - wy * nk });
    },
    [setViewBoth],
  );
  const zoomBy = (f: number) => {
    const r = canvasRef.current?.getBoundingClientRect();
    if (r) zoomAt(r.left + r.width / 2, r.top + r.height / 2, viewRef.current.k * f);
  };

  // The wheel has to be a non-passive listener to stop the page from scrolling.
  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const wheel = (e: WheelEvent) => {
      e.preventDefault();
      zoomAt(e.clientX, e.clientY, viewRef.current.k * Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.0015)));
    };
    el.addEventListener('wheel', wheel, { passive: false });
    return () => el.removeEventListener('wheel', wheel);
  }, [zoomAt]);

  /** Frame everything on the map (or go back to the start for an empty one). */
  const fit = useCallback(
    (l: Layout) => {
      const { w, h } = sizeRef.current;
      if (!w) return;
      if (!l.hexes.length) return setViewBoth({ x: w / 2, y: h / 2, k: START_K });
      let x0 = Infinity;
      let y0 = Infinity;
      let x1 = -Infinity;
      let y1 = -Infinity;
      const grow = (x: number, y: number, m: number) => {
        x0 = Math.min(x0, x - m);
        y0 = Math.min(y0, y - m);
        x1 = Math.max(x1, x + m);
        y1 = Math.max(y1, y + m);
      };
      for (const hx of l.hexes) {
        const c = hexCenter(hx.q, hx.r);
        grow(c.x, c.y, S);
      }
      for (const p of l.ports) {
        const pl = portLayout(p.q, p.r, p.side);
        grow(pl.sx, pl.sy, 46);
      }
      // keep clear of the buttons along the top and the zoom buttons at the bottom
      const sea = canvasRef.current?.getBoundingClientRect();
      const top = topRef.current && sea ? Math.max(0, topRef.current.getBoundingClientRect().bottom - sea.top) + 12 : 64;
      const bottom = 56;
      const k = clampK(Math.min((w - 40) / (x1 - x0), (h - top - bottom) / (y1 - y0), 1.1));
      setViewBoth({ k, x: w / 2 - ((x0 + x1) / 2) * k, y: top + (h - top - bottom) / 2 - ((y0 + y1) / 2) * k });
    },
    [setViewBoth],
  );

  // ---- changing the map (everything that goes through here can be undone)
  const commit = useCallback((next: Layout) => {
    const cur = layoutRef.current;
    if (next === cur) return;
    setPast((p) => [...p.slice(-(HISTORY - 1)), cur]);
    setFuture([]);
    layoutRef.current = next;
    setLayout(next);
    setDirty(true);
  }, []);

  /** Commit, dropping any harbour that the change left off the coast. */
  const commitTidy = useCallback(
    (next: Layout) => {
      const { layout: tidy, removed } = normalizePorts(next);
      commit(tidy);
      if (removed) showToast(`${plural(removed, 'port')} covered by the change ${removed === 1 ? 'was' : 'were'} removed. Undo brings ${removed === 1 ? 'it' : 'them'} back.`);
    },
    [commit, showToast],
  );

  const undo = useCallback(() => {
    if (!past.length) return;
    const prev = past[past.length - 1];
    setPast(past.slice(0, -1));
    setFuture([layout, ...future]);
    layoutRef.current = prev;
    setLayout(prev);
    setSel(null);
    setDirty(true);
  }, [past, future, layout]);
  const redo = useCallback(() => {
    if (!future.length) return;
    const next = future[0];
    setFuture(future.slice(1));
    setPast([...past, layout]);
    layoutRef.current = next;
    setLayout(next);
    setSel(null);
    setDirty(true);
  }, [past, future, layout]);

  const removeHex = (l: Layout, id: number): Layout => {
    const h = l.hexes.find((x) => x.id === id);
    if (!h) return l;
    return { hexes: l.hexes.filter((x) => x.id !== id), ports: l.ports.filter((p) => !(p.q === h.q && p.r === h.r)) };
  };

  const removeSelected = useCallback(() => {
    if (!sel) return;
    const l = layoutRef.current;
    commit(sel.kind === 'hex' ? removeHex(l, sel.id) : { ...l, ports: l.ports.filter((p) => p.id !== sel.id) });
    setSel(null);
  }, [sel, commit]);

  const clearAll = () => {
    if (!layout.hexes.length) return;
    commit({ hexes: [], ports: [] });
    setSel(null);
    fit({ hexes: [], ports: [] });
  };

  // ---- adding with a click or the keyboard instead of a drag: the free spot nearest to the middle of the screen
  const addNear = useCallback(
    (piece: Piece) => {
      const l = layoutRef.current;
      const { w, h } = sizeRef.current;
      const v = viewRef.current;
      const cx = (w / 2 - v.x) / v.k;
      const cy = (h / 2 - v.y) / v.k;
      if (piece === 'port') {
        const e = nearestCoastEdge(l, cx, cy, null, Infinity);
        if (!e) return showToast(l.hexes.length ? 'There is no free coast left for a port.' : 'Add a hex first: a port goes on the coast of a hex.');
        const id = nextId.current++;
        commit({ ...l, ports: [...l.ports, { id, ...e, kind: null }] });
        return setSel({ kind: 'port', id });
      }
      if (l.hexes.length >= MAX_HEXES) return showToast(`A map can have at most ${MAX_HEXES} hexes.`);
      let cell = pixelToHex(cx, cy);
      if (l.hexes.length) {
        const taken = new Set(l.hexes.map((x) => cellKey(x.q, x.r)));
        let best = Infinity;
        for (const x of l.hexes) {
          for (const [dq, dr] of STEPS) {
            const q = x.q + dq;
            const r = x.r + dr;
            if (taken.has(cellKey(q, r)) || !inRange(q, r)) continue;
            const c = hexCenter(q, r);
            const d = Math.hypot(c.x - cx, c.y - cy);
            if (d < best) {
              best = d;
              cell = { q, r };
            }
          }
        }
      }
      if (!inRange(cell.q, cell.r)) return showToast('That is as far as a map can reach.');
      const id = nextId.current++;
      commit({ ...l, hexes: [...l.hexes, { id, ...cell, terrain: piece === 'desert' ? 'desert' : null, number: null }] });
      setSel({ kind: 'hex', id });
    },
    [commit, showToast],
  );

  // ---- dragging pieces
  const targetFor = useCallback((d: Drag): Target => {
    const bar = barRef.current?.getBoundingClientRect();
    if (bar && d.y >= bar.top) return { t: 'bar' };
    const c = canvasRef.current?.getBoundingClientRect();
    if (!c || d.x < c.left || d.x > c.right || d.y < c.top) return { t: 'none' };
    const w = toWorld(d.x, d.y);
    const l = layoutRef.current;
    if (d.kind === 'hex') {
      const { q, r } = pixelToHex(w.x, w.y);
      const free = !l.hexes.some((h) => h.id !== d.id && h.q === q && h.r === r);
      return { t: 'cell', q, r, ok: free && inRange(q, r) && (d.id !== null || l.hexes.length < MAX_HEXES) };
    }
    const e = nearestCoastEdge(l, w.x, w.y, d.id);
    return e ? { t: 'edge', ...e } : { t: 'none' };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const finishDrag = useCallback(
    (d: Drag) => {
      const l = layoutRef.current;
      const t = d.target;
      if (d.id === null) {
        // a new piece from the bar
        if (!d.moved && t.t === 'bar') return addNear(d.kind === 'port' ? 'port' : d.hex.terrain === 'desert' ? 'desert' : 'wild');
        if (d.kind === 'hex') {
          if (t.t === 'cell' && t.ok) {
            const id = nextId.current++;
            commit({ ...l, hexes: [...l.hexes, { id, q: t.q, r: t.r, terrain: d.hex.terrain, number: null }] });
            setSel({ kind: 'hex', id });
          } else if (t.t === 'cell' && l.hexes.length >= MAX_HEXES) showToast(`A map can have at most ${MAX_HEXES} hexes.`);
        } else if (t.t === 'edge') {
          const id = nextId.current++;
          commit({ ...l, ports: [...l.ports, { id, q: t.q, r: t.r, side: t.side, kind: null }] });
          setSel({ kind: 'port', id });
        } else if (t.t === 'none' && !l.hexes.length) showToast('Add a hex first: a port goes on the coast of a hex.');
        return;
      }
      // a piece that is already on the map
      if (!d.moved) return setSel({ kind: d.kind, id: d.id });
      if (t.t === 'bar') {
        commit(d.kind === 'hex' ? removeHex(l, d.id) : { ...l, ports: l.ports.filter((p) => p.id !== d.id) });
        return setSel(null);
      }
      if (d.kind === 'hex' && t.t === 'cell' && t.ok) {
        const old = l.hexes.find((h) => h.id === d.id);
        if (!old || (old.q === t.q && old.r === t.r)) return setSel({ kind: 'hex', id: d.id });
        commitTidy({
          hexes: l.hexes.map((h) => (h.id === d.id ? { ...h, q: t.q, r: t.r } : h)),
          // the harbours of a hex go with it
          ports: l.ports.map((p) => (p.q === old.q && p.r === old.r ? { ...p, q: t.q, r: t.r } : p)),
        });
        return setSel({ kind: 'hex', id: d.id });
      }
      if (d.kind === 'port' && t.t === 'edge') {
        commit({ ...l, ports: l.ports.map((p) => (p.id === d.id ? { ...p, q: t.q, r: t.r, side: t.side } : p)) });
        return setSel({ kind: 'port', id: d.id });
      }
    },
    [addNear, commit, commitTidy, showToast],
  );

  const beginDrag = useCallback(
    (e: ReactPointerEvent, kind: 'hex' | 'port', id: number | null, hex: EdHex, port: EdPort) => {
      if (e.pointerType === 'mouse' && e.button !== 0) return;
      e.stopPropagation();
      e.preventDefault();
      const d: Drag = { kind, id, hex, port, startX: e.clientX, startY: e.clientY, x: e.clientX, y: e.clientY, moved: false, target: { t: 'none' } };
      d.target = targetFor(d);
      dragRef.current = d;
      setDrag(d);
      setPanel(null);
    },
    [targetFor],
  );

  const onHexDown = useCallback((e: ReactPointerEvent, hex: EdHex) => beginDrag(e, 'hex', hex.id, hex, newPort()), [beginDrag]);
  const onPortDown = useCallback((e: ReactPointerEvent, port: EdPort) => beginDrag(e, 'port', port.id, newHex(null), port), [beginDrag]);
  const onTileDown = (e: ReactPointerEvent, piece: Piece) =>
    beginDrag(e, piece === 'port' ? 'port' : 'hex', null, newHex(piece === 'desert' ? 'desert' : null), newPort());

  const dragging = drag !== null;
  useLayoutEffect(() => {
    if (!dragging) return;
    const move = (ev: PointerEvent) => {
      const d = dragRef.current;
      if (!d) return;
      const next: Drag = { ...d, x: ev.clientX, y: ev.clientY, moved: d.moved || Math.hypot(ev.clientX - d.startX, ev.clientY - d.startY) > 5 };
      next.target = targetFor(next);
      dragRef.current = next;
      setDrag(next);
    };
    const end = (finish: boolean) => (ev: PointerEvent) => {
      const d = dragRef.current;
      dragRef.current = null;
      setDrag(null);
      if (d && finish) {
        const last: Drag = { ...d, x: ev.clientX, y: ev.clientY, moved: d.moved || Math.hypot(ev.clientX - d.startX, ev.clientY - d.startY) > 5 };
        last.target = targetFor(last);
        finishDrag(last);
      }
    };
    const up = end(true);
    const cancel = end(false);
    const key = (ev: KeyboardEvent) => {
      if (ev.key === 'Escape') {
        dragRef.current = null;
        setDrag(null);
      }
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', cancel);
    window.addEventListener('keydown', key);
    return () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      window.removeEventListener('pointercancel', cancel);
      window.removeEventListener('keydown', key);
    };
  }, [dragging, targetFor, finishDrag]);

  // ---- moving round the sea: drag the water to pan, pinch or wheel to zoom
  const pointers = useRef(new Map<number, { x: number; y: number }>());
  const pinch = useRef<{ k0: number; d0: number; wx: number; wy: number } | null>(null);
  const startPinch = () => {
    const [a, b] = [...pointers.current.values()];
    const mid = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
    const w = toWorld(mid.x, mid.y);
    pinch.current = { k0: viewRef.current.k, d0: Math.hypot(a.x - b.x, a.y - b.y) || 1, wx: w.x, wy: w.y };
  };
  const panDown = (e: ReactPointerEvent<SVGSVGElement>) => {
    if (e.pointerType === 'mouse' && e.button !== 0 && e.button !== 1) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    pointers.current.set(e.pointerId, { x: e.clientX, y: e.clientY });
    setSel(null);
    setPanel(null);
    if (pointers.current.size === 2) startPinch();
  };
  const panMove = (e: ReactPointerEvent<SVGSVGElement>) => {
    const p = pointers.current.get(e.pointerId);
    if (!p) return;
    const v = viewRef.current;
    if (pointers.current.size === 1) {
      setViewBoth({ ...v, x: v.x + e.clientX - p.x, y: v.y + e.clientY - p.y });
      p.x = e.clientX;
      p.y = e.clientY;
    } else if (pointers.current.size === 2 && pinch.current) {
      p.x = e.clientX;
      p.y = e.clientY;
      const [a, b] = [...pointers.current.values()];
      const g = pinch.current;
      const k = clampK((g.k0 * (Math.hypot(a.x - b.x, a.y - b.y) || 1)) / g.d0);
      const r = canvasRef.current!.getBoundingClientRect();
      setViewBoth({ k, x: (a.x + b.x) / 2 - r.left - g.wx * k, y: (a.y + b.y) / 2 - r.top - g.wy * k });
    }
  };
  const panUp = (e: ReactPointerEvent<SVGSVGElement>) => {
    pointers.current.delete(e.pointerId);
    pinch.current = null;
    if (pointers.current.size === 2) startPinch();
  };

  // ---- keys
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (el && (el.tagName === 'INPUT' || el.tagName === 'SELECT' || el.tagName === 'TEXTAREA')) return;
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.key.toLowerCase() === 'z') {
        e.preventDefault();
        return e.shiftKey ? redo() : undo();
      }
      if (mod && e.key.toLowerCase() === 'y') {
        e.preventDefault();
        return redo();
      }
      if ((e.key === 'Delete' || e.key === 'Backspace') && sel) {
        e.preventDefault();
        removeSelected();
      } else if (e.key === 'Escape') {
        setPanel(null);
        setSel(null);
      }
    };
    window.addEventListener('keydown', key);
    return () => window.removeEventListener('keydown', key);
  }, [undo, redo, removeSelected, sel]);

  useEffect(() => {
    if (!dirty || !layout.hexes.length) return;
    const warn = (e: BeforeUnloadEvent) => {
      e.preventDefault();
      e.returnValue = '';
    };
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [dirty, layout.hexes.length]);

  const leave = () => {
    if (dirty && layout.hexes.length && !window.confirm('Leave the map editor? The map you are editing has not been downloaded.')) return;
    onBack();
  };

  // ---- opening and saving
  const openText = useCallback(
    (text: string, fileName: string) => {
      const fallback = fileName.replace(/\.json$/i, '').trim().slice(0, MAX_NAME_CHARS) || 'My map';
      const r = parseMapJson(text, fallback);
      if (!r.ok) return setNotice({ tone: 'error', title: `Couldn't open ${fileName}`, lines: r.errors });
      if (layoutRef.current.hexes.length && dirtyRef.current && !window.confirm(`Replace the map you are editing with “${r.meta.name}”? Your changes have not been downloaded.`)) return;
      nextId.current = Math.max(r.layout.hexes.length, r.layout.ports.length) + 1;
      layoutRef.current = r.layout;
      setLayout(r.layout);
      setPast([]);
      setFuture([]);
      setMeta(r.meta);
      setDirty(false);
      setSel(null);
      setPanel(null);
      fit(r.layout);
      if (r.notes.length) setNotice({ tone: 'info', title: `Opened “${r.meta.name}”`, lines: r.notes });
      else {
        setNotice(null);
        showToast(`Opened “${r.meta.name}”`);
      }
    },
    [fit, showToast],
  );
  const openFile = async (file: File) => {
    if (file.size > MAX_MAP_BYTES) return setNotice({ tone: 'error', title: `Couldn't open ${file.name}`, lines: [`The file is bigger than ${MAX_MAP_BYTES / 1024} KB, which is the most a map may be.`] });
    try {
      openText(await file.text(), file.name);
    } catch {
      setNotice({ tone: 'error', title: `Couldn't open ${file.name}`, lines: ['The file could not be read.'] });
    }
  };
  const onPick = (e: ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0];
    e.target.value = ''; // so the same file can be chosen again
    if (f) void openFile(f);
  };
  const onDrop = (e: DragEvent) => {
    const f = e.dataTransfer.files?.[0];
    if (!f) return;
    e.preventDefault();
    void openFile(f);
  };

  const issues = useMemo(() => checkMap(layout, meta), [layout, meta]);
  const pools = useMemo(() => planPools(layout, meta), [layout, meta]);
  const players = useMemo(() => resolvePlayers(layout, meta), [layout, meta]);
  const errors = issues.filter((i) => i.level === 'error');
  const warnings = issues.filter((i) => i.level === 'warning');
  const empty = layout.hexes.length === 0;

  const download = () => {
    if (empty) return showToast('Add a hex before downloading.');
    if (errors.length) {
      setPanel('issues');
      return showToast(`Fix ${plural(errors.length, 'problem')} before downloading.`);
    }
    const name = fileNameFor(meta.name);
    const url = URL.createObjectURL(new Blob([mapFileText(layout, meta)], { type: 'application/json' }));
    const a = document.createElement('a');
    a.href = url;
    a.download = name;
    document.body.appendChild(a);
    a.click();
    a.remove();
    window.setTimeout(() => URL.revokeObjectURL(url), 2000);
    setDirty(false);
    showToast(`Downloaded ${name}`);
  };

  const setM = (patch: Partial<Meta>) => {
    setMeta((m) => ({ ...m, ...patch }));
    setDirty(true);
  };

  // ---- what is drawn
  const lifted = drag && drag.moved && drag.id !== null ? drag : null;
  const liftedHex = lifted?.kind === 'hex' ? layout.hexes.find((h) => h.id === lifted.id) : undefined;
  const hexTiles = layout.hexes.filter((h) => h.id !== liftedHex?.id);
  const portTiles = layout.ports.filter((p) => !(lifted?.kind === 'port' && p.id === lifted.id) && !(liftedHex && p.q === liftedHex.q && p.r === liftedHex.r));
  const showGrid = meta.seaGrid || (drag !== null && drag.kind === 'hex');
  const view0 = { x: -view.x / view.k, y: -view.y / view.k, w: size.w / view.k, h: size.h / view.k };
  const ghostLive = drag !== null && (drag.id === null || drag.moved);
  const ghostBad = drag !== null && (drag.target.t === 'none' || (drag.target.t === 'cell' && !drag.target.ok) || (drag.target.t === 'bar' && drag.id === null));
  const trash = ghostLive && drag!.id !== null && drag!.target.t === 'bar';
  const gridW = Math.sqrt(3) * S;

  return (
    <div className={`me${drag?.moved || (drag && drag.id === null) ? ' me-dragging' : ''}`} onDragOver={(e) => e.dataTransfer.types.includes('Files') && e.preventDefault()} onDrop={onDrop}>
      <div className="me-sea" ref={canvasRef}>
        <svg className="me-canvas" role="application" aria-label="Map canvas" onPointerDown={panDown} onPointerMove={panMove} onPointerUp={panUp} onPointerCancel={panUp}>
          <PieceDefs uid="w" />
          <pattern id="me-grid" patternUnits="userSpaceOnUse" width={gridW} height={3 * S}>
            {[
              [0, 0],
              [gridW, 0],
              [gridW / 2, 1.5 * S],
              [0, 3 * S],
              [gridW, 3 * S],
            ].map(([x, y], i) => (
              <polygon key={i} points={hexPoints(x, y, S)} fill="none" stroke="#7fc0ee" strokeWidth={1.4} />
            ))}
          </pattern>
          <g transform={`translate(${view.x} ${view.y}) scale(${view.k})`}>
            {showGrid && <rect x={view0.x} y={view0.y} width={view0.w} height={view0.h} fill="url(#me-grid)" opacity={meta.seaGrid ? 0.55 : 0.28} pointerEvents="none" />}
            <Shore hexes={hexTiles} />
            {hexTiles.map((h) => (
              <HexTile key={h.id} hex={h} selected={sel?.kind === 'hex' && sel.id === h.id} onDown={onHexDown} />
            ))}
            {portTiles.map((p) => (
              <PortTile key={p.id} port={p} selected={sel?.kind === 'port' && sel.id === p.id} onDown={onPortDown} />
            ))}

            {/* where a dragged piece would land */}
            {ghostLive && drag!.kind === 'hex' && liftedHex && (
              <polygon points={hexPoints(hexCenter(liftedHex.q, liftedHex.r).x, hexCenter(liftedHex.q, liftedHex.r).y, S * 0.93)} fill="rgba(255,255,255,0.1)" stroke="#fff" strokeOpacity={0.5} strokeWidth={3} strokeDasharray="8 8" pointerEvents="none" />
            )}
            {drag && drag.kind === 'hex' && drag.target.t === 'cell' && (
              <polygon
                points={hexPoints(hexCenter(drag.target.q, drag.target.r).x, hexCenter(drag.target.q, drag.target.r).y, S * 0.95)}
                fill={drag.target.ok ? 'rgba(255,255,255,0.3)' : 'rgba(216,50,43,0.35)'}
                stroke={drag.target.ok ? '#fff' : '#ff8a80'}
                strokeWidth={4}
                strokeDasharray="10 6"
                pointerEvents="none"
              />
            )}
            {ghostLive && drag!.kind === 'port' &&
              freeCoastEdges(layout, drag!.id).map((e) => {
                const p = portLayout(e.q, e.r, e.side);
                const on = drag!.target.t === 'edge' && drag!.target.q === e.q && drag!.target.r === e.r && drag!.target.side === e.side;
                return <circle key={`${e.q},${e.r},${e.side}`} cx={p.sx} cy={p.sy} r={on ? 0 : 7} fill="#fff" opacity={0.8} pointerEvents="none" />;
              })}
            {drag && drag.kind === 'port' && drag.target.t === 'edge' && (() => {
              const p = portLayout(drag.target.q, drag.target.r, drag.target.side);
              return (
                <g opacity={0.7} pointerEvents="none">
                  <Piers sx={p.sx} sy={p.sy} v1={p.v1} v2={p.v2} />
                  <g transform={`translate(${p.sx} ${p.sy})`}>
                    <Ship kind={drag.port.kind} uid="w" />
                  </g>
                </g>
              );
            })()}
          </g>
        </svg>

        {empty && !drag && (
          <div className="me-empty" aria-hidden="true">
            <p>Drag a piece from the bar onto the sea to start,</p>
            <p>or import a map file to edit.</p>
          </div>
        )}

        <div className="me-zoom">
          <button className="me-pill" onClick={() => zoomBy(1.25)} aria-label="Zoom in" title="Zoom in">
            +
          </button>
          <button className="me-pill" onClick={() => zoomBy(0.8)} aria-label="Zoom out" title="Zoom out">
            −
          </button>
          <button className="me-pill" onClick={() => fit(layout)} aria-label="Fit the map to the screen" title="Fit the map to the screen">
            Fit
          </button>
        </div>
        {toast && (
          <div className="me-toast" role="status">
            {toast}
          </div>
        )}
      </div>

      {/* top: back, name and actions */}
      <header className="me-top" ref={topRef}>
        <a className="changelog-back" href="/" onClick={linkClick(leave)}>
          <span aria-hidden="true">←</span> Back to lobby
        </a>
        <input
          className="me-name"
          type="text"
          value={meta.name}
          maxLength={MAX_NAME_CHARS}
          onChange={(e) => setM({ name: e.target.value })}
          aria-label="Map name"
          placeholder="Map name"
          spellCheck={false}
        />
        <div className="me-actions">
          <button className="me-pill" onClick={undo} disabled={!past.length} aria-label="Undo" title="Undo (Ctrl+Z)">
            ↶
          </button>
          <button className="me-pill" onClick={redo} disabled={!future.length} aria-label="Redo" title="Redo (Ctrl+Shift+Z)">
            ↷
          </button>
          <button className="me-pill" onClick={() => fileRef.current?.click()} title="Open a map file to edit">
            Import
          </button>
          <button className={`me-pill${panel === 'settings' ? ' me-pill--on' : ''}`} onClick={() => setPanel(panel === 'settings' ? null : 'settings')} aria-expanded={panel === 'settings'}>
            Settings
          </button>
          {!empty && (
            <button
              className={`me-pill me-status ${errors.length ? 'me-status--bad' : warnings.length ? 'me-status--warn' : 'me-status--ok'}${panel === 'issues' ? ' me-pill--on' : ''}`}
              onClick={() => setPanel(panel === 'issues' ? null : 'issues')}
              aria-expanded={panel === 'issues'}
            >
              {errors.length ? plural(errors.length, 'problem') : warnings.length ? plural(warnings.length, 'note') : '✓ Ready'}
            </button>
          )}
          <button className="me-pill me-download" onClick={download} disabled={empty} title={empty ? 'Add a hex first' : 'Download the map as a JSON file'}>
            Download
          </button>
        </div>
        <input ref={fileRef} type="file" accept=".json,application/json" hidden onChange={onPick} />
      </header>

      {panel === 'settings' && (
        <section className="me-panel" aria-label="Map settings">
          <h2>Map settings</h2>
          <label htmlFor="me-author">Author</label>
          <input id="me-author" type="text" value={meta.author} maxLength={MAX_NAME_CHARS} onChange={(e) => setM({ author: e.target.value })} placeholder="Optional" />

          <label>Players</label>
          <label className="me-check">
            <input type="checkbox" checked={meta.playersAuto} onChange={(e) => setM(e.target.checked ? { playersAuto: true } : { playersAuto: false, playersMin: players.min, playersMax: players.max })} />
            Fit to the size of the map
          </label>
          {meta.playersAuto ? (
            <p className="muted me-small">
              {players.min} to {players.max} players, by how many starting settlements fit on the map.
            </p>
          ) : (
            <div className="me-row">
              <select aria-label="Fewest players" value={meta.playersMin} onChange={(e) => setM({ playersMin: +e.target.value })}>
                {[2, 3, 4].map((n) => (
                  <option key={n}>{n}</option>
                ))}
              </select>
              <span>to</span>
              <select aria-label="Most players" value={meta.playersMax} onChange={(e) => setM({ playersMax: +e.target.value })}>
                {[2, 3, 4].map((n) => (
                  <option key={n}>{n}</option>
                ))}
              </select>
              <span>players</span>
            </div>
          )}

          <label>Deserts among the striped hexes</label>
          <div className="me-row">
            <button className="small" onClick={() => setM({ randomDeserts: Math.max(0, pools.deserts - 1) })} disabled={pools.wildHexes === 0 || pools.deserts <= 0} aria-label="One fewer desert">
              −
            </button>
            <strong aria-live="polite">{pools.deserts}</strong>
            <button className="small" onClick={() => setM({ randomDeserts: Math.min(maxRandomDeserts(layout), pools.deserts + 1) })} disabled={pools.wildHexes === 0 || pools.deserts >= maxRandomDeserts(layout)} aria-label="One more desert">
              +
            </button>
            <button className={`small${meta.randomDeserts === null ? ' on' : ''}`} onClick={() => setM({ randomDeserts: null })} disabled={meta.randomDeserts === null}>
              Auto
            </button>
          </div>
          <p className="muted me-small">
            {pools.wildHexes === 0
              ? 'No striped hexes on the map yet.'
              : `Of ${plural(pools.wildHexes, 'striped hex', 'striped hexes')}, ${pools.deserts} ${pools.deserts === 1 ? 'is' : 'are'} dealt as ${pools.deserts === 1 ? 'a desert' : 'deserts'}; the rest are shared out evenly between the five resources. Placed deserts are always deserts.`}
          </p>

          <label className="me-check">
            <input type="checkbox" checked={meta.seaGrid} onChange={(e) => setM({ seaGrid: e.target.checked })} />
            Draw a faint hex grid across the sea in games
          </label>
          <div className="me-panel-actions">
            <button className="small" onClick={clearAll} disabled={empty}>
              Clear the map
            </button>
            <button className="small primary" onClick={() => setPanel(null)}>
              Done
            </button>
          </div>
        </section>
      )}

      {panel === 'issues' && (
        <section className="me-panel" aria-label="Problems with the map">
          <h2>{errors.length ? 'Fix these to download' : 'The map'}</h2>
          {issues.length === 0 && <p className="muted">No problems. The map is ready to download.</p>}
          <ul className="me-issues">
            {[...errors, ...warnings].map((i, n) => (
              <li key={n} className={`me-issue me-issue--${i.level}`}>
                {i.text}
              </li>
            ))}
          </ul>
          {warnings.length > 0 && !errors.length && <p className="muted me-small">Notes don't stop a download; they are things the game would also mention.</p>}
        </section>
      )}

      {notice && (
        <div className={`me-notice me-notice--${notice.tone}`} role={notice.tone === 'error' ? 'alert' : 'status'}>
          <button className="me-notice-x" onClick={() => setNotice(null)} aria-label="Dismiss">
            ×
          </button>
          <strong>{notice.title}</strong>
          <ul>
            {notice.lines.map((l, i) => (
              <li key={i}>{l}</li>
            ))}
          </ul>
        </div>
      )}

      {/* bottom: the pieces */}
      <footer className={`me-bar${trash ? ' me-bar--trash' : ''}`} ref={barRef}>
        <div className="me-tiles">
          {TILES.map((t) => (
            <div
              key={t.piece}
              className="me-tile"
              role="button"
              tabIndex={0}
              aria-label={`${t.label}: ${t.hint}. Drag onto the sea, or press Enter to add one.`}
              onPointerDown={(e) => onTileDown(e, t.piece)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  addNear(t.piece);
                }
              }}
            >
              <TileArt piece={t.piece} />
              <span className="me-tile-label">{t.label}</span>
              <span className="me-tile-hint">{t.hint}</span>
            </div>
          ))}
        </div>
        <p className="me-count" aria-live="polite">
          {trash ? 'Drop here to remove' : `${plural(layout.hexes.length, 'hex', 'hexes')} · ${plural(layout.ports.length, 'port')}`}
          {!trash && !empty && <span className="me-count-hint"> · drag a piece back here to remove it</span>}
        </p>
      </footer>

      {/* the piece under the pointer */}
      {ghostLive && drag && (
        <svg className={`me-ghost${ghostBad ? ' me-ghost--bad' : ''}${trash ? ' me-ghost--trash' : ''}`} aria-hidden="true">
          <PieceDefs uid="g" />
          <g transform={`translate(${drag.x} ${drag.y}) scale(${view.k})`}>
            {drag.kind === 'hex' ? (
              <>
                <HexShape hex={liftedHex ?? drag.hex} uid="g" />
                {liftedHex &&
                  layout.ports
                    .filter((p) => p.q === liftedHex.q && p.r === liftedHex.r)
                    .map((p) => {
                      const pl = portLayout(0, 0, p.side);
                      return (
                        <g key={p.id}>
                          <Piers sx={pl.sx} sy={pl.sy} v1={pl.v1} v2={pl.v2} />
                          <g transform={`translate(${pl.sx} ${pl.sy})`}>
                            <Ship kind={p.kind} uid="g" />
                          </g>
                        </g>
                      );
                    })}
              </>
            ) : (
              <Ship kind={drag.port.kind} uid="g" />
            )}
          </g>
        </svg>
      )}
    </div>
  );
}
