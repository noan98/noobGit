import { useEffect } from "react";
import { Box } from "@chakra-ui/react";
import { motion, useAnimation } from "framer-motion";
import type {
  Explanation,
  FileChange,
  RiskAssessment,
  RiskLevel,
} from "../api";
import { fadeIn, shakeXKeyframes, spring, transitions } from "../theme/motion";
import { StatusBadge } from "./StatusBadge";

const levelLabel: Record<RiskLevel, string> = {
  safe: "安全な操作",
  caution: "注意が必要な操作",
  destructive: "危険な操作",
};

// 危険度 → セマンティックカラートークン群（src/theme.ts）。badge の塗りに使う。
const levelTone: Record<RiskLevel, "success" | "warning" | "danger"> = {
  safe: "success",
  caution: "warning",
  destructive: "danger",
};

interface Props {
  title: string;
  assessment: RiskAssessment;
  explanation: Explanation;
  onConfirm: () => void;
  onCancel: () => void;
  // reset_hard 時のみ渡す。staged + unstaged の変更ファイル一覧。
  affectedFiles?: FileChange[];
}

export function ConfirmDialog({
  title,
  assessment,
  explanation,
  onConfirm,
  onCancel,
  affectedFiles,
}: Props) {
  const tone = levelTone[assessment.level];
  const isDestructive = assessment.level === "destructive";

  // ダイアログのアニメーション制御。
  // destructive の場合は scale-in に続けて水平震えを実行して危険を訴える。
  const dialogControls = useAnimation();
  useEffect(() => {
    void (async () => {
      await dialogControls.start({
        opacity: 1,
        scale: 1,
        transition: spring.snappy,
      });
      if (isDestructive) {
        await dialogControls.start({
          x: [...shakeXKeyframes],
          transition: { duration: 0.3, ease: "easeOut" },
        });
      }
    })();
  }, [dialogControls, isDestructive]);

  // destructive ではキャンセルを右（優先位置）に置き autoFocus でデフォルトフォーカスを与える。
  const cancelBtn = (
    <button className="btn" onClick={onCancel} autoFocus={isDestructive}>
      やめておく
    </button>
  );
  const confirmBtn = (
    <button
      className={`btn btn-confirm risk-${assessment.level}`}
      onClick={onConfirm}
    >
      理解して実行する
    </button>
  );

  return (
    // オーバーレイはフェードイン、ダイアログは useAnimation でスケールイン
    // （destructive の場合はさらに水平震え）で現れる。
    <motion.div
      className="overlay"
      role="dialog"
      aria-modal="true"
      variants={fadeIn}
      initial="hidden"
      animate="visible"
    >
      <motion.div
        className={`dialog risk-${assessment.level}`}
        initial={{ opacity: 0, scale: 0.96, x: 0 }}
        animate={dialogControls}
        exit={{ opacity: 0, scale: 0.96, transition: transitions.fast }}
      >
        <div className="dialog-head">
          {/* 危険度バッジはセマンティックトークンで塗る（danger/warning/success の
              solid 色と、その上に載せる onSolid 文字色）。data-theme に追従する。 */}
          <Box
            as="span"
            bg={`${tone}.solid`}
            color="neutral.onSolid"
            fontSize="12px"
            px="8px"
            py="2px"
            borderRadius="12px"
          >
            {levelLabel[assessment.level]}
          </Box>
          <h2>{title}</h2>
        </div>

        <section className="explain">
          <p className="explain-what">{explanation.what}</p>
          <p className="explain-why">{explanation.why}</p>
        </section>

        <section className="reasons">
          <h3>確認してください</h3>
          <ul>
            {assessment.reasons.map((r, i) => (
              <li key={i}>{r}</li>
            ))}
          </ul>
        </section>

        {/* reset_hard 時のみ表示: 失われる変更ファイルの一覧 */}
        {affectedFiles !== undefined && (
          <section className="affected-files-section">
            <h3>失われる変更</h3>
            {affectedFiles.length === 0 ? (
              <p className="affected-files-clean">
                変更なし — 安全にリセットできます
              </p>
            ) : (
              <div className="affected-files-list">
                {affectedFiles.map((f) => (
                  <div key={f.path} className="affected-file">
                    <StatusBadge kind={f.kind} />
                    <span className="affected-file-path">{f.path}</span>
                  </div>
                ))}
              </div>
            )}
          </section>
        )}

        <div className="flags">
          <span className={assessment.reversible ? "flag-ok" : "flag-warn"}>
            {assessment.reversible
              ? "あとから取り消せます"
              : "取り消しできません"}
          </span>
          {assessment.permanent_data_loss && (
            <span className="flag-danger">
              未保存の変更が失われる可能性があります
            </span>
          )}
        </div>

        {assessment.recommended_alternative && (
          <p className="alt">💡 {assessment.recommended_alternative}</p>
        )}

        {/* on_trouble は折りたたみ表示。ダイアログが長くなりすぎず、必要な人だけ開ける。 */}
        <details className="trouble-details">
          <summary className="trouble-summary">困ったときは</summary>
          <p className="trouble">{explanation.on_trouble}</p>
        </details>

        {/* destructive: [実行（左・非優先）] [やめておく（右・優先・フォーカス）]
            その他:      [やめておく（左）]   [実行（右）] */}
        <div className="dialog-actions">
          {isDestructive ? (
            <>
              {confirmBtn}
              {cancelBtn}
            </>
          ) : (
            <>
              {cancelBtn}
              {confirmBtn}
            </>
          )}
        </div>
      </motion.div>
    </motion.div>
  );
}
