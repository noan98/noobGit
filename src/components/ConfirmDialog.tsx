import type { Explanation, RiskAssessment, RiskLevel } from "../api";

const levelLabel: Record<RiskLevel, string> = {
  safe: "安全な操作",
  caution: "注意が必要な操作",
  destructive: "危険な操作",
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
  return (
    <div className="overlay" role="dialog" aria-modal="true">
      <div className={`dialog risk-${assessment.level}`}>
        <div className="dialog-head">
          <span className={`risk-badge risk-${assessment.level}`}>
            {levelLabel[assessment.level]}
          </span>
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
      </div>
    </div>
  );
}
