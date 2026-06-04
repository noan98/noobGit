import type { CommitInfo } from "../api";
import { EmptyState } from "./EmptyState";

interface Props {
  commits: CommitInfo[];
  onReset: (commit: CommitInfo) => void;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
  // コミット入力欄へ誘導する（Empty State の「コミットへ」ボタン用）。
  onGoToCommit: () => void;
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

export function HistoryPanel({
  commits,
  onReset,
  hasMore,
  loadingMore,
  onLoadMore,
  onGoToCommit,
}: Props) {
  return (
    <div className="panel">
      <div className="panel-head">
        <h2>履歴</h2>
      </div>

      {commits.length === 0 ? (
        <EmptyState
          icon="📝"
          title="まだコミットがありません"
          description="最初のコミットを作って、変更の記録を始めましょう。"
          action={{ label: "コミットへ", onClick: onGoToCommit }}
        />
      ) : (
        <>
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
                <button
                  className="link danger"
                  title="このコミットの状態まで作業ツリーを戻します（ハードリセット）"
                  onClick={() => onReset(c)}
                >
                  ここまで戻す
                </button>
              </li>
            ))}
          </ul>
          {hasMore && (
            <div className="load-more">
              <button
                className="btn btn-small"
                onClick={onLoadMore}
                disabled={loadingMore}
              >
                {loadingMore ? "読み込み中…" : "もっと見る"}
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
