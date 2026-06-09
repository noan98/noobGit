import { useMemo, useState } from "react";
import type { CommitInfo } from "../api";

// リベースの種類。squash は複数コミットを1つにまとめる、reword は1つのメッセージを書き換える。
type RebaseMode = "squash" | "reword";

interface Props {
  // 選択されたコミット。HistoryPanel で新しい順（先頭が HEAD に近い）に並んでいる前提。
  selected: CommitInfo[];
  // 実行ハンドラ。squash は対象 oid 列（新しい順）とメッセージ、reword はメッセージのみ。
  onSquash: (commitOids: string[], message: string) => void;
  onReword: (message: string) => void;
  onCancel: () => void;
}

// squash/reword を選んで実行する簡易ウィザード。
// 並び替えはスコープ外（squash と reword のみ）。離れたコミットの reorder は扱わない。
export function RebaseWizard({ selected, onSquash, onReword, onCancel }: Props) {
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

  const canSquash = selected.length >= 2;
  const canReword = selected.length >= 1;
  const trimmed = message.trim();
  const canRun =
    trimmed.length > 0 && (mode === "squash" ? canSquash : canReword);

  function run() {
    if (!canRun) return;
    if (mode === "squash") {
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
    <div className="overlay" role="dialog" aria-modal="true">
      <div className="dialog">
        <div className="dialog-head">
          <h2>コミット履歴の整理（リベース）</h2>
        </div>

        <section className="explain">
          <p className="explain-what">
            選んだコミットをまとめたり（squash）、メッセージを書き換えたり（reword）できます。
            まだ送信（push）していないコミットに対して行うのが安全です。
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
        </div>

        {/* 対象コミットの一覧 */}
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

        {/* メッセージ入力 */}
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
