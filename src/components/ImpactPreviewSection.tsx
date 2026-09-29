// 確認ダイアログの「操作別プレビュー」（#196）。
// core が計算した ImpactPreview を kind ごとに描き分ける表示専用コンポーネント。
// Git ロジックは持たない。新しい kind（#205 の rebase の並べ替え・削除など）は
// api.ts の ImpactPreview に足したら、ここに分岐を 1 つ足すだけでよい。
import type {
  CommitInfo,
  DiffLine,
  FileChange,
  FileDiff,
  ImpactPreview,
} from "../api";
import { Icon } from "./Icon";
import { StatusBadge } from "./StatusBadge";

function FileList({ files }: { files: FileChange[] }) {
  return (
    <div className="affected-files-list">
      {files.map((f) => (
        <div key={f.path} className="affected-file">
          <StatusBadge kind={f.kind} />
          <span className="affected-file-path">{f.path}</span>
        </div>
      ))}
    </div>
  );
}

function CommitList({ commits }: { commits: CommitInfo[] }) {
  return (
    <div className="affected-files-list">
      {commits.map((c) => (
        <div key={c.id} className="affected-file">
          <Icon name="commit" />
          <span className="affected-file-path">{c.short_id}</span>
          <span>{c.summary}</span>
        </div>
      ))}
    </div>
  );
}

const SIGN: Record<DiffLine["kind"], string> = {
  addition: "+",
  deletion: "-",
  context: " ",
  hunk: "",
};

function DiffBlock({ label, diff }: { label: string; diff: FileDiff }) {
  return (
    <div className="impact-diff">
      <div className="impact-diff-label">{label}</div>
      {diff.is_binary ? (
        <p className="impact-note">バイナリファイルのため、差分は表示できません。</p>
      ) : (
        <div className="impact-diff-scroll">
          <table className="diff-table">
            <tbody>
              {diff.lines.map((l, i) => (
                <tr key={i} className={`diff-${l.kind}`}>
                  <td className="diff-sign">{SIGN[l.kind]}</td>
                  <td className="diff-content">{l.content}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {diff.truncated && (
        <p className="impact-note">長いため、途中までを表示しています。</p>
      )}
    </div>
  );
}

export function ImpactPreviewSection({ preview }: { preview: ImpactPreview }) {
  switch (preview.kind) {
    case "lost_changes":
      return (
        <section className="affected-files-section" data-testid="impact-preview">
          <h3>失われる変更</h3>
          {preview.files.length === 0 ? (
            <p className="affected-files-clean">変更なし — 安全にリセットできます</p>
          ) : (
            <FileList files={preview.files} />
          )}
        </section>
      );

    case "discarded_diffs":
      return (
        <section className="affected-files-section" data-testid="impact-preview">
          <h3>失われる差分</h3>
          {preview.diffs.length === 0 ? (
            <p className="affected-files-clean">破棄される変更は見つかりませんでした</p>
          ) : (
            preview.diffs.map((d) => (
              <div key={d.path} className="impact-file">
                <div className="affected-file-path">{d.path}</div>
                {d.staged && <DiffBlock label="ステージ済みの変更" diff={d.staged} />}
                {d.unstaged && <DiffBlock label="まだステージしていない変更" diff={d.unstaged} />}
              </div>
            ))
          )}
          {preview.omitted_files > 0 && (
            <p className="impact-note">ほか {preview.omitted_files} ファイルの差分は省略しています。</p>
          )}
        </section>
      );

    case "unique_commits":
      return (
        <section className="affected-files-section" data-testid="impact-preview">
          <h3>「{preview.branch}」にしかないコミット（{preview.commits.length}件{preview.truncated ? "以上" : ""}）</h3>
          {preview.commits.length === 0 ? (
            <p className="affected-files-clean">
              すべて他のブランチにも含まれているので、コミットは失われません
            </p>
          ) : (
            <>
              <CommitList commits={preview.commits} />
              <p className="impact-note">
                ブランチを消すと、これらのコミットは他から辿れなくなります。
              </p>
            </>
          )}
        </section>
      );

    case "overwritten_commits":
      return (
        <section className="affected-files-section" data-testid="impact-preview">
          <h3>リモートから消えるコミット（{preview.commits.length}件{preview.truncated ? "以上" : ""}）</h3>
          {preview.commits.length === 0 ? (
            <p className="affected-files-clean">
              「{preview.remote_ref}」の上書きで消えるコミットはありません
            </p>
          ) : (
            <>
              <CommitList commits={preview.commits} />
              <p className="impact-note">
                「{preview.remote_ref}」に載っているコミットです（最後に取得した時点の状態）。
                共同作業者の変更が含まれている可能性があります。
              </p>
            </>
          )}
        </section>
      );

    case "stash_overlap":
      return (
        <section className="affected-files-section" data-testid="impact-preview">
          <h3>衝突しうるファイル</h3>
          {preview.overlapping.length === 0 ? (
            <p className="affected-files-clean">
              退避の {preview.stash_file_count} ファイルと、今の変更が重なるものはありません
            </p>
          ) : (
            <>
              <FileList files={preview.overlapping} />
              <p className="impact-note">
                今の作業ツリーでも変更されているファイルです（退避は全 {preview.stash_file_count} ファイル）。
              </p>
            </>
          )}
        </section>
      );

    case "rebase_plan":
      return (
        <section className="affected-files-section" data-testid="impact-preview">
          <h3>履歴の変更前と変更後</h3>
          <div className="rebase-compare">
            <div>
              <h4>変更前（新しい順）</h4>
              <CommitList commits={preview.before} />
            </div>
            <div>
              <h4>変更後（新しい順）</h4>
              <CommitList commits={preview.after} />
            </div>
          </div>
          {preview.dropped.length > 0 && (
            <>
              <h4>履歴から消えるコミット（{preview.dropped.length}件）</h4>
              <CommitList commits={preview.dropped} />
              <p className="impact-note impact-note-warn">
                <Icon name="warning" /> これらのコミットの変更内容は、作業ツリーからも消えます。
                直後なら Undo で元に戻せます。
              </p>
            </>
          )}
          {preview.published && (
            <p className="impact-note impact-note-warn">
              <Icon name="warning" /> すでに公開（push）済みのコミットを含みます。整理すると、
              次の送信で強制 push が必要になり、共同作業者に影響します。
            </p>
          )}
        </section>
      );

    case "rewritten_commits":
      return (
        <section className="affected-files-section" data-testid="impact-preview">
          <h3>書き換わるコミット（{preview.commits.length}件）</h3>
          <CommitList commits={preview.commits} />
          {preview.published && (
            <p className="impact-note impact-note-warn">
              <Icon name="warning" /> すでに公開（push）済みのコミットです。書き換えると、
              次の送信で強制 push が必要になり、共同作業者に影響します。
            </p>
          )}
        </section>
      );
  }
}
