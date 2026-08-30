// #167 進捗フィードバック
//
// fetch / pull / push 実行中に「今どの段階か・どれくらい進んだか」を表示する
// プログレスバー。オブジェクト数がまだ分からない段階（接続待ちなど）では
// 不確定（indeterminate）表示にし、完了後は成功/失敗を2秒表示してから
// AnimatePresence で自動的にフェードアウトする。
import { motion, AnimatePresence } from "framer-motion";
import type { NetworkProgress, NetworkProgressStage } from "../api";
import type { NetworkProgressState } from "../hooks/useNetworkProgress";
import { transitions, spring } from "../theme/motion";

interface Props {
  state: NetworkProgressState;
}

const STAGE_LABEL: Record<NetworkProgressStage, string> = {
  connecting: "サーバーに接続しています…",
  receiving_objects: "オブジェクトを受信中…",
  resolving_deltas: "差分を展開中…",
  sending_objects: "オブジェクトを送信中…",
};

function describeProgress(progress: NetworkProgress | null): string {
  if (!progress) return STAGE_LABEL.connecting;
  const base = STAGE_LABEL[progress.stage];
  if (progress.total_objects > 0) {
    const percent = Math.round(
      (progress.received_objects / progress.total_objects) * 100,
    );
    return `${base} ${progress.received_objects}/${progress.total_objects} (${percent}%)`;
  }
  return base;
}

function percentOf(progress: NetworkProgress | null): number | null {
  if (!progress || progress.total_objects <= 0) return null;
  return Math.min(
    100,
    Math.round((progress.received_objects / progress.total_objects) * 100),
  );
}

export function NetworkProgressBar({ state }: Props) {
  const { visible, label, phase, progress, errorMessage } = state;
  const percent = percentOf(progress);
  const isDeterminate = phase === "running" && percent !== null;

  let statusText: string;
  if (phase === "success") {
    statusText = "完了 ✓";
  } else if (phase === "error") {
    statusText = errorMessage ? `失敗しました: ${errorMessage}` : "失敗しました";
  } else {
    statusText = describeProgress(progress);
  }

  return (
    <AnimatePresence>
      {visible && (
        <motion.div
          key="network-progress-bar"
          className={`network-progress network-progress-${phase}`}
          role="status"
          aria-live="polite"
          initial={{ opacity: 0, y: -6, height: 0 }}
          animate={{ opacity: 1, y: 0, height: "auto" }}
          exit={{ opacity: 0, y: -6, height: 0 }}
          transition={transitions.normal}
        >
          <div className="network-progress-row">
            <span className="network-progress-label">{label}</span>
            <span className="network-progress-text">{statusText}</span>
          </div>
          <div className="network-progress-track">
            {phase === "running" && !isDeterminate ? (
              <div className="network-progress-fill network-progress-fill-indeterminate" />
            ) : (
              <motion.div
                className="network-progress-fill"
                initial={{ width: 0 }}
                animate={{
                  width:
                    phase === "running" && percent !== null
                      ? `${percent}%`
                      : "100%",
                }}
                transition={
                  phase === "running" ? spring.gentle : transitions.fast
                }
              />
            )}
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
