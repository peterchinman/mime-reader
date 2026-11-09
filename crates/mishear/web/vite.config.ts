import { defineConfig } from 'vite';

// Node's, when Vite loads this file (no @types/node needed for one variable).
declare const process: { env: Record<string, string | undefined> };

export default defineConfig({
  // Where the site is served from: "/" locally, "/mime-reader/" on GitHub
  // Pages (see .github/workflows/mishear-pages.yml).
  base: process.env.MISHEAR_BASE ?? '/',
  // The wasm-pack package resolves its `.wasm` via `new URL(..., import.meta.url)`;
  // pre-bundling would break that during `vite dev`.
  optimizeDeps: { exclude: ['mishear'] },
  worker: { format: 'es' },
  server: {
    // The public-domain poems (for "random line") live at the repo root.
    fs: { allow: ['../../..'] },
  },
});
