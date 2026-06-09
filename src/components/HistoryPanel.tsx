import { useState } from "react";
import type { CommitInfo } from "../api";
import { EmptyState } from "./EmptyState";

interface Props {
  commits: CommitInfo[];
  currentBranch: string | null;
  onReset: (commit: CommitInfo) => void;
  onCherryPick: (commit: CommitInfo) => void;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
  // コミット入力欄へ誘導する（Empty State の「コミットへ」ボタン用）。
  onGoToCommit: () => void;
}

// Unix 秒を「N分前」「N時間前」などの相対表記に変換する。
function formatRelativeTime(unixSeconds: number): string {
  const diff = Math.floor(Date.now() / 1000) - unixSeconds;
  if (diff < 60) return "たった今";
  if (diff < 3600) return `${Math.floor(diff / 60)}分前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}時間前`;
  if (diff < 86400 * 30) return `${Math.floor(diff / 86400)}日前`;
  if (diff < 86400 * 365) return `${Math.floor(diff / (86400 * 30))}ヶ月前`;
  return `${Math.floor(diff / (86400 * 365))}年前`;
}

// 著者名から 2 文字のイニシャルを生成する。
function authorInitials(name: string): string {
  const parts = name.trim().split(/\s+/);
  if (parts.length >= 2) {
    return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
  }
  return name.slice(0, 2).toUpperCase();
}

// 著者名から決定論的なアバター背景色を生成する（同じ名前は常に同じ色）。
const AVATAR_PALETTES = [
  { bg: "#ddf4ff", fg: "#0969da" },
  { bg: "#dafbe1", fg: "#1a7f37" },
  { bg: "#fff1cc", fg: "#9a6700" },
  { bg: "#ffebe9", fg: "#cf222e" },
  { bg: "#faf0ff", fg: "#8250df" },
  { bg: "#fff1e5", fg: "#bc4c00" },
  { bg: "#e6f0ff", fg: "#1b60d1" },
  { bg: "#e6ffec", fg: "#116329" },
];

function authorPalette(name: string) {
  let hash = 0;
  for (let i = 0; i < name.length; i++) hash = (hash * 31 + name.charCodeAt(i)) >>> 0;
  return AVATAR_PALETTES[hash % AVATAR_PALETTES.length];
}

// ショートハッシュのコピーボタン。クリック後に「コピーしました」表示を一瞬出す。
function CopyHashButton({ shortId }: { shortId: string }) {
  const [copied, setCopied] = useState(false);

  async function handleCopy(e: React.MouseEvent) {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(shortId);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // クリップボードアクセス失敗時は何もしない
    }
  }

  return (
    <button
      className="commit-hash-copy"
      onClick={handleCopy}
      title={copied ? "コピーしました" : "ハッシュをコピー"}
      aria-label={`ハッシュ ${shortId} をコピー`}
    >
      <code className="sha">{shortId}</code>
      <span className="copy-icon">{copied ? "✓" : "⎘"}</span>
    </button>
  );
}

export function HistoryPanel({
  commits,
  currentBranch,
  onReset,
  onCherryPick,
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
            {commits.map((c, idx) => {
              const isHead = idx === 0;
              const palette = authorPalette(c.author_name);
              const initials = authorInitials(c.author_name);
              return (
                <li key={c.id} className="commit-row">
                  {/* 著者アバター */}
                  <div
                    className="commit-avatar"
                    style={{ background: palette.bg, color: palette.fg }}
                    title={c.author_name}
                    aria-hidden="true"
                  >
                    {initials}
                  </div>

                  {/* メイン情報 */}
                  <div className="commit-body">
                    <div className="commit-top">
                      <span className="summary">
                        {c.summary || "(メッセージなし)"}
                      </span>
                      {isHead && currentBranch && (
                        <span className="branch-badge" title="現在のブランチ">
                          {currentBranch}
                        </span>
                      )}
                    </div>
                    <div className="commit-bottom">
                      <span className="meta">{c.author_name}</span>
                      <span className="meta-sep">·</span>
                      <span className="meta" title={new Date(c.time * 1000).toLocaleString("ja-JP")}>
                        {formatRelativeTime(c.time)}
                      </span>
                      <CopyHashButton shortId={c.short_id} />
                    </div>
                  </div>

                  {/* 操作ボタン */}
                  <div className="commit-actions-inline">
                    <button
                      className="link commit-cherry-pick-btn"
                      title="このコミットの変更を、いまのブランチにコピーします（cherry-pick）"
                      onClick={() => onCherryPick(c)}
                    >
                      コピー
                    </button>
                    <button
                      className="link danger commit-reset-btn"
                      title="このコミットの状態まで作業ツリーを戻します（ハードリセット）"
                      onClick={() => onReset(c)}
                    >
                      戻す
                    </button>
                  </div>
                </li>
              );
            })}
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
