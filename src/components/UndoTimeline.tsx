// #48 Undo タイムライン
import { AnimatePresence, motion } from "framer-motion";
import type { UndoEntry, OperationKind } from "../api";
import { slideInFromBottom } from "../theme/motion";

interface Props {
  // 新しい順（先頭が最新の操作）で渡す。
  entries: UndoEntry[];
}

// OperationKind ごとの日本語ラベルとアイコン。
const OP_LABEL: Record<OperationKind, string> = {
  stage: "ステージ",
  unstage: "ステージ解除",
  commit: "コミット",
  amend_commit: "コミット修正",
  discard: "変更の破棄",
  stash_save: "変更の退避",
  stash_apply: "退避の適用",
  stash_pop: "退避の取り出し",
  create_branch: "ブランチ作成",
  switch_branch: "ブランチ切り替え",
  delete_branch: "ブランチ削除",
  reset_hard: "ハードリセット",
  fetch: "リモート取得",
  pull: "変更の取り込み",
  push: "リモートへ送信",
  force_push: "強制送信",
  cherry_pick: "コミットのコピー",
  create_tag: "タグ作成",
  delete_tag: "タグ削除",
  rebase: "履歴の整理",
  merge: "ブランチの統合",
};

const OP_ICON: Record<OperationKind, string> = {
  stage: "📥",
  unstage: "📤",
  commit: "💾",
  amend_commit: "✏️",
  discard: "🗑️",
  stash_save: "📦",
  stash_apply: "📬",
  stash_pop: "📭",
  create_branch: "🌿",
  switch_branch: "🔀",
  delete_branch: "✂️",
  reset_hard: "⏪",
  fetch: "🔄",
  pull: "⬇️",
  push: "⬆️",
  force_push: "⚡",
  cherry_pick: "🍒",
  create_tag: "🏷️",
  delete_tag: "🚫",
  rebase: "🔧",
  merge: "🔗",
};

// #48 Undo タイムライン: 取り消し履歴をタイムライン形式で表示するパネル。
export function UndoTimeline({ entries }: Props) {
  return (
    <div className="panel">
      <h2>取り消し履歴</h2>
      {entries.length === 0 ? (
        <p className="empty-hint">取り消せる操作はありません</p>
      ) : (
        <ul className="undo-timeline-list">
          <AnimatePresence initial={false}>
            {entries.map((entry, index) => (
              <motion.li
                key={`${entry.op}-${index}-${entry.description}`}
                className="undo-timeline-item"
                variants={slideInFromBottom}
                initial="hidden"
                animate="visible"
                exit="exit"
                layout
              >
                <span
                  className="undo-timeline-icon"
                  aria-hidden="true"
                >
                  {OP_ICON[entry.op]}
                </span>
                <div className="undo-timeline-body">
                  <span className="undo-timeline-op">
                    {OP_LABEL[entry.op]}
                  </span>
                  <span className="undo-timeline-desc">
                    {entry.description}
                  </span>
                </div>
                {/* 最新エントリ（先頭）に「最新」バッジを表示する。 */}
                {index === 0 && (
                  <span className="undo-timeline-badge">最新</span>
                )}
              </motion.li>
            ))}
          </AnimatePresence>
        </ul>
      )}
    </div>
  );
}
