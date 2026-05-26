import { useState } from "react";
import type { StashInfo } from "../api";

interface Props {
  stashes: StashInfo[];
  // 退避できる変更があるか（クリーンなら退避ボタンを無効にする）。
  canStash: boolean;
  onSave: (message: string) => void;
  onApply: (index: number) => void;
  onPop: (index: number) => void;
}

// 退避（stash）パネル。変更を一時的にしまい、あとから取り出す。
export function StashPanel({ stashes, canStash, onSave, onApply, onPop }: Props) {
  const [message, setMessage] = useState("");

  function submitSave() {
    onSave(message.trim());
    setMessage("");
  }

  return (
    <div className="panel">
      <div className="panel-head">
        <h2>退避（stash）</h2>
      </div>

      <p className="hint">
        いまの変更を消さずに一時的にしまえます。ブランチを切り替えたいときに便利です。
      </p>

      <div className="stash-save">
        <input
          value={message}
          placeholder="退避の覚え書き（任意）"
          onChange={(e) => setMessage(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && canStash && submitSave()}
        />
        <button
          className="btn btn-small"
          onClick={submitSave}
          disabled={!canStash}
          title={
            canStash
              ? "いまの変更を一時的にしまいます"
              : "しまえる変更がありません"
          }
        >
          退避する
        </button>
      </div>

      {stashes.length === 0 ? (
        <p className="empty">退避はありません。</p>
      ) : (
        <ul className="stashes">
          {stashes.map((s) => (
            <li key={s.id}>
              <code className="sha">{s.id.slice(0, 7)}</code>
              <span className="summary">{s.message || "(メッセージなし)"}</span>
              <span className="branch-actions">
                <button
                  className="link"
                  onClick={() => onApply(s.index)}
                  title="取り出して戻します（退避は一覧に残します）"
                >
                  適用
                </button>
                <button
                  className="link"
                  onClick={() => onPop(s.index)}
                  title="取り出して戻し、この退避を一覧から取り除きます"
                >
                  取り出す
                </button>
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
