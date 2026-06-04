import { useRef, useState } from "react";
import type { BranchGraph, BranchInfo, BranchRelation } from "../api";
import { EmptyState } from "./EmptyState";

interface Props {
  branches: BranchInfo[];
  graph: BranchGraph | null;
  onCreate: (name: string) => void;
  onSwitch: (name: string) => void;
  onDelete: (name: string) => void;
  onPush: (name: string) => void;
  onForcePush: (name: string) => void;
  // ネットワーク操作中は true。送信・強制送信ボタンを無効化して二重実行を防ぐ。
  networkBusy?: boolean;
}

export function BranchPanel({
  branches,
  graph,
  onCreate,
  onSwitch,
  onDelete,
  onPush,
  onForcePush,
  networkBusy = false,
}: Props) {
  const [newName, setNewName] = useState("");
  const newNameInput = useRef<HTMLInputElement>(null);
  const local = branches.filter((b) => !b.is_remote);
  const remote = branches.filter((b) => b.is_remote);

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

      <ul className="branches">
        {local.map((b) => {
          const rel = relByName.get(b.name);
          return (
            <li key={b.name} className={b.is_head ? "current" : ""}>
              <div className="branch-row">
                <span className="branch-name">
                  {b.is_head && <span className="head-mark">●</span>}
                  {b.name}
                  {b.is_protected && (
                    <span className="protected" title="保護ブランチ">
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
                    className="link"
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
                    <button className="link" onClick={() => onSwitch(b.name)}>
                      切り替え
                    </button>
                  )}
                  {!b.is_head && (
                    <button
                      className="link danger"
                      onClick={() => onDelete(b.name)}
                    >
                      削除
                    </button>
                  )}
                  <button
                    className="link danger"
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
                  <span className="ahead-behind">
                    {" "}
                    ↑{likelyBase.ahead} / ↓{likelyBase.behind}
                  </span>
                </div>
              )}

              {rel && !b.is_head && !rel.merged_into_current && (
                <div className="branch-relation">
                  <span className="ahead-behind" title="現在のブランチに対する先行/遅れのコミット数">
                    現在のブランチに対して ↑{rel.ahead} / ↓{rel.behind}
                  </span>
                </div>
              )}
            </li>
          );
        })}
      </ul>

      {local.length <= 1 && (
        <EmptyState
          icon="🌿"
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
    </div>
  );
}
