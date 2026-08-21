import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed port and needs to see its own errors, not vite's overlay.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  // Vite 8 bundles with rolldown and minifies with oxc; asking for esbuild
  // here loads a deprecated path that is no longer installed.
  build: { target: "chrome110", minify: "oxc", sourcemap: false },
});
