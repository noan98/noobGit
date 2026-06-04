import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  type BranchGraph,
  type BranchInfo,
  type CommitInfo,
  type Explanation,
  type FileDiff,
  type Identity,
  type IdentityScope,
  type OperationKind,
  type RepoStatus,
  type RiskAssessment,
  type StashInfo,
  type UndoEntry,
} from "./api";
import { StatusPanel } from "./components/StatusPanel";
import { StashPanel } from "./components/StashPanel";
import { HistoryPanel } from "./components/HistoryPanel";
import { BranchPanel } from "./components/BranchPanel";
import { ConfirmDialog } from "./components/ConfirmDialog";
import {
  DiffPanel,
  type DiffSelection,
  type DiffSource,
} from "./components/DiffPanel";
import { IdentityDialog } from "./components/IdentityDialog";
import { ThemeToggle } from "./components/ThemeToggle";

// 履歴の初期表示件数。初回表示を軽くするため小さめにし、「もっと見る」で追記する。
const LOG_PAGE_SIZE = 30;

// 取得・取り込みの既定リモート名。多くのリポジトリはクローン元を origin と呼ぶ。
const DEFAULT_REMOTE = "origin";

// 再取得する範囲。操作の性質に応じて必要な部分だけを真にして冗長な I/O を避ける。
interface RefreshParts {
  status?: boolean;
  branches?: boolean;
  log?: boolean;
  undo?: boolean;
  stash?: boolean;
}

// リポジトリを開いた直後や手動更新で使う全件再取得。
const FULL_REFRESH: RefreshParts = {
  status: true,
  branches: true,
  log: true,
  undo: true,
  stash: true,
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
  // amend は直前コミットを作り直す。status・log・ブランチ関係が変わり、undo も積まれる。
  amend_commit: { status: true, branches: true, log: true, undo: true },
  // 破棄は作業ツリーの状態だけが変わる（undo は記録しない）。
  discard: { status: true },
  // 退避は作業ツリーがクリーンになり、退避一覧と undo が変わる。
  stash_save: { status: true, undo: true, stash: true },
  // 適用は作業ツリーへ取り出すだけ（退避は一覧に残る）。
  stash_apply: { status: true },
  // 取り出し（pop）は作業ツリーに戻し、退避一覧から消える。
  stash_pop: { status: true, stash: true },
  // 作成はブランチ一覧だけ。HEAD も作業ツリーも動かさない。
  create_branch: { branches: true, undo: true },
  // 切り替えは HEAD が動くので作業ツリー・ブランチ・履歴すべてが変わりうる。
  switch_branch: FULL_REFRESH,
  // 削除はブランチ一覧だけ。
  delete_branch: { branches: true, undo: true },
  // ハードリセットは HEAD が動くので status・log とブランチ関係が変わる。
  reset_hard: { status: true, branches: true, log: true, undo: true },
  // fetch はリモート追跡ブランチを更新するだけ（作業ツリー・HEAD は不変）。
  fetch: { branches: true },
  // pull（FF）は作業ツリー・HEAD・ブランチ関係が動くので全件。
  pull: FULL_REFRESH,
  // push はリモートを更新するだけでローカルの作業ツリーや履歴は動かない。リモート追跡や
  // upstream 表示が変わりうるのでブランチ情報だけ取り直す。
  push: { branches: true },
  force_push: { branches: true },
  merge: FULL_REFRESH,
};

interface Guard {
  title: string;
  assessment: RiskAssessment;
  explanation: Explanation;
  action: () => Promise<void>;
  refresh: RefreshParts;
  // ネットワーク操作の場合 true。確認ダイアログ経由で exec を呼ぶときに isNetworkBusy を立てる。
  networkOp?: boolean;
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
  const [stashes, setStashes] = useState<StashInfo[]>([]);

  // 現在読み込み済みのコミット件数。再取得時に「もっと見る」で広げた範囲を保つため、
  // クロージャの陳腐化を避けて常に最新値を参照できるよう ref で持つ。
  const loadedCount = useRef(0);
  useEffect(() => {
    loadedCount.current = commits.length;
  }, [commits]);

  const [commitMsg, setCommitMsg] = useState("");
  // コミット入力欄への参照。履歴が空のときの「コミットへ」誘導でフォーカスする。
  const commitInput = useRef<HTMLTextAreaElement>(null);
  const [error, setError] = useState<string | null>(null);
  // ネットワーク操作（fetch / pull / push）の実行中フラグ。
  // true の間は fetch / pull / push ボタンを無効化して二重実行を防ぐ。
  const [isNetworkBusy, setIsNetworkBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [guard, setGuard] = useState<Guard | null>(null);

  // 差分プレビュー: 選択中ファイルと、その差分。
  const [selectedFile, setSelectedFile] = useState<DiffSelection | null>(null);
  const [diff, setDiff] = useState<FileDiff | null>(null);
  const [diffLoading, setDiffLoading] = useState(false);

  // 初回セットアップ用の identity 状態。null は未取得、name/email が揃えば設定済み。
  const [identity, setIdentity] = useState<Identity | null>(null);
  const [showIdentity, setShowIdentity] = useState(false);
  const identityComplete = !!(identity && identity.name && identity.email);

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
        if (parts.stash) tasks.push(api.getStashes(repoPath).then(setStashes));
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

  // identity の取得は補助的なので、失敗しても画面表示は止めない（バナーで案内に倒す）。
  const loadIdentity = useCallback(async () => {
    if (!repoPath) return;
    try {
      setIdentity(await api.getIdentity(repoPath));
    } catch {
      setIdentity(null);
    }
  }, [repoPath]);

  useEffect(() => {
    if (opened) {
      void refresh();
      void loadIdentity();
    }
  }, [opened, refresh, loadIdentity]);

  // 選択中ファイルの差分を取得する。参照元（ステージ済み / 未ステージ /
  // コンフリクト）で呼ぶコマンドが変わる。
  const loadDiff = useCallback(
    async (sel: DiffSelection | null) => {
      if (!repoPath || !sel) {
        setDiff(null);
        return;
      }
      setDiffLoading(true);
      try {
        const d =
          sel.source === "staged"
            ? await api.getDiffStaged(repoPath, sel.path)
            : sel.source === "conflicted"
              ? await api.getDiffConflict(repoPath, sel.path)
              : await api.getDiffUnstaged(repoPath, sel.path);
        setDiff(d);
        setError(null);
      } catch (e) {
        setDiff(null);
        setError(String(e));
      } finally {
        setDiffLoading(false);
      }
    },
    [repoPath],
  );

  // 選択が変わったとき、または status の再取得で変更内容が変わったときに差分を取り直す。
  useEffect(() => {
    void loadDiff(selectedFile);
  }, [selectedFile, status, loadDiff]);

  // ファイル名クリックで選択。同じものを再クリックしたら選択解除。
  const selectFile = useCallback((path: string, source: DiffSource) => {
    setSelectedFile((cur) =>
      cur && cur.path === path && cur.source === source
        ? null
        : { path, source },
    );
  }, []);

  async function openRepo() {
    if (!repoPath.trim()) return;
    setNotice(null);
    const ok = await refresh();
    if (ok) setOpened(true);
  }

  async function saveIdentity(
    name: string,
    email: string,
    scope: IdentityScope,
  ) {
    try {
      await api.setIdentity(repoPath, name, email, scope);
      setIdentity(await api.getIdentity(repoPath));
      setShowIdentity(false);
      setError(null);
      setNotice(
        scope === "global"
          ? "名前とメールを設定しました（このPC全体）。"
          : "名前とメールを設定しました（このリポジトリ）。",
      );
    } catch (e) {
      setError(String(e));
    }
  }

  // 安全な操作はそのまま実行し、結果を更新する。
  // refresh を省略した場合は全件再取得（取り消しなど影響範囲が読めない操作向け）。
  // networkOp: true を渡すと実行中に isNetworkBusy を立て、完了・失敗時に必ず下ろす。
  const exec = useCallback(
    async (
      action: () => Promise<void>,
      opts: { successMsg?: string; refresh?: RefreshParts; networkOp?: boolean } = {},
    ) => {
      if (opts.networkOp) setIsNetworkBusy(true);
      try {
        await action();
        setError(null);
        if (opts.successMsg) setNotice(opts.successMsg);
        await refresh(opts.refresh ?? FULL_REFRESH);
      } catch (e) {
        setNotice(null);
        setError(String(e));
      } finally {
        if (opts.networkOp) setIsNetworkBusy(false);
      }
    },
    [refresh],
  );

  // リスクを評価し、危険なら確認ダイアログを挟んでから実行する。
  // networkOp: true を渡すとネットワーク操作として isNetworkBusy を管理する。
  async function guarded(
    title: string,
    op: OperationKind,
    action: () => Promise<void>,
    targetBranch?: string,
    networkOp?: boolean,
  ) {
    try {
      const [assessment, explanation] = await Promise.all([
        api.assess(repoPath, op, targetBranch),
        api.explain(op),
      ]);
      const parts = REFRESH_BY_OP[op];
      if (assessment.level === "safe") {
        await exec(action, { refresh: parts, networkOp });
      } else {
        setGuard({ title, assessment, explanation, action, refresh: parts, networkOp });
      }
    } catch (e) {
      setError(String(e));
    }
  }

  async function confirmGuard() {
    if (!guard) return;
    const { action, refresh: parts, networkOp } = guard;
    setGuard(null);
    await exec(action, { refresh: parts, networkOp });
  }

  function doCommit() {
    const msg = commitMsg.trim();
    if (!msg) return;
    // 名前・メール未設定のままコミットすると失敗するので、先にセットアップへ案内する。
    if (!identityComplete) {
      setError(null);
      setNotice("コミットの前に、名前とメールアドレスを設定しましょう。");
      setShowIdentity(true);
      return;
    }
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

  // 取得（fetch）: リモートの最新情報だけを取り込む安全操作。確認なしで実行する。
  function doFetch() {
    void exec(
      async () => {
        const outcome = await api.fetch(repoPath, DEFAULT_REMOTE);
        setNotice(
          outcome.updated_refs > 0
            ? `リモート「${outcome.remote}」から最新情報を取得しました（追跡ブランチ ${outcome.updated_refs} 件を更新）。`
            : `リモート「${outcome.remote}」を確認しました。新しい変更はありませんでした。`,
        );
      },
      { refresh: REFRESH_BY_OP.fetch, networkOp: true },
    );
  }

  // 取り込み（pull）: fetch 後、fast-forward できるときだけ取り込む。pull は注意操作
  // なので guarded を通し、確認ダイアログを挟む。分岐して取り込めない場合はエラー表示。
  function doPull() {
    const branch = status?.branch;
    if (!branch) {
      setError(
        "現在のブランチが特定できないため取り込めません（detached HEAD の可能性があります）。",
      );
      return;
    }
    void guarded(
      "リモートから取り込む",
      "pull",
      async () => {
        const outcome = await api.pull(repoPath, DEFAULT_REMOTE, branch);
        setNotice(
          outcome.kind === "up_to_date"
            ? "すでに最新の状態でした。取り込むものはありません。"
            : `リモートの変更を取り込みました（${outcome.commit.short_id} まで前進）。`,
        );
      },
      branch,
      true, // networkOp
    );
  }

  // 変更の破棄。元に戻せない破壊的操作なので必ず guarded を通す。
  function doDiscard(path: string) {
    void guarded(`「${path}」の変更を破棄`, "discard", () =>
      api.discardPath(repoPath, path),
    );
  }

  // 直前のコミットを修正（amend）。コミットと同様に名前・メール未設定なら先に案内する。
  function doAmend() {
    if (commits.length === 0) return;
    if (!identityComplete) {
      setError(null);
      setNotice("コミットの修正の前に、名前とメールアドレスを設定しましょう。");
      setShowIdentity(true);
      return;
    }
    const msg = commitMsg.trim();
    void guarded("直前のコミットを修正", "amend_commit", async () => {
      await api.amendCommit(repoPath, msg);
      setCommitMsg("");
      setNotice("直前のコミットを修正しました。");
    });
  }

  // 退避（保存）。安全操作なので guarded はダイアログを出さずそのまま実行する。
  function doStashSave(message: string) {
    void guarded("変更を退避", "stash_save", async () => {
      await api.stashSave(repoPath, message);
      setNotice("変更を退避しました。作業ツリーをきれいにしました。");
    });
  }

  // 退避の適用（一覧に残す）。コンフリクトの可能性があるため guarded を通す。
  function doStashApply(index: number) {
    void guarded("退避を適用", "stash_apply", async () => {
      await api.stashApply(repoPath, index);
      setNotice("退避した変更を取り出しました（退避は一覧に残しています）。");
    });
  }

  // 退避の取り出し（pop・一覧から削除）。コンフリクトの可能性があるため guarded を通す。
  function doStashPop(index: number) {
    void guarded("退避を取り出す", "stash_pop", async () => {
      await api.stashPop(repoPath, index);
      setNotice("退避した変更を取り出し、一覧から取り除きました。");
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
          <button
            className="btn btn-small"
            onClick={doFetch}
            disabled={isNetworkBusy}
            title="リモートの最新情報だけを取得します（作業中のファイルは変わりません）"
          >
            {isNetworkBusy ? (
              <>
                <span className="network-spinner">🔄</span>取得中…
              </>
            ) : (
              "🔄 取得"
            )}
          </button>
          <button
            className="btn btn-small"
            onClick={doPull}
            disabled={isNetworkBusy}
            title="リモートの変更を取り込みます（安全に進められるときだけ取り込みます）"
          >
            {isNetworkBusy ? (
              <>
                <span className="network-spinner">⬇</span>取り込み中…
              </>
            ) : (
              "⬇ 取り込む"
            )}
          </button>
          {undoInfo && (
            <button className="btn btn-undo" onClick={doUndo}>
              ↩ 取り消す: {undoInfo.description}
            </button>
          )}
          <button
            className="btn btn-small"
            onClick={() => setShowIdentity(true)}
            title="コミット作者の名前とメールアドレスを設定します"
          >
            👤 名前/メール
          </button>
          <ThemeToggle />
          <button className="btn btn-small" onClick={() => void refresh()}>
            更新
          </button>
          <button
            className="btn btn-small"
            onClick={() => {
              setOpened(false);
              setStatus(null);
              setIdentity(null);
              // 次に開くリポジトリは初期件数から軽く表示し直す。
              setCommits([]);
              setHasMoreCommits(false);
              setStashes([]);
              setSelectedFile(null);
              setDiff(null);
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
      {!identityComplete && (
        <div className="banner setup">
          <span>
            👋 コミットには「名前」と「メールアドレス」の設定が必要です。
          </span>
          <button
            className="btn btn-small"
            onClick={() => setShowIdentity(true)}
          >
            設定する
          </button>
        </div>
      )}

      <main className="columns">
        <section className="col">
          {status && (
            <StatusPanel
              status={status}
              selected={selectedFile}
              onSelect={selectFile}
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
              onDiscard={doDiscard}
            />
          )}

          <DiffPanel
            selection={selectedFile}
            diff={diff}
            loading={diffLoading}
          />

          <div className="panel commit-box">
            <h2>コミット</h2>
            <textarea
              ref={commitInput}
              value={commitMsg}
              placeholder="このコミットで何をしたか書きましょう（例: ログイン画面を追加）"
              onChange={(e) => setCommitMsg(e.target.value)}
            />
            <div className="commit-actions">
              <button
                className="btn"
                onClick={doCommit}
                disabled={!commitMsg.trim()}
              >
                コミットする
              </button>
              <button
                className="btn btn-small"
                onClick={doAmend}
                disabled={commits.length === 0}
                title="直前のコミットを書き換えます。メッセージ欄が空ならメッセージはそのまま、ステージした変更を取り込みます。"
              >
                直前を修正
              </button>
            </div>
          </div>

          <StashPanel
            stashes={stashes}
            canStash={!!status && !status.is_clean}
            onSave={doStashSave}
            onApply={doStashApply}
            onPop={doStashPop}
          />
        </section>

        <section className="col">
          <HistoryPanel
            commits={commits}
            hasMore={hasMoreCommits}
            loadingMore={loadingMore}
            onLoadMore={loadMore}
            onGoToCommit={() => {
              commitInput.current?.focus();
              commitInput.current?.scrollIntoView({
                behavior: "smooth",
                block: "center",
              });
            }}
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
            networkBusy={isNetworkBusy}
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
            onPush={(name) =>
              void guarded(
                `ブランチ「${name}」を送信`,
                "push",
                () =>
                  api.push(
                    repoPath,
                    "origin",
                    `refs/heads/${name}:refs/heads/${name}`,
                    false,
                  ),
                name,
                true, // networkOp
              )
            }
            onForcePush={(name) =>
              void guarded(
                `ブランチ「${name}」を強制送信`,
                "force_push",
                () =>
                  api.push(
                    repoPath,
                    "origin",
                    `refs/heads/${name}:refs/heads/${name}`,
                    true,
                  ),
                name,
                true, // networkOp
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

      {showIdentity && (
        <IdentityDialog
          current={identity}
          onSave={(name, email, scope) =>
            void saveIdentity(name, email, scope)
          }
          onCancel={() => setShowIdentity(false)}
        />
      )}
    </div>
  );
}
