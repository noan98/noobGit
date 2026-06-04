import type { RepoStatus } from "../api";
import type { DiffSelection, DiffSource } from "./DiffPanel";
import { StatusBadge } from "./StatusBadge";
import { EmptyState } from "./EmptyState";

interface Props {
  status: RepoStatus;
  selected: DiffSelection | null;
  onStageAll: () => void;
  onStagePath: (path: string) => void;
  onUnstage: (path: string) => void;
  onDiscard: (path: string) => void;
  onSelect: (path: string, source: DiffSource) => void;
}

export function StatusPanel({
  status,
  selected,
  onStageAll,
  onStagePath,
  onUnstage,
  onDiscard,
  onSelect,
}: Props) {
  const hasUnstaged = status.unstaged.length > 0 || status.untracked.length > 0;

  const isSelected = (path: string, source: DiffSource) =>
    !!selected && selected.path === path && selected.source === source;

  const pathClass = (path: string, source: DiffSource) =>
    `path path-btn${isSelected(path, source) ? " selected" : ""}`;

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

      {status.is_clean && (
        <EmptyState
          icon="✨"
          title="変更はありません"
          description="ファイルを編集すると、その変更がここに表示されます。きれいな状態です。"
        />
      )}

      {status.staged.length > 0 && (
        <div className="group">
          <h3>コミット予定（ステージ済み）</h3>
          <ul>
            {status.staged.map((f) => (
              <li key={`s-${f.path}`}>
                <StatusBadge kind={f.kind} />
                <button
                  type="button"
                  className={pathClass(f.path, "staged")}
                  onClick={() => onSelect(f.path, "staged")}
                  title="クリックで差分を表示"
                >
                  {f.path}
                </button>
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
                <StatusBadge kind={f.kind} />
                <button
                  type="button"
                  className={pathClass(f.path, "unstaged")}
                  onClick={() => onSelect(f.path, "unstaged")}
                  title="クリックで差分を表示"
                >
                  {f.path}
                </button>
                <button className="link" onClick={() => onStagePath(f.path)}>
                  ステージ
                </button>
                <button
                  className="link danger"
                  onClick={() => onDiscard(f.path)}
                  title="この変更を捨てて、最後にコミットした状態に戻します（元に戻せません）"
                >
                  破棄
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
                <StatusBadge kind="untracked" />
                <button
                  type="button"
                  className={pathClass(p, "unstaged")}
                  onClick={() => onSelect(p, "unstaged")}
                  title="クリックで差分を表示"
                >
                  {p}
                </button>
                <button className="link" onClick={() => onStagePath(p)}>
                  ステージ
                </button>
                <button
                  className="link danger"
                  onClick={() => onDiscard(p)}
                  title="この新しいファイルを削除します（元に戻せません）"
                >
                  破棄
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
                <StatusBadge kind="conflicted" />
                <button
                  type="button"
                  className={pathClass(p, "conflicted")}
                  onClick={() => onSelect(p, "conflicted")}
                  title="クリックで差分を表示"
                >
                  {p}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
