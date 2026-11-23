import { useCallback, useEffect, useRef, useState } from 'react';
import { expandBoard } from './boardGeometry';
import { applyPatch } from './patch';
import type { GameView, LobbyPlayer, MapInfo, ServerMsg, Settings, TimerMode } from './types';

export type Screen =
  | { kind: 'idle' }
  | { kind: 'lobby'; you: number; players: LobbyPlayer[]; timer: TimerMode; settings: Settings; map: string; maps: MapInfo[] }
  | { kind: 'game'; view: GameView };

/**
 * Websocket connection to the Rust server. The reconnect token lives in
 * sessionStorage (per browser tab), so a page reload rejoins the same seat.
 *
 * The server sends the whole game view (board included, in its short form) once, and after that only what changed
 * (`patch`), so the view the page sees is kept up to date here. If a patch doesn't fit, or one is missed, the whole view is asked for again.
 */
export function useGame() {
  const [screen, setScreen] = useState<Screen>({ kind: 'idle' });
  const [error, setError] = useState<string | null>(null);
  const [online, setOnline] = useState(false);
  const [name, setName] = useState<string | null>(() => sessionStorage.getItem('catan-name'));
  const [color, setColor] = useState<string | null>(() => sessionStorage.getItem('catan-color'));
  const ws = useRef<WebSocket | null>(null);

  useEffect(() => {
    if (!name) return;
    let closed = false;
    let timer: number | undefined;
    const open = () => {
      const proto = location.protocol === 'https:' ? 'wss' : 'ws';
      const sock = new WebSocket(`${proto}://${location.host}/ws`);
      ws.current = sock;
      // The view as of the last message and that message's number, for the patches that follow it.
      let held: { view: GameView; seq: number } | null = null;
      // Set once a whole view has been asked for, until it arrives: patches in the meantime fit nothing.
      let resyncing = false;
      const resync = (why: unknown) => {
        console.warn('Asking for the whole game view again:', why);
        resyncing = true;
        held = null;
        if (sock.readyState === WebSocket.OPEN) sock.send(JSON.stringify({ type: 'resync' }));
      };
      sock.onopen = () => {
        setOnline(true);
        sock.send(
          JSON.stringify({ type: 'join', name, color, token: sessionStorage.getItem('catan-token'), delta: true }),
        );
      };
      sock.onmessage = (e) => {
        const m = JSON.parse(e.data) as ServerMsg;
        switch (m.type) {
          case 'welcome':
            sessionStorage.setItem('catan-token', m.token);
            break;
          case 'lobby':
            held = null;
            resyncing = false;
            setScreen({ kind: 'lobby', you: m.you, players: m.players, timer: m.timer, settings: m.settings, map: m.map, maps: m.maps });
            break;
          case 'game': {
            // The board comes in its short form (see boardGeometry.ts); it is worked out in full here, once, and
            // patches then carry it along untouched. (A server that doesn't send patches sends no `seq` and a
            // whole view every time.)
            let view: GameView;
            try {
              view = { ...m.view, board: expandBoard(m.view.board) };
            } catch (err) {
              console.error('The board could not be read:', err);
              setError('The server sent a board this page cannot read. Reload the page, and if it keeps happening the page and the server are out of step.');
              break;
            }
            held = { view, seq: m.seq ?? -1 };
            resyncing = false;
            setScreen({ kind: 'game', view });
            break;
          }
          case 'patch':
            if (resyncing) break;
            if (!held) {
              resync('a patch with no view to patch');
              break;
            }
            try {
              if (m.seq !== held.seq + 1) throw new Error(`patch ${m.seq} came after ${held.seq}`);
              const view = applyPatch(held.view, m);
              held = { view, seq: m.seq };
              setScreen({ kind: 'game', view });
            } catch (err) {
              resync(err);
            }
            break;
          case 'error':
            setError(m.message);
            if (m.code === 'color_taken') {
              // Someone took our color between picking it and joining: back to the join screen.
              sessionStorage.removeItem('catan-name');
              sessionStorage.removeItem('catan-token');
              setName(null);
            }
            break;
        }
      };
      sock.onclose = () => {
        setOnline(false);
        if (!closed) timer = window.setTimeout(open, 1000);
      };
    };
    open();
    return () => {
      closed = true;
      window.clearTimeout(timer);
      ws.current?.close();
    };
  }, [name, color]);

  useEffect(() => {
    if (!error) return;
    const t = window.setTimeout(() => setError(null), 4500);
    return () => window.clearTimeout(t);
  }, [error]);

  const send = useCallback((msg: object) => {
    if (ws.current?.readyState === WebSocket.OPEN) ws.current.send(JSON.stringify(msg));
  }, []);

  const join = useCallback((n: string, c: string) => {
    sessionStorage.setItem('catan-name', n);
    sessionStorage.setItem('catan-prefill', n);
    sessionStorage.setItem('catan-color', c);
    setColor(c);
    setName(n);
  }, []);

  const leave = useCallback(() => {
    sessionStorage.removeItem('catan-name');
    sessionStorage.removeItem('catan-token');
    setName(null);
    setScreen({ kind: 'idle' });
  }, []);

  return { screen, error, online, name, send, join, leave };
}
