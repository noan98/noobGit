import { useState } from "react";
import type { FileChange, StashInfo } from "../api";
import { changeKindLabel } from "../api";

interface Props {
  stashes: StashInfo[];
  // 退避できる変更があるか（クリーンなら退避ボタンを無効にする）。
  canStash: boolean;
  onSave: (message: string) => void;
  onApply: (index: number) => void;
  onPop: (index: number) => void;
  // 指定退避の変更ファイル一覧を取得する（退避は適用しない安全な操作）。
  onLoadDiff: (index: number) => Promise<FileChange[]>;
}

// 退避（stash）パネル。変更を一時的にしまい、あとから取り出す。
export function StashPanel({
  stashes,
  canStash,
  onSave,
  onApply,
  onPop,
  onLoadDiff,
}: Props) {
  const [message, setMessage] = useState("");
  // 展開中の退避 id → その差分（読み込み中は undefined）。
  const [expanded, setExpanded] = useState<Record<string, FileChange[] | undefined>>(
    {},
  );

  function submitSave() {
    onSave(message.trim());
    setMessage("");
  }

  async function toggleDiff(s: StashInfo) {
    // すでに開いていれば閉じる。
    if (s.id in expanded) {
      setExpanded((prev) => {
        const next = { ...prev };
        delete next[s.id];
        return next;
      });
      return;
    }
    // 読み込み中（undefined）を立てておき、取得後に差し替える。
    setExpanded((prev) => ({ ...prev, [s.id]: undefined }));
    try {
      const files = await onLoadDiff(s.index);
      setExpanded((prev) => ({ ...prev, [s.id]: files }));
    } catch {
      // 取得に失敗したら閉じる（エラー表示は App 側のトーストに委ねる）。
      setExpanded((prev) => {
        const next = { ...prev };
        delete next[s.id];
        return next;
      });
    }
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
          placeholder="退避の覚え書き（任意・空なら自動で名前を付けます）"
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
          {stashes.map((s) => {
            const isOpen = s.id in expanded;
            const files = expanded[s.id];
            return (
              <li key={s.id}>
                <code className="sha">{s.id.slice(0, 7)}</code>
                <span className="summary">{s.message || "(メッセージなし)"}</span>
                <span
                  className="badge"
                  title={`${s.file_count} 個のファイルが変更されています`}
                >
                  {s.file_count} ファイル
                </span>
                <span className="branch-actions">
                  <button
                    className="link"
                    onClick={() => void toggleDiff(s)}
                    title="この退避に含まれる変更ファイルの一覧を表示します（適用はしません）"
                  >
                    {isOpen ? "差分を隠す" : "差分を見る"}
                  </button>
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

                {isOpen && (
                  <div className="stash-diff">
                    {files === undefined ? (
                      <p className="empty">読み込み中…</p>
                    ) : files.length === 0 ? (
                      <p className="empty">変更ファイルはありません。</p>
                    ) : (
                      <ul className="stash-diff-files">
                        {files.map((f) => (
                          <li key={f.path}>
                            <span className={`tag tag-${f.kind}`}>
                              {changeKindLabel[f.kind]}
                            </span>
                            <span className="path">{f.path}</span>
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
