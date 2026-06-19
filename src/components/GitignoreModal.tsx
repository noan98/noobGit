/**
 * #70 .gitignore 管理 UI — `.gitignore` の内容を閲覧するモーダル。
 *
 * StatusPanel の「無視リスト」ボタンから開く。現在の `.gitignore` の中身を
 * そのまま（スクロール可能な領域で）表示し、初心者が「いま何が無視されているか」を
 * 確認できるようにする。ファイルがまだ無い場合は、その旨と簡単な説明を表示する。
 *
 * 表示専用。パターンの追加は StatusPanel の各ファイル行の「無視」ボタンから行う。
 */
import { useEffect } from "react";
import { motion } from "framer-motion";
import { fadeIn, spring, transitions } from "../theme/motion";

interface Props {
  // .gitignore の内容。ファイルがまだ無い場合は null。
  content: string | null;
  onClose: () => void;
}

export function GitignoreModal({ content, onClose }: Props) {
  // Escape キーで閉じる。
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent): void {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [onClose]);

  // null（ファイル無し）と空文字（空ファイル）を区別して案内する。
  const isMissing = content === null;
  const isEmpty = content !== null && content.trim() === "";

  return (
    <motion.div
      className="overlay"
      role="dialog"
      aria-modal="true"
      aria-label=".gitignore の内容"
      variants={fadeIn}
      initial="hidden"
      animate="visible"
      onClick={onClose}
    >
      <motion.div
        className="dialog"
        initial={{ opacity: 0, scale: 0.96 }}
        animate={{ opacity: 1, scale: 1, transition: spring.snappy }}
        exit={{ opacity: 0, scale: 0.96, transition: transitions.fast }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="dialog-head">
          <h2>.gitignore の内容</h2>
        </div>

        <p style={{ fontSize: "13px", color: "var(--muted)", marginBottom: "8px" }}>
          <code>.gitignore</code> は、Git に無視させたいファイルを 1 行ずつ書いておく
          ファイルです。ここに書いたファイルはコミット対象に出てこなくなります。
        </p>

        {isMissing ? (
          <p style={{ fontSize: "13px", color: "var(--muted)" }}>
            このリポジトリにはまだ <code>.gitignore</code> がありません。
            ファイル行の「無視」ボタンを押すと、新しく作成して追記します。
          </p>
        ) : isEmpty ? (
          <p style={{ fontSize: "13px", color: "var(--muted)" }}>
            <code>.gitignore</code> はありますが、中身は空です。
          </p>
        ) : (
          <pre
            style={{
              maxHeight: "50vh",
              overflow: "auto",
              margin: 0,
              padding: "10px 12px",
              background: "var(--bg)",
              border: "1px solid var(--border)",
              borderRadius: "var(--radius-sm)",
              fontSize: "12px",
              lineHeight: "1.5",
              whiteSpace: "pre",
            }}
          >
            {content}
          </pre>
        )}

        <div className="dialog-actions">
          <button className="btn" onClick={onClose} autoFocus>
            閉じる
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}
