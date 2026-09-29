// #48 Undo タイムライン
import { AnimatePresence, motion } from "framer-motion";
import type { UndoEntry, OperationKind, UndoApplicability } from "../api";
import { slideInFromBottom } from "../theme/motion";
import { Icon, type IconName } from "./Icon";

interface Props {
  // 新しい順（先頭が最新の操作）で渡す。
  entries: UndoEntry[];
  // #201 entries と同じ順序・同じ長さの適用可否。無い（長さが合わない）ときは全件「適用可」扱い。
  applicability?: UndoApplicability[];
  // #201 適用不能な履歴を整理する。
  onPrune?: () => void;
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
  // stash_drop は undo を記録しない操作だが、OperationKind は網羅的に扱う必要があるため
  // ラベル・アイコンだけは用意しておく（実際にタイムラインへ現れることは無い）。
  stash_drop: "退避の削除",
  create_branch: "ブランチ作成",
  switch_branch: "ブランチ切り替え",
  switch_branch_with_stash: "退避して切り替え",
  delete_branch: "ブランチ削除",
  reset_hard: "ハードリセット",
  fetch: "リモート取得",
  pull: "変更の取り込み",
  push: "プッシュ（リモートへ送信）",
  force_push: "強制プッシュ",
  cherry_pick: "コミットのコピー",
  revert: "コミットの打ち消し",
  create_tag: "タグ作成",
  delete_tag: "タグ削除",
  rebase: "履歴の整理",
  merge: "ブランチの統合",
  remove_remote: "リモートを削除",
  restore_file: "ファイルを復元",
  bisect_start: "Bisect の開始",
  bisect_reset: "Bisect の終了",
  // クローンはネットワーク操作で undo journal には記録されないが、Record<OperationKind, ...>
  // を満たすためのラベルは用意しておく。
  clone: "クローン",
  rescue_detached_head: "ブランチで安全にする",
};

// 操作ごとのアイコン。見た目の定義は Icon.tsx に集約している（絵文字は使わない）。
const OP_ICON: Record<OperationKind, IconName> = {
  stage: "stage",
  unstage: "unstage",
  commit: "commit",
  amend_commit: "amend",
  discard: "discard",
  stash_save: "stash",
  stash_apply: "stashApply",
  stash_pop: "stashPop",
  stash_drop: "discard",
  create_branch: "branch",
  switch_branch: "branchSwitch",
  switch_branch_with_stash: "branchSwitch",
  delete_branch: "branchDelete",
  reset_hard: "reset",
  fetch: "fetch",
  pull: "pull",
  push: "push",
  force_push: "forcePush",
  cherry_pick: "cherryPick",
  revert: "revert",
  create_tag: "tag",
  delete_tag: "tagDelete",
  rebase: "rebase",
  merge: "merge",
  remove_remote: "remoteRemove",
  restore_file: "restore",
  bisect_start: "bisect",
  bisect_reset: "bisect",
  clone: "clone",
  rescue_detached_head: "branch",
};

// #48 Undo タイムライン: 取り消し履歴をタイムライン形式で表示するパネル。
export function UndoTimeline({ entries, applicability, onPrune }: Props) {
  const usable =
    applicability !== undefined && applicability.length === entries.length;
  const statusOf = (index: number): UndoApplicability | null =>
    usable ? applicability[index] : null;
  const hasStale = entries.some(
    (_, i) => statusOf(i)?.status === "unresolvable",
  );
  return (
    <div className="panel">
      <h2>取り消し履歴</h2>
      {hasStale && onPrune && (
        <button type="button" className="undo-prune-btn" onClick={onPrune}>
          使えなくなった履歴を整理する
        </button>
      )}
      {entries.length === 0 ? (
        <p className="empty-hint">取り消せる操作はありません</p>
      ) : (
        <ul className="undo-timeline-list">
          <AnimatePresence initial={false}>
            {entries.map((entry, index) => {
              const st = statusOf(index);
              // 危険（risky）は次に適用される最新エントリでのみ意味を持つ。
              // それ以前のものは「最新を先に取り消すまで」の参考値なので表示しない。
              const stale =
                st?.status === "unresolvable" || st?.status === "already_undone";
              const risky = st?.status === "risky" && index === 0;
              const note =
                st?.status === "unresolvable"
                  ? st.reason
                  : st?.status === "already_undone"
                    ? "すでに取り消した状態になっているため、この履歴は何も変えません。"
                    : risky && st?.status === "risky"
                      ? st.reason
                      : null;
              return (
              <motion.li
                key={`${entry.op}-${index}-${entry.description}`}
                className={`undo-timeline-item${stale ? " undo-timeline-item-stale" : ""}${risky ? " undo-timeline-item-risky" : ""}`}
                variants={slideInFromBottom}
                initial="hidden"
                animate="visible"
                exit="exit"
                layout
              >
                <span className="undo-timeline-icon">
                  <Icon name={OP_ICON[entry.op]} />
                </span>
                <div className="undo-timeline-body">
                  <span className="undo-timeline-op">
                    {OP_LABEL[entry.op]}
                  </span>
                  <span className="undo-timeline-desc">
                    {entry.description}
                  </span>
                  {note && (
                    <span
                      className={`undo-timeline-note${risky ? " undo-timeline-note-risky" : ""}`}
                    >
                      {note}
                    </span>
                  )}
                </div>
                {/* 最新エントリ（先頭）に「最新」バッジを表示する。 */}
                {index === 0 && (
                  <span className="undo-timeline-badge">最新</span>
                )}
              </motion.li>
              );
            })}
          </AnimatePresence>
        </ul>
      )}
    </div>
  );
}
