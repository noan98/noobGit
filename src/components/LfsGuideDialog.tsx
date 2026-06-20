/**
 * #81 LFS ガイド UI — ステージ前に大容量・バイナリファイルを検出したときのガイドダイアログ。
 *
 * 5MB 超のファイルやバイナリ拡張子のファイルをステージしようとした際に表示する。
 * ユーザーは「キャンセル」「.gitignore に追加」「それでもステージする」の
 * 3 択から選べる。Git LFS を使って大きなファイルを別管理にすることを提案する。
 */
import { useEffect } from "react";
import { motion, useAnimation } from "framer-motion";
import { fadeIn, spring, transitions } from "../theme/motion";
import type { LfsCandidate } from "../api";

interface Props {
  /** 検出された LFS 移行候補ファイルの一覧（1件以上）。 */
  candidates: LfsCandidate[];
  /** 警告を無視してそのままステージを続ける。 */
  onStageAnyway: () => void;
  /** 指定パスを .gitignore に追加する（ファイルごとに呼ばれる）。 */
  onAddToGitignore: (pattern: string) => void;
  /** ステージをキャンセルする。 */
  onCancel: () => void;
}

/** バイト数を人間が読みやすい形式に変換する（例: 6291456 → "6.0 MB"）。 */
function formatBytes(bytes: number): string {
  if (bytes === 0) return "不明";
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
  if (bytes >= 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${bytes} B`;
}

export function LfsGuideDialog({
  candidates,
  onStageAnyway,
  onAddToGitignore,
  onCancel,
}: Props) {
  // ダイアログを scale-in で現れさせる。
  const dialogControls = useAnimation();
  useEffect(() => {
    void dialogControls.start({
      opacity: 1,
      scale: 1,
      transition: spring.snappy,
    });
  }, [dialogControls]);

  // Escape キーでキャンセル。
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent): void {
      if (e.key === "Escape") {
        e.preventDefault();
        onCancel();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [onCancel]);

  return (
    <motion.div
      className="overlay"
      role="dialog"
      aria-modal="true"
      aria-label="大容量・バイナリファイルのガイド"
      variants={fadeIn}
      initial="hidden"
      animate="visible"
    >
      <motion.div
        className="dialog risk-caution"
        initial={{ opacity: 0, scale: 0.96 }}
        animate={dialogControls}
        exit={{ opacity: 0, scale: 0.96, transition: transitions.fast }}
        style={{ maxWidth: "560px" }}
      >
        <div className="dialog-head">
          <span
            style={{
              background: "var(--caution)",
              color: "#fff",
              fontSize: "12px",
              padding: "2px 10px",
              borderRadius: "12px",
              fontWeight: 700,
            }}
          >
            Git LFS 推奨
          </span>
          <h2>大容量・バイナリファイルを検出しました</h2>
        </div>

        <section className="explain">
          <p className="explain-what" style={{ fontWeight: 600 }}>
            以下のファイルは、大きなサイズまたはバイナリ形式（画像・動画・実行ファイルなど）です。
          </p>
          <p className="explain-why" style={{ fontSize: "13px" }}>
            大きなファイルやバイナリを Git に直接コミットすると、
            <strong>リポジトリが肥大化してクローンやプルが遅く</strong>なります。
            <br />
            <strong>Git LFS（Large File Storage）</strong> は大きなファイルを
            Git の外部に別管理する仕組みで、履歴をスリムに保てます。
            不要なら <code>.gitignore</code> に追加して管理対象から外すことも検討してください。
          </p>
        </section>

        <section className="reasons" style={{ marginBottom: "12px" }}>
          <h3>検出されたファイル</h3>
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              gap: "10px",
              maxHeight: "280px",
              overflowY: "auto",
            }}
          >
            {candidates.map((c) => (
              <div
                key={c.path}
                style={{
                  padding: "10px 12px",
                  background: "var(--bg)",
                  border: "1px solid var(--caution)",
                  borderRadius: "var(--radius-sm)",
                  fontSize: "13px",
                }}
              >
                <div
                  style={{
                    fontFamily: "monospace",
                    fontWeight: 700,
                    marginBottom: "2px",
                    color: "var(--caution)",
                    wordBreak: "break-all",
                  }}
                >
                  {c.path}
                </div>
                <div
                  style={{
                    fontSize: "12px",
                    color: "var(--fg-muted, var(--fg))",
                    marginBottom: "4px",
                  }}
                >
                  サイズ: {formatBytes(c.size_bytes)}
                </div>
                <div style={{ color: "var(--fg)", lineHeight: "1.5" }}>
                  {c.reason}
                </div>
                <div style={{ marginTop: "8px" }}>
                  <button
                    className="btn btn-small"
                    onClick={() => onAddToGitignore(c.path)}
                    title=".gitignore に追加して管理対象から外す"
                  >
                    .gitignore に追加
                  </button>
                </div>
              </div>
            ))}
          </div>
        </section>

        <p className="alt" style={{ fontSize: "13px" }}>
          ファイルを .gitignore に追加するか、Git LFS を導入してからステージすることをおすすめします。
          どうしても今すぐステージする場合は「それでもステージする」を選んでください。
        </p>

        {/* ボタン配置: 「実行（左・非優先）」「やめる（右・優先・フォーカス）」 */}
        <div className="dialog-actions">
          <button
            className="btn btn-confirm risk-caution"
            onClick={onStageAnyway}
          >
            それでもステージする
          </button>
          <button className="btn" onClick={onCancel} autoFocus>
            キャンセル
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}
