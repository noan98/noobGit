import { useEffect, useState } from "react";
import { api, type CommitInfo } from "../api";

interface Props {
  repoPath: string;
  // 履歴を表示する対象ファイルのパス。
  path: string;
  onClose: () => void;
}

// ファイル別履歴で読み込むコミットの最大件数。
const FILE_LOG_MAX = 50;

// Unix 秒を「N分前」などの相対表記に変換する（HistoryPanel と同じ方針）。
function formatRelativeTime(unixSeconds: number): string {
  const diff = Math.floor(Date.now() / 1000) - unixSeconds;
  if (diff < 60) return "たった今";
  if (diff < 3600) return `${Math.floor(diff / 60)}分前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}時間前`;
  if (diff < 86400 * 30) return `${Math.floor(diff / 86400)}日前`;
  if (diff < 86400 * 365) return `${Math.floor(diff / (86400 * 30))}ヶ月前`;
  return `${Math.floor(diff / (86400 * 365))}年前`;
}

// 1ファイルのコミット履歴を一覧表示するモーダル。
// 特定ファイルを変更したコミットだけを新しい順に並べる。
export function FileHistoryView({ repoPath, path, onClose }: Props) {
  const [commits, setCommits] = useState<CommitInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setCommits(null);
    setError(null);
    api
      .getFileLog(repoPath, path, FILE_LOG_MAX)
      .then((cs) => {
        if (!cancelled) setCommits(cs);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [repoPath, path]);

  return (
    <div
      className="overlay"
      role="dialog"
      aria-modal="true"
      onClick={onClose}
    >
      <div className="dialog" onClick={(e) => e.stopPropagation()}>
        <div className="dialog-head">
          <h2>「{path}」の変更履歴</h2>
        </div>

        {error && <p className="error">{error}</p>}

        {!error && commits === null && <p className="meta">読み込み中…</p>}

        {!error && commits !== null && commits.length === 0 && (
          <p className="meta">
            このファイルを変更したコミットは見つかりませんでした。
          </p>
        )}

        {!error && commits !== null && commits.length > 0 && (
          <ul className="commits file-history-commits">
            {commits.map((c) => (
              <li key={c.id} className="commit-row">
                <div className="commit-body">
                  <div className="commit-top">
                    <span className="summary">
                      {c.summary || "(メッセージなし)"}
                    </span>
                  </div>
                  <div className="commit-bottom">
                    <span className="meta">{c.author_name}</span>
                    <span className="meta-sep">·</span>
                    <span
                      className="meta"
                      title={new Date(c.time * 1000).toLocaleString("ja-JP")}
                    >
                      {formatRelativeTime(c.time)}
                    </span>
                    <code className="sha">{c.short_id}</code>
                  </div>
                </div>
              </li>
            ))}
          </ul>
        )}

        <div className="dialog-actions">
          <button className="btn" onClick={onClose}>
            閉じる
          </button>
        </div>
      </div>
    </div>
  );
}
