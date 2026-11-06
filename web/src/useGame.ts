import { useCallback, useEffect, useRef, useState } from 'react';
import type { GameView, LobbyPlayer, MapInfo, ServerMsg, Settings, TimerMode } from './types';

export type Screen =
  | { kind: 'idle' }
  | { kind: 'lobby'; you: number; players: LobbyPlayer[]; timer: TimerMode; settings: Settings; map: string; maps: MapInfo[] }
  | { kind: 'game'; view: GameView };

/**
 * Websocket connection to the Rust server. The reconnect token lives in
 * sessionStorage (per browser tab), so a page reload rejoins the same seat.
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
      sock.onopen = () => {
        setOnline(true);
        sock.send(
          JSON.stringify({ type: 'join', name, color, token: sessionStorage.getItem('catan-token') }),
        );
      };
      sock.onmessage = (e) => {
        const m = JSON.parse(e.data) as ServerMsg;
        switch (m.type) {
          case 'welcome':
            sessionStorage.setItem('catan-token', m.token);
            break;
          case 'lobby':
            setScreen({ kind: 'lobby', you: m.you, players: m.players, timer: m.timer, settings: m.settings, map: m.map, maps: m.maps });
            break;
          case 'game':
            setScreen({ kind: 'game', view: m.view });
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
