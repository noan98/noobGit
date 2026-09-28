import { describe, it, expect } from "vitest";
import { cycleActiveTabId, reorderByIds, tabCycleDirection } from "../tabOrder";

describe("tabCycleDirection", () => {
  it("Ctrl+Tab で 1（次のタブ）を返す", () => {
    const e = new KeyboardEvent("keydown", { key: "Tab", ctrlKey: true });
    expect(tabCycleDirection(e)).toBe(1);
  });

  it("Ctrl+Shift+Tab で -1（前のタブ）を返す", () => {
    const e = new KeyboardEvent("keydown", {
      key: "Tab",
      ctrlKey: true,
      shiftKey: true,
    });
    expect(tabCycleDirection(e)).toBe(-1);
  });

  it("Cmd+Tab（metaKey）でも Ctrl と同様に扱う", () => {
    const e = new KeyboardEvent("keydown", { key: "Tab", metaKey: true });
    expect(tabCycleDirection(e)).toBe(1);
  });

  it("修飾キーなしの Tab では null を返す", () => {
    const e = new KeyboardEvent("keydown", { key: "Tab" });
    expect(tabCycleDirection(e)).toBeNull();
  });

  it("Ctrl+Tab 以外のキーでは null を返す", () => {
    const e = new KeyboardEvent("keydown", { key: "a", ctrlKey: true });
    expect(tabCycleDirection(e)).toBeNull();
  });
});

describe("cycleActiveTabId", () => {
  const ids = ["a", "b", "c"];

  it("direction: 1 で次のタブへ進むこと", () => {
    expect(cycleActiveTabId(ids, "a", 1)).toBe("b");
    expect(cycleActiveTabId(ids, "b", 1)).toBe("c");
  });

  it("末尾から先頭へラップすること", () => {
    expect(cycleActiveTabId(ids, "c", 1)).toBe("a");
  });

  it("direction: -1 で前のタブへ進むこと", () => {
    expect(cycleActiveTabId(ids, "c", -1)).toBe("b");
    expect(cycleActiveTabId(ids, "b", -1)).toBe("a");
  });

  it("先頭から末尾へラップすること（逆方向）", () => {
    expect(cycleActiveTabId(ids, "a", -1)).toBe("c");
  });

  it("タブが 1 つ以下なら activeId をそのまま返すこと", () => {
    expect(cycleActiveTabId(["a"], "a", 1)).toBe("a");
    expect(cycleActiveTabId([], "a", 1)).toBe("a");
  });

  it("activeId が一覧に無い場合は activeId をそのまま返すこと", () => {
    expect(cycleActiveTabId(ids, "z", 1)).toBe("z");
  });
});

describe("reorderByIds", () => {
  interface Item {
    id: string;
    label: string;
  }
  const items: Item[] = [
    { id: "a", label: "A" },
    { id: "b", label: "B" },
    { id: "c", label: "C" },
  ];

  it("orderedIds の順に並べ替えること", () => {
    const result = reorderByIds(items, ["c", "a", "b"]);
    expect(result.map((i) => i.id)).toEqual(["c", "a", "b"]);
  });

  it("元の item オブジェクトの参照を保つこと（不要な再生成をしない）", () => {
    const result = reorderByIds(items, ["b", "a", "c"]);
    expect(result[1]).toBe(items[0]);
  });

  it("orderedIds に無い id の item は元の相対順のまま末尾に残ること", () => {
    const result = reorderByIds(items, ["c"]);
    expect(result.map((i) => i.id)).toEqual(["c", "a", "b"]);
  });

  it("items に存在しない id は無視すること", () => {
    const result = reorderByIds(items, ["z", "c", "a", "b"]);
    expect(result.map((i) => i.id)).toEqual(["c", "a", "b"]);
  });

  it("orderedIds が空でも items をそのままの順で返すこと", () => {
    const result = reorderByIds(items, []);
    expect(result.map((i) => i.id)).toEqual(["a", "b", "c"]);
  });
});
