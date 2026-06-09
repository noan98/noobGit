import { useCallback, useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import {
  api,
  type BlameHunk,
  type BranchGraph,
  type BranchInfo,
  type CommitInfo,
  type Explanation,
  type FileChange,
  type FileDiff,
  type Identity,
  type IdentityScope,
  type LogFilter,
  type OperationKind,
  type RepoStatus,
  type RiskAssessment,
  type StashInfo,
  type UndoEntry,
} from "./api";
import { showToast } from "./components/Toaster";
import { StatusPanel } from "./components/StatusPanel";
import { FileHistoryView } from "./components/FileHistoryView";
import { StashPanel } from "./components/StashPanel";
import { HistoryPanel } from "./components/HistoryPanel";
import { BranchPanel } from "./components/BranchPanel";
import {
  StatusPanelSkeleton,
  HistoryPanelSkeleton,
  BranchPanelSkeleton,
} from "./components/SkeletonPanels";
import { ConfirmDialog } from "./components/ConfirmDialog";
import {
  DiffPanel,
  type DiffSelection,
  type DiffSource,
} from "./components/DiffPanel";
import { IdentityDialog } from "./components/IdentityDialog";
import { CommitDiffViewer } from "./components/CommitDiffViewer";
import { BlameView } from "./components/BlameView";
import { ThemeToggle } from "./components/ThemeToggle";

// 履歴の初期表示件数。初回表示を軽くするため小さめにし、「もっと見る」で追記する。
const LOG_PAGE_SIZE = 30;

// Conventional Commits プレフィックス定義 (#77)。
const COMMIT_PREFIXES: { label: string; desc: string }[] = [
  { label: "feat:", desc: "新機能の追加" },
  { label: "fix:", desc: "バグ修正" },
  { label: "docs:", desc: "ドキュメント変更" },
  { label: "refactor:", desc: "リファクタリング" },
  { label: "chore:", desc: "雑務・設定変更" },
];

// プレフィックスを件名（1行目）の先頭に挿入する。
// 既存の Conventional Commits プレフィックスがあれば置き換える。
function insertCommitPrefix(current: string, prefix: string): string {
  const lines = current.split("\n");
  const cleaned = lines[0].replace(/^[a-z]+(!)?:\s*/, "");
  lines[0] = `${prefix} ${cleaned}`;
  return lines.join("\n");
}

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
};

interface Guard {
  title: string;
  assessment: RiskAssessment;
  explanation: Explanation;
  action: () => Promise<void>;
  refresh: RefreshParts;
  // ネットワーク操作の場合 true。確認ダイアログ経由で exec を呼ぶときに isNetworkBusy を立てる。
  networkOp?: boolean;
  // reset_hard 時のみ設定。ConfirmDialog に失われる変更ファイル一覧を渡す。
  affectedFiles?: FileChange[];
}

export default function App() {
  const [repoPath, setRepoPath] = useState("");
  const [opened, setOpened] = useState(false);
  // リポジトリの初期読み込み中フラグ。true の間は各パネルをスケルトンで表示する。
  const [repoLoading, setRepoLoading] = useState(false);

  const [status, setStatus] = useState<RepoStatus | null>(null);
  const [branches, setBranches] = useState<BranchInfo[]>([]);
  const [branchGraph, setBranchGraph] = useState<BranchGraph | null>(null);
  const [commits, setCommits] = useState<CommitInfo[]>([]);
  const [hasMoreCommits, setHasMoreCommits] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [undoInfo, setUndoInfo] = useState<UndoEntry | null>(null);
  const [stashes, setStashes] = useState<StashInfo[]>([]);

  // 履歴の絞り込み条件。空オブジェクトは「条件なし（全件）」を表す。
  const [logFilter, setLogFilter] = useState<LogFilter>({});
  // 履歴の検索（再取得）中フラグ。HistoryPanel のスピナー表示に使う。
  const [searching, setSearching] = useState(false);

  // refresh / loadMore のクロージャから常に最新の条件を参照するための ref。
  const logFilterRef = useRef<LogFilter>({});
  useEffect(() => {
    logFilterRef.current = logFilter;
  }, [logFilter]);

  // 条件が一つでも設定されていれば true（getLog に渡す filter を絞るかの判断に使う）。
  function hasFilter(f: LogFilter): boolean {
    return (
      (f.message != null && f.message !== "") ||
      (f.author != null && f.author !== "") ||
      f.since != null ||
      f.until != null
    );
  }

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

  // コミット間差分ビューアー: 比較の基準（base）と対象（target）、取得した差分。
  // base のみ選択中は target が null（2 つ目の選択待ち）。両方揃うと差分を表示する。
  const [compareBase, setCompareBase] = useState<CommitInfo | null>(null);
  const [compareTarget, setCompareTarget] = useState<CommitInfo | null>(null);
  const [commitDiffs, setCommitDiffs] = useState<FileDiff[] | null>(null);
  const [commitDiffLoading, setCommitDiffLoading] = useState(false);

  // ファイル別変更履歴を表示中の対象パス。null は非表示。
  const [historyPath, setHistoryPath] = useState<string | null>(null);

  // blame（変更履歴）ビュー: 対象パスと、その blame 結果。
  const [blamePath, setBlamePath] = useState<string | null>(null);
  const [blameHunks, setBlameHunks] = useState<BlameHunk[] | null>(null);
  const [blameLoading, setBlameLoading] = useState(false);
  const [blameError, setBlameError] = useState<string | null>(null);

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
          // 検索条件があればそれを渡す（無ければ未指定で全件＝従来動作）。
          const filter = logFilterRef.current;
          const arg = hasFilter(filter) ? filter : undefined;
          tasks.push(
            api.getLog(repoPath, 0, want, arg).then((cs) => {
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

  // コミット間差分の取得。base が null のときは target の親との比較になる。
  const loadCommitDiff = useCallback(
    async (base: CommitInfo | null, target: CommitInfo) => {
      if (!repoPath) return;
      setCommitDiffLoading(true);
      try {
        const ds = await api.getDiffBetween(
          repoPath,
          base?.id ?? null,
          target.id,
        );
        setCommitDiffs(ds);
        setError(null);
      } catch (e) {
        setCommitDiffs([]);
        setError(String(e));
      } finally {
        setCommitDiffLoading(false);
      }
    },
    [repoPath],
  );

  // ファイルの変更履歴（blame）を開く。取得は補助的なので失敗してもダイアログ内に
  // エラーを表示するだけで、メインのバナーは汚さない。
  const openBlame = useCallback(
    async (path: string) => {
      if (!repoPath) return;
      setBlamePath(path);
      setBlameHunks(null);
      setBlameError(null);
      setBlameLoading(true);
      try {
        setBlameHunks(await api.getBlame(repoPath, path));
      } catch (e) {
        setBlameError(String(e));
      } finally {
        setBlameLoading(false);
      }
    },
    [repoPath],
  );

  // 履歴から比較対象を選ぶ。1 つ目で base を選択（target 待ち）、2 つ目で target を
  // 確定して差分を取得する。base をもう一度押すと選択を解除する。
  const onCompareSelect = useCallback(
    (commit: CommitInfo) => {
      if (compareBase && compareBase.id === commit.id) {
        // 基準を取り消す。
        setCompareBase(null);
        return;
      }
      if (!compareBase) {
        // 1 つ目の選択 = 基準。差分はまだ表示しない。
        setCompareBase(commit);
        setCompareTarget(null);
        setCommitDiffs(null);
        return;
      }
      // 2 つ目の選択 = 比較対象。base→target の差分を取得して表示する。
      setCompareTarget(commit);
      void loadCommitDiff(compareBase, commit);
    },
    [compareBase, loadCommitDiff],
  );

  // コミット間差分の表示を閉じ、選択状態もリセットする。
  const closeCommitDiff = useCallback(() => {
    setCompareBase(null);
    setCompareTarget(null);
    setCommitDiffs(null);
  }, []);

  function closeBlame() {
    setBlamePath(null);
    setBlameHunks(null);
    setBlameError(null);
  }

  async function openRepo() {
    if (!repoPath.trim()) return;
    setNotice(null);
    // 先に開いた状態にしてスケルトンを表示し、その裏で初期読み込みを行う。
    setRepoLoading(true);
    setOpened(true);
    const ok = await refresh();
    // 開けなかった場合は元の入力画面に戻す（エラーはバナーで表示済み）。
    if (!ok) setOpened(false);
    setRepoLoading(false);
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
      const msg =
        scope === "global"
          ? "名前とメールを設定しました（このPC全体）。"
          : "名前とメールを設定しました（このリポジトリ）。";
      setNotice(msg);
      showToast(msg, "success");
    } catch (e) {
      const msg = String(e);
      setError(msg);
      showToast(msg, "error");
    }
  }

  // 安全な操作はそのまま実行し、結果を更新する。
  // refresh を省略した場合は全件再取得（取り消しなど影響範囲が読めない操作向け）。
  // networkOp: true を渡すと実行中に isNetworkBusy を立て、完了・失敗時に必ず下ろす。
  // 成功・失敗の結果はトースト通知でも伝える（既存バナーと併用）。
  const exec = useCallback(
    async (
      action: () => Promise<void>,
      opts: { successMsg?: string; refresh?: RefreshParts; networkOp?: boolean } = {},
    ) => {
      if (opts.networkOp) setIsNetworkBusy(true);
      try {
        await action();
        setError(null);
        if (opts.successMsg) {
          setNotice(opts.successMsg);
          showToast(opts.successMsg, "success");
        }
        await refresh(opts.refresh ?? FULL_REFRESH);
      } catch (e) {
        setNotice(null);
        const msg = String(e);
        setError(msg);
        showToast(msg, "error");
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
        // reset_hard の場合のみ失われる変更ファイル一覧を取得してダイアログに渡す。
        // 取得失敗はベストエフォートで無視（ファイルリストなしでダイアログを表示）。
        let affectedFiles: FileChange[] | undefined;
        if (op === "reset_hard") {
          try {
            const s = await api.getStatus(repoPath);
            affectedFiles = [...s.staged, ...s.unstaged];
          } catch {
            affectedFiles = undefined;
          }
        }
        setGuard({ title, assessment, explanation, action, refresh: parts, networkOp, affectedFiles });
      }
    } catch (e) {
      const msg = String(e);
      setError(msg);
      showToast(msg, "error");
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
      showToast("コミットの前に、名前とメールアドレスを設定しましょう。", "warning");
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
  // 検索条件があれば同じ条件で続きを取得する（条件と無関係なコミットが混ざらない）。
  function loadMore() {
    if (loadingMore || !repoPath) return;
    setLoadingMore(true);
    void (async () => {
      try {
        const filter = logFilterRef.current;
        const arg = hasFilter(filter) ? filter : undefined;
        const more = await api.getLog(
          repoPath,
          commits.length,
          LOG_PAGE_SIZE,
          arg,
        );
        setCommits((prev) => [...prev, ...more]);
        setHasMoreCommits(more.length === LOG_PAGE_SIZE);
        setError(null);
      } catch (e) {
        const errMsg = String(e);
        setError(errMsg);
        showToast(errMsg, "error");
      } finally {
        setLoadingMore(false);
      }
    })();
  }

  // 履歴パネルからの検索。条件を保存し、ページングをリセットして先頭から取り直す。
  // 検索中は HistoryPanel にスピナーを出すため searching を立てる。
  const runSearch = useCallback(
    (filter: LogFilter) => {
      // 条件が変わらないなら何もしない（初回マウント時の空→空の無駄打ちも防ぐ）。
      const prev = logFilterRef.current;
      const same =
        (prev.message ?? "") === (filter.message ?? "") &&
        (prev.author ?? "") === (filter.author ?? "") &&
        (prev.since ?? null) === (filter.since ?? null) &&
        (prev.until ?? null) === (filter.until ?? null);
      if (same) return;
      // 新しい条件を即座に ref へ反映（refresh のログ取得が最新条件を見るように）。
      logFilterRef.current = filter;
      setLogFilter(filter);
      if (!repoPath) return;
      setSearching(true);
      void (async () => {
        // ページングはリセットし、先頭ページから取り直す。
        const arg = hasFilter(filter) ? filter : undefined;
        try {
          const cs = await api.getLog(repoPath, 0, LOG_PAGE_SIZE, arg);
          setCommits(cs);
          setHasMoreCommits(cs.length === LOG_PAGE_SIZE);
          setError(null);
        } catch (e) {
          const errMsg = String(e);
          setError(errMsg);
          showToast(errMsg, "error");
        } finally {
          setSearching(false);
        }
      })();
    },
    [repoPath],
  );

  function doUndo() {
    void exec(async () => {
      const desc = await api.undoLast(repoPath);
      // 取り消し完了はトーストで通知（exec の successMsg 経路を使わず直接呼ぶ）。
      showToast(`取り消しました: ${desc}`, "success");
    });
  }

  // 取得（fetch）: リモートの最新情報だけを取り込む安全操作。確認なしで実行する。
  function doFetch() {
    void exec(
      async () => {
        const outcome = await api.fetch(repoPath, DEFAULT_REMOTE);
        const msg =
          outcome.updated_refs > 0
            ? `リモート「${outcome.remote}」から最新情報を取得しました（追跡ブランチ ${outcome.updated_refs} 件を更新）。`
            : `リモート「${outcome.remote}」を確認しました。新しい変更はありませんでした。`;
        showToast(msg, "info");
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
        const msg =
          outcome.kind === "up_to_date"
            ? "すでに最新の状態でした。取り込むものはありません。"
            : `リモートの変更を取り込みました（${outcome.commit.short_id} まで前進）。`;
        showToast(msg, "success");
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
      showToast("コミットの修正の前に、名前とメールアドレスを設定しましょう。", "warning");
      setShowIdentity(true);
      return;
    }
    const msg = commitMsg.trim();
    void guarded("直前のコミットを修正", "amend_commit", async () => {
      await api.amendCommit(repoPath, msg);
      setCommitMsg("");
      showToast("直前のコミットを修正しました。", "success");
    });
  }

  // 退避（保存）。安全操作なので guarded はダイアログを出さずそのまま実行する。
  function doStashSave(message: string) {
    void guarded("変更を退避", "stash_save", async () => {
      await api.stashSave(repoPath, message);
      showToast("変更を退避しました。作業ツリーをきれいにしました。", "success");
    });
  }

  // 退避の適用（一覧に残す）。コンフリクトの可能性があるため guarded を通す。
  function doStashApply(index: number) {
    void guarded("退避を適用", "stash_apply", async () => {
      await api.stashApply(repoPath, index);
      showToast("退避した変更を取り出しました（退避は一覧に残しています）。", "success");
    });
  }

  // 退避の取り出し（pop・一覧から削除）。コンフリクトの可能性があるため guarded を通す。
  function doStashPop(index: number) {
    void guarded("退避を取り出す", "stash_pop", async () => {
      await api.stashPop(repoPath, index);
      showToast("退避した変更を取り出し、一覧から取り除きました。", "success");
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
              // 履歴の絞り込みもリセットする。
              setLogFilter({});
              logFilterRef.current = {};
              setStashes([]);
              setSelectedFile(null);
              setDiff(null);
              setCompareBase(null);
              setCompareTarget(null);
              setCommitDiffs(null);
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
          <AnimatePresence mode="wait">
            {repoLoading ? (
              <motion.div
                key="status-skeleton"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.15 }}
              >
                <StatusPanelSkeleton />
              </motion.div>
            ) : (
              status && (
                <motion.div
                  key="status-content"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  transition={{ duration: 0.2 }}
                >
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
                    onShowHistory={(p) => setHistoryPath(p)}
                    onBlame={(p) => void openBlame(p)}
                  />
                </motion.div>
              )
            )}
          </AnimatePresence>

          <DiffPanel
            selection={selectedFile}
            diff={diff}
            loading={diffLoading}
          />

          <div className="panel commit-box">
            <h2>コミット</h2>
            {/* Conventional Commits プレフィックスボタン (#77) */}
            <div className="prefix-buttons">
              {COMMIT_PREFIXES.map(({ label, desc }) => (
                <button
                  key={label}
                  className="btn btn-small prefix-btn"
                  onClick={() => {
                    setCommitMsg((prev) => insertCommitPrefix(prev, label));
                    commitInput.current?.focus();
                  }}
                  title={desc}
                >
                  {label}
                </button>
              ))}
            </div>
            <textarea
              ref={commitInput}
              value={commitMsg}
              placeholder="このコミットで何をしたか書きましょう（例: ログイン画面を追加）"
              onChange={(e) => setCommitMsg(e.target.value)}
            />
            {/* 文字数カウンター (#77) */}
            {commitMsg.length > 0 && (() => {
              const subject = commitMsg.split("\n")[0];
              const len = subject.length;
              const color = len <= 50 ? "var(--safe)" : len <= 72 ? "var(--caution)" : "var(--destructive)";
              const hint = len > 72
                ? "短くまとめると見やすくなります"
                : len > 50
                  ? "本文への移動を検討してください"
                  : null;
              return (
                <div className="char-count">
                  <span style={{ color, fontWeight: 600 }}>{len}</span>
                  <span className="char-limit">/ 50 字推奨</span>
                  {hint && <span className="char-hint">{hint}</span>}
                </div>
              );
            })()}
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
          <AnimatePresence mode="wait">
            {repoLoading ? (
              <motion.div
                key="history-skeleton"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.15 }}
              >
                <HistoryPanelSkeleton />
              </motion.div>
            ) : (
              <motion.div
                key="history-content"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                transition={{ duration: 0.2 }}
              >
                <HistoryPanel
                  commits={commits}
                  currentBranch={status?.branch ?? null}
                  hasMore={hasMoreCommits}
                  loadingMore={loadingMore}
                  onLoadMore={loadMore}
                  onSearch={runSearch}
                  searching={searching}
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
                  onCompareSelect={onCompareSelect}
                  compareBaseId={compareBase?.id ?? null}
                />
              </motion.div>
            )}
          </AnimatePresence>

          {compareTarget && (
            <CommitDiffViewer
              base={compareBase}
              target={compareTarget}
              diffs={commitDiffs}
              loading={commitDiffLoading}
              onClose={closeCommitDiff}
            />
          )}
        </section>

        <section className="col">
          <AnimatePresence mode="wait">
            {repoLoading ? (
              <motion.div
                key="branch-skeleton"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.15 }}
              >
                <BranchPanelSkeleton />
              </motion.div>
            ) : (
              <motion.div
                key="branch-content"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                transition={{ duration: 0.2 }}
              >
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
              </motion.div>
            )}
          </AnimatePresence>
        </section>
      </main>

      {guard && (
        <ConfirmDialog
          title={guard.title}
          assessment={guard.assessment}
          explanation={guard.explanation}
          affectedFiles={guard.affectedFiles}
          onConfirm={() => void confirmGuard()}
          onCancel={() => setGuard(null)}
        />
      )}

      {historyPath && (
        <FileHistoryView
          repoPath={repoPath}
          path={historyPath}
          onClose={() => setHistoryPath(null)}
        />
      )}

      {blamePath && (
        <BlameView
          path={blamePath}
          hunks={blameHunks}
          loading={blameLoading}
          error={blameError}
          onClose={closeBlame}
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
