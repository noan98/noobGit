/*
 * BisectWizard — バグ混入コミットを二分探索で見つけるウィザード (#184)。
 *
 * RebaseWizard / ConflictWizard と同じ overlay + dialog のパターンを使い、
 * ステップ遷移には OnboardingWizard と同じ framer-motion の fadeIn を使う。
 *
 * 画面は `status`（親が持つ core::model::BisectStatus）に応じて自動的に切り替わる:
 * - status が null              → Step1（壊れているコミット）/ Step2（動いていたコミット）の選択
 * - status.is_done が false     → Step3「このコミットでバグはありますか？」（はい/いいえ/中止）
 * - status.is_done が true      → 原因コミットと差分の表示、終了ボタン
 *
 * 「開始」「終了（中止）」は core のガード（未コミット変更のブロックなど）を経るため、
 * 親（RepoWorkspace）側で guarded() を通して呼ぶ。判定（はい/いいえ）はステージ相当の
 * 軽い操作として exec() 直呼びにする（親の設計判断）。
 */
import { useEffect, useId, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { api, type BisectStatus, type CommitInfo, type FileDiff } from "../api";
import { useModalA11y } from "../hooks/useModalA11y";
import { fadeIn } from "../theme/motion";
import { Icon } from "./Icon";
import { CommitDiffViewer } from "./CommitDiffViewer";

interface Props {
  repoPath: string;
  // 現在の Bisect セッション状態。null は未開始（Step1/Step2 の選択画面を出す）。
  status: BisectStatus | null;
  // Step1/Step2 の選択肢に使う、読み込み済みのコミット一覧（新しい順）。
  commits: CommitInfo[];
  // Bisect を開始する（bad/good は revspec: コミットID・ブランチ名など）。
  onStart: (bad: string, good: string) => void;
  // 現在のコミットについて good/bad を判定する。
  onMark: (commitId: string, isGood: boolean) => void;
  // Bisect セッションを終了し、元のブランチへ戻す。
  onReset: () => void;
  // ウィザードを閉じる（Bisect セッション自体は終了しない。バナーから再度開ける）。
  onClose: () => void;
}

function commitLabel(c: CommitInfo): string {
  return `${c.short_id} ${c.summary || "(メッセージなし)"}`;
}

export function BisectWizard({
  repoPath,
  status,
  commits,
  onStart,
  onMark,
  onReset,
  onClose,
}: Props) {
  const [step, setStep] = useState<"bad" | "good">("bad");
  const [badInput, setBadInput] = useState(commits[0]?.id ?? "");
  const [goodInput, setGoodInput] = useState("");

  const titleId = useId();
  // #151 フォーカストラップ + Esc キーで閉じる（セッション自体は終了しない）。
  const dialogRef = useModalA11y<HTMLDivElement>({ onEscape: onClose });

  // 完了時（is_done）の原因コミットの差分（親コミットとの比較）。found_commit が
  // 変わるたびに取り直す。非破壊な読み取りなので取得失敗はベストエフォートで無視する。
  const [foundDiffs, setFoundDiffs] = useState<FileDiff[] | null>(null);
  const [foundDiffsLoading, setFoundDiffsLoading] = useState(false);
  const foundId = status?.is_done ? (status.found_commit?.id ?? null) : null;
  useEffect(() => {
    if (!repoPath || !foundId) {
      setFoundDiffs(null);
      return;
    }
    let cancelled = false;
    setFoundDiffsLoading(true);
    void api
      .getDiffBetween(repoPath, null, foundId)
      .then((ds) => {
        if (!cancelled) setFoundDiffs(ds);
      })
      .catch(() => {
        if (!cancelled) setFoundDiffs([]);
      })
      .finally(() => {
        if (!cancelled) setFoundDiffsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [repoPath, foundId]);

  function startBisect() {
    const bad = badInput.trim();
    const good = goodInput.trim();
    if (!bad || !good) return;
    onStart(bad, good);
  }

  // ---- 未開始: Step1（壊れているコミット）/ Step2（動いていたコミット） ----
  if (!status) {
    return (
      <div className="overlay" role="dialog" aria-modal="true" aria-labelledby={titleId}>
        <div className="dialog bisect-wizard" ref={dialogRef}>
          <div className="dialog-head">
            <h2 id={titleId}>
              <Icon name="bisect" /> バグ混入コミットを探す（Bisect）
            </h2>
          </div>

          <section className="explain">
            <p className="explain-what">
              「壊れている」コミットと「動いていた」コミットを指定すると、その間のコミットを
              1つずつ自動でチェックアウトしながら、はい/いいえに答えるだけで
              バグが混入したコミットを二分探索で見つけます。
            </p>
            <p className="explain-why">
              調査中は一時的に「detached HEAD」（どのブランチも指さない状態）になります。
              ブランチは失われず、いつでも元に戻せます。
            </p>
          </section>

          <AnimatePresence mode="wait">
            {step === "bad" ? (
              <motion.div
                key="bad"
                variants={fadeIn}
                initial="hidden"
                animate="visible"
                exit="exit"
                className="bisect-step"
              >
                <h3>Step 1: 壊れているコミット</h3>
                <p className="bisect-hint">
                  いまバグが起きている（動いていない）コミットを選びます。既定では最新のコミット（HEAD）です。
                </p>
                <select
                  className="bisect-select"
                  value={commits.some((c) => c.id === badInput) ? badInput : ""}
                  onChange={(e) => setBadInput(e.target.value)}
                  aria-label="壊れているコミットを一覧から選ぶ"
                >
                  <option value="">履歴から選ぶ…</option>
                  {commits.map((c) => (
                    <option key={c.id} value={c.id}>
                      {commitLabel(c)}
                    </option>
                  ))}
                </select>
                <input
                  type="text"
                  className="bisect-manual-input"
                  placeholder="または、コミットのハッシュ・ブランチ名を直接入力"
                  value={badInput}
                  onChange={(e) => setBadInput(e.target.value)}
                  aria-label="壊れているコミットを直接入力"
                />
              </motion.div>
            ) : (
              <motion.div
                key="good"
                variants={fadeIn}
                initial="hidden"
                animate="visible"
                exit="exit"
                className="bisect-step"
              >
                <h3>Step 2: 動いていたコミット</h3>
                <p className="bisect-hint">
                  まだバグが起きていなかった（正常に動いていた）ころのコミットを選びます。
                  壊れているコミットより前のものを選んでください。
                </p>
                <select
                  className="bisect-select"
                  value={commits.some((c) => c.id === goodInput) ? goodInput : ""}
                  onChange={(e) => setGoodInput(e.target.value)}
                  aria-label="動いていたコミットを一覧から選ぶ"
                >
                  <option value="">履歴から選ぶ…</option>
                  {commits.map((c) => (
                    <option key={c.id} value={c.id}>
                      {commitLabel(c)}
                    </option>
                  ))}
                </select>
                <input
                  type="text"
                  className="bisect-manual-input"
                  placeholder="または、コミットのハッシュ・ブランチ名を直接入力"
                  value={goodInput}
                  onChange={(e) => setGoodInput(e.target.value)}
                  aria-label="動いていたコミットを直接入力"
                />
              </motion.div>
            )}
          </AnimatePresence>

          <div className="dialog-actions">
            <button className="btn" onClick={onClose}>
              やめておく
            </button>
            {step === "bad" ? (
              <button
                className="btn btn-confirm risk-caution"
                onClick={() => setStep("good")}
                disabled={!badInput.trim()}
              >
                次へ <Icon name="chevronRight" />
              </button>
            ) : (
              <>
                <button className="btn btn-small" onClick={() => setStep("bad")}>
                  戻る
                </button>
                <button
                  className="btn btn-confirm risk-caution"
                  onClick={startBisect}
                  disabled={!goodInput.trim()}
                >
                  <Icon name="bisect" /> Bisect を開始
                </button>
              </>
            )}
          </div>
        </div>
      </div>
    );
  }

  // ---- 完了: 原因コミットと差分の表示 ----
  if (status.is_done) {
    const found = status.found_commit;
    return (
      <div className="overlay" role="dialog" aria-modal="true" aria-labelledby={titleId}>
        <div className="dialog bisect-wizard bisect-wizard-done" ref={dialogRef}>
          <div className="dialog-head">
            <h2 id={titleId}>
              <Icon name="check" /> 原因コミットが見つかりました
            </h2>
          </div>

          {found && (
            <div className="bisect-commit-card bisect-found">
              <code className="sha">{found.short_id}</code>
              <span className="bisect-commit-summary">
                {found.summary || "(メッセージなし)"}
              </span>
              <div className="bisect-commit-meta">
                {found.author_name} ・{" "}
                {new Date(found.time * 1000).toLocaleString("ja-JP")}
              </div>
            </div>
          )}

          <p className="bisect-hint">
            {status.tested_count} 回の判定で、このコミットが最初にバグを持ち込んだと特定できました。
          </p>

          {found && (
            <CommitDiffViewer
              base={null}
              target={found}
              diffs={foundDiffs}
              loading={foundDiffsLoading}
              onClose={onClose}
            />
          )}

          <div className="dialog-actions">
            <button className="btn btn-confirm risk-caution" onClick={onReset}>
              <Icon name="branchSwitch" /> Bisect を終了して元のブランチへ戻る
            </button>
          </div>
        </div>
      </div>
    );
  }

  // ---- 進行中: Step3「このコミットでバグはありますか？」 ----
  const current = status.current_commit;
  return (
    <div className="overlay" role="dialog" aria-modal="true" aria-labelledby={titleId}>
      <div className="dialog bisect-wizard" ref={dialogRef}>
        <div className="dialog-head">
          <h2 id={titleId}>
            <Icon name="bisect" /> このコミットでバグはありますか？
          </h2>
        </div>

        <p className="bisect-progress">
          判定 {status.tested_count} 回目 ・ 残り約 {status.remaining_steps} 回
        </p>

        {current && (
          <div className="bisect-commit-card">
            <code className="sha">{current.short_id}</code>
            <span className="bisect-commit-summary">
              {current.summary || "(メッセージなし)"}
            </span>
            <div className="bisect-commit-meta">
              {current.author_name} ・{" "}
              {new Date(current.time * 1000).toLocaleString("ja-JP")}
            </div>
          </div>
        )}

        <p className="bisect-hint">
          いま作業ツリーはこのコミットの内容になっています。動作を確認して、
          バグがあるかどうかを答えてください。
        </p>

        <div className="dialog-actions bisect-answer-actions">
          <button
            className="btn btn-small"
            onClick={onClose}
            title="ウィザードを閉じます（Bisect セッションは続けたままです）"
          >
            閉じる
          </button>
          <button className="btn" onClick={onReset}>
            中止する
          </button>
          <button
            className="btn btn-confirm risk-safe"
            onClick={() => current && onMark(current.id, true)}
            disabled={!current}
          >
            いいえ（動いていた）
          </button>
          <button
            className="btn btn-confirm risk-destructive"
            onClick={() => current && onMark(current.id, false)}
            disabled={!current}
          >
            はい（バグがある）
          </button>
        </div>
      </div>
    </div>
  );
}
