import type { BlameHunk } from "../api";

interface Props {
  // 対象ファイルのパス（作業ツリー内の相対パス）。
  path: string;
  // blame の結果（行のかたまりの配列）。null は読み込み中。
  hunks: BlameHunk[] | null;
  loading: boolean;
  // 取得に失敗したときのメッセージ。
  error: string | null;
  onClose: () => void;
}

// Unix 秒を「N分前」「N時間前」などの相対表記に変換する（HistoryPanel と同じ規則）。
function formatRelativeTime(unixSeconds: number): string {
  const diff = Math.floor(Date.now() / 1000) - unixSeconds;
  if (diff < 60) return "たった今";
  if (diff < 3600) return `${Math.floor(diff / 60)}分前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}時間前`;
  if (diff < 86400 * 30) return `${Math.floor(diff / 86400)}日前`;
  if (diff < 86400 * 365) return `${Math.floor(diff / (86400 * 30))}ヶ月前`;
  return `${Math.floor(diff / (86400 * 365))}年前`;
}

/*
 * BlameView — ファイルの各行が「最後にどのコミットで変更されたか」を表示する。
 *
 * 左側にコミットの short_id・著者・相対日時、右側にその行範囲（lines_start から
 * lines_count 行）を表示する。git2 の blame は内容そのものを持たないため、ここでは
 * 「どのコミットが何行目から何行ぶんを担当しているか」を一覧で見せる。
 */
export function BlameView({ path, hunks, loading, error, onClose }: Props) {
  return (
    <div
      className="overlay"
      role="dialog"
      aria-modal="true"
      onClick={onClose}
    >
      <div className="dialog blame-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="dialog-head">
          <h2>変更履歴（blame）</h2>
          <span className="diff-source">{path}</span>
        </div>

        <p className="blame-help">
          各行を最後に変更したコミットを表示します。左がコミット、右がそのコミットが
          担当する行の範囲です。
        </p>

        {loading && <p className="empty">読み込み中…</p>}

        {!loading && error && <p className="error">{error}</p>}

        {!loading && !error && hunks && hunks.length === 0 && (
          <p className="empty">表示できる変更履歴がありません。</p>
        )}

        {!loading && !error && hunks && hunks.length > 0 && (
          <div className="blame-body">
            <table className="blame-table">
              <tbody>
                {hunks.map((h, i) => {
                  const end = h.lines_start + h.lines_count - 1;
                  const range =
                    h.lines_count === 1
                      ? `${h.lines_start}`
                      : `${h.lines_start}–${end}`;
                  return (
                    <tr key={i} className="blame-row">
                      <td className="blame-commit">
                        <code className="sha">{h.short_id}</code>
                        <span
                          className="meta"
                          title={h.message_short}
                        >
                          {h.message_short || "(メッセージなし)"}
                        </span>
                      </td>
                      <td className="blame-author meta">{h.author_name}</td>
                      <td
                        className="blame-time meta"
                        title={new Date(h.time * 1000).toLocaleString("ja-JP")}
                      >
                        {formatRelativeTime(h.time)}
                      </td>
                      <td className="blame-lines">
                        <span className="blame-lineno">{range} 行目</span>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
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
