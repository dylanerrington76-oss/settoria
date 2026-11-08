// The one extra "page" the app has: /changelog. The server answers every unknown path with index.html, so the
// address works when typed in or reloaded. Moving between the app and the changelog with the links below does not
// reload the page, so the websocket stays connected and "Back to lobby" returns to the live lobby at once.
import { MouseEvent, useCallback, useEffect, useState } from 'react';

const CHANGELOG_PATH = '/changelog';
const onChangelogPath = () => location.pathname.replace(/\/+$/, '') === CHANGELOG_PATH;

export function useRoute() {
  const [changelog, setChangelog] = useState(onChangelogPath);
  useEffect(() => {
    const sync = () => setChangelog(onChangelogPath());
    window.addEventListener('popstate', sync); // the browser's own back and forward buttons
    return () => window.removeEventListener('popstate', sync);
  }, []);
  const go = useCallback((to: 'app' | 'changelog') => {
    history.pushState(null, '', to === 'changelog' ? CHANGELOG_PATH : '/');
    setChangelog(to === 'changelog');
  }, []);
  return { changelog, go };
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
