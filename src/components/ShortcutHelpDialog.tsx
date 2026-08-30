/**
 * ショートカット一覧ヘルプダイアログ。
 * ? または F1 キーで開閉する。
 */
import { useId } from "react";
import { motion } from "framer-motion";
import { fadeIn, spring, transitions } from "../theme/motion";
import { useModalA11y } from "../hooks/useModalA11y";

// ショートカット一覧の定義。
const SHORTCUTS: { key: string; desc: string }[] = [
  { key: "Ctrl + K", desc: "コマンドパレットを開く" },
  { key: "Ctrl + Enter", desc: "コミット（メッセージ入力済みのとき）" },
  { key: "Ctrl + Shift + A", desc: "全ファイルをステージ" },
  { key: "Ctrl + Z", desc: "Undo（取り消し可能なときのみ）" },
  { key: "Ctrl + R", desc: "ステータス再取得" },
  { key: "Ctrl + P", desc: "現在ブランチをプッシュ" },
  { key: "? / F1", desc: "このヘルプを表示" },
];

interface Props {
  onClose: () => void;
}

export function ShortcutHelpDialog({ onClose }: Props) {
  const titleId = useId();
  // #151 フォーカストラップ + Esc キーで閉じる（共通フック）。
  const dialogRef = useModalA11y<HTMLDivElement>({ onEscape: onClose });

  return (
    <motion.div
      className="overlay"
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      variants={fadeIn}
      initial="hidden"
      animate="visible"
      onClick={onClose}
    >
      <motion.div
        ref={dialogRef}
        className="dialog"
        initial={{ opacity: 0, scale: 0.96 }}
        animate={{ opacity: 1, scale: 1, transition: spring.snappy }}
        exit={{ opacity: 0, scale: 0.96, transition: transitions.fast }}
        // ダイアログ内のクリックはオーバーレイへ伝播させない。
        onClick={(e) => e.stopPropagation()}
      >
        <div className="dialog-head">
          <h2 id={titleId}>キーボードショートカット</h2>
        </div>

        <table className="shortcut-table">
          <thead>
            <tr>
              <th>キー</th>
              <th>操作</th>
            </tr>
          </thead>
          <tbody>
            {SHORTCUTS.map(({ key, desc }) => (
              <tr key={key}>
                <td>
                  <kbd className="shortcut-key">{key}</kbd>
                </td>
                <td>{desc}</td>
              </tr>
            ))}
          </tbody>
        </table>

        <p className="shortcut-note">
          Mac では Ctrl の代わりに Cmd（⌘）が使えます。
          <br />
          テキスト入力欄にフォーカスがある場合、Ctrl+K と Ctrl+Enter 以外のショートカットは無効です。
        </p>

        <div className="dialog-actions">
          <button className="btn" onClick={onClose} autoFocus>
            閉じる
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}
