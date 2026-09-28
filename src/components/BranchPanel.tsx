import { useEffect, useRef, useState } from "react";
import type {
  BranchGraph,
  BranchInfo,
  BranchRelation,
  MergedBranchInfo,
} from "../api";
import { EmptyState } from "./EmptyState";
import { AheadBehindBadge } from "./AheadBehindBadge";
import { Icon } from "./Icon";
// #274 危険度カラー: push・delete_branch はブランチごとに結果が変わりうる
// （保護ブランチかどうか）ため、raw な riskLevels マップを受け取ってこの中で引く。
import { riskTriggerClassFor, type RiskLevels } from "../lib/risk";

interface Props {
  branches: BranchInfo[];
  graph: BranchGraph | null;
  // #269 ブランチクリーンアップ: マージ済み（＝安全に削除できる）ローカルブランチの一覧。
  // 保護ブランチ・現在ブランチはすでに除かれている。
  mergedBranches: MergedBranchInfo[];
  onCreate: (name: string) => void;
  onSwitch: (name: string) => void;
  onDelete: (name: string) => void;
  onMerge: (name: string) => void;
  onPush: (name: string) => void;
  onForcePush: (name: string) => void;
  // #269 選択したマージ済みブランチをまとめて削除する。
  onBulkDeleteMerged: (names: string[]) => void;
  // ネットワーク操作中は true。送信・強制送信ボタンを無効化して二重実行を防ぐ。
  networkBusy?: boolean;
  // 保護ブランチ名の一覧（#169）。設定は git config に保存され、リポジトリごとに独立する。
  protectedBranches: string[];
  onAddProtected: (name: string) => void;
  onRemoveProtected: (name: string) => void;
  // #274 危険度カラー。未取得の間は空オブジェクト（Safe相当の通常スタイル）。
  riskLevels?: RiskLevels;
}

export function BranchPanel({
  branches,
  graph,
  mergedBranches,
  onCreate,
  onSwitch,
  onDelete,
  onMerge,
  onPush,
  onForcePush,
  onBulkDeleteMerged,
  networkBusy = false,
  protectedBranches,
  onAddProtected,
  onRemoveProtected,
  riskLevels = {},
}: Props) {
  const [newName, setNewName] = useState("");
  const newNameInput = useRef<HTMLInputElement>(null);
  const [newProtectedName, setNewProtectedName] = useState("");
  // 保護を外すのは安全性を弱める操作なので、ワンクリックでは外さず確認を挟む。
  // 確認待ちのブランチ名（null = 確認待ちなし）。
  const [pendingUnprotect, setPendingUnprotect] = useState<string | null>(null);

  function submitAddProtected() {
    const name = newProtectedName.trim();
    if (name) {
      onAddProtected(name);
      setNewProtectedName("");
    }
  }
  const local = branches.filter((b) => !b.is_remote);
  const remote = branches.filter((b) => b.is_remote);

  // #269 マージ済みブランチの一括整理。
  const [cleanupOpen, setCleanupOpen] = useState(false);
  const [selectedMerged, setSelectedMerged] = useState<Set<string>>(
    new Set(),
  );

  // 一覧が更新されたら（削除完了・ブランチ操作後の再取得など）、もう候補に
  // 無い名前を選択から取り除く。
  useEffect(() => {
    setSelectedMerged((prev) => {
      const names = new Set(mergedBranches.map((m) => m.name));
      const next = new Set([...prev].filter((n) => names.has(n)));
      return next.size === prev.size ? prev : next;
    });
  }, [mergedBranches]);

  function toggleMerged(name: string) {
    setSelectedMerged((prev) => {
      const next = new Set(prev);
      if (next.has(name)) {
        next.delete(name);
      } else {
        next.add(name);
      }
      return next;
    });
  }

  function toggleAllMerged() {
    setSelectedMerged((prev) =>
      prev.size === mergedBranches.length
        ? new Set()
        : new Set(mergedBranches.map((m) => m.name)),
    );
  }

  function submitBulkDelete() {
    if (selectedMerged.size === 0) return;
    onBulkDeleteMerged([...selectedMerged]);
  }

  // ブランチ名 → 現在ブランチとの関係。バッジ表示の参照に使う。
  const relByName = new Map<string, BranchRelation>(
    (graph?.relations ?? []).map((r) => [r.name, r]),
  );
  const likelyBase = graph?.likely_base ?? null;

  function submitCreate() {
    const name = newName.trim();
    if (name) {
      onCreate(name);
      setNewName("");
    }
  }

  return (
    <div className="panel">
      <div className="panel-head">
        <h2>ブランチ</h2>
      </div>

      <div className="branch-create">
        <input
          ref={newNameInput}
          value={newName}
          placeholder="新しいブランチ名"
          onChange={(e) => setNewName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && submitCreate()}
        />
        <button className="btn btn-small" onClick={submitCreate}>
          作成
        </button>
      </div>

      {/* #269 ブランチクリーンアップ: マージ済みブランチの一括検出・削除導線。
          対象は保護ブランチ・現在ブランチを除いた「取り込み済み」ローカルブランチのみ。 */}
      <div className="branch-cleanup">
        <button
          type="button"
          className="btn btn-small"
          onClick={() => setCleanupOpen((v) => !v)}
          disabled={mergedBranches.length === 0}
          title={
            mergedBranches.length === 0
              ? "取り込み済みのローカルブランチはありません。"
              : "取り込み済みのローカルブランチをまとめて削除できます。"
          }
        >
          <Icon name="branchCleanup" /> マージ済みブランチを整理
          {mergedBranches.length > 0 && `（${mergedBranches.length}）`}
        </button>

        {cleanupOpen && mergedBranches.length > 0 && (
          <div className="branch-cleanup-panel">
            <label className="branch-cleanup-select-all">
              <input
                type="checkbox"
                checked={selectedMerged.size === mergedBranches.length}
                onChange={toggleAllMerged}
              />
              すべて選択
            </label>
            <ul className="branch-cleanup-list">
              {mergedBranches.map((m) => (
                <li key={m.name}>
                  <label>
                    <input
                      type="checkbox"
                      checked={selectedMerged.has(m.name)}
                      onChange={() => toggleMerged(m.name)}
                    />
                    <span className="branch-cleanup-name">{m.name}</span>
                    <span className="branch-cleanup-meta">
                      （先端 {m.short_id}）→ {m.merged_into} に取り込み済み
                    </span>
                  </label>
                </li>
              ))}
            </ul>
            <button
              type="button"
              className="link danger"
              disabled={selectedMerged.size === 0}
              onClick={submitBulkDelete}
            >
              選択したブランチを削除（{selectedMerged.size}）
            </button>
          </div>
        )}
      </div>

      <ul className="branches">
        {local.map((b) => {
          const rel = relByName.get(b.name);
          return (
            <li key={b.name} className={b.is_head ? "current" : ""}>
              <div className="branch-row">
                <span className="branch-name">
                  {b.is_head && (
                    <span className="head-mark">
                      <Icon name="current" label="現在のブランチ" />
                    </span>
                  )}
                  {b.name}
                  {b.is_protected && (
                    <span className="protected" title="保護ブランチ">
                      <Icon name="protected" />
                      保護
                    </span>
                  )}
                  {rel && !b.is_head && rel.merged_into_current && (
                    <span
                      className="badge merged"
                      title="現在のブランチに取り込み済み。削除しても変更は失われません。"
                    >
                      取り込み済み
                    </span>
                  )}
                  {rel && !b.is_head && !rel.merged_into_current && (
                    <span
                      className="badge unmerged"
                      title="現在のブランチにまだ取り込まれていない独自のコミットがあります。削除前に注意してください。"
                    >
                      未取り込み
                    </span>
                  )}
                </span>
                <span className="branch-actions">
                  <button
                    // #274 危険度カラー: 保護ブランチ（main/master等）への送信だけ注意色。
                    className={`link ${riskTriggerClassFor(riskLevels, "push", b.name)}`}
                    onClick={() => onPush(b.name)}
                    disabled={networkBusy}
                    title={
                      networkBusy
                        ? "ネットワーク操作が進行中です"
                        : "このブランチのコミットをリモート（origin）へ送信します"
                    }
                  >
                    {networkBusy ? "送信中…" : "送信"}
                  </button>
                  {!b.is_head && (
                    <button
                      className={`link ${riskTriggerClassFor(riskLevels, "switch_branch")}`}
                      onClick={() => onSwitch(b.name)}
                    >
                      切り替え
                    </button>
                  )}
                  {!b.is_head && (
                    <button
                      className={`link ${riskTriggerClassFor(riskLevels, "merge")}`}
                      onClick={() => onMerge(b.name)}
                      title="このブランチの変更を現在のブランチに取り込みます（マージ）"
                    >
                      マージ
                    </button>
                  )}
                  {!b.is_head && (
                    <button
                      // #274 危険度カラー: 保護ブランチの削除は destructive、それ以外は caution。
                      className={`link ${riskTriggerClassFor(riskLevels, "delete_branch", b.name)}`}
                      onClick={() => onDelete(b.name)}
                    >
                      削除
                    </button>
                  )}
                  <button
                    className={`link ${riskTriggerClassFor(riskLevels, "force_push")}`}
                    onClick={() => onForcePush(b.name)}
                    disabled={networkBusy}
                    title={
                      networkBusy
                        ? "ネットワーク操作が進行中です"
                        : "リモートの履歴を上書きします（強制push）。とても危険です。"
                    }
                  >
                    {networkBusy ? "送信中…" : "強制送信"}
                  </button>
                </span>
              </div>

              {b.is_head && likelyBase && (
                <div className="branch-relation" title="Git は派生元を記録しないため、分岐点（merge-base）からの推定です。">
                  派生元（推定）:{" "}
                  <strong>{likelyBase.name}</strong>
                  {likelyBase.ambiguous && (
                    <span className="ambiguous">（候補が複数あり不確実）</span>
                  )}
                  {" "}
                  <AheadBehindBadge
                    ahead={likelyBase.ahead}
                    behind={likelyBase.behind}
                    upstream={likelyBase.name}
                  />
                </div>
              )}

              {rel && !b.is_head && !rel.merged_into_current && (
                <div className="branch-relation">
                  現在のブランチに対して{" "}
                  <AheadBehindBadge
                    ahead={rel.ahead}
                    behind={rel.behind}
                    upstream={b.upstream}
                  />
                </div>
              )}
            </li>
          );
        })}
      </ul>

      {local.length <= 1 && (
        <EmptyState
          icon={<Icon name="branch" />}
          title="ブランチはまだ 1 つだけです"
          description="ブランチを作ると、いまの状態を壊さずに安全に新機能を試せます。"
          action={{
            label: "ブランチを作る",
            onClick: () => newNameInput.current?.focus(),
          }}
        />
      )}

      {remote.length > 0 && (
        <div className="group">
          <h3>リモート</h3>
          <ul className="branches">
            {remote.map((b) => (
              <li key={b.name}>
                <span className="branch-name remote">{b.name}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="protected-branches-settings">
        <h3>
          <Icon name="protected" /> 保護ブランチの設定
        </h3>
        <p className="settings-field-help">
          保護ブランチへの削除・強制送信（force push）は「破壊的」操作として強く警告されます。
          一覧を空にすると既定値（main / master）に戻ります。
        </p>

        {protectedBranches.length > 0 ? (
          <ul className="protected-branches-list">
            {protectedBranches.map((name) => (
              <li key={name}>
                <Icon name="protected" />
                <span>{name}</span>
                {pendingUnprotect === name ? (
                  <>
                    <span className="protected-unprotect-confirm">
                      保護を外すと、削除や強制送信の警告が弱まります。
                    </span>
                    <button
                      className="link"
                      onClick={() => {
                        setPendingUnprotect(null);
                        onRemoveProtected(name);
                      }}
                    >
                      外す
                    </button>
                    <button
                      className="link"
                      onClick={() => setPendingUnprotect(null)}
                    >
                      やめる
                    </button>
                  </>
                ) : (
                  <button
                    className="link"
                    onClick={() => setPendingUnprotect(name)}
                    title={`「${name}」を保護対象から外す`}
                  >
                    <Icon name="close" label={`「${name}」を保護対象から外す`} />
                  </button>
                )}
              </li>
            ))}
          </ul>
        ) : (
          <p className="protected-branches-empty">読み込み中…</p>
        )}

        <div className="branch-create">
          <input
            value={newProtectedName}
            placeholder="保護するブランチ名（例: release）"
            onChange={(e) => setNewProtectedName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && submitAddProtected()}
          />
          <button className="btn btn-small" onClick={submitAddProtected}>
            追加
          </button>
        </div>
      </div>
    </div>
  );
}
