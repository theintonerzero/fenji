import { defineConfig } from "vite";

// The Tauri shell loads `dist/` (tauri.conf.json's frontendDist), and `pnpm dev` serves the same
// files on a fixed port for `cargo tauri dev`.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    target: "es2022",
    // Every asset a file of its own: the CSP (default-src 'self') refuses data: URLs, which would
    // otherwise carry the small fonts.
    assetsInlineLimit: 0,
    outDir: "dist",
    emptyOutDir: true,
  },
});
