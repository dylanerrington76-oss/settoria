// The extra "pages" the app has: /changelog and /map-editor. The server answers every unknown path with index.html,
// so the address works when typed in or reloaded. Moving between the app and these pages with the links below does
// not reload the page, so the websocket stays connected and "Back to lobby" returns to the live lobby at once.
import { MouseEvent, useCallback, useEffect, useState } from 'react';

export type Page = 'app' | 'changelog' | 'editor';
const PATHS: Record<Page, string> = { app: '/', changelog: '/changelog', editor: '/map-editor' };
const currentPage = (): Page => {
  const path = location.pathname.replace(/\/+$/, '');
  return path === PATHS.changelog ? 'changelog' : path === PATHS.editor ? 'editor' : 'app';
};

export function useRoute() {
  const [page, setPage] = useState<Page>(currentPage);
  useEffect(() => {
    const sync = () => setPage(currentPage());
    window.addEventListener('popstate', sync); // the browser's own back and forward buttons
    return () => window.removeEventListener('popstate', sync);
  }, []);
  const go = useCallback((to: Page) => {
    history.pushState(null, '', PATHS[to]);
    setPage(to);
  }, []);
  return { page, changelog: page === 'changelog', editor: page === 'editor', go };
}

/**
 * Click handler for an <a href> that is also handled in the app. A plain left click goes through `go`; a click
 * with a modifier key (open in a new tab, and so on) is left to the browser, so the href still works.
 */
export const linkClick = (go: () => void) => (e: MouseEvent<HTMLAnchorElement>) => {
  if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
  e.preventDefault();
  go();
};
