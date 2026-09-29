// #204 ローカルエラー診断ダイアログ
// ステージ・コミット・チェックアウトなどローカル操作の失敗を、
// ロック競合 / 権限 / リポジトリ破損 / ディスク満杯 / その他の種別ごとに
// 初心者向けの日本語（これは何か・なぜ起きたか・どうすればよいか）で案内する。
// 文言は core（explain_local_error）が唯一の出典で、ここでは表示するだけ。

import { useId } from "react";
import { motion, AnimatePresence } from "framer-motion";
import type { LocalErrorExplanation, LocalErrorKind } from "../api";
import { useModalA11y } from "../hooks/useModalA11y";
import { Icon, type IconName } from "./Icon";

interface Props {
  explanation: LocalErrorExplanation;
  raw: string;
  onClose: () => void;
}

const ICON_BY_KIND: Record<LocalErrorKind, IconName> = {
  lock_busy: "lockBusy",
  permission_denied: "permissionDenied",
  repo_corrupted: "repoCorrupted",
  disk_full: "diskFull",
  other: "warning",
};

export function LocalErrorDialog({ explanation, raw, onClose }: Props) {
  const titleId = useId();
  const dialogRef = useModalA11y<HTMLDivElement>({ onEscape: onClose });

  return (
    <AnimatePresence>
      <motion.div
        className="dialog-backdrop"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        onClick={onClose}
      >
        <motion.div
          ref={dialogRef}
          className="dialog network-error-dialog"
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
          initial={{ opacity: 0, scale: 0.95, y: 8 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          exit={{ opacity: 0, scale: 0.95, y: 8 }}
          transition={{ duration: 0.18 }}
          onClick={(e) => e.stopPropagation()}
        >
          <div className="network-error-header">
            <span className="network-error-icon">
              <Icon name={ICON_BY_KIND[explanation.kind]} />
            </span>
            <h2 className="network-error-title" id={titleId}>
              {explanation.title}
            </h2>
          </div>

          <p className="network-error-what">{explanation.what}</p>

          <h3 className="network-error-steps-heading">なぜ起きたか</h3>
          <p className="network-error-what">{explanation.why}</p>

          <h3 className="network-error-steps-heading">解決手順</h3>
          <ol className="network-error-steps">
            {explanation.steps.map((step, i) => (
              <li key={i}>{step}</li>
            ))}
          </ol>

          <details className="network-error-details">
            <summary>エラー詳細（詳しい人向け）</summary>
            <pre className="network-error-raw">{raw}</pre>
          </details>

          <div className="dialog-actions">
            <button className="btn" onClick={onClose}>
              閉じる
            </button>
          </div>
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
}
