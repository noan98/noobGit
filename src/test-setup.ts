import "@testing-library/jest-dom";
import { vi } from "vitest";

// Tauri の invoke はデスクトップ環境にのみ存在するため、テスト環境ではモックする。
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

// window.__TAURI__ が参照される場合のための最小スタブ。
Object.defineProperty(window, "__TAURI__", {
  value: {},
  writable: true,
});
