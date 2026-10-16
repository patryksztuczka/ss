import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite-plus";

const DEV_SERVER_PORT = 5183;

export default defineConfig({
  root: "src/renderer",
  plugins: [react(), tailwindcss()],
  server: {
    port: DEV_SERVER_PORT,
    strictPort: true,
  },
  base: "./",
  build: {
    outDir: "../../dist-electron/renderer",
    emptyOutDir: true,
  },
  pack: [
    {
      entry: { main: "src/main/index.ts" },
      format: "cjs",
      outDir: "dist-electron",
      outExtensions: () => ({ js: ".cjs" }),
      // Bundle everything except Electron itself: the packaged app ships no node_modules.
      deps: { neverBundle: ["electron"], alwaysBundle: [/.*/], onlyBundle: false },
      sourcemap: true,
      clean: false,
    },
    {
      entry: { preload: "src/preload/index.ts" },
      format: "cjs",
      outDir: "dist-electron",
      outExtensions: () => ({ js: ".cjs" }),
      deps: { neverBundle: ["electron"], onlyBundle: false },
      sourcemap: true,
      // Keep the renderer build and the compiled helper that also live in dist-electron.
      clean: false,
    },
  ],
  run: {
    tasks: {
      build: {
        command: "vp pack && node scripts/build-native.mjs && vp build",
      },
      dev: {
        command: "node scripts/dev.mjs",
        cache: false,
      },
      dist: {
        command: "vp run build && electron-builder --mac --publish never",
        cache: false,
      },
    },
  },
});
