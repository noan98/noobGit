// #272: useListNav のテスト。
import { act, renderHook } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { useListNav } from "../useListNav";

// React.KeyboardEvent の代わりに、フックが実際に使うプロパティだけを
// 持つダミーオブジェクトを渡す（jsdom の本物のイベントは不要）。
function keyEvent(key: string) {
  return { key, preventDefault: vi.fn() } as unknown as React.KeyboardEvent;
}

describe("useListNav", () => {
  it("初期状態では activeIndex が -1（未フォーカス）", () => {
    const { result } = renderHook(() => useListNav({ itemCount: 5 }));
    expect(result.current.activeIndex).toBe(-1);
  });

  it("ArrowDown で先頭（0）に進み、以降は +1 ずつ進む", () => {
    const { result } = renderHook(() => useListNav({ itemCount: 3 }));

    act(() => result.current.onKeyDown(keyEvent("ArrowDown")));
    expect(result.current.activeIndex).toBe(0);

    act(() => result.current.onKeyDown(keyEvent("ArrowDown")));
    expect(result.current.activeIndex).toBe(1);
  });

  it("末尾で ArrowDown してもそれ以上進まない（ループしない）", () => {
    const { result } = renderHook(() => useListNav({ itemCount: 2 }));

    act(() => result.current.onKeyDown(keyEvent("ArrowDown")));
    act(() => result.current.onKeyDown(keyEvent("ArrowDown")));
    act(() => result.current.onKeyDown(keyEvent("ArrowDown")));

    expect(result.current.activeIndex).toBe(1);
  });

  it("ArrowUp で戻る", () => {
    const { result } = renderHook(() => useListNav({ itemCount: 5 }));

    act(() => result.current.onKeyDown(keyEvent("ArrowDown")));
    act(() => result.current.onKeyDown(keyEvent("ArrowDown")));
    act(() => result.current.onKeyDown(keyEvent("ArrowUp")));

    expect(result.current.activeIndex).toBe(0);
  });

  it("End で末尾へ、Home で先頭へ移動する", () => {
    const { result } = renderHook(() => useListNav({ itemCount: 10 }));

    act(() => result.current.onKeyDown(keyEvent("End")));
    expect(result.current.activeIndex).toBe(9);

    act(() => result.current.onKeyDown(keyEvent("Home")));
    expect(result.current.activeIndex).toBe(0);
  });

  it("Enter でフォーカス中の行の onActivate が呼ばれる", () => {
    const onActivate = vi.fn();
    const { result } = renderHook(() => useListNav({ itemCount: 3, onActivate }));

    act(() => result.current.onKeyDown(keyEvent("ArrowDown")));
    act(() => result.current.onKeyDown(keyEvent("ArrowDown")));
    act(() => result.current.onKeyDown(keyEvent("Enter")));

    expect(onActivate).toHaveBeenCalledWith(1);
  });

  it("スペースキーでも onActivate が呼ばれる", () => {
    const onActivate = vi.fn();
    const { result } = renderHook(() => useListNav({ itemCount: 3, onActivate }));

    act(() => result.current.onKeyDown(keyEvent("ArrowDown")));
    act(() => result.current.onKeyDown(keyEvent(" ")));

    expect(onActivate).toHaveBeenCalledWith(0);
  });

  it("onActivate 省略時は Enter を押してもエラーにならず何も起きない", () => {
    const { result } = renderHook(() => useListNav({ itemCount: 3 }));

    act(() => result.current.onKeyDown(keyEvent("ArrowDown")));
    expect(() => act(() => result.current.onKeyDown(keyEvent("Enter")))).not.toThrow();
  });

  it("未フォーカス（activeIndex -1）で Enter を押しても onActivate は呼ばれない", () => {
    const onActivate = vi.fn();
    const { result } = renderHook(() => useListNav({ itemCount: 3, onActivate }));

    act(() => result.current.onKeyDown(keyEvent("Enter")));

    expect(onActivate).not.toHaveBeenCalled();
  });

  it("件数が減ってフォーカス位置が範囲外になったら末尾に丸められる", () => {
    const { result, rerender } = renderHook(
      ({ itemCount }) => useListNav({ itemCount }),
      { initialProps: { itemCount: 5 } },
    );

    act(() => result.current.onKeyDown(keyEvent("End")));
    expect(result.current.activeIndex).toBe(4);

    rerender({ itemCount: 2 });
    expect(result.current.activeIndex).toBe(1);
  });

  it("setActiveIndex でマウス操作などから直接フォーカス位置を設定できる", () => {
    const { result } = renderHook(() => useListNav({ itemCount: 5 }));

    act(() => result.current.setActiveIndex(3));

    expect(result.current.activeIndex).toBe(3);
  });
  it("行の中のボタン等から届いたキー入力（バブリング）は扱わず、preventDefault もしない", () => {
    const onActivate = vi.fn();
    const { result } = renderHook(() => useListNav({ itemCount: 3, onActivate }));
    act(() => result.current.setActiveIndex(1));

    const container = {};
    const childButton = {};
    const e = {
      key: "Enter",
      target: childButton,
      currentTarget: container,
      preventDefault: vi.fn(),
    } as unknown as React.KeyboardEvent;
    act(() => result.current.onKeyDown(e));

    expect(onActivate).not.toHaveBeenCalled();
    expect(e.preventDefault).not.toHaveBeenCalled();
  });

  it("修飾キー付き（Ctrl+Enter など）は横取りしない", () => {
    const onActivate = vi.fn();
    const { result } = renderHook(() => useListNav({ itemCount: 3, onActivate }));
    act(() => result.current.setActiveIndex(0));

    const e = { key: "Enter", ctrlKey: true, preventDefault: vi.fn() } as unknown as React.KeyboardEvent;
    act(() => result.current.onKeyDown(e));

    expect(onActivate).not.toHaveBeenCalled();
    expect(e.preventDefault).not.toHaveBeenCalled();
  });
});
