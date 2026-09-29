import { describe, it, expect } from "vitest";
import {
  formatActivityLine,
  formatActivityText,
  formatTimestamp,
  outcomeLabel,
} from "../activityLog";
import type { ActivityEntry } from "../../api";

// ローカル時刻から UNIX 秒を作る（タイムゾーンに依存せず期待値を固定するため）。
const ts = (h: number, m: number, s: number) =>
  Math.floor(new Date(2026, 8, 29, h, m, s).getTime() / 1000);

const ok: ActivityEntry = {
  timestamp: ts(9, 5, 3),
  op: "stage",
  summary: "ステージ（コミット準備）: a.txt",
  outcome: { status: "success" },
};
const failed: ActivityEntry = {
  timestamp: ts(9, 6, 0),
  op: "push",
  summary: "リモートへ送信",
  outcome: { status: "failed", message: "接続できません\n  ネットワークを確認" },
};
const undone: ActivityEntry = {
  timestamp: ts(9, 7, 30),
  op: "commit",
  summary: "取り消し（コミット）: コミットを取り消す",
  outcome: { status: "undone" },
};

describe("formatTimestamp", () => {
  it("ゼロ埋めした日時にする", () => {
    expect(formatTimestamp(ts(9, 5, 3))).toBe("2026-09-29 09:05:03");
  });
});

describe("outcomeLabel", () => {
  it("結果ごとの日本語ラベル", () => {
    expect(outcomeLabel(ok.outcome)).toBe("成功");
    expect(outcomeLabel(failed.outcome)).toBe("失敗");
    expect(outcomeLabel(undone.outcome)).toBe("取り消し");
  });
});

describe("formatActivityLine", () => {
  it("成功は日時・結果・説明", () => {
    expect(formatActivityLine(ok)).toBe(
      "2026-09-29 09:05:03  [成功]  ステージ（コミット準備）: a.txt",
    );
  });
  it("失敗は理由を 1 行に畳んで添える", () => {
    expect(formatActivityLine(failed)).toBe(
      "2026-09-29 09:06:00  [失敗]  リモートへ送信（理由: 接続できません ネットワークを確認）",
    );
  });
});

describe("formatActivityText", () => {
  it("見出し + 時系列の行を改行でつなぐ", () => {
    const text = formatActivityText([ok, failed, undone], "myrepo");
    expect(text.split("\n")).toEqual([
      "noobGit 操作ログ（myrepo）",
      "2026-09-29 09:05:03  [成功]  ステージ（コミット準備）: a.txt",
      "2026-09-29 09:06:00  [失敗]  リモートへ送信（理由: 接続できません ネットワークを確認）",
      "2026-09-29 09:07:30  [取り消し]  取り消し（コミット）: コミットを取り消す",
    ]);
  });
  it("空なら記録なしの旨を返す", () => {
    expect(formatActivityText([])).toBe(
      "noobGit 操作ログ\n（記録された操作はありません）",
    );
  });
});
