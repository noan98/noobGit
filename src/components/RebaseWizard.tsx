import { useEffect, useId, useMemo, useRef, useState } from "react";
import type { CommitInfo, ImpactPreview, RebaseStep } from "../api";
import { useModalA11y } from "../hooks/useModalA11y";
import { Term } from "./Term";
import { Icon } from "./Icon";

// リベースの種類。squash は複数コミットを1つにまとめる、reword は1つのメッセージを書き換える、
// plan は並べ替え・削除（drop）・書き換え・まとめるを1つのプランで混在させる。
type RebaseMode = "squash" | "reword" | "plan";

// プラン編集の 1 行分の操作。
type PlanAction = "pick" | "drop" | "reword" | "squash";

interface PlanRow {
  commit: CommitInfo;
  action: PlanAction;
  // reword のときの新しいメッセージ。
  message: string;
}

// 画面上の行（新しい順）を、core に渡すプラン（古い順）へ変換する。
function rowsToPlan(rows: PlanRow[]): RebaseStep[] {
  return [...rows].reverse().map((r): RebaseStep => {
    switch (r.action) {
      case "drop":
        return { action: "drop", oid: r.commit.id };
      case "squash":
        return { action: "squash", oid: r.commit.id };
      case "reword":
        return { action: "reword", oid: r.commit.id, message: r.message };
      default:
        return { action: "pick", oid: r.commit.id };
    }
  });
}

interface Props {
  // 選択されたコミット。HistoryPanel で新しい順（先頭が HEAD に近い）に並んでいる前提。
  selected: CommitInfo[];
  // 実行ハンドラ。squash は対象 oid 列（新しい順）とメッセージ、reword はメッセージのみ。
  onSquash: (commitOids: string[], message: string) => void;
  onReword: (message: string) => void;
  // プラン（古い順）の実行。並べ替え・削除・reword・squash を混在できる。
  onRunPlan: (plan: RebaseStep[]) => void;
  // プランの「変更前 → 変更後」プレビューを core に計算させる（読み取り専用）。
  // 不正なプランは reject される（メッセージをそのまま画面に出す）。
  loadPlanPreview: (plan: RebaseStep[]) => Promise<ImpactPreview>;
  onCancel: () => void;
}

// squash / reword / 並べ替え・削除（plan）を選んで実行するウィザード。
// 対象は HEAD から連続するコミットの範囲だけ（core 側が検証する）。
export function RebaseWizard({
  selected,
  onSquash,
  onReword,
  onRunPlan,
  loadPlanPreview,
  onCancel,
}: Props) {
  // 選択数に応じて初期モードを決める。2つ以上なら squash、1つなら reword。
  const initialMode: RebaseMode = selected.length >= 2 ? "squash" : "reword";
  const [mode, setMode] = useState<RebaseMode>(initialMode);

  // squash のメッセージ初期値: 選んだコミットのメッセージを古い順に連結する。
  const initialSquashMsg = useMemo(
    () =>
      [...selected]
        .reverse()
        .map((c) => c.summary)
        .filter((s) => s.length > 0)
        .join("\n\n"),
    [selected],
  );
  // reword のメッセージ初期値: 選んだ1つ（または先頭）のメッセージ。
  const initialRewordMsg = selected[0]?.summary ?? "";

  const [message, setMessage] = useState(
    initialMode === "squash" ? initialSquashMsg : initialRewordMsg,
  );

  // モード切り替え時に既定メッセージを入れ替える。
  function switchMode(next: RebaseMode) {
    setMode(next);
    setMessage(next === "squash" ? initialSquashMsg : initialRewordMsg);
  }

  // 並べ替え・削除モードの行（新しい順）。
  const [rows, setRows] = useState<PlanRow[]>(() =>
    selected.map((c) => ({ commit: c, action: "pick", message: c.summary })),
  );
  const plan = useMemo(() => rowsToPlan(rows), [rows]);
  const [planPreview, setPlanPreview] = useState<ImpactPreview | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);

  function moveRow(index: number, delta: -1 | 1) {
    setRows((prev) => {
      const to = index + delta;
      if (to < 0 || to >= prev.length) return prev;
      const next = [...prev];
      [next[index], next[to]] = [next[to], next[index]];
      return next;
    });
  }
  function updateRow(index: number, patch: Partial<PlanRow>) {
    setRows((prev) => prev.map((r, i) => (i === index ? { ...r, ...patch } : r)));
  }

  // 親が毎回作り直す関数でも効果が再実行されないよう、ref 経由で最新のものを呼ぶ。
  const loadPreviewRef = useRef(loadPlanPreview);
  loadPreviewRef.current = loadPlanPreview;

  // プランが変わるたびに、core に「変更前 → 変更後」を計算させる。
  // 少し待ってから呼び（連打対策）、古い結果は捨てる。
  useEffect(() => {
    if (mode !== "plan") return;
    let cancelled = false;
    const timer = setTimeout(() => {
      loadPreviewRef.current(plan).then(
        (p) => {
          if (cancelled) return;
          setPlanPreview(p);
          setPlanError(null);
        },
        (e) => {
          if (cancelled) return;
          setPlanPreview(null);
          setPlanError(String(e));
        },
      );
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [mode, plan]);

  const titleId = useId();
  // #151 フォーカストラップ + Esc キーでキャンセル
  // （このウィザードは選択・下書きの段階で、実際の破壊的操作の確認は
  // guarded() 経由の ConfirmDialog が別途担うため、ここでは Esc を無効化しない）。
  const dialogRef = useModalA11y<HTMLDivElement>({ onEscape: onCancel });

  const canSquash = selected.length >= 2;
  const canPlan = selected.length >= 2;
  const canReword = selected.length >= 1;
  const trimmed = message.trim();
  const planRewordsFilled = rows.every(
    (r) => r.action !== "reword" || r.message.trim().length > 0,
  );
  const canRun =
    mode === "plan"
      ? canPlan && planRewordsFilled && planPreview !== null && planError === null
      : trimmed.length > 0 && (mode === "squash" ? canSquash : canReword);

  function run() {
    if (!canRun) return;
    if (mode === "plan") {
      onRunPlan(plan);
    } else if (mode === "squash") {
      // squash は新しい順の oid 列を渡す（core 側が HEAD からの連続性を検証する）。
      onSquash(
        selected.map((c) => c.id),
        trimmed,
      );
    } else {
      onReword(trimmed);
    }
  }

  return (
    <div className="overlay" role="dialog" aria-modal="true" aria-labelledby={titleId}>
      <div className="dialog" ref={dialogRef}>
        <div className="dialog-head">
          <h2 id={titleId}>コミット履歴の整理（<Term k="rebase">リベース</Term>）</h2>
        </div>

        <section className="explain">
          <p className="explain-what">
            選んだコミットをまとめたり（squash）、メッセージを書き換えたり（reword）、
            順番を入れ替えたり、取り除いたり（drop）できます。
            まだプッシュ（送信）していないコミットに対して行うのが安全です。
          </p>
        </section>

        {/* モード選択 */}
        <div className="rebase-modes">
          <label className={mode === "squash" ? "rebase-mode active" : "rebase-mode"}>
            <input
              type="radio"
              name="rebase-mode"
              checked={mode === "squash"}
              disabled={!canSquash}
              onChange={() => switchMode("squash")}
            />
            <span>まとめる（squash）</span>
          </label>
          <label className={mode === "reword" ? "rebase-mode active" : "rebase-mode"}>
            <input
              type="radio"
              name="rebase-mode"
              checked={mode === "reword"}
              disabled={!canReword}
              onChange={() => switchMode("reword")}
            />
            <span>メッセージを書き換える（reword）</span>
          </label>
          <label className={mode === "plan" ? "rebase-mode active" : "rebase-mode"}>
            <input
              type="radio"
              name="rebase-mode"
              checked={mode === "plan"}
              disabled={!canPlan}
              onChange={() => setMode("plan")}
            />
            <span>並べ替え・削除（drop）</span>
          </label>
        </div>

        {mode === "plan" && (
          <section className="rebase-targets" data-testid="rebase-plan">
            <h3>手順（新しいコミットが上）</h3>
            <p className="rebase-hint">
              上下ボタンで順番を入れ替え、操作を選びます。「削除」を選んだコミットの変更は履歴から消えます。
              「下にまとめる」は、すぐ下（古い方）の残るコミットに取り込みます。
            </p>
            <ul className="rebase-plan-list">
              {rows.map((r, i) => (
                <li
                  key={r.commit.id}
                  className={
                    r.action === "drop" ? "rebase-plan-row is-dropped" : "rebase-plan-row"
                  }
                >
                  <span className="rebase-plan-move">
                    <button
                      type="button"
                      className="btn btn-small"
                      aria-label={`「${r.commit.summary}」を上へ移動`}
                      disabled={i === 0}
                      onClick={() => moveRow(i, -1)}
                    >
                      <Icon name="moveUp" />
                    </button>
                    <button
                      type="button"
                      className="btn btn-small"
                      aria-label={`「${r.commit.summary}」を下へ移動`}
                      disabled={i === rows.length - 1}
                      onClick={() => moveRow(i, 1)}
                    >
                      <Icon name="moveDown" />
                    </button>
                  </span>
                  <code className="sha">{r.commit.short_id}</code>
                  <span className="rebase-commit-summary">
                    {r.commit.summary || "(メッセージなし)"}
                  </span>
                  <label className="rebase-plan-drop">
                    <input
                      type="checkbox"
                      checked={r.action === "drop"}
                      onChange={(e) =>
                        updateRow(i, { action: e.target.checked ? "drop" : "pick" })
                      }
                    />{" "}
                    削除
                  </label>
                  <select
                    aria-label={`「${r.commit.summary}」の操作`}
                    value={r.action}
                    onChange={(e) => updateRow(i, { action: e.target.value as PlanAction })}
                  >
                    <option value="pick">そのまま</option>
                    <option value="reword">メッセージを書き換える</option>
                    <option value="squash">下にまとめる</option>
                    <option value="drop">削除（drop）</option>
                  </select>
                  {r.action === "reword" && (
                    <input
                      type="text"
                      className="rebase-plan-reword"
                      aria-label="新しいメッセージ"
                      value={r.message}
                      onChange={(e) => updateRow(i, { message: e.target.value })}
                    />
                  )}
                </li>
              ))}
            </ul>

            {planError ? (
              <p className="rebase-hint rebase-plan-error" role="alert">
                <Icon name="warning" /> {planError}
              </p>
            ) : planPreview && planPreview.kind === "rebase_plan" ? (
              <div className="rebase-compare" data-testid="rebase-plan-preview">
                <div>
                  <h4>変更前</h4>
                  <ul className="rebase-commit-list">
                    {planPreview.before.map((c) => (
                      <li key={c.id}>
                        <code className="sha">{c.short_id}</code>
                        <span className="rebase-commit-summary">{c.summary}</span>
                      </li>
                    ))}
                  </ul>
                </div>
                <div>
                  <h4>変更後</h4>
                  <ul className="rebase-commit-list">
                    {planPreview.after.map((c) => (
                      <li key={c.id}>
                        <code className="sha">{c.short_id}</code>
                        <span className="rebase-commit-summary">{c.summary}</span>
                      </li>
                    ))}
                  </ul>
                </div>
              </div>
            ) : null}
          </section>
        )}

        {/* 対象コミットの一覧 */}
        {mode !== "plan" && (
        <section className="rebase-targets">
          <h3>
            {mode === "squash"
              ? `まとめる対象（${selected.length} 個 → 1 個）`
              : "書き換える対象"}
          </h3>
          {mode === "reword" && selected.length !== 1 ? (
            <p className="rebase-hint">
              メッセージの書き換えは、最新のコミットを1つだけ選んでください。
            </p>
          ) : (
            <ul className="rebase-commit-list">
              {(mode === "reword" ? selected.slice(0, 1) : selected).map((c) => (
                <li key={c.id}>
                  <code className="sha">{c.short_id}</code>
                  <span className="rebase-commit-summary">
                    {c.summary || "(メッセージなし)"}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </section>
        )}

        {/* メッセージ入力 */}
        {mode !== "plan" && (
        <section className="rebase-message">
          <h3>
            {mode === "squash"
              ? "まとめた後のメッセージ"
              : "新しいメッセージ"}
          </h3>
          <textarea
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            placeholder="このコミットで何をしたか書きましょう"
            rows={4}
          />
        </section>
        )}

        <div className="dialog-actions">
          <button className="btn" onClick={onCancel}>
            やめておく
          </button>
          <button
            className="btn btn-confirm risk-destructive"
            onClick={run}
            disabled={!canRun}
          >
            実行する
          </button>
        </div>
      </div>
    </div>
  );
}
