import { defineConfig } from 'vite';

export default defineConfig({
  // The wasm-pack package resolves its `.wasm` via `new URL(..., import.meta.url)`.
  // Excluding it from esbuild's dependency pre-bundling keeps that resolution
  // intact during `vite dev`.
  optimizeDeps: { exclude: ['mime-reader'] },
  // Two pages: rhyme distance tuning (/) and echo tuning (/echo.html).
  // Inputs resolve against the project root (this directory).
  build: {
    rollupOptions: {
      input: {
        main: 'index.html',
        echo: 'echo.html',
      },
    },
  },
  server: {
    // `mime-reader` is a sibling dir (web/pkg) symlinked into node_modules,
    // i.e. outside this app's root. The shared pair fixture lives at the repo
    // root, so allow Vite to serve files from the whole workspace.
    fs: { allow: ['../..'] },
  },
});
