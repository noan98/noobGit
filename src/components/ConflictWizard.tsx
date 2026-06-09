import type { ConflictFile } from "../api";

interface Props {
  // コンフリクト中のファイル一覧（status.conflicted から組み立てる）。
  conflicts: ConflictFile[];
  // 選択中ファイル（差分プレビューに反映するためのコールバックに使う）。
  selectedPath: string | null;
  // ファイル名クリックで差分プレビューに表示するためのコールバック。
  onSelect: (path: string) => void;
  // 「解消済みとしてマーク」ボタン。markResolved を呼ぶ。
  onMarkResolved: (path: string) => void;
}

/*
 * ConflictWizard — コンフリクト解消サポートウィザード (#54)。
 *
 * マージや stash の取り出しで競合したファイルの一覧と、競合の目印の意味・
 * 解消の手順を平易な日本語で示す。各ファイルを正しい内容に直して保存したら、
 * 「解消済みとしてマーク」ボタンでステージし、続けてコミットできるようにする。
 * 差分の中身は既存の差分プレビュー（DiffPanel）で確認する。
 */
export function ConflictWizard({
  conflicts,
  selectedPath,
  onSelect,
  onMarkResolved,
}: Props) {
  if (conflicts.length === 0) return null;

  return (
    <div className="panel conflict-wizard">
      <div className="panel-head">
        <h2>⚠️ コンフリクトの解消</h2>
        <span className="conflict-count">{conflicts.length} 件</span>
      </div>

      <div className="conflict-guide">
        <p>
          いくつかのファイルで「両方の変更がぶつかった（コンフリクト）」状態になりました。
          あわてなくて大丈夫です。次の手順で 1 つずつ直していきましょう。
        </p>
        <ol>
          <li>下のファイル名をクリックして、競合している中身を確認します。</li>
          <li>
            ファイルを開くと、次の目印で競合箇所が囲まれています。
            <ul>
              <li>
                <code>&lt;&lt;&lt;&lt;&lt;&lt;&lt;</code> 〜{" "}
                <code>=======</code> … 今のブランチ側の内容
              </li>
              <li>
                <code>=======</code> 〜 <code>&gt;&gt;&gt;&gt;&gt;&gt;&gt;</code>{" "}
                … 取り込もうとした相手側の内容
              </li>
            </ul>
          </li>
          <li>
            どちらを残すか（または両方を合わせるか）を決めて、目印の行
            （<code>&lt;&lt;&lt;&lt;&lt;&lt;&lt;</code> /{" "}
            <code>=======</code> / <code>&gt;&gt;&gt;&gt;&gt;&gt;&gt;</code>）も
            消して、正しい内容にして保存します。
          </li>
          <li>
            直し終えたら、そのファイルの「解消済みとしてマーク」を押します。
            すべて解消したら、いつものようにコミットして完了です。
          </li>
        </ol>
      </div>

      <ul className="conflict-list">
        {conflicts.map((c) => {
          const isSelected = selectedPath === c.path;
          return (
            <li
              key={c.path}
              className={
                isSelected ? "conflict-item selected" : "conflict-item"
              }
            >
              <button
                type="button"
                className="conflict-path"
                onClick={() => onSelect(c.path)}
                title="クリックで競合箇所を差分プレビューに表示します"
              >
                {c.path}
              </button>
              {!c.has_ancestor && (
                <span
                  className="conflict-note"
                  title="共通の元が無い競合です（両側で新しく追加された等）。どちらの内容にするか選んでください。"
                >
                  共通の元なし
                </span>
              )}
              <button
                type="button"
                className="btn btn-small"
                onClick={() => onMarkResolved(c.path)}
                title="このファイルを直して保存したあとに押してください。解消済みとしてステージします。"
              >
                解消済みとしてマーク
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
