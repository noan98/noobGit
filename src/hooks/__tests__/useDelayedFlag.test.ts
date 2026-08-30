import { act, renderHook } from "@testing-library/react";
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { useDelayedFlag } from "../useDelayedFlag";

describe("useDelayedFlag", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("loading が false のままなら true にならない", () => {
    const { result } = renderHook(() => useDelayedFlag(false, 100));
    expect(result.current).toBe(false);

    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(result.current).toBe(false);
  });

  it("delayMs 以内に loading が終われば表示フラグは一度も立たない（ちらつき防止）", () => {
    const { result, rerender } = renderHook(
      ({ loading }) => useDelayedFlag(loading, 100),
      { initialProps: { loading: true } },
    );
    expect(result.current).toBe(false);

    // 100ms 経つ前にロードが終わる。
    act(() => {
      vi.advanceTimersByTime(60);
    });
    rerender({ loading: false });

    // delay 経過分だけ進めても、フラグは立たないままであること。
    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(result.current).toBe(false);
  });

  it("delayMs を超えて loading が続けば true になる", () => {
    const { result } = renderHook(() => useDelayedFlag(true, 100));
    expect(result.current).toBe(false);

    act(() => {
      vi.advanceTimersByTime(99);
    });
    expect(result.current).toBe(false);

    act(() => {
      vi.advanceTimersByTime(2);
    });
    expect(result.current).toBe(true);
  });

  it("表示中に loading が false になれば即座にフラグが下がる", () => {
    const { result, rerender } = renderHook(
      ({ loading }) => useDelayedFlag(loading, 100),
      { initialProps: { loading: true } },
    );

    act(() => {
      vi.advanceTimersByTime(150);
    });
    expect(result.current).toBe(true);

    rerender({ loading: false });
    expect(result.current).toBe(false);
  });

  it("delayMs 未指定なら既定の 100ms が使われる", () => {
    const { result } = renderHook(() => useDelayedFlag(true));

    act(() => {
      vi.advanceTimersByTime(99);
    });
    expect(result.current).toBe(false);

    act(() => {
      vi.advanceTimersByTime(2);
    });
    expect(result.current).toBe(true);
  });
});
