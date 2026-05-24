import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  type BranchGraph,
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

// 履歴の初期表示件数。初回表示を軽くするため小さめにし、「もっと見る」で追記する。
const LOG_PAGE_SIZE = 30;

// 再取得する範囲。操作の性質に応じて必要な部分だけを真にして冗長な I/O を避ける。
interface RefreshParts {
  status?: boolean;
  branches?: boolean;
  log?: boolean;
  undo?: boolean;
}

// リポジトリを開いた直後や手動更新で使う全件再取得。
const FULL_REFRESH: RefreshParts = {
  status: true,
  branches: true,
  log: true,
  undo: true,
};

// 各操作が画面のどの部分に影響するか。これに載っていない部分は再取得しない。
// undo はどの書き込み操作でも履歴エントリが変わるため常に含める。
const REFRESH_BY_OP: Record<OperationKind, RefreshParts> = {
  // ステージ系は作業ツリーの状態だけが変わる。
  stage: { status: true, undo: true },
  unstage: { status: true, undo: true },
  // コミットは status（ステージ消化）と log（新コミット）に効く。HEAD が動くので
  // ブランチ関係（取り込み済み判定・ahead/behind）も変わるため branches も更新する。
  commit: { status: true, branches: true, log: true, undo: true },
  // 作成はブランチ一覧だけ。HEAD も作業ツリーも動かさない。
  create_branch: { branches: true, undo: true },
  // 切り替えは HEAD が動くので作業ツリー・ブランチ・履歴すべてが変わりうる。
  switch_branch: FULL_REFRESH,
  // 削除はブランチ一覧だけ。
  delete_branch: { branches: true, undo: true },
  // ハードリセットは HEAD が動くので status・log とブランチ関係が変わる。
  reset_hard: { status: true, branches: true, log: true, undo: true },
  // 以下は現状 UI から呼ばれないが、型を網羅させるため安全側（全件）にしておく。
  pull: FULL_REFRESH,
  push: FULL_REFRESH,
  force_push: FULL_REFRESH,
  merge: FULL_REFRESH,
};

interface Guard {
  title: string;
  assessment: RiskAssessment;
  explanation: Explanation;
  action: () => Promise<void>;
  refresh: RefreshParts;
}

export default function App() {
  const [repoPath, setRepoPath] = useState("");
  const [opened, setOpened] = useState(false);

  const [status, setStatus] = useState<RepoStatus | null>(null);
  const [branches, setBranches] = useState<BranchInfo[]>([]);
  const [branchGraph, setBranchGraph] = useState<BranchGraph | null>(null);
  const [commits, setCommits] = useState<CommitInfo[]>([]);
  const [hasMoreCommits, setHasMoreCommits] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [undoInfo, setUndoInfo] = useState<UndoEntry | null>(null);

  // 現在読み込み済みのコミット件数。再取得時に「もっと見る」で広げた範囲を保つため、
  // クロージャの陳腐化を避けて常に最新値を参照できるよう ref で持つ。
  const loadedCount = useRef(0);
  useEffect(() => {
    loadedCount.current = commits.length;
  }, [commits]);

  const [commitMsg, setCommitMsg] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [guard, setGuard] = useState<Guard | null>(null);

  const refresh = useCallback(
    async (parts: RefreshParts = FULL_REFRESH): Promise<boolean> => {
      if (!repoPath) return false;
      try {
        const tasks: Promise<unknown>[] = [];
        if (parts.status) tasks.push(api.getStatus(repoPath).then(setStatus));
        if (parts.branches) {
          tasks.push(api.getBranches(repoPath).then(setBranches));
          tasks.push(api.getBranchGraph(repoPath).then(setBranchGraph));
        }
        if (parts.log) {
          // すでに「もっと見る」で広げていれば、その件数を保ったまま先頭から取り直す。
          const want = Math.max(LOG_PAGE_SIZE, loadedCount.current);
          tasks.push(
            api.getLog(repoPath, 0, want).then((cs) => {
              setCommits(cs);
              setHasMoreCommits(cs.length === want);
            }),
          );
        }
        if (parts.undo) tasks.push(api.peekUndo(repoPath).then(setUndoInfo));
        await Promise.all(tasks);
        setError(null);
        return true;
      } catch (e) {
        setError(String(e));
        return false;
      }
    },
    [repoPath],
  );

  useEffect(() => {
    if (opened) void refresh();
  }, [opened, refresh]);

  async function openRepo() {
    if (!repoPath.trim()) return;
    setNotice(null);
    const ok = await refresh();
    if (ok) setOpened(true);
  }

  // 安全な操作はそのまま実行し、結果を更新する。
  // refresh を省略した場合は全件再取得（取り消しなど影響範囲が読めない操作向け）。
  const exec = useCallback(
    async (
      action: () => Promise<void>,
      opts: { successMsg?: string; refresh?: RefreshParts } = {},
    ) => {
      try {
        await action();
        setError(null);
        if (opts.successMsg) setNotice(opts.successMsg);
        await refresh(opts.refresh ?? FULL_REFRESH);
      } catch (e) {
        setNotice(null);
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
      const parts = REFRESH_BY_OP[op];
      if (assessment.level === "safe") {
        await exec(action, { refresh: parts });
      } else {
        setGuard({ title, assessment, explanation, action, refresh: parts });
      }
    } catch (e) {
      setError(String(e));
    }
  }

  async function confirmGuard() {
    if (!guard) return;
    const { action, refresh: parts } = guard;
    setGuard(null);
    await exec(action, { refresh: parts });
  }

  function doCommit() {
    const msg = commitMsg.trim();
    if (!msg) return;
    void exec(
      async () => {
        await api.commit(repoPath, msg);
        setCommitMsg("");
      },
      { successMsg: "コミットしました。", refresh: REFRESH_BY_OP.commit },
    );
  }

  // 「もっと見る」: 末尾から次のページを読み、現在の一覧に追記する。
  function loadMore() {
    if (loadingMore || !repoPath) return;
    setLoadingMore(true);
    void (async () => {
      try {
        const more = await api.getLog(repoPath, commits.length, LOG_PAGE_SIZE);
        setCommits((prev) => [...prev, ...more]);
        setHasMoreCommits(more.length === LOG_PAGE_SIZE);
        setError(null);
      } catch (e) {
        setError(String(e));
      } finally {
        setLoadingMore(false);
      }
    })();
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
              // 次に開くリポジトリは初期件数から軽く表示し直す。
              setCommits([]);
              setHasMoreCommits(false);
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
              onStageAll={() =>
                void exec(() => api.stageAll(repoPath), {
                  refresh: REFRESH_BY_OP.stage,
                })
              }
              onStagePath={(p) =>
                void exec(() => api.stagePath(repoPath, p), {
                  refresh: REFRESH_BY_OP.stage,
                })
              }
              onUnstage={(p) =>
                void exec(() => api.unstage(repoPath, p), {
                  refresh: REFRESH_BY_OP.unstage,
                })
              }
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
          <HistoryPanel
            commits={commits}
            hasMore={hasMoreCommits}
            loadingMore={loadingMore}
            onLoadMore={loadMore}
            onReset={(c) =>
              void guarded(
                `「${c.short_id}」までハードリセット`,
                "reset_hard",
                () => api.resetHard(repoPath, c.id),
              )
            }
          />
        </section>

        <section className="col">
          <BranchPanel
            branches={branches}
            graph={branchGraph}
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
