import type { DiffLineKind, FileDiff } from "../api";

export interface DiffSelection {
  path: string;
  staged: boolean;
}

interface Props {
  selection: DiffSelection | null;
  diff: FileDiff | null;
  loading: boolean;
}

function sign(kind: DiffLineKind): string {
  if (kind === "addition") return "+";
  if (kind === "deletion") return "-";
  return "";
}

export function DiffPanel({ selection, diff, loading }: Props) {
  return (
    <div className="panel diff-panel">
      <div className="panel-head">
        <h2>差分プレビュー</h2>
        {selection && (
          <span className="diff-source">
            {selection.staged ? "ステージ済み" : "未ステージ"}
          </span>
        )}
      </div>

      {!selection && (
        <p className="empty">
          ファイルを選ぶと、変更の中身（差分）がここに表示されます。
        </p>
      )}

      {selection && (
        <>
          <p className="diff-path">{selection.path}</p>

          {loading && <p className="empty">読み込み中…</p>}

          {!loading && diff?.is_binary && (
            <p className="empty">バイナリのため差分は表示できません。</p>
          )}

          {!loading && diff && !diff.is_binary && diff.lines.length === 0 && (
            <p className="empty">選択したファイルに差分はありません。</p>
          )}

          {!loading && diff && !diff.is_binary && diff.lines.length > 0 && (
            <>
              <div className="diff-body">
                <table className="diff-table">
                  <tbody>
                    {diff.lines.map((line, i) => (
                      <tr key={i} className={`diff-line diff-${line.kind}`}>
                        <td className="diff-lineno">{line.old_lineno ?? ""}</td>
                        <td className="diff-lineno">{line.new_lineno ?? ""}</td>
                        <td className="diff-sign">{sign(line.kind)}</td>
                        <td className="diff-content">{line.content || " "}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              {diff.truncated && (
                <p className="empty">
                  差分が大きいため、最初の{diff.lines.length}行のみ表示しています。
                </p>
              )}
            </>
          )}
        </>
      )}
    </div>
  );
}
