import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi, describe, it, expect } from "vitest";
import { TabBar, type TabItem } from "../TabBar";

// テスト用のタブ 2 件: リポジトリを開いているタブと、未オープンの新しいタブ。
const TABS: TabItem[] = [
  { id: "tab-1", label: "my-project", openedPath: "C:\\work\\my-project" },
  { id: "tab-2", label: "新しいタブ", openedPath: null },
];

function renderTabBar(
  overrides: Partial<Parameters<typeof TabBar>[0]> = {},
) {
  const handlers = {
    onSelect: vi.fn(),
    onClose: vi.fn(),
    onAdd: vi.fn(),
  };
  render(
    <TabBar
      tabs={TABS}
      activeId="tab-1"
      onSelect={handlers.onSelect}
      onClose={handlers.onClose}
      onAdd={handlers.onAdd}
      {...overrides}
    />,
  );
  return handlers;
}

describe("TabBar", () => {
  it("タブのラベルが表示され、アクティブタブに aria-selected が付くこと", () => {
    renderTabBar();
    const tabs = screen.getAllByRole("tab");
    expect(tabs).toHaveLength(2);
    expect(tabs[0]).toHaveTextContent("my-project");
    expect(tabs[0]).toHaveAttribute("aria-selected", "true");
    expect(tabs[1]).toHaveTextContent("新しいタブ");
    expect(tabs[1]).toHaveAttribute("aria-selected", "false");
  });

  it("タブをクリックすると onSelect がそのタブの id で呼ばれること", async () => {
    const user = userEvent.setup();
    const handlers = renderTabBar();
    await user.click(screen.getByRole("tab", { name: /新しいタブ/ }));
    expect(handlers.onSelect).toHaveBeenCalledWith("tab-2");
    expect(handlers.onClose).not.toHaveBeenCalled();
  });

  it("閉じるボタンで onClose が呼ばれ、onSelect は呼ばれないこと", async () => {
    const user = userEvent.setup();
    const handlers = renderTabBar();
    await user.click(
      screen.getByRole("button", { name: "タブ「my-project」を閉じる" }),
    );
    expect(handlers.onClose).toHaveBeenCalledWith("tab-1");
    expect(handlers.onSelect).not.toHaveBeenCalled();
  });

  it("「新しいタブ」ボタンで onAdd が呼ばれること", async () => {
    const user = userEvent.setup();
    const handlers = renderTabBar();
    await user.click(screen.getByRole("button", { name: "新しいタブ" }));
    expect(handlers.onAdd).toHaveBeenCalledTimes(1);
  });
});
