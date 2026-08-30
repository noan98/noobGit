import "@testing-library/jest-dom";
import { vi } from "vitest";

// Tauri の invoke はデスクトップ環境にのみ存在するため、テスト環境ではモックする。
// Channel（#167 進捗フィードバック）も同様にモックする。実際の IPC は行わず、
// onmessage を保持するだけの最小実装にする（テストから ch.onmessage(...) を直接
// 呼び出せば進捗イベントをシミュレートできる）。
class MockChannel<T = unknown> {
  onmessage: (response: T) => void = () => {};
  constructor(onmessage?: (response: T) => void) {
    if (onmessage) this.onmessage = onmessage;
  }
}

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  Channel: MockChannel,
}));

// window.__TAURI__ が参照される場合のための最小スタブ。
Object.defineProperty(window, "__TAURI__", {
  value: {},
  writable: true,
});
