import { useState } from "react";
import type { TagInfo } from "../api";

interface Props {
  tags: TagInfo[];
  // タグを付けられるか（コミットが1件も無いと付けられない）。
  canTag: boolean;
  onCreate: (name: string, message?: string) => void;
  onDelete: (name: string) => void;
}

// タグ（目印）パネル。リリース地点などに覚えやすい名前を付け、一覧・削除できる。
export function TagPanel({ tags, canTag, onCreate, onDelete }: Props) {
  const [name, setName] = useState("");
  const [message, setMessage] = useState("");

  function submitCreate() {
    const n = name.trim();
    if (!n) return;
    const m = message.trim();
    onCreate(n, m || undefined);
    setName("");
    setMessage("");
  }

  return (
    <div className="panel">
      <div className="panel-head">
        <h2>タグ</h2>
      </div>

      <p className="hint">
        コミットに覚えやすい目印を付けられます（例: <code>v1.0.0</code>）。
        メッセージを書くと「注釈付きタグ」になります。
      </p>

      <div className="tag-create">
        <input
          value={name}
          placeholder="タグ名（例: v1.0.0）"
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && canTag && submitCreate()}
        />
        <input
          value={message}
          placeholder="メッセージ（任意）"
          onChange={(e) => setMessage(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && canTag && submitCreate()}
        />
        <button
          className="btn btn-small"
          onClick={submitCreate}
          disabled={!canTag || !name.trim()}
          title={
            canTag
              ? "いまのコミットにタグを付けます"
              : "まだコミットが無いためタグを付けられません"
          }
        >
          タグを作成
        </button>
      </div>

      {tags.length === 0 ? (
        <p className="empty">タグはありません。</p>
      ) : (
        <ul className="tags">
          {tags.map((t) => (
            <li key={t.name}>
              <span className="branch-name">
                {t.name}
                {t.message && (
                  <span className="badge merged" title={t.message}>
                    注釈付き
                  </span>
                )}
              </span>
              <code className="sha">{t.target_short_id}</code>
              <span className="branch-actions">
                <button
                  className="link danger"
                  onClick={() => onDelete(t.name)}
                  title="このタグ（目印）を削除します。コミットは消えません。"
                >
                  削除
                </button>
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
