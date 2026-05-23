import { useState } from "react";
import type { BranchInfo } from "../api";

interface Props {
  branches: BranchInfo[];
  onCreate: (name: string) => void;
  onSwitch: (name: string) => void;
  onDelete: (name: string) => void;
}

export function BranchPanel({ branches, onCreate, onSwitch, onDelete }: Props) {
  const [newName, setNewName] = useState("");
  const local = branches.filter((b) => !b.is_remote);
  const remote = branches.filter((b) => b.is_remote);

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
        {local.map((b) => (
          <li key={b.name} className={b.is_head ? "current" : ""}>
            <span className="branch-name">
              {b.is_head && <span className="head-mark">●</span>}
              {b.name}
              {b.is_protected && (
                <span className="protected" title="保護ブランチ">
                  保護
                </span>
              )}
            </span>
            <span className="branch-actions">
              {!b.is_head && (
                <button className="link" onClick={() => onSwitch(b.name)}>
                  切り替え
                </button>
              )}
              {!b.is_head && (
                <button className="link danger" onClick={() => onDelete(b.name)}>
                  削除
                </button>
              )}
            </span>
          </li>
        ))}
      </ul>

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
