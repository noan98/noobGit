// #208 操作アクティビティログ: 「自分が何をしたか」を新しい順に表示するパネル。
import { useState } from "react";
import type { ActivityEntry } from "../api";
import { formatActivityText, formatTimestamp, outcomeLabel } from "../lib/activityLog";
import { Icon, type IconName } from "./Icon";
import { OP_ICON } from "./UndoTimeline";

interface Props {
  // 古い順（時系列）で渡す。表示は新しい順、コピーは時系列のまま。
  entries: ActivityEntry[];
  repoName?: string;
  onRefresh?: () => void;
  onClear?: () => void;
}

const OUTCOME_ICON: Record<ActivityEntry["outcome"]["status"], IconName> = {
  success: "check",
  failed: "warning",
  undone: "undo",
};

export function ActivityLog({ entries, repoName, onRefresh, onClear }: Props) {
  const [copied, setCopied] = useState(false);
  const newestFirst = [...entries].reverse();

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(formatActivityText(entries, repoName));
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // クリップボードに書けなくても、コピーボタンが効かないだけ（画面は壊さない）。
    }
  }

  return (
    <div className="panel">
      <h2>操作ログ</h2>
      <p className="empty-hint">
        noobGit で行った操作を時系列で記録しています（取り消せない操作や失敗も含みます）。
        先輩に状況を伝えるときは「テキストとしてコピー」を使ってください。
      </p>
      <div className="activity-log-actions">
        <button
          type="button"
          onClick={() => void handleCopy()}
          disabled={entries.length === 0}
        >
          <Icon name="copy" /> {copied ? "コピーしました" : "テキストとしてコピー"}
        </button>
        {onRefresh && (
          <button type="button" onClick={onRefresh}>
            <Icon name="refresh" /> 更新
          </button>
        )}
        {onClear && (
          <button type="button" onClick={onClear} disabled={entries.length === 0}>
            <Icon name="discard" /> ログを消去
          </button>
        )}
      </div>
      {entries.length === 0 ? (
        <p className="empty-hint">記録された操作はまだありません</p>
      ) : (
        <ul className="undo-timeline-list">
          {newestFirst.map((entry, index) => (
            <li
              key={`${entry.timestamp}-${index}`}
              className={`undo-timeline-item activity-item activity-${entry.outcome.status}`}
            >
              <span className="undo-timeline-icon">
                <Icon name={OP_ICON[entry.op]} />
              </span>
              <div className="undo-timeline-body">
                <span className="undo-timeline-op">{entry.summary}</span>
                <span className="undo-timeline-desc activity-meta">
                  {formatTimestamp(entry.timestamp)}
                </span>
                {entry.outcome.status === "failed" && (
                  <span className="undo-timeline-note undo-timeline-note-risky">
                    {entry.outcome.message}
                  </span>
                )}
              </div>
              <span className={`activity-badge activity-badge-${entry.outcome.status}`}>
                <Icon name={OUTCOME_ICON[entry.outcome.status]} />{" "}
                {outcomeLabel(entry.outcome)}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
