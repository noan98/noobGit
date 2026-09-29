// #208 操作アクティビティログの整形（純粋関数）。
// 「先輩に貼る」ためのプレーンテキストを作る。Git ロジックは含めない。
import type { ActivityEntry, ActivityOutcome } from "../api";

const pad = (n: number): string => String(n).padStart(2, "0");

// UNIX 秒 → 「2026-09-29 12:34:56」（ローカル時刻）。
export function formatTimestamp(unixSeconds: number): string {
  const d = new Date(unixSeconds * 1000);
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ` +
    `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
  );
}

// 結果の短いラベル。
export function outcomeLabel(outcome: ActivityOutcome): string {
  switch (outcome.status) {
    case "success":
      return "成功";
    case "failed":
      return "失敗";
    case "undone":
      return "取り消し";
  }
}

// エラーメッセージを 1 行に畳む（複数行だと貼ったときに読みにくい）。
function oneLine(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

// 1 エントリ → 1 行。例:
//   2026-09-29 12:34:56  [失敗]  リモートへ送信 ...（理由: 接続できません）
export function formatActivityLine(entry: ActivityEntry): string {
  const base = `${formatTimestamp(entry.timestamp)}  [${outcomeLabel(entry.outcome)}]  ${oneLine(entry.summary)}`;
  return entry.outcome.status === "failed"
    ? `${base}（理由: ${oneLine(entry.outcome.message)}）`
    : base;
}

// ログ全体 → コピー用プレーンテキスト。`entries` は古い順（時系列）で渡す。
// `repoName` を渡すと見出しに添える。ログが空なら、その旨だけの 1 行を返す。
export function formatActivityText(
  entries: ActivityEntry[],
  repoName?: string,
): string {
  const head = repoName
    ? `noobGit 操作ログ（${repoName}）`
    : "noobGit 操作ログ";
  if (entries.length === 0) return `${head}\n（記録された操作はありません）`;
  return [head, ...entries.map(formatActivityLine)].join("\n");
}
