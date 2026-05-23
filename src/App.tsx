import { useCallback, useEffect, useState } from "react";
import {
  api,
  type BranchInfo,
  type CommitInfo,
  type Explanation,
  type OperationKind,
  type RepoStatus,
  type RiskAssessment,
  type UndoEntry,
} from "./api";
import { StatusPanel } from "./components/StatusPanel";
import { HistoryPanel } from "./components/HistoryPanel";
import { BranchPanel } from "./components/BranchPanel";
import { ConfirmDialog } from "./components/ConfirmDialog";

interface Guard {
  title: string;
  assessment: RiskAssessment;
  explanation: Explanation;
  action: () => Promise<void>;
}

export default function App() {
  const [repoPath, setRepoPath] = useState("");
  const [opened, setOpened] = useState(false);

  const [status, setStatus] = useState<RepoStatus | null>(null);
  const [branches, setBranches] = useState<BranchInfo[]>([]);
  const [commits, setCommits] = useState<CommitInfo[]>([]);
  const [undoInfo, setUndoInfo] = useState<UndoEntry | null>(null);

  const [commitMsg, setCommitMsg] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [guard, setGuard] = useState<Guard | null>(null);

  const refresh = useCallback(async () => {
    if (!repoPath) return;
    try {
      const [st, br, lg, undo] = await Promise.all([
        api.getStatus(repoPath),
        api.getBranches(repoPath),
        api.getLog(repoPath, 50),
        api.peekUndo(repoPath),
      ]);
      setStatus(st);
      setBranches(br);
      setCommits(lg);
      setUndoInfo(undo);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [repoPath]);

  useEffect(() => {
    if (opened) void refresh();
  }, [opened, refresh]);

  async function openRepo() {
    if (!repoPath.trim()) return;
    setOpened(true);
    setNotice(null);
    await refresh();
  }

  // 安全な操作はそのまま実行し、結果を更新する。
  const exec = useCallback(
    async (action: () => Promise<void>, successMsg?: string) => {
      try {
        await action();
        setError(null);
        if (successMsg) setNotice(successMsg);
        await refresh();
      } catch (e) {
        setError(String(e));
      }
    },
    [refresh],
  );

  // リスクを評価し、危険なら確認ダイアログを挟んでから実行する。
  async function guarded(
    title: string,
    op: OperationKind,
    action: () => Promise<void>,
    targetBranch?: string,
  ) {
    try {
      const [assessment, explanation] = await Promise.all([
        api.assess(repoPath, op, targetBranch),
        api.explain(op),
      ]);
      if (assessment.level === "safe") {
        await exec(action);
      } else {
        setGuard({ title, assessment, explanation, action });
      }
    } catch (e) {
      setError(String(e));
    }
  }

  async function confirmGuard() {
    if (!guard) return;
    const action = guard.action;
    setGuard(null);
    await exec(action);
  }

  function doCommit() {
    const msg = commitMsg.trim();
    if (!msg) return;
    void exec(async () => {
      await api.commit(repoPath, msg);
      setCommitMsg("");
    }, "コミットしました。");
  }

  function doUndo() {
    void exec(async () => {
      const desc = await api.undoLast(repoPath);
      setNotice(`取り消しました: ${desc}`);
    });
  }

  if (!opened) {
    return (
      <div className="welcome">
        <h1>noobGit</h1>
        <p className="tagline">ジュニアエンジニアが安心して使えるGitツール</p>
        <div className="open-box">
          <input
            value={repoPath}
            placeholder="Gitリポジトリのフォルダパスを入力 (例: C:\\Users\\you\\project)"
            onChange={(e) => setRepoPath(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && openRepo()}
          />
          <button className="btn" onClick={openRepo}>
            開く
          </button>
        </div>
        {error && <p className="error">{error}</p>}
      </div>
    );
  }

  return (
    <div className="app">
      <header className="topbar">
        <div className="repo-info">
          <strong>noobGit</strong>
          <span className="current-branch">
            {status?.branch ? `🌿 ${status.branch}` : "(ブランチ不明)"}
          </span>
        </div>
        <div className="topbar-actions">
          {undoInfo && (
            <button className="btn btn-undo" onClick={doUndo}>
              ↩ 取り消す: {undoInfo.description}
            </button>
          )}
          <button className="btn btn-small" onClick={() => void refresh()}>
            更新
          </button>
          <button
            className="btn btn-small"
            onClick={() => {
              setOpened(false);
              setStatus(null);
            }}
          >
            別のリポジトリ
          </button>
        </div>
      </header>

      {error && (
        <div className="banner error" onClick={() => setError(null)}>
          {error}
        </div>
      )}
      {notice && (
        <div className="banner notice" onClick={() => setNotice(null)}>
          {notice}
        </div>
      )}

      <main className="columns">
        <section className="col">
          {status && (
            <StatusPanel
              status={status}
              onStageAll={() => void exec(() => api.stageAll(repoPath))}
              onStagePath={(p) => void exec(() => api.stagePath(repoPath, p))}
              onUnstage={(p) => void exec(() => api.unstage(repoPath, p))}
            />
          )}

          <div className="panel commit-box">
            <h2>コミット</h2>
            <textarea
              value={commitMsg}
              placeholder="このコミットで何をしたか書きましょう（例: ログイン画面を追加）"
              onChange={(e) => setCommitMsg(e.target.value)}
            />
            <button
              className="btn"
              onClick={doCommit}
              disabled={!commitMsg.trim()}
            >
              コミットする
            </button>
          </div>
        </section>

        <section className="col">
          <HistoryPanel commits={commits} />
        </section>

        <section className="col">
          <BranchPanel
            branches={branches}
            onCreate={(name) =>
              void guarded("ブランチを作成", "create_branch", () =>
                api.createBranch(repoPath, name),
              )
            }
            onSwitch={(name) =>
              void guarded(
                `ブランチ「${name}」へ切り替え`,
                "switch_branch",
                () => api.switchBranch(repoPath, name),
                name,
              )
            }
            onDelete={(name) =>
              void guarded(
                `ブランチ「${name}」を削除`,
                "delete_branch",
                () => api.deleteBranch(repoPath, name),
                name,
              )
            }
          />
        </section>
      </main>

      {guard && (
        <ConfirmDialog
          title={guard.title}
          assessment={guard.assessment}
          explanation={guard.explanation}
          onConfirm={() => void confirmGuard()}
          onCancel={() => setGuard(null)}
        />
      )}
    </div>
  );
}
