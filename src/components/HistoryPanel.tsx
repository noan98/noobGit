import type { CommitInfo } from "../api";

interface Props {
  commits: CommitInfo[];
}

function formatTime(unixSeconds: number): string {
  const d = new Date(unixSeconds * 1000);
  return d.toLocaleString("ja-JP", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function HistoryPanel({ commits }: Props) {
  return (
    <div className="panel">
      <div className="panel-head">
        <h2>履歴</h2>
      </div>

      {commits.length === 0 ? (
        <p className="empty">まだコミットがありません。最初のコミットをしてみましょう。</p>
      ) : (
        <ul className="commits">
          {commits.map((c) => (
            <li key={c.id}>
              <code className="sha">{c.short_id}</code>
              <div className="commit-body">
                <span className="summary">{c.summary || "(メッセージなし)"}</span>
                <span className="meta">
                  {c.author_name} ・ {formatTime(c.time)}
                </span>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
