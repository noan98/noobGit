import { act, render, screen } from "@testing-library/react";
import { vi, describe, it, expect, beforeEach } from "vitest";
import { Sidebar } from "../Sidebar";

const WIDTH_KEY = "noobgit_sidebar_width";

// テストに必要な最小限の props。ブランチ等の一覧は空でよい。
function renderSidebar() {
  return render(
    <Sidebar
      view="status"
      onSelectView={vi.fn()}
      changeCount={0}
      conflictCount={0}
      branches={[]}
      tags={[]}
      remotes={[]}
      stashes={[]}
      undoCount={0}
      onSwitchBranch={vi.fn()}
    />,
  );
}

function getHandle() {
  return screen.getByRole("separator", { name: "サイドバー幅の変更" });
}

describe("Sidebar のリサイズ", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("保存済みの幅がなければ既定幅（240px）で表示されること", () => {
    renderSidebar();
    const handle = getHandle();
    expect(handle).toHaveAttribute("aria-valuenow", "240");
    expect(handle).toHaveAttribute("aria-valuemin", "160");
    expect(handle).toHaveAttribute("aria-valuemax", "480");
    expect(handle).toHaveAttribute("aria-orientation", "vertical");
  });

  it("localStorage に保存済みの幅があれば復元されること", () => {
    localStorage.setItem(WIDTH_KEY, "300");
    renderSidebar();
    expect(getHandle()).toHaveAttribute("aria-valuenow", "300");
  });

  it("壊れた保存値（範囲外・非数値）は既定幅にフォールバックしてクランプされること", () => {
    localStorage.setItem(WIDTH_KEY, "99999");
    renderSidebar();
    // 最大幅にクランプされる。
    expect(getHandle()).toHaveAttribute("aria-valuenow", "480");
  });

  it("非数値の保存値は既定幅になること", () => {
    localStorage.setItem(WIDTH_KEY, "not-a-number");
    renderSidebar();
    expect(getHandle()).toHaveAttribute("aria-valuenow", "240");
  });

  it("localStorage が使えない環境でも既定幅で表示されること", () => {
    const getItemSpy = vi
      .spyOn(Storage.prototype, "getItem")
      .mockImplementation(() => {
        throw new Error("blocked");
      });
    renderSidebar();
    expect(getHandle()).toHaveAttribute("aria-valuenow", "240");
    getItemSpy.mockRestore();
  });

  it("右矢印キーで幅が広がり、localStorage に保存されること", () => {
    renderSidebar();
    const handle = getHandle();
    handle.focus();
    act(() => {
      handle.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
      );
    });
    expect(handle).toHaveAttribute("aria-valuenow", "256"); // 240 + 16
    expect(localStorage.getItem(WIDTH_KEY)).toBe("256");
  });

  it("左矢印キーで幅が狭まり、最小幅でクランプされること", () => {
    localStorage.setItem(WIDTH_KEY, "170");
    renderSidebar();
    const handle = getHandle();
    handle.focus();
    // 170 - 16 = 154 だが最小幅 160 でクランプされる。
    act(() => {
      handle.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true }),
      );
    });
    expect(handle).toHaveAttribute("aria-valuenow", "160");
    expect(localStorage.getItem(WIDTH_KEY)).toBe("160");
  });

  it("End キーで最大幅、Home キーで最小幅になること", () => {
    renderSidebar();
    const handle = getHandle();
    handle.focus();
    act(() => {
      handle.dispatchEvent(
        new KeyboardEvent("keydown", { key: "End", bubbles: true }),
      );
    });
    expect(handle).toHaveAttribute("aria-valuenow", "480");
    act(() => {
      handle.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Home", bubbles: true }),
      );
    });
    expect(handle).toHaveAttribute("aria-valuenow", "160");
  });

  it("ドラッグ（pointerdown → pointermove → pointerup）で幅が変わり保存されること", () => {
    renderSidebar();
    const handle = getHandle();

    act(() => {
      handle.dispatchEvent(
        new PointerEvent("pointerdown", {
          bubbles: true,
          clientX: 240,
          button: 0,
        }),
      );
    });
    act(() => {
      window.dispatchEvent(
        new PointerEvent("pointermove", { bubbles: true, clientX: 290 }),
      );
    });
    expect(handle).toHaveAttribute("aria-valuenow", "290"); // 240 + (290-240)
    act(() => {
      window.dispatchEvent(new PointerEvent("pointerup", { bubbles: true }));
    });

    expect(localStorage.getItem(WIDTH_KEY)).toBe("290");
  });
});
