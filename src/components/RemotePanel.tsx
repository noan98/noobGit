import { useState } from "react";
import type { RemoteInfo } from "../api";

interface Props {
  remotes: RemoteInfo[];
  onAdd: (name: string, url: string) => void;
  onSetUrl: (name: string, url: string) => void;
  onRemove: (name: string) => void;
}

// リモートリポジトリ管理パネル。一覧表示・追加・URL変更・削除ができる。
export function RemotePanel({ remotes, onAdd, onSetUrl, onRemove }: Props) {
  const [newName, setNewName] = useState("");
  const [newUrl, setNewUrl] = useState("");
  // 編集中のリモート名（null = 非編集状態）。
  const [editingName, setEditingName] = useState<string | null>(null);
  const [editUrl, setEditUrl] = useState("");

  function submitAdd() {
    const n = newName.trim();
    const u = newUrl.trim();
    if (!n || !u) return;
    onAdd(n, u);
    setNewName("");
    setNewUrl("");
  }

  function startEdit(remote: RemoteInfo) {
    setEditingName(remote.name);
    setEditUrl(remote.fetch_url);
  }

  function submitEdit(name: string) {
    const u = editUrl.trim();
    if (!u) return;
    onSetUrl(name, u);
    setEditingName(null);
    setEditUrl("");
  }

  function cancelEdit() {
    setEditingName(null);
    setEditUrl("");
  }

  return (
    <div className="panel">
      <div className="panel-head">
        <h2>リモート</h2>
      </div>

      <div className="branch-create">
        <input
          value={newName}
          placeholder="リモート名（例: origin）"
          onChange={(e) => setNewName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && submitAdd()}
        />
        <input
          value={newUrl}
          placeholder="URL（例: https://github.com/...）"
          onChange={(e) => setNewUrl(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && submitAdd()}
        />
        <button
          className="btn btn-small"
          onClick={submitAdd}
          disabled={!newName.trim() || !newUrl.trim()}
          title="リモートリポジトリの接続先を追加します"
        >
          追加
        </button>
      </div>

      {remotes.length === 0 ? (
        <p className="empty">リモートが設定されていません。</p>
      ) : (
        <ul className="branches">
          {remotes.map((r) => (
            <li key={r.name}>
              {editingName === r.name ? (
                <div className="branch-row">
                  <span className="branch-name">{r.name}</span>
                  <div className="branch-actions">
                    <input
                      value={editUrl}
                      autoFocus
                      onChange={(e) => setEditUrl(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") submitEdit(r.name);
                        if (e.key === "Escape") cancelEdit();
                      }}
                      style={{ width: "100%", marginRight: "4px" }}
                    />
                    <button
                      className="btn btn-small"
                      onClick={() => submitEdit(r.name)}
                      disabled={!editUrl.trim()}
                    >
                      保存
                    </button>
                    <button
                      className="link"
                      onClick={cancelEdit}
                    >
                      キャンセル
                    </button>
                  </div>
                </div>
              ) : (
                <div className="branch-row">
                  <span className="branch-name">
                    {r.name}
                    {r.push_url && (
                      <span
                        className="badge unmerged"
                        title={`push URL: ${r.push_url}`}
                      >
                        push 別URL
                      </span>
                    )}
                  </span>
                  <span
                    className="path"
                    title={r.push_url ? `fetch: ${r.fetch_url}\npush: ${r.push_url}` : r.fetch_url}
                    style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                  >
                    {r.fetch_url}
                  </span>
                  <span className="branch-actions">
                    <button
                      className="link"
                      onClick={() => startEdit(r)}
                      title="fetch URL を変更します"
                    >
                      URL変更
                    </button>
                    <button
                      className="link danger"
                      onClick={() => onRemove(r.name)}
                      title="このリモートの設定を削除します。コミットや作業ファイルには影響しません。"
                    >
                      削除
                    </button>
                  </span>
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
