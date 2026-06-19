import type { DiffLine, DiffLineKind, FileDiff } from "../api";
import { langFromPath } from "../lib/highlight";
import { HighlightedCode } from "./HighlightedCode";

export type DiffSource = "staged" | "unstaged" | "conflicted";

export interface DiffSelection {
  path: string;
  source: DiffSource;
}

interface Props {
  selection: DiffSelection | null;
  diff: FileDiff | null;
  loading: boolean;
  // 未ステージ差分の hunk ヘッダー行で「この塊だけステージ」したときに呼ばれる。
  // 引数はその hunk のヘッダー文字列（例 `@@ -1,3 +1,4 @@`）。
  onStageHunk?: (hunkHeader: string) => void;
}

const sourceLabel: Record<DiffSource, string> = {
  staged: "ステージ済み",
  unstaged: "未ステージ",
  conflicted: "コンフリクト",
};

function sign(kind: DiffLineKind): string {
  if (kind === "addition") return "+";
  if (kind === "deletion") return "-";
  return "";
}

// コンフリクトの目印（<<<<<<< / ======= / >>>>>>> / |||||||）で始まる行か。
function isConflictMarker(content: string): boolean {
  return /^(<<<<<<<|=======|>>>>>>>|\|\|\|\|\|\|\|)/.test(content);
}

function lineClass(line: DiffLine, conflicted: boolean): string {
  if (conflicted && isConflictMarker(line.content)) {
    return "diff-line diff-conflict-marker";
  }
  return `diff-line diff-${line.kind}`;
}

export function DiffPanel({ selection, diff, loading, onStageHunk }: Props) {
  // 未ステージ差分のときだけ、hunk 単位の部分ステージを出す。
  const canStageHunk = selection?.source === "unstaged" && !!onStageHunk;
  // ファイルの拡張子から shiki 言語名を決定する。
  const lang = selection ? langFromPath(selection.path) : "text";

  return (
    <div className="panel diff-panel">
      <div className="panel-head">
        <h2>差分プレビュー</h2>
        {selection && (
          <span className="diff-source">{sourceLabel[selection.source]}</span>
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

          {!loading && diff?.is_conflicted && (
            <p className="diff-conflict-note">
              このファイルはコンフリクト中です。
              <code>&lt;&lt;&lt;&lt;&lt;&lt;&lt;</code> 〜{" "}
              <code>=======</code> 〜 <code>&gt;&gt;&gt;&gt;&gt;&gt;&gt;</code>{" "}
              で囲まれた部分が競合箇所です。正しい内容に直して保存し、ステージしてください。
            </p>
          )}

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
                      <tr key={i} className={lineClass(line, diff.is_conflicted)}>
                        <td className="diff-lineno">{line.old_lineno ?? ""}</td>
                        <td className="diff-lineno">{line.new_lineno ?? ""}</td>
                        <td className="diff-sign">{sign(line.kind)}</td>
                        <td className="diff-content">
                          {line.kind === "hunk" && canStageHunk ? (
                            // hunk 行でステージボタンあり: プレーン表示 + ボタン。
                            <span className="diff-hunk-row">
                              <span className="diff-hunk-header">
                                {line.content || " "}
                              </span>
                              <button
                                type="button"
                                className="btn btn-small diff-hunk-stage"
                                title="この変更の塊（hunk）だけをステージします"
                                onClick={() => onStageHunk?.(line.content)}
                              >
                                この塊だけステージ
                              </button>
                            </span>
                          ) : (
                            // その他の行: シンタックスハイライトを適用する。
                            <HighlightedCode
                              code={line.content}
                              lang={lang}
                              isHunk={line.kind === "hunk"}
                            />
                          )}
                        </td>
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
