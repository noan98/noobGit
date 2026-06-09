import type { CommitInfo, DiffLineKind, FileDiff } from "../api";

interface Props {
  // 比較の基準（古い側）のコミット。null のときは target の親との比較。
  base: CommitInfo | null;
  // 比較対象（新しい側）のコミット。
  target: CommitInfo;
  // 取得済みの差分（ファイルごと）。読み込み中は null。
  diffs: FileDiff[] | null;
  loading: boolean;
  // 比較表示を閉じる。
  onClose: () => void;
}

function sign(kind: DiffLineKind): string {
  if (kind === "addition") return "+";
  if (kind === "deletion") return "-";
  return "";
}

// コミットの短い表記（短縮ハッシュ + 要約）。
function commitLabel(c: CommitInfo): string {
  return `${c.short_id} ${c.summary || "(メッセージなし)"}`;
}

export function CommitDiffViewer({
  base,
  target,
  diffs,
  loading,
  onClose,
}: Props) {
  return (
    <div className="panel commit-diff-viewer">
      <div className="panel-head">
        <h2>コミット間の差分</h2>
        <button className="btn btn-small" onClick={onClose}>
          閉じる
        </button>
      </div>

      <p className="diff-path">
        {base ? commitLabel(base) : "(親コミット)"} → {commitLabel(target)}
      </p>

      {loading && <p className="empty">読み込み中…</p>}

      {!loading && diffs && diffs.length === 0 && (
        <p className="empty">2 つのコミット間に変更はありません。</p>
      )}

      {!loading &&
        diffs &&
        diffs.map((file) => (
          <div key={file.path} className="commit-diff-file">
            <h3 className="commit-diff-filename">{file.path}</h3>

            {file.is_binary ? (
              <p className="empty">バイナリのため差分は表示できません。</p>
            ) : file.lines.length === 0 ? (
              <p className="empty">このファイルに表示できる差分はありません。</p>
            ) : (
              <>
                <div className="diff-body">
                  <table className="diff-table">
                    <tbody>
                      {file.lines.map((line, i) => (
                        <tr key={i} className={`diff-line diff-${line.kind}`}>
                          <td className="diff-lineno">
                            {line.old_lineno ?? ""}
                          </td>
                          <td className="diff-lineno">
                            {line.new_lineno ?? ""}
                          </td>
                          <td className="diff-sign">{sign(line.kind)}</td>
                          <td className="diff-content">
                            {line.content || " "}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
                {file.truncated && (
                  <p className="empty">
                    差分が大きいため、最初の{file.lines.length}行のみ表示しています。
                  </p>
                )}
              </>
            )}
          </div>
        ))}
    </div>
  );
}
