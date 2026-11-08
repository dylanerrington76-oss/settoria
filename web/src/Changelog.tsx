// The changelog page. The text comes from docs/Changelog.md, so adding an entry there is all it takes:
//
//   ### [v2026.11.07]
//   What changed, in a sentence or two. Lines starting with "- " become a bullet list.
//
// Entries are shown newest first whatever order the file lists them in.
import { Fragment } from 'react';
import changelogSource from '../../docs/Changelog.md?raw';
import { linkClick } from './route';

export interface ChangelogEntry {
  /** "2026.11.06": the version heading without its "v" and brackets. */
  date: string;
  text: string;
  bullets: string[];
}

export function parseChangelog(source: string): ChangelogEntry[] {
  const entries: ChangelogEntry[] = [];
  let current: ChangelogEntry | null = null;
  for (const raw of source.split(/\r?\n/)) {
    const line = raw.trim();
    const version = /^#{1,6}\s*\[v?([^\]]+)\]/.exec(line);
    if (version) {
      current = { date: version[1].trim(), text: '', bullets: [] };
      entries.push(current);
    } else if (/^#{1,6}\s/.test(line)) {
      current = null; // some other heading, such as the "Changelog" title: its text is not an entry
    } else if (current && line) {
      const bullet = /^[-*]\s+(.*)$/.exec(line);
      if (bullet) current.bullets.push(bullet[1]);
      else current.text = current.text ? `${current.text} ${line}` : line;
    }
  }
  return entries.sort((a, b) => b.date.localeCompare(a.date, undefined, { numeric: true }));
}

const ENTRIES = parseChangelog(changelogSource);

/** A hand-drawn looking dark stroke, thicker toward the right, that separates two entries. */
function Divider() {
  return (
    <div className="cl-divider" aria-hidden="true">
      <svg viewBox="0 0 1000 24" preserveAspectRatio="none">
        <path d="M0 15 C120 9 230 17 350 12 S570 10 700 8 S900 3 1000 6 L1000 15 C900 19 770 17 650 19 S380 21 240 22 S60 22 0 19 Z" />
      </svg>
    </div>
  );
}

export default function Changelog({ onBack }: { onBack: () => void }) {
  return (
    <div className="changelog">
      {/* The picture sits behind the scrolling text and never moves. */}
      <div className="changelog-bg" aria-hidden="true" />
      <a className="changelog-back" href="/" onClick={linkClick(onBack)}>
        <span aria-hidden="true">←</span> Back to lobby
      </a>
      <main className="changelog-scroll" tabIndex={0} aria-label="Changelog">
        <div className="changelog-inner">
          <h1>Changelog</h1>
          {ENTRIES.length === 0 && <p className="changelog-entry">No changes recorded yet.</p>}
          {ENTRIES.map((e, i) => (
            <Fragment key={e.date}>
              {i > 0 && <Divider />}
              <article className="changelog-entry">
                <p>
                  {e.date}
                  {e.text ? `: ${e.text}` : ':'}
                </p>
                {e.bullets.length > 0 && (
                  <ul>
                    {e.bullets.map((b, j) => (
                      <li key={j}>{b}</li>
                    ))}
                  </ul>
                )}
              </article>
            </Fragment>
          ))}
        </div>
      </main>
    </div>
  );
}
