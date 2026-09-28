/**
 * #70 .gitignore 管理 UI — `.gitignore` の内容を閲覧・編集するモーダル。
 *
 * StatusPanel の「無視リスト」ボタンから開く。現在の `.gitignore` の中身を
 * そのまま（スクロール可能な領域で）表示し、初心者が「いま何が無視されているか」を
 * 確認できるようにする。ファイルがまだ無い場合は、その旨と簡単な説明を表示する。
 *
 * #173 バリデーション・重複検知:
 * 手入力欄からパターンを直接追加できる。入力するたびに（300ms デバウンスして）
 * `core::ops::check_gitignore_pattern` を呼び、glob 構文として不正なら赤字で
 * 理由を表示し、すでに同じパターンが `.gitignore` にあれば重複である旨を表示する。
 * どちらの場合も「追加」ボタンは無効化される（不正なパターンを書き込ませない／
 * 意味の無い重複追記をさせない）。実際の書き込み（`git2` 呼び出し）は
 * `core::ops::add_to_gitignore` を薄くラップした `onAdd` 経由で行い、UI 層に
 * Git ロジックを持ち込まない。
 */
import { useEffect, useId, useRef, useState } from "react";
import { motion } from "framer-motion";
import { fadeIn, spring, transitions } from "../theme/motion";
import { api, type GitignorePatternCheck } from "../api";
import { useModalA11y } from "../hooks/useModalA11y";

interface Props {
  // .gitignore の内容。ファイルがまだ無い場合は null。
  content: string | null;
  repoPath: string;
  // #173 手入力欄からパターンを追加する（実際の書き込みは呼び出し元＝
  // RepoWorkspace が core 経由で行い、成功・失敗はトースト通知で伝える）。
  onAdd: (pattern: string) => Promise<void>;
  onClose: () => void;
}

// 入力中のリアルタイムバリデーションのデバウンス間隔（ミリ秒）。
const VALIDATE_DEBOUNCE_MS = 300;

export function GitignoreModal({ content, repoPath, onAdd, onClose }: Props) {
  const titleId = useId();
  // #151 フォーカストラップ + Esc キーで閉じる（共通フック）。
  const dialogRef = useModalA11y<HTMLDivElement>({ onEscape: onClose });

  // null（ファイル無し）と空文字（空ファイル）を区別して案内する。
  const isMissing = content === null;
  const isEmpty = content !== null && content.trim() === "";

  // #173 手入力欄の状態。
  const [pattern, setPattern] = useState("");
  const [check, setCheck] = useState<GitignorePatternCheck | null>(null);
  const [checking, setChecking] = useState(false);
  const [adding, setAdding] = useState(false);
  // 直近のリクエストだけを反映するための連番（デバウンス中に入力が変わったときの
  // レース対策。古いレスポンスが後から返ってきても無視する）。
  const requestIdRef = useRef(0);

  useEffect(() => {
    const trimmed = pattern.trim();
    if (trimmed === "") {
      setCheck(null);
      setChecking(false);
      return;
    }
    setChecking(true);
    const requestId = ++requestIdRef.current;
    const timer = window.setTimeout(() => {
      api
        .checkGitignorePattern(repoPath, trimmed)
        .then((result) => {
          if (requestIdRef.current === requestId) {
            setCheck(result);
            setChecking(false);
          }
        })
        .catch(() => {
          if (requestIdRef.current === requestId) {
            // チェック自体の失敗（通信エラーなど）はブロックしない。
            // 実際の書き込み時に core 側で改めて検証されるため、ここでは
            // エラー表示せず「未検証」のまま扱う。
            setCheck(null);
            setChecking(false);
          }
        });
    }, VALIDATE_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [pattern, repoPath]);

  const trimmedPattern = pattern.trim();
  const canAdd =
    trimmedPattern !== "" &&
    !checking &&
    !adding &&
    check !== null &&
    check.valid &&
    !check.duplicate;

  async function handleAdd() {
    if (!canAdd) return;
    setAdding(true);
    try {
      await onAdd(trimmedPattern);
    } finally {
      // 成功・失敗いずれも入力欄はクリアする。失敗はトースト通知で伝わる。
      setPattern("");
      setCheck(null);
      setAdding(false);
    }
  }

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
        onClick={(e) => e.stopPropagation()}
      >
        <div className="dialog-head">
          <h2 id={titleId}>.gitignore の内容</h2>
        </div>

        <p style={{ fontSize: "13px", color: "var(--muted)", marginBottom: "8px" }}>
          <code>.gitignore</code> は、Git に無視させたいファイルを 1 行ずつ書いておく
          ファイルです。ここに書いたファイルはコミット対象に出てこなくなります。
        </p>

        {/* #173 手入力欄: パターンを直接追加する。 */}
        <label className="field" style={{ marginBottom: "4px" }}>
          <span>新しいパターンを追加</span>
          <input
            value={pattern}
            placeholder="例: *.log や build/"
            onChange={(e) => setPattern(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                void handleAdd();
              }
            }}
            aria-invalid={check !== null && !check.valid}
          />
        </label>
        {check && !check.valid && check.error && (
          <p
            role="alert"
            style={{
              color: "var(--destructive)",
              fontSize: "12px",
              margin: "0 0 8px",
            }}
          >
            {check.error}
          </p>
        )}
        {check && check.valid && check.duplicate && (
          <p style={{ color: "var(--muted)", fontSize: "12px", margin: "0 0 8px" }}>
            このパターンはすでに .gitignore にあります（追加してもスキップされます）。
          </p>
        )}
        <div style={{ marginBottom: "14px" }}>
          <button
            className="btn btn-small"
            onClick={() => void handleAdd()}
            disabled={!canAdd}
          >
            {adding ? "追加中…" : "追加"}
          </button>
        </div>

        {isMissing ? (
          <p style={{ fontSize: "13px", color: "var(--muted)" }}>
            このリポジトリにはまだ <code>.gitignore</code> がありません。
            上の欄からパターンを追加すると新しく作成されます。
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
