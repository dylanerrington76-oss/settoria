import type { GamePatch, GameView, PatchOp, PatchStep } from './types';

/**
 * Keeps the game view up to date from the patches the server sends (`src/patch.rs` makes them, and says what
 * an op is). Nothing here knows about the game: it follows paths into the view and changes what they lead to.
 *
 * The view is never changed in place. What a patch touches is copied, along the path to it, and everything it
 * doesn't touch stays the very same object, so the parts of the page that look at the board, or at one
 * player, can tell nothing about them changed.
 */

/** How many of the newest log events the view holds: `LOG_WINDOW` in `src/game.rs`. */
export const LOG_WINDOW = 50;

/** The patch doesn't fit the view it was meant for. The cure is to ask the server for the whole view again. */
export class PatchError extends Error {}

function setIn(node: unknown, path: PatchStep[], depth: number, op: PatchOp): unknown {
  const here = path.slice(0, depth).join('/') || '(top)';
  if (node === null || typeof node !== 'object') throw new PatchError(`nothing to step into at ${here}`);
  const step = path[depth];
  if (Array.isArray(node) !== (typeof step === 'number')) throw new PatchError(`bad step ${String(step)} at ${here}`);
  const holder = node as Record<PatchStep, unknown>;
  const last = depth === path.length - 1;
  const sets = op.length > 1;

  let child: unknown;
  if (last) {
    if (sets && holder[step] === op[1]) return node; // the same number, string or null: nothing to copy
  } else {
    child = setIn(holder[step], path, depth + 1, op);
    if (child === holder[step]) return node;
  }
  const copy: Record<PatchStep, unknown> = Array.isArray(node) ? (node.slice() as unknown as Record<PatchStep, unknown>) : { ...holder };
  if (!last) copy[step] = child;
  else if (sets) copy[step] = op[1];
  else delete copy[step];
  return copy;
}

/**
 * The view after the patch. Throws `PatchError` if the patch doesn't fit it: a path that leads nowhere, or log
 * events that don't follow on from the ones held.
 */
export function applyPatch(view: GameView, patch: GamePatch): GameView {
  let next: unknown = view;
  for (const op of patch.ops) {
    const path = op[0];
    if (!Array.isArray(path) || path.length === 0) throw new PatchError('an op without a path');
    next = setIn(next, path, 0, op);
  }
  let out = next as GameView;
  if (patch.log && patch.log.length > 0) {
    const held = view.log_start + view.log.length;
    if (patch.log_from !== held) throw new PatchError(`log events from ${patch.log_from}, but the log held ends at ${held}`);
    const log = view.log.concat(patch.log);
    const dropped = Math.max(0, log.length - LOG_WINDOW);
    out = { ...out, log: dropped > 0 ? log.slice(dropped) : log, log_start: view.log_start + dropped };
  }
  return out;
}
