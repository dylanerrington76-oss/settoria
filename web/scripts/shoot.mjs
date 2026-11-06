// Screenshots the game screen from the dev-only fixtures (src/mock/fixtures.ts)
// with Puppeteer, so UI changes can be checked without running the Rust server.
//
//   npm run shots                       # all states, 1510x862 -> web/shots/*.png
//   npm run shots -- discard 1280 720   # one state at a custom size
//
// Set CHROME_PATH if Chrome/Chromium is not in a standard location.
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync } from 'node:fs';
import puppeteer from 'puppeteer-core';

const STATES = ['setup', 'roll', 'discard', 'main', 'steal'];
const [only, w = '1510', h = '862'] = process.argv.slice(2);
const chrome =
  process.env.CHROME_PATH ??
  [
    '/usr/bin/google-chrome',
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    'C:/Program Files/Google/Chrome/Application/chrome.exe',
  ].find(existsSync);
if (!chrome) throw new Error('Chrome not found: set CHROME_PATH');

const PORT = 5199;
const vite = spawn('npx', ['vite', '--port', String(PORT), '--host', '127.0.0.1', '--strictPort'], { stdio: 'ignore', shell: process.platform === 'win32' });
const stop = () => vite.kill();
process.on('exit', stop);

const up = async () => {
  for (let i = 0; i < 60; i++) {
    try {
      if ((await fetch(`http://127.0.0.1:${PORT}/mock.html`)).ok) return;
    } catch {}
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error('vite did not start');
};

try {
  await up();
  mkdirSync('shots', { recursive: true });
  // keep scrollbars: they are part of the real look (headless hides them by default)
  const browser = await puppeteer.launch({ executablePath: chrome, headless: true, args: ['--no-sandbox'], ignoreDefaultArgs: ['--hide-scrollbars'] });
  for (const s of only ? [only] : STATES) {
    const page = await browser.newPage();
    await page.setViewport({ width: +w, height: +h });
    page.on('pageerror', (e) => console.error('page error:', e.message));
    await page.goto(`http://127.0.0.1:${PORT}/mock.html?s=${s}`, { waitUntil: 'networkidle0' });
    await page.evaluate(() => document.fonts.ready);
    await page.screenshot({ path: `shots/${s}.png` });
    console.log('shots/' + s + '.png');
    await page.close();
  }
  await browser.close();
} finally {
  stop();
}
