import { describe, it, expect } from "vitest";
import type { ChangeKind, DiffLine, FileDiff } from "../../api";
import {
  changedLineCount,
  filterAndSortFileDiffs,
  filterFileDiffs,
  matchesQuery,
  sortFileDiffs,
} from "../commitDiffFilter";

// テスト用の FileDiff を簡単に組み立てるヘルパー。
function makeDiff(
  path: string,
  kind: ChangeKind,
  opts: { additions?: number; deletions?: number; contexts?: number } = {},
): FileDiff {
  const lines: DiffLine[] = [];
  const push = (n: number, lineKind: DiffLine["kind"]) => {
    for (let i = 0; i < n; i++) {
      lines.push({ kind: lineKind, old_lineno: null, new_lineno: null, content: "" });
    }
  };
  push(opts.additions ?? 0, "addition");
  push(opts.deletions ?? 0, "deletion");
  push(opts.contexts ?? 0, "context");

  return {
    path,
    is_binary: false,
    truncated: false,
    is_conflicted: false,
    kind,
    lines,
  };
}

describe("changedLineCount", () => {
  it("追加行と削除行の合計を数え、文脈行は数えない", () => {
    const diff = makeDiff("a.txt", "modified", {
      additions: 3,
      deletions: 2,
      contexts: 10,
    });
    expect(changedLineCount(diff)).toBe(5);
  });

  it("バイナリ等で lines が空なら 0", () => {
    const diff = makeDiff("bin.png", "modified");
    expect(changedLineCount(diff)).toBe(0);
  });
});

describe("matchesQuery", () => {
  it("空文字列（または空白のみ）は常に一致する", () => {
    expect(matchesQuery("src/App.tsx", "")).toBe(true);
    expect(matchesQuery("src/App.tsx", "   ")).toBe(true);
  });

  it("大文字小文字を無視した部分一致で判定する", () => {
    expect(matchesQuery("src/App.tsx", "app")).toBe(true);
    expect(matchesQuery("src/App.tsx", "APP.TSX")).toBe(true);
    expect(matchesQuery("src/App.tsx", "missing")).toBe(false);
  });
});

describe("filterFileDiffs", () => {
  const diffs = [
    makeDiff("added.txt", "added"),
    makeDiff("modified.txt", "modified"),
    makeDiff("deleted.txt", "deleted"),
    makeDiff("renamed.txt", "renamed"),
    // ボタンに出さない種別も混じる想定。
    makeDiff("type_change.txt", "type_change"),
  ];

  it("kinds が空集合なら変更種別を問わず全件が対象になる", () => {
    const result = filterFileDiffs(diffs, new Set(), "");
    expect(result).toHaveLength(diffs.length);
  });

  it("kinds を指定するとその種別だけに絞り込まれる", () => {
    const result = filterFileDiffs(diffs, new Set(["added", "deleted"]), "");
    expect(result.map((d) => d.path).sort()).toEqual([
      "added.txt",
      "deleted.txt",
    ]);
  });

  it("ファイル名検索と変更種別フィルタを組み合わせられる", () => {
    const result = filterFileDiffs(diffs, new Set(["added", "modified"]), "mod");
    expect(result.map((d) => d.path)).toEqual(["modified.txt"]);
  });

  it("フィルタボタンにない種別（type_change）でも未選択時は表示される", () => {
    const result = filterFileDiffs(diffs, new Set(), "type_change");
    expect(result.map((d) => d.path)).toEqual(["type_change.txt"]);
  });
});

describe("sortFileDiffs", () => {
  it("path: ファイル名の辞書順に並べる", () => {
    const diffs = [makeDiff("b.txt", "modified"), makeDiff("a.txt", "modified")];
    const result = sortFileDiffs(diffs, "path");
    expect(result.map((d) => d.path)).toEqual(["a.txt", "b.txt"]);
  });

  it("changes: 変更行数が多い順に並べる", () => {
    const diffs = [
      makeDiff("small.txt", "modified", { additions: 1 }),
      makeDiff("large.txt", "modified", { additions: 10, deletions: 5 }),
      makeDiff("mid.txt", "modified", { additions: 3 }),
    ];
    const result = sortFileDiffs(diffs, "changes");
    expect(result.map((d) => d.path)).toEqual([
      "large.txt",
      "mid.txt",
      "small.txt",
    ]);
  });

  it("changes: 変更行数が同数のときファイル名順で安定させる", () => {
    const diffs = [
      makeDiff("z.txt", "modified", { additions: 2 }),
      makeDiff("a.txt", "modified", { additions: 2 }),
    ];
    const result = sortFileDiffs(diffs, "changes");
    expect(result.map((d) => d.path)).toEqual(["a.txt", "z.txt"]);
  });

  it("kind: 追加→変更→削除→リネーム→その他の順に並べる", () => {
    const diffs = [
      makeDiff("r.txt", "renamed"),
      makeDiff("d.txt", "deleted"),
      makeDiff("a.txt", "added"),
      makeDiff("m.txt", "modified"),
    ];
    const result = sortFileDiffs(diffs, "kind");
    expect(result.map((d) => d.path)).toEqual([
      "a.txt",
      "m.txt",
      "d.txt",
      "r.txt",
    ]);
  });

  it("元の配列を変更しない", () => {
    const diffs = [makeDiff("b.txt", "modified"), makeDiff("a.txt", "modified")];
    const original = [...diffs];
    sortFileDiffs(diffs, "path");
    expect(diffs).toEqual(original);
  });
});

describe("filterAndSortFileDiffs", () => {
  it("絞り込みと並び替えを両方適用する", () => {
    const diffs = [
      makeDiff("b_added.txt", "added", { additions: 1 }),
      makeDiff("a_added.txt", "added", { additions: 5 }),
      makeDiff("modified.txt", "modified", { additions: 100 }),
    ];
    const result = filterAndSortFileDiffs(
      diffs,
      new Set(["added"]),
      "",
      "changes",
    );
    expect(result.map((d) => d.path)).toEqual(["a_added.txt", "b_added.txt"]);
  });
});
