// Small footer for the pre-game screens
import { linkClick } from './route';

const AUTHOR_NAME = 'Dylan Errington';
const GITHUB_URL = 'https://github.com/dylanerrington76-oss/settoria';
const DROPBOX_URL = 'https://www.dropbox.com/scl/fo/ocif8131i5kwiltcsrwt6/APjsNsMQU3npu6gxSD8yGoQ?rlkey=5keoen992lrb4hkz54ey18tae&st=do0b6zdx&dl=0';
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
      <a href={DROPBOX_URL} target="_blank" rel="noopener noreferrer">
        more maps
      </a>
      <a href={`mailto:${CONTACT_EMAIL}`}>contact</a>
    </footer>
  );
}
