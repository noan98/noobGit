import { changeKindLabel, type RepoStatus } from "../api";

interface Props {
  status: RepoStatus;
  onStageAll: () => void;
  onStagePath: (path: string) => void;
  onUnstage: (path: string) => void;
}

export function StatusPanel({
  status,
  onStageAll,
  onStagePath,
  onUnstage,
}: Props) {
  const hasUnstaged = status.unstaged.length > 0 || status.untracked.length > 0;

  return (
    <div className="panel">
      <div className="panel-head">
        <h2>変更</h2>
        <button
          className="btn btn-small"
          onClick={onStageAll}
          disabled={!hasUnstaged}
          title="すべての変更をコミット対象に加えます"
        >
          すべてステージ
        </button>
      </div>

      {status.is_clean && <p className="empty">変更はありません。きれいな状態です。</p>}

      {status.staged.length > 0 && (
        <div className="group">
          <h3>コミット予定（ステージ済み）</h3>
          <ul>
            {status.staged.map((f) => (
              <li key={`s-${f.path}`}>
                <span className={`tag tag-${f.kind}`}>
                  {changeKindLabel[f.kind]}
                </span>
                <span className="path">{f.path}</span>
                <button
                  className="link"
                  onClick={() => onUnstage(f.path)}
                  title="コミット対象から外します（変更は残ります）"
                >
                  外す
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {status.unstaged.length > 0 && (
        <div className="group">
          <h3>変更あり（未ステージ）</h3>
          <ul>
            {status.unstaged.map((f) => (
              <li key={`u-${f.path}`}>
                <span className={`tag tag-${f.kind}`}>
                  {changeKindLabel[f.kind]}
                </span>
                <span className="path">{f.path}</span>
                <button className="link" onClick={() => onStagePath(f.path)}>
                  ステージ
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {status.untracked.length > 0 && (
        <div className="group">
          <h3>新しいファイル（未追跡）</h3>
          <ul>
            {status.untracked.map((p) => (
              <li key={`n-${p}`}>
                <span className="tag tag-untracked">未追跡</span>
                <span className="path">{p}</span>
                <button className="link" onClick={() => onStagePath(p)}>
                  ステージ
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {status.conflicted.length > 0 && (
        <div className="group">
          <h3>コンフリクト</h3>
          <ul>
            {status.conflicted.map((p) => (
              <li key={`c-${p}`}>
                <span className="tag tag-conflicted">コンフリクト</span>
                <span className="path">{p}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
