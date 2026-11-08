import { defineConfig } from 'vite';

export default defineConfig({
  // The wasm-pack package resolves its `.wasm` via `new URL(..., import.meta.url)`;
  // pre-bundling would break that during `vite dev`.
  optimizeDeps: { exclude: ['mishear'] },
  worker: { format: 'es' },
  server: {
    // The public-domain poems (for "random line") live at the repo root.
    fs: { allow: ['../../..'] },
  },
});
