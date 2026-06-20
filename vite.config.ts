import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// Tauri は固定ポートの devUrl を期待するため strictPort にする。
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test-setup.ts"],
  },
});
