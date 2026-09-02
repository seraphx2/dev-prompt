/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [svelte()],

  // Tauri expects a fixed port, fail if that port is not available
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // tell vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },

  // Env variables starting with the item of `envPrefix` will be exposed in tauri's source code through `import.meta.env`.
  envPrefix: ["VITE_", "TAURI_ENV_*"],

  // Unit tests for the pure TS helpers (fuzzy ranking, hotkey classification).
  // `npm test`. Component/Svelte tests aren't set up — those helpers stay
  // framework-free on purpose.
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
    // vitest 4's default `forks` (and `threads`) pool crashes here with
    // "Cannot read properties of undefined (reading 'config')" whenever the cwd
    // resolves with a lowercase Windows drive letter (e.g. `d:\git\...`).
    // vmThreads is unaffected, and these are pure-function tests with no
    // DOM/native deps, so the lighter isolation costs us nothing.
    pool: "vmThreads",
  },
  build: {
    // Tauri uses Chromium on Windows and WebKit on macOS and Linux
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    // don't minify for debug builds
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    // produce sourcemaps for debug builds
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
}));
