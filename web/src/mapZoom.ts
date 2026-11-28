import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { MouseEvent as ReactMouseEvent, MutableRefObject, PointerEvent as ReactPointerEvent, RefObject } from 'react';

/**
 * Zoom and pan for the board, and only the board.
 *
 * The board is a host `<div>` holding two stacked `<svg>`s with the same `viewBox`, which fits the whole map into the
 * space it is given: the static map (tiles, numbers, harbours, shoreline; thousands of elements on a big map) in its own
 * `<div>` that the browser keeps as a GPU layer (`will-change: transform`), and a small overlay for what changes during
 * a game (roads, buildings, robber, click targets). Zooming never touches anything outside the host: the overlay's
 * content sits in one `<g>` (the "layer") moved by `translate(x y) scale(k)`, and the static `<div>` is moved by the
 * same view as a pixel transform (`pixelView`), so the big map is only ever re-composited while a finger or the wheel is
 * at work, never re-drawn. The host clips what falls outside it, so a zoomed-in map stays inside its own area and the
 * buttons, dice, hand and sidebar around it are exactly as they were.
 *
 * Coordinates: "board units" are the units the board is drawn in (what its `viewBox` is measured in). A point `p` on the
 * board is drawn at `(x, y) + k * p`, in the same units. The part of the `<svg>` you can see is a rectangle in these
 * units (`viewport`), because the `viewBox` is fitted with `preserveAspectRatio="xMidYMid meet"`.
 *
 * The maths is plain functions at the top of this file (tested in scripts/map-zoom-test.mjs); the hook at the bottom
 * wires them to the mouse wheel, to dragging, to pinching and to the + and − buttons.
 */

export const MIN_ZOOM = 1;
export const MAX_ZOOM = 8;
/** How much one press of + or − zooms. */
export const BUTTON_STEP = 1.5;
/** How far (in screen pixels) a zoomed-in map may be dragged past its own edge, so a piece in a corner can be moved out from under a button. */
const EDGE_PAD_PX = 56;
/** How far a pointer has to travel before a press counts as a drag rather than a click (touch fingers wobble more than a mouse). */
const SLOP_MOUSE = 5;
const SLOP_TOUCH = 10;
/** How long after the zoom last changed the static map is re-drawn at its new size, so that it is sharp and not a stretched picture. */
const SETTLE_MS = 200;

export interface Box {
  x0: number;
  y0: number;
  w: number;
  h: number;
}
export interface View {
  k: number;
  x: number;
  y: number;
}
export interface Size {
  w: number;
  h: number;
}
export interface Viewport {
  /** Screen pixels per board unit when not zoomed (what `preserveAspectRatio="meet"` makes of the box). */
  s: number;
  /** The visible rectangle, in board units. */
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

/** The board neither zoomed nor moved. */
export const HOME: View = { k: 1, x: 0, y: 0 };

export const clampZoom = (k: number): number => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, k));

/** The visible rectangle of an `<svg>` of `size` pixels whose `viewBox` is `box`. Before the first measurement (`size` null) the box itself. */
export function viewportOf(box: Box, size: Size | null): Viewport {
  const s = size && size.w > 0 && size.h > 0 ? Math.min(size.w / box.w, size.h / box.h) : 1;
  const w = size && size.w > 0 && size.h > 0 ? size.w / s : box.w;
  const h = size && size.w > 0 && size.h > 0 ? size.h / s : box.h;
  const cx = box.x0 + box.w / 2;
  const cy = box.y0 + box.h / 2;
  return { s, x0: cx - w / 2, y0: cy - h / 2, x1: cx + w / 2, y1: cy + h / 2 };
}

/** Where a point of the screen (client coordinates, with the `<svg>`'s client rectangle) is, in board units. */
export function toBoard(box: Box, vp: Viewport, rect: { left: number; top: number; width: number; height: number }, clientX: number, clientY: number): { x: number; y: number } {
  return {
    x: box.x0 + box.w / 2 + (clientX - (rect.left + rect.width / 2)) / vp.s,
    y: box.y0 + box.h / 2 + (clientY - (rect.top + rect.height / 2)) / vp.s,
  };
}

/** `view` zoomed to `k`, keeping the board point that is under `(ux, uy)` (board units, as drawn) under it. */
export function zoomAt(view: View, k: number, ux: number, uy: number): View {
  const nk = clampZoom(k);
  const px = (ux - view.x) / view.k;
  const py = (uy - view.y) / view.k;
  return { k: nk, x: ux - nk * px, y: uy - nk * py };
}

/** One axis of `clampView`: the content spans `[lo, lo + size]` before it is scaled; the window is `[v0, v1]`. */
function clampAxis(t: number, k: number, lo: number, size: number, v0: number, v1: number, pad: number): number {
  if (k * size <= v1 - v0) return (v0 + v1) / 2 - k * (lo + size / 2); // it all fits: keep it centred, nothing to pan
  const most = v0 + pad - k * lo; // the content's left edge may not go further right than this
  const least = v1 - pad - k * (lo + size); // ... nor its right edge further left than this
  return Math.min(most, Math.max(least, t));
}

/** `view` with the zoom in range and the board kept where it can be seen: no panning when it all fits, otherwise it may not leave the window by more than `pad`. */
export function clampView(view: View, box: Box, vp: Viewport, pad = EDGE_PAD_PX / vp.s): View {
  const k = clampZoom(view.k);
  return {
    k,
    x: clampAxis(view.x, k, box.x0, box.w, vp.x0, vp.x1, pad),
    y: clampAxis(view.y, k, box.y0, box.h, vp.y0, vp.y1, pad),
  };
}

/**
 * The same view as a transform in screen pixels, for an HTML element that is as big as the `<svg>` and holds the board
 * drawn unzoomed (`transform-origin: 0 0`). A board point `u` is at `s * (u - c) + (W/2, H/2)` in that element (`s` and
 * the element's size `W` x `H` as in `viewportOf`, `c` the middle of `box`) and at `s * (x + k * u - c) + (W/2, H/2)` in
 * the view `(k, x, y)`, so the element needs `scale(k)` and the shift returned here. Before the first measurement
 * (`size` null) there is nothing to convert with, and the view is returned as it is.
 */
export function pixelView(view: View, box: Box, size: Size | null): View {
  if (!size || size.w <= 0 || size.h <= 0) return view;
  const { s } = viewportOf(box, size);
  const cx = box.x0 + box.w / 2;
  const cy = box.y0 + box.h / 2;
  return {
    k: view.k,
    x: (size.w / 2) * (1 - view.k) + s * (view.x + (view.k - 1) * cx),
    y: (size.h / 2) * (1 - view.k) + s * (view.y + (view.k - 1) * cy),
  };
}

export type ZoomEdge = 'min' | 'max' | 'mid';
export const edgeOf = (k: number): ZoomEdge => (k <= MIN_ZOOM + 1e-6 ? 'min' : k >= MAX_ZOOM - 1e-6 ? 'max' : 'mid');

/** What the + and − buttons (which live outside the board) can ask of it. */
export interface MapZoomApi {
  zoomIn(): void;
  zoomOut(): void;
  reset(): void;
}

interface Gesture {
  mode: 'none' | 'pan' | 'pinch';
  moved: boolean;
  /** pan: where the press began and where the last move was (client pixels) */
  sx: number;
  sy: number;
  lx: number;
  ly: number;
  /** pinch: the finger distance at the start, the board point under the fingers' midpoint, and the zoom then */
  d0: number;
  cx: number;
  cy: number;
  k0: number;
}
const idle = (): Gesture => ({ mode: 'none', moved: false, sx: 0, sy: 0, lx: 0, ly: 0, d0: 1, cx: 0, cy: 0, k0: 1 });

const reducedMotion = (): boolean => typeof window !== 'undefined' && typeof window.matchMedia === 'function' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;

interface Options {
  /** The element that is as big as the board's space and holds both layers: it is measured, listens for the wheel and the pointers, and clips. */
  host: RefObject<HTMLElement>;
  box: Box;
  size: Size | null;
  /** A new value puts the board back as it was (a different board was dealt). */
  resetKey: unknown;
  api?: MutableRefObject<MapZoomApi | null>;
  /** Called when the zoom reaches its limits or leaves them, so a button can grey out. */
  onEdge?: (edge: ZoomEdge) => void;
}

/**
 * Makes the board zoomable and draggable. Put `layer` on the `<g>` that holds what is drawn over the map, `staticLayer`
 * on the `<div>` that holds the map itself, and spread `handlers` on the host. Both are moved straight in the DOM while a
 * finger or the wheel is at work, not through React state, so the (large) board is not redrawn at every pointer event;
 * `zoom` (rounded) is the only state, and the board uses it to keep its click targets a good size.
 */
export function useMapZoom({ host, box, size, resetKey, api, onEdge }: Options) {
  const layer = useRef<SVGGElement>(null);
  const staticLayer = useRef<HTMLDivElement>(null);
  const settle = useRef<{ timer: number | undefined; k: number }>({ timer: undefined, k: 1 });
  const view = useRef<View>(HOME);
  const live = useRef({ box, size, onEdge });
  live.current = { box, size, onEdge };
  const lastEdge = useRef<ZoomEdge | null>(null); // null: nothing reported yet, so the first apply tells the buttons where things stand
  const [zoom, setZoom] = useState(1);

  const apply = useCallback((next: View, smooth = false) => {
    const { box: b, size: sz } = live.current;
    const vp = viewportOf(b, sz);
    const v = clampView(next, b, vp);
    view.current = v;
    const transition = smooth && !reducedMotion() ? 'transform 0.18s ease-out' : 'none';
    // back at home exactly (not "nearly": zooming out leaves float dust) the layers need no transform at all
    const home = Math.abs(v.k - 1) < 1e-9 && Math.abs(v.x) < 1e-6 && Math.abs(v.y) < 1e-6;
    const g = layer.current;
    if (g) {
      g.style.transition = transition;
      g.style.transform = home ? '' : `translate(${v.x}px, ${v.y}px) scale(${v.k})`;
    }
    const m = staticLayer.current;
    if (m) {
      const p = pixelView(v, b, sz);
      m.style.transition = transition;
      m.style.transform = home ? '' : `translate(${p.x}px, ${p.y}px) scale(${p.k})`;
      // The browser keeps a `will-change: transform` layer as the picture it drew at the old zoom and only stretches it, so a
      // moment after the zoom last changed it is told to draw the map again at its new size (panning never needs this).
      if (v.k !== settle.current.k) {
        settle.current.k = v.k;
        window.clearTimeout(settle.current.timer);
        settle.current.timer = window.setTimeout(() => {
          m.style.willChange = 'auto';
          requestAnimationFrame(() => requestAnimationFrame(() => (m.style.willChange = ''))); // '' gives it back to the stylesheet (`will-change: transform`)
        }, SETTLE_MS);
      }
    }
    setZoom(Math.round(v.k * 20) / 20);
    const edge = edgeOf(v.k);
    if (edge !== lastEdge.current) {
      lastEdge.current = edge;
      live.current.onEdge?.(edge);
    }
  }, []);

  /** The board point under the screen point, or the middle of the board when the host isn't there. */
  const boardPoint = useCallback(
    (clientX: number, clientY: number) => {
      const { box: b, size: sz } = live.current;
      const el = host.current;
      if (!el) return { x: b.x0 + b.w / 2, y: b.y0 + b.h / 2 };
      return toBoard(b, viewportOf(b, sz), el.getBoundingClientRect(), clientX, clientY);
    },
    [host],
  );

  const zoomAtClient = useCallback(
    (clientX: number, clientY: number, k: number, smooth = false) => {
      const u = boardPoint(clientX, clientY);
      apply(zoomAt(view.current, k, u.x, u.y), smooth);
    },
    [apply, boardPoint],
  );

  // a new board starts unzoomed; a resized window keeps the zoom but may need the board pulled back into view
  useLayoutEffect(() => apply(HOME), [apply, resetKey]);
  useLayoutEffect(() => apply(view.current), [apply, size?.w, size?.h]);

  useEffect(() => () => window.clearTimeout(settle.current.timer), []);

  // the + and − buttons zoom about the middle of what is showing
  useEffect(() => {
    if (!api) return;
    const step = (f: number) => {
      const b = live.current.box;
      apply(zoomAt(view.current, view.current.k * f, b.x0 + b.w / 2, b.y0 + b.h / 2), true);
    };
    api.current = { zoomIn: () => step(BUTTON_STEP), zoomOut: () => step(1 / BUTTON_STEP), reset: () => apply(HOME, true) };
    return () => {
      api.current = null;
    };
  }, [api, apply]);

  // The mouse wheel. React's onWheel is passive and can't stop the page scrolling, so this is a native listener.
  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      let d = e.deltaY;
      if (e.deltaMode === 1) d *= 33; // lines (Firefox)
      else if (e.deltaMode === 2) d *= 400; // pages
      d = Math.max(-300, Math.min(300, d));
      // a trackpad pinch arrives as a wheel with ctrl held, in much smaller steps
      const f = Math.exp(-d * (e.ctrlKey ? 0.01 : 0.0018));
      if (f !== 1) zoomAtClient(e.clientX, e.clientY, view.current.k * f);
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [host, zoomAtClient]);

  // Dragging with one pointer pans; two fingers pinch (and pan while they do).
  const pointers = useRef(new Map<number, { x: number; y: number }>());
  const gesture = useRef<Gesture>(idle());
  /** Set once a press turned into a drag or a pinch, so the click that follows it doesn't also place a piece. */
  const swallowClick = useRef(false);

  const capture = (el: Element, id: number) => {
    try {
      el.setPointerCapture(id);
    } catch {
      /* the pointer is already gone */
    }
  };
  const release = (el: Element, id: number) => {
    try {
      if (el.hasPointerCapture(id)) el.releasePointerCapture(id);
    } catch {
      /* nothing to release */
    }
  };

  const onPointerDown = (e: ReactPointerEvent<HTMLElement>) => {
    if (e.pointerType === 'mouse') {
      if (e.button !== 0) return;
      pointers.current.clear(); // a mouse has one pointer; drop one left over from a release outside the window
    }
    if (pointers.current.size >= 2) return; // a third finger is ignored
    swallowClick.current = false;
    pointers.current.set(e.pointerId, { x: e.clientX, y: e.clientY });
    const g = gesture.current;
    if (pointers.current.size === 1) {
      Object.assign(g, idle(), { mode: 'pan', sx: e.clientX, sy: e.clientY, lx: e.clientX, ly: e.clientY });
    } else {
      const [a, b] = [...pointers.current.values()];
      const mid = boardPoint((a.x + b.x) / 2, (a.y + b.y) / 2);
      const v = view.current;
      Object.assign(g, idle(), { mode: 'pinch', moved: true, d0: Math.max(1, Math.hypot(a.x - b.x, a.y - b.y)), cx: (mid.x - v.x) / v.k, cy: (mid.y - v.y) / v.k, k0: v.k });
      swallowClick.current = true;
      for (const id of pointers.current.keys()) capture(e.currentTarget, id);
    }
  };

  const onPointerMove = (e: ReactPointerEvent<HTMLElement>) => {
    const p = pointers.current.get(e.pointerId);
    if (!p) return;
    p.x = e.clientX;
    p.y = e.clientY;
    const g = gesture.current;
    if (g.mode === 'pinch' && pointers.current.size === 2) {
      const [a, b] = [...pointers.current.values()];
      const k = clampZoom((g.k0 * Math.hypot(a.x - b.x, a.y - b.y)) / g.d0);
      const mid = boardPoint((a.x + b.x) / 2, (a.y + b.y) / 2);
      apply({ k, x: mid.x - k * g.cx, y: mid.y - k * g.cy });
    } else if (g.mode === 'pan') {
      if (!g.moved) {
        const slop = e.pointerType === 'mouse' ? SLOP_MOUSE : SLOP_TOUCH;
        if (Math.hypot(e.clientX - g.sx, e.clientY - g.sy) < slop) return;
        g.moved = true;
        swallowClick.current = true;
        capture(e.currentTarget, e.pointerId); // from here the drag is ours, even outside the board
        e.currentTarget.dataset.panning = '1';
      }
      const s = viewportOf(live.current.box, live.current.size).s;
      const v = view.current;
      apply({ k: v.k, x: v.x + (e.clientX - g.lx) / s, y: v.y + (e.clientY - g.ly) / s });
      g.lx = e.clientX;
      g.ly = e.clientY;
    }
  };

  const onPointerEnd = (e: ReactPointerEvent<HTMLElement>) => {
    if (!pointers.current.delete(e.pointerId)) return;
    release(e.currentTarget, e.pointerId);
    const g = gesture.current;
    if (pointers.current.size === 1) {
      // one finger of a pinch lifted: the other carries on as a pan
      const [rest] = [...pointers.current.values()];
      Object.assign(g, idle(), { mode: 'pan', moved: true, sx: rest.x, sy: rest.y, lx: rest.x, ly: rest.y });
    } else if (pointers.current.size === 0) {
      Object.assign(g, idle());
      delete e.currentTarget.dataset.panning;
    }
  };

  const onClickCapture = (e: ReactMouseEvent<HTMLElement>) => {
    if (!swallowClick.current) return;
    swallowClick.current = false;
    e.stopPropagation();
    e.preventDefault();
  };

  return {
    layer,
    staticLayer,
    /** The board's zoom, to 1/20, for sizing click targets. */
    zoom,
    handlers: { onPointerDown, onPointerMove, onPointerUp: onPointerEnd, onPointerCancel: onPointerEnd, onClickCapture },
  };
}
