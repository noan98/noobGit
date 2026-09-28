import { configDefaults, defineConfig } from "vitest/config";
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
    // e2e/ は tauri-driver + WebdriverIO 用の別スイート（#177）。ファイル名は
    // `*.e2e.ts` で vitest の既定パターン（`*.test.ts` / `*.spec.ts`）とは
    // そもそも一致しないが、念のため明示的にも除外しておく。
    exclude: [...configDefaults.exclude, "e2e/**"],
  },
});
