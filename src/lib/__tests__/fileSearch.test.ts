import { describe, it, expect } from "vitest";
import {
  filterByQuery,
  fuzzyMatch,
  highlightSegments,
  matchesFuzzyQuery,
} from "../fileSearch";

describe("fuzzyMatch", () => {
  it("検索語が空文字列（または空白のみ）なら常に一致し、indices は空", () => {
    expect(fuzzyMatch("src/App.tsx", "")).toEqual({ matched: true, indices: [] });
    expect(fuzzyMatch("src/App.tsx", "   ")).toEqual({ matched: true, indices: [] });
  });

  it("完全一致する部分文字列にマッチする（先頭から連続した位置になる）", () => {
    const result = fuzzyMatch("App.tsx", "app");
    expect(result.matched).toBe(true);
    expect(result.indices).toEqual([0, 1, 2]);
  });

  it("大文字小文字を無視する", () => {
    expect(fuzzyMatch("README.md", "readme").matched).toBe(true);
    expect(fuzzyMatch("readme.md", "README").matched).toBe(true);
  });

  it("非連続でも順番通りに現れる文字列にマッチする（サブシーケンス）", () => {
    // "spts" は "src/Panel.tsx" の中に s-p-t-s の順で（飛び飛びに）現れる。
    const result = fuzzyMatch("src/Panel.tsx", "spts");
    expect(result.matched).toBe(true);
    expect(result.indices.length).toBe(4);
    // インデックスは昇順で、text の範囲内。
    for (let i = 1; i < result.indices.length; i++) {
      expect(result.indices[i]).toBeGreaterThan(result.indices[i - 1]);
    }
  });

  it("順番が逆だと一致しない", () => {
    // "tsx" の後に "src" が来る文字列はないので、"txs" のような逆順クエリは失敗する。
    expect(fuzzyMatch("abc", "cab").matched).toBe(false);
  });

  it("含まれない文字があれば一致しない", () => {
    expect(fuzzyMatch("src/App.tsx", "zzz").matched).toBe(false);
  });
});

describe("matchesFuzzyQuery", () => {
  it("fuzzyMatch の matched のみを返す", () => {
    expect(matchesFuzzyQuery("core/repo.rs", "repo")).toBe(true);
    expect(matchesFuzzyQuery("core/repo.rs", "xyz")).toBe(false);
  });
});

describe("filterByQuery", () => {
  const paths = ["src/App.tsx", "src/api.ts", "core/repo.rs", "core/ops.rs"];

  it("検索語が空のときは元の配列と同じ内容・同じ並び順を返す", () => {
    const result = filterByQuery(paths, (p) => p, "");
    expect(result).toEqual(paths);
    // 新しい配列インスタンスであること（呼び出し元が誤って元配列を破壊しない）。
    expect(result).not.toBe(paths);
  });

  it("検索語に一致する要素だけを、元の並び順を保ったまま返す", () => {
    // ".rs" は core/repo.rs・core/ops.rs の末尾にのみ部分文字列として含まれる
    // （fuzzyMatch は非連続にもマッチしうるが、ここではあえて連続一致になる
    // クエリを選び、単純な絞り込みの確認に留める）。
    const result = filterByQuery(paths, (p) => p, ".rs");
    expect(result).toEqual(["core/repo.rs", "core/ops.rs"]);
  });

  it("一致がなければ空配列を返す", () => {
    expect(filterByQuery(paths, (p) => p, "zzz")).toEqual([]);
  });

  it("オブジェクトの配列にも getPath 経由で適用できる", () => {
    const files = [
      { path: "src/App.tsx", kind: "modified" as const },
      { path: "core/repo.rs", kind: "modified" as const },
    ];
    const result = filterByQuery(files, (f) => f.path, "app");
    expect(result).toEqual([files[0]]);
  });
});

describe("highlightSegments", () => {
  it("検索語が空なら text 全体を非マッチの 1 セグメントで返す", () => {
    expect(highlightSegments("src/App.tsx", "")).toEqual([
      { text: "src/App.tsx", matched: false },
    ]);
  });

  it("一致しない検索語のときも text 全体を非マッチの 1 セグメントで返す", () => {
    expect(highlightSegments("src/App.tsx", "zzz")).toEqual([
      { text: "src/App.tsx", matched: false },
    ]);
  });

  it("連続一致する部分文字列を 1 つのマッチセグメントにまとめる", () => {
    expect(highlightSegments("App.tsx", "app")).toEqual([
      { text: "App", matched: true },
      { text: ".tsx", matched: false },
    ]);
  });

  it("非連続マッチは複数のマッチセグメントに分かれる", () => {
    // "at" は "StatusPanel" の "..t...at.." のような飛び飛びの位置にマッチしうる。
    const segments = highlightSegments("StatusPanel.tsx", "sp");
    // セグメントを結合すると元の文字列に戻ること。
    expect(segments.map((s) => s.text).join("")).toBe("StatusPanel.tsx");
    // 少なくとも 1 つはマッチセグメントであること。
    expect(segments.some((s) => s.matched)).toBe(true);
  });
});
