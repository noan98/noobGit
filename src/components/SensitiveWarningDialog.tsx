/**
 * #69 機密ファイル検出 UI — ステージ前に機密性の高いファイルを警告するダイアログ。
 *
 * `.env`・秘密鍵・DB ファイルなどがステージされようとした際に表示する。
 * ユーザーは「キャンセル」「.gitignore に追加」「それでもステージする」の
 * 3 択から選べる。Git に機密情報を載せると push 後は実質回収できない旨を
 * 目立つ赤系の警告色で伝える。
 */
import { useEffect } from "react";
import { motion, useAnimation } from "framer-motion";
import { fadeIn, spring, transitions } from "../theme/motion";
import type { SensitiveWarning } from "../api";

interface Props {
  /** 検出された機密ファイルの警告一覧（1件以上）。 */
  warnings: SensitiveWarning[];
  /** 警告を無視してそのままステージを続ける。 */
  onStageAnyway: () => void;
  /** 指定パスを .gitignore に追加する（ファイルごとに呼ばれる）。 */
  onAddToGitignore: (pattern: string) => void;
  /** ステージをキャンセルする。 */
  onCancel: () => void;
}

export function SensitiveWarningDialog({
  warnings,
  onStageAnyway,
  onAddToGitignore,
  onCancel,
}: Props) {
  // ダイアログを scale-in + 水平震えで現れさせ、危険を訴える。
  const dialogControls = useAnimation();
  useEffect(() => {
    void (async () => {
      await dialogControls.start({
        opacity: 1,
        scale: 1,
        transition: spring.snappy,
      });
      // 赤い警告なので destructive と同様に震わせる。
      await dialogControls.start({
        x: [0, -6, 6, -5, 5, -3, 3, 0],
        transition: { duration: 0.35, ease: "easeOut" },
      });
    })();
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
      aria-label="機密ファイルの警告"
      variants={fadeIn}
      initial="hidden"
      animate="visible"
    >
      <motion.div
        className="dialog risk-destructive"
        initial={{ opacity: 0, scale: 0.96, x: 0 }}
        animate={dialogControls}
        exit={{ opacity: 0, scale: 0.96, transition: transitions.fast }}
        style={{ maxWidth: "560px" }}
      >
        <div className="dialog-head">
          <span
            style={{
              background: "var(--destructive)",
              color: "#fff",
              fontSize: "12px",
              padding: "2px 10px",
              borderRadius: "12px",
              fontWeight: 700,
            }}
          >
            機密情報の可能性
          </span>
          <h2>機密ファイルをステージしようとしています</h2>
        </div>

        <section className="explain">
          <p className="explain-what" style={{ fontWeight: 600, color: "var(--destructive)" }}>
            以下のファイルには、パスワード・秘密鍵・個人情報などの
            機密情報が含まれている可能性があります。
          </p>
          <p className="explain-why" style={{ fontSize: "13px" }}>
            Git に機密情報を一度コミットして push すると、
            <strong>履歴に永久に残り</strong>、削除しても遅すぎることがほとんどです。
            まず .gitignore に追加して管理対象から外すことを強くおすすめします。
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
            {warnings.map((w) => (
              <div
                key={w.path}
                style={{
                  padding: "10px 12px",
                  background: "var(--bg)",
                  border: "1px solid var(--destructive)",
                  borderRadius: "var(--radius-sm)",
                  fontSize: "13px",
                }}
              >
                <div
                  style={{
                    fontFamily: "monospace",
                    fontWeight: 700,
                    marginBottom: "4px",
                    color: "var(--destructive)",
                    wordBreak: "break-all",
                  }}
                >
                  {w.path}
                </div>
                <div style={{ color: "var(--fg)", lineHeight: "1.5" }}>
                  {w.reason}
                </div>
                <div style={{ marginTop: "8px" }}>
                  <button
                    className="btn btn-small"
                    onClick={() => onAddToGitignore(w.path)}
                    title=".gitignore に追加して管理対象から外す"
                  >
                    .gitignore に追加
                  </button>
                </div>
              </div>
            ))}
          </div>
        </section>

        <div className="flags">
          <span className="flag-danger">
            push 後は実質的に取り消せません
          </span>
        </div>

        <p className="alt" style={{ fontSize: "13px" }}>
          ファイルを .gitignore に追加してステージを取り消すか、
          本当にコミットする必要があるか確認してください。
        </p>

        {/* ボタン配置: 危険操作と同じく「実行（左・非優先）」「やめる（右・優先・フォーカス）」 */}
        <div className="dialog-actions">
          <button
            className="btn btn-confirm risk-destructive"
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
