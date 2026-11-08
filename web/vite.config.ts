import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// In dev (`npm run dev`) the page is served by Vite and the websocket is
// and /api calls are proxied to the Rust server on port 3000.
export default defineConfig({
  plugins: [react()],
  server: {
    // The changelog page reads ../docs/Changelog.md, which is outside this folder.
    fs: { allow: ['..'] },
    proxy: {
      '/ws': { target: 'ws://localhost:3000', ws: true },
      '/api': 'http://localhost:3000',
    },
  },
});
