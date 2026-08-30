import { describe, it, expect } from "vitest";
import { isPaletteShortcut } from "../useGlobalShortcuts";

// #181: コマンドパレットは Ctrl+K（Mac では Cmd+K）で開く。
describe("isPaletteShortcut", () => {
  it("Ctrl+K で true を返す", () => {
    const e = new KeyboardEvent("keydown", { key: "k", ctrlKey: true });
    expect(isPaletteShortcut(e)).toBe(true);
  });

  it("Cmd+K（metaKey）で true を返す", () => {
    const e = new KeyboardEvent("keydown", { key: "k", metaKey: true });
    expect(isPaletteShortcut(e)).toBe(true);
  });

  it("修飾キーなしの k では false を返す", () => {
    const e = new KeyboardEvent("keydown", { key: "k" });
    expect(isPaletteShortcut(e)).toBe(false);
  });

  it("Ctrl+K 以外のキーでは false を返す", () => {
    const e = new KeyboardEvent("keydown", { key: "p", ctrlKey: true });
    expect(isPaletteShortcut(e)).toBe(false);
  });

  it("Shift+K のような別の組み合わせでは false を返す", () => {
    const e = new KeyboardEvent("keydown", { key: "k", shiftKey: true });
    expect(isPaletteShortcut(e)).toBe(false);
  });
});
