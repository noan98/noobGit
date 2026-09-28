import { describe, it, expect } from "vitest";
import {
  applyCommitSuggestion,
  buildCommitSuggestions,
  CONVENTIONAL_COMMIT_PREFIXES,
  filterConventionalPrefixes,
} from "../commitSuggest";

describe("filterConventionalPrefixes", () => {
  it("前方一致で絞り込む（大文字小文字を無視）", () => {
    const got = filterConventionalPrefixes("FE", 5);
    expect(got.map((p) => p.label)).toEqual(["feat:"]);
  });

  it("prefix が空なら全件を返す", () => {
    const got = filterConventionalPrefixes("", 10);
    expect(got).toEqual(CONVENTIONAL_COMMIT_PREFIXES);
  });

  it("max 件までに切り詰める", () => {
    const got = filterConventionalPrefixes("", 2);
    expect(got).toHaveLength(2);
  });

  it("一致が無ければ空配列", () => {
    expect(filterConventionalPrefixes("xyz", 5)).toEqual([]);
  });
});

describe("buildCommitSuggestions", () => {
  it("履歴の一致があればそれを優先する", () => {
    const got = buildCommitSuggestions(
      ["fix: バグA", "fix: バグB"],
      "fix",
      5,
    );
    expect(got).toEqual([{ text: "fix: バグA" }, { text: "fix: バグB" }]);
  });

  it("履歴の一致が無ければ Conventional Commits プレフィックスをフォールバックにする", () => {
    const got = buildCommitSuggestions([], "fe", 5);
    expect(got).toEqual([{ text: "feat:", desc: "新機能の追加" }]);
  });

  it("max 件までに切り詰める（履歴あり）", () => {
    const got = buildCommitSuggestions(
      ["a", "b", "c", "d", "e", "f"],
      "",
      5,
    );
    expect(got).toHaveLength(5);
  });

  it("入力中の件名と完全に同じ候補は出さない（確定後に再び開かないように）", () => {
    expect(
      buildCommitSuggestions(["fix: バグA", "fix: バグA 追加"], "fix: バグA", 5),
    ).toEqual([{ text: "fix: バグA 追加" }]);
    // 履歴が自分自身だけなら、完全一致するプレフィックスも出さず空になる。
    expect(buildCommitSuggestions(["feat:"], "feat:", 5)).toEqual([]);
  });
});

describe("applyCommitSuggestion", () => {
  it("履歴の件名候補は件名全体を置き換える", () => {
    const got = applyCommitSuggestion("fi", "fix: バグを修正");
    expect(got.text).toBe("fix: バグを修正");
    expect(got.cursor).toBe("fix: バグを修正".length);
  });

  it("Conventional Commits プレフィックス単体は末尾に半角スペースを補う", () => {
    const got = applyCommitSuggestion("fe", "feat:");
    expect(got.text).toBe("feat: ");
    expect(got.cursor).toBe("feat: ".length);
  });

  it("本文（2 行目以降）はそのまま残す", () => {
    const got = applyCommitSuggestion("fi\n\n本文です", "fix: バグを修正");
    expect(got.text).toBe("fix: バグを修正\n\n本文です");
  });
});
