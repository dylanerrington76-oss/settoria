import { defineConfig, loadEnv } from 'vite';
import react from '@vitejs/plugin-react';

// In dev (`npm run dev`) the page is served by Vite and the websocket and /api calls are proxied to the
// Rust server, on localhost:3000 unless API_TARGET says otherwise (the docker-compose dev profile sets it
// to the server's container, `settoria:3000`).
export default defineConfig(({ mode }) => {
  const target = loadEnv(mode, '.', 'API_').API_TARGET || 'localhost:3000';
  return {
    plugins: [react()],
    server: {
      // The changelog page reads ../docs/Changelog.md, which is outside this folder.
      fs: { allow: ['..'] },
      proxy: {
        '/ws': { target: `ws://${target}`, ws: true },
        '/api': `http://${target}`,
      },
    },
  };
});
