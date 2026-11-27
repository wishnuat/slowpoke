import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

// Serves the real UI with the Tauri APIs mocked (UI review and screenshots).
export default defineConfig({
  root: path.resolve(import.meta.dirname, ".."),
  plugins: [react()],
  resolve: {
    alias: {
      "@tauri-apps/api/core": path.resolve(import.meta.dirname, "mock-core.ts"),
      "@tauri-apps/api/event": path.resolve(import.meta.dirname, "mock-event.ts"),
      "@tauri-apps/api/window": path.resolve(import.meta.dirname, "mock-window.ts"),
    },
  },
  server: { port: 1460, strictPort: true },
});
