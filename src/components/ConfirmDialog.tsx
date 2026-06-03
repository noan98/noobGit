import { Box } from "@chakra-ui/react";
import { motion } from "framer-motion";
import type { Explanation, RiskAssessment, RiskLevel } from "../api";
import { fadeIn, scaleIn } from "../theme/motion";

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
}

export function ConfirmDialog({
  title,
  assessment,
  explanation,
  onConfirm,
  onCancel,
}: Props) {
  const tone = levelTone[assessment.level];
  return (
    // オーバーレイはフェードイン、ダイアログはスケールインで現れる。
    // 動きのパラメータは motion トークン（fadeIn / scaleIn）に集約している。
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
        variants={scaleIn}
        initial="hidden"
        animate="visible"
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

        <p className="trouble">{explanation.on_trouble}</p>

        <div className="dialog-actions">
          <button className="btn" onClick={onCancel}>
            やめておく
          </button>
          <button
            className={`btn btn-confirm risk-${assessment.level}`}
            onClick={onConfirm}
          >
            理解して実行する
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}
