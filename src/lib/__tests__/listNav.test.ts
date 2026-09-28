// #272: listNav の純粋関数のテスト。
import { describe, it, expect } from "vitest";
import { clampIndex, firstIndex, lastIndex, nextIndex, prevIndex } from "../listNav";

describe("nextIndex", () => {
  it("フォーカスなし（-1）から先頭（0）に進む", () => {
    expect(nextIndex(-1, 5)).toBe(0);
  });

  it("通常は +1 する", () => {
    expect(nextIndex(2, 5)).toBe(3);
  });

  it("末尾ではそのままクランプする（ループしない）", () => {
    expect(nextIndex(4, 5)).toBe(4);
  });

  it("一覧が空なら -1 を返す", () => {
    expect(nextIndex(0, 0)).toBe(-1);
    expect(nextIndex(-1, 0)).toBe(-1);
  });
});

describe("prevIndex", () => {
  it("フォーカスなし（-1）からは先頭（0）に留まる", () => {
    expect(prevIndex(-1, 5)).toBe(0);
  });

  it("通常は -1 する", () => {
    expect(prevIndex(3, 5)).toBe(2);
  });

  it("先頭ではそのままクランプする（ループしない）", () => {
    expect(prevIndex(0, 5)).toBe(0);
  });

  it("一覧が空なら -1 を返す", () => {
    expect(prevIndex(0, 0)).toBe(-1);
  });
});

describe("firstIndex / lastIndex（Home / End）", () => {
  it("firstIndex は 0 件なら -1、それ以外は 0", () => {
    expect(firstIndex(0)).toBe(-1);
    expect(firstIndex(10)).toBe(0);
  });

  it("lastIndex は 0 件なら -1、それ以外は末尾", () => {
    expect(lastIndex(0)).toBe(-1);
    expect(lastIndex(10)).toBe(9);
  });
});

describe("clampIndex（件数が変化したときの補正）", () => {
  it("フォーカスが無い（-1）ならそのまま -1", () => {
    expect(clampIndex(-1, 10)).toBe(-1);
  });

  it("件数が減って範囲外になったら末尾に丸める", () => {
    expect(clampIndex(8, 3)).toBe(2);
  });

  it("範囲内ならそのまま", () => {
    expect(clampIndex(1, 3)).toBe(1);
  });

  it("件数が 0 になったら -1", () => {
    expect(clampIndex(2, 0)).toBe(-1);
  });
});

describe("ゾーン間移動（ステージ済み → 未ステージのような、結合済み配列での境界越え）", () => {
  // StatusPanel はステージ済み・未ステージ・未追跡・コンフリクトを表示順で
  // 結合した 1 本の配列に対してこの関数を使う。境界での挙動を検証する。
  it("あるゾーンの末尾で ↓ すると、結合配列の次の要素（次ゾーンの先頭）に進む", () => {
    // ゾーン A: index 0-1（2件）, ゾーン B: index 2-4（3件）の想定。
    const totalCombined = 5;
    const lastOfZoneA = 1;
    expect(nextIndex(lastOfZoneA, totalCombined)).toBe(2); // ゾーン B の先頭
  });

  it("あるゾーンの先頭で ↑ すると、結合配列の前の要素（前ゾーンの末尾）に進む", () => {
    const totalCombined = 5;
    const firstOfZoneB = 2;
    expect(prevIndex(firstOfZoneB, totalCombined)).toBe(1); // ゾーン A の末尾
  });
});
