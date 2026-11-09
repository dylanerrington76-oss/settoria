// Small footer for the pre-game screens
import { linkClick } from './route';

const AUTHOR_NAME = 'Dylan Errington';
const GITHUB_URL = 'https://github.com/dylanerrington76-oss/settoria';
const CONTACT_EMAIL = 'dylan.errington23@icloud.com';

export default function Footer({ onChangelog, onMapEditor }: { onChangelog: () => void; onMapEditor: () => void }) {
  return (
    <footer className="site-footer">
      <span>Created by {AUTHOR_NAME}</span>
      <a href={GITHUB_URL} target="_blank" rel="noopener noreferrer">
        github
      </a>
      <a href="/changelog" onClick={linkClick(onChangelog)}>
        changelog
      </a>
      <a href="/map-editor" onClick={linkClick(onMapEditor)}>
        map editor
      </a>
      <a href={`mailto:${CONTACT_EMAIL}`}>contact</a>
    </footer>
  );
}
