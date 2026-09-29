import { useEffect, useId } from "react";
import { Box } from "@chakra-ui/react";
import { motion, useAnimation } from "framer-motion";
import type {
  Explanation,
  ImpactPreview,
  RiskAssessment,
  RiskLevel,
} from "../api";
import { fadeIn, shakeXKeyframes, spring, transitions } from "../theme/motion";
import { ImpactPreviewSection } from "./ImpactPreviewSection";
import { useModalA11y } from "../hooks/useModalA11y";
import { Icon } from "./Icon";
import { TermText } from "./Term";

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
  // #196 操作別の影響プレビュー（reset_hard の失われる変更、discard の差分、
  // delete_branch / force push / squash のコミット一覧など）。計算に失敗した
  // 場合や対象外の操作では渡さない（プレビューなしで通常どおり確認できる）。
  preview?: ImpactPreview;
  // #269 マージ済みブランチの一括削除時のみ渡す。削除対象のブランチ名一覧。
  affectedBranches?: string[];
}

export function ConfirmDialog({
  title,
  assessment,
  explanation,
  onConfirm,
  onCancel,
  preview,
  affectedBranches,
}: Props) {
  const tone = levelTone[assessment.level];
  const isDestructive = assessment.level === "destructive";

  // #151 タイトルを aria-labelledby で結び付けるための id。
  const titleId = useId();

  // #151 フォーカストラップ + Esc 制御。destructive のときだけ Esc を無効化し、
  // ボタンクリックでの明示的な選択を必須にする（誤操作防止）。
  const dialogRef = useModalA11y<HTMLDivElement>({
    onEscape: onCancel,
    disableEscape: isDestructive,
  });

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

  // キャンセルに autoFocus でデフォルトフォーカスを与える（destructive では右＝優先位置）。
  // 全レベルで付けるのは、説明文中の用語（Term, tabIndex=0）へ初期フォーカスが
  // 落ちて解説が勝手に開くのを防ぐため。従来も DOM 順で最初のボタン＝キャンセルだった。
  const cancelBtn = (
    <button className="btn" onClick={onCancel} autoFocus>
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
      role={isDestructive ? "alertdialog" : "dialog"}
      aria-modal="true"
      aria-labelledby={titleId}
      variants={fadeIn}
      initial="hidden"
      animate="visible"
    >
      <motion.div
        ref={dialogRef}
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
          <h2 id={titleId}>{title}</h2>
        </div>

        <section className="explain">
          <p className="explain-what">
            <TermText text={explanation.what} />
          </p>
          <p className="explain-why">
            <TermText text={explanation.why} />
          </p>
        </section>

        <section className="reasons">
          <h3>確認してください</h3>
          <ul>
            {assessment.reasons.map((r, i) => (
              <li key={i}>
                <TermText text={r} />
              </li>
            ))}
          </ul>
        </section>

        {/* #269 マージ済みブランチの一括削除時のみ表示: 削除対象のブランチ一覧 */}
        {affectedBranches !== undefined && (
          <section className="affected-files-section">
            <h3>削除するブランチ（{affectedBranches.length}件）</h3>
            <div className="affected-files-list">
              {affectedBranches.map((name) => (
                <div key={name} className="affected-file">
                  <Icon name="branch" />
                  <span className="affected-file-path">{name}</span>
                </div>
              ))}
            </div>
          </section>
        )}

        {/* #196 操作別の影響プレビュー */}
        {preview && <ImpactPreviewSection preview={preview} />}

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
          <p className="alt">
            <Icon name="hint" />{" "}
            <TermText text={assessment.recommended_alternative} />
          </p>
        )}

        {/* on_trouble は折りたたみ表示。ダイアログが長くなりすぎず、必要な人だけ開ける。 */}
        <details className="trouble-details">
          <summary className="trouble-summary">
            <Icon name="chevronRight" className="trouble-summary-chevron" />
            困ったときは
          </summary>
          {/* 折りたたみ中の内容はフォーカスできないため、フォーカストラップの
              末尾候補にならないよう <Term> は付けずに素のテキストで出す。 */}
          <p className="trouble">{explanation.on_trouble}</p>
        </details>

        {/* #151 destructive では Esc キーでの取り消しを無効化する（誤操作防止）。
            キーボード操作をブロックするだけでなく、その旨を必ず視覚的に示す。 */}
        {isDestructive && (
          <p className="esc-disabled-note" role="note">
            <Icon name="warning" /> Esc キーでは閉じられません。下のボタンで選んでください。
          </p>
        )}

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
