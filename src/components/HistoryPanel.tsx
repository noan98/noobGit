import { useEffect, useRef, useState } from "react";
import type { CommitInfo, LogFilter } from "../api";
import { EmptyState } from "./EmptyState";

interface Props {
  commits: CommitInfo[];
  currentBranch: string | null;
  onReset: (commit: CommitInfo) => void;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
  // コミット入力欄へ誘導する（Empty State の「コミットへ」ボタン用）。
  onGoToCommit: () => void;
  // 差分比較で選んだコミット。最初のクリックで base、2 つ目で target になる。
  onCompareSelect: (commit: CommitInfo) => void;
  // 比較で選択中のコミット ID（最初に選んだ base 側）。ハイライト表示に使う。
  compareBaseId: string | null;
  // 検索条件が変わったとき（デバウンス後）に親へ通知して再取得をトリガする。
  // 条件が空になったら filter は空オブジェクト（条件なし）になる。
  onSearch: (filter: LogFilter) => void;
  // 検索（再取得）の実行中かどうか。スピナー表示に使う。
  searching: boolean;
}

// 入力の遅延（ミリ秒）。打鍵のたびに再取得せず、入力が落ち着いてから 1 回だけ呼ぶ。
const SEARCH_DEBOUNCE_MS = 300;

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
  hasMore,
  loadingMore,
  onLoadMore,
  onGoToCommit,
  onCompareSelect,
  compareBaseId,
  onSearch,
  searching,
}: Props) {
  // 検索ボックスの入力値。入力のたびに即時反映し、再取得はデバウンスして行う。
  const [messageQuery, setMessageQuery] = useState("");
  const [authorQuery, setAuthorQuery] = useState("");
  // 検索条件が一つでも入力されているか（Empty State の出し分けに使う）。
  const isSearching = messageQuery.trim() !== "" || authorQuery.trim() !== "";

  // 最新の onSearch を参照するための ref。デバウンス内でクロージャが陳腐化するのを防ぐ。
  const onSearchRef = useRef(onSearch);
  useEffect(() => {
    onSearchRef.current = onSearch;
  }, [onSearch]);

  // 入力が落ち着いたら（デバウンス後）に親へ条件を通知する。
  useEffect(() => {
    const handle = setTimeout(() => {
      const filter: LogFilter = {};
      const m = messageQuery.trim();
      const a = authorQuery.trim();
      if (m) filter.message = m;
      if (a) filter.author = a;
      onSearchRef.current(filter);
    }, SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(handle);
  }, [messageQuery, authorQuery]);

  return (
    <div className="panel">
      <div className="panel-head">
        <h2>履歴</h2>
        {compareBaseId && (
          <span className="compare-hint" title="もう 1 つコミットを選ぶと差分を表示します">
            比較対象を選択中…
          </span>
        )}
        {searching && (
          <span className="history-searching" role="status">
            <span className="network-spinner">🔄</span>検索中…
          </span>
        )}
      </div>

      {/* メッセージ・作者での絞り込み検索。入力は 300ms デバウンスして再取得する。 */}
      <div className="history-search">
        <input
          type="search"
          className="history-search-input"
          value={messageQuery}
          placeholder="メッセージで検索"
          aria-label="コミットメッセージで検索"
          onChange={(e) => setMessageQuery(e.target.value)}
        />
        <input
          type="search"
          className="history-search-input"
          value={authorQuery}
          placeholder="作者で検索（名前・メール）"
          aria-label="作者で検索"
          onChange={(e) => setAuthorQuery(e.target.value)}
        />
      </div>

      {commits.length === 0 ? (
        isSearching ? (
          <EmptyState
            icon="🔍"
            title="一致するコミットがありません"
            description="検索条件を変えるか、入力を消すとすべての履歴に戻ります。"
          />
        ) : (
          <EmptyState
            icon="📝"
            title="まだコミットがありません"
            description="最初のコミットを作って、変更の記録を始めましょう。"
            action={{ label: "コミットへ", onClick: onGoToCommit }}
          />
        )
      ) : (
        <>
          <ul className="commits">
            {commits.map((c, idx) => {
              const isHead = idx === 0;
              const palette = authorPalette(c.author_name);
              const initials = authorInitials(c.author_name);
              const isCompareBase = compareBaseId === c.id;
              return (
                <li
                  key={c.id}
                  className={`commit-row${isCompareBase ? " compare-base" : ""}`}
                >
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

                  {/* 差分比較ボタン。1 つ目で base、2 つ目で target を選ぶ。 */}
                  <button
                    className={`link commit-compare-btn${isCompareBase ? " active" : ""}`}
                    title={
                      isCompareBase
                        ? "比較対象（基準）に選択中。もう一度押すと解除します"
                        : compareBaseId
                          ? "このコミットとの差分を表示します"
                          : "差分比較の基準にします（もう 1 つ選ぶと差分を表示）"
                    }
                    onClick={() => onCompareSelect(c)}
                  >
                    {isCompareBase ? "基準" : "比較"}
                  </button>

                  {/* ハードリセットボタン */}
                  <button
                    className="link danger commit-reset-btn"
                    title="このコミットの状態まで作業ツリーを戻します（ハードリセット）"
                    onClick={() => onReset(c)}
                  >
                    戻す
                  </button>
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
