/*
 * RepoWorkspace — 1 つのリポジトリタブ分のワークスペース (#263)。
 *
 * 旧 App.tsx の本体。リポジトリを開く前の初期画面（WelcomeScreen）から、開いた後の
 * 全ビュー・操作フローまで、1 タブ分の状態をすべてここで持つ。タブの管理（追加・
 * 切り替え・閉じる・セッション保存/復元）は App.tsx が担い、本コンポーネントは
 * タブごとに 1 インスタンスずつマウントされる。
 *
 * 非アクティブなタブもマウントしたまま隠す（切り替えのたびに再読込しないため）。
 * そのため window に登録するグローバルショートカット類は、active フラグで
 * アクティブなタブだけが反応するようにする。
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import {
  api,
  isSwitchBlockedByChanges,
  type BisectStatus,
  type BlameHunk,
  type BranchGraph,
  type BranchInfo,
  type CommitInfo,
  type ConflictFile,
  type Explanation,
  type FileChange,
  type FileDiff,
  type Identity,
  type IdentityScope,
  type LfsCandidate,
  type LogFilter,
  type MergedBranchInfo,
  type NetworkErrorKind,
  type OperationKind,
  type RepoStatus,
  type RiskAssessment,
  type SensitiveWarning,
  type StashInfo,
  type TagInfo,
  type RemoteInfo,
  type UndoEntry,
} from "./api";
import { Icon } from "./components/Icon";
import { showToast } from "./components/Toaster";
import { StatusPanel } from "./components/StatusPanel";
import { FileHistoryView } from "./components/FileHistoryView";
import { StashPanel } from "./components/StashPanel";
import { HistoryPanel } from "./components/HistoryPanel";
import { BranchPanel } from "./components/BranchPanel";
import { TagPanel } from "./components/TagPanel";
import { RemotePanel } from "./components/RemotePanel"; // #71 リモート管理
import {
  StatusPanelSkeleton,
  HistoryPanelSkeleton,
  BranchPanelSkeleton,
  StashPanelSkeleton,
  TagPanelSkeleton,
} from "./components/SkeletonPanels";
import { useDelayedFlag } from "./hooks/useDelayedFlag"; // #171 スケルトンのちらつき防止
import { useRiskLevels } from "./hooks/useRiskLevels"; // #274 操作トリガーボタンの危険度カラー
import { riskTriggerClassFor } from "./lib/risk"; // #274 操作トリガーボタンの危険度カラー
import { ConfirmDialog } from "./components/ConfirmDialog";
import { ConflictWizard } from "./components/ConflictWizard";
import { StashPopFollowUp } from "./components/StashPopFollowUp";
import { RebaseWizard } from "./components/RebaseWizard";
import { BisectWizard } from "./components/BisectWizard"; // #184 Bisect
import { DetachedHeadBanner } from "./components/DetachedHeadBanner"; // #197 detached HEAD
import {
  DiffPanel,
  type DiffSelection,
  type DiffSource,
} from "./components/DiffPanel";
import { IdentityDialog } from "./components/IdentityDialog";
import { CommitDiffViewer } from "./components/CommitDiffViewer";
import { BlameView } from "./components/BlameView";
import { ThemeToggle } from "./components/ThemeToggle";
import { WelcomeScreen, rememberRepo } from "./components/WelcomeScreen";
import { OnboardingWizard } from "./components/OnboardingWizard"; // #64 オンボーディング
import { ShortcutHelpDialog } from "./components/ShortcutHelpDialog"; // #63 ショートカット
import { SettingsDialog } from "./components/SettingsDialog"; // 設定（表示言語）
import { useLanguage } from "./i18n";
import { GitignoreModal } from "./components/GitignoreModal"; // #70 .gitignore 管理
import { useGlobalShortcuts, isPaletteShortcut } from "./hooks/useGlobalShortcuts"; // #63 ショートカット
import { Sidebar, type MainView } from "./components/Sidebar"; // SourceTree 風レイアウトのサイドバー
import { UndoTimeline } from "./components/UndoTimeline"; // #48 Undo タイムライン
import { ExplainTooltip } from "./components/ExplainTooltip"; // #104 操作説明ツールチップ
import { CommandPalette, type PaletteCommand } from "./components/CommandPalette"; // #105 コマンドパレット
import { NetworkErrorDialog } from "./components/NetworkErrorDialog"; // #126 ネットワーク診断
import { SensitiveWarningDialog } from "./components/SensitiveWarningDialog"; // #69 機密ファイル検出
import { LfsGuideDialog } from "./components/LfsGuideDialog"; // #81 LFS ガイド
import { NetworkProgressBar } from "./components/NetworkProgressBar"; // #167 進捗フィードバック
import { useNetworkProgress } from "./hooks/useNetworkProgress"; // #167 進捗フィードバック
import { transitions } from "./theme/motion";
import {
  BODY_WRAP_LIMIT,
  getCommitSubject,
  getSubjectLength,
  SUBJECT_LIMIT,
} from "./lib/commitMessage"; // #172 50/72 文字ガイドライン
import {
  applyCommitSuggestion,
  buildCommitSuggestions,
  type CommitSuggestion,
} from "./lib/commitSuggest"; // #185 コミットメッセージ補完（過去履歴 + Conventional Commits）

// 履歴の初期表示件数。初回表示を軽くするため小さめにし、「もっと見る」で追記する。
const LOG_PAGE_SIZE = 30;

// #185 コミットメッセージ件名の補完: 入力が落ち着いてから履歴を問い合わせるまでの
// デバウンス時間と、ドロップダウンに表示する候補の最大件数。
const COMMIT_SUGGEST_DEBOUNCE_MS = 300;
const COMMIT_SUGGEST_MAX = 5;

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

// #172 件名の 50 文字ガイドラインを説明するツールチップを「初回のみ」表示するための
// localStorage キー。読み書きとも失敗しても致命的ではないので try/catch で保護する。
const SUBJECT_HINT_SEEN_KEY = "noobgit_commit_subject_hint_seen";

function hasSeenSubjectHint(): boolean {
  try {
    return localStorage.getItem(SUBJECT_HINT_SEEN_KEY) === "1";
  } catch {
    // 読み取れない場合はヒントを表示する側に倒す（実害はない）。
    return false;
  }
}

function markSubjectHintSeen(): void {
  try {
    localStorage.setItem(SUBJECT_HINT_SEEN_KEY, "1");
  } catch {
    // 保存できなくても表示は続行する（次回また出るだけ）。
  }
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
  tags?: boolean;
}

// リポジトリを開いた直後や手動更新で使う全件再取得。
const FULL_REFRESH: RefreshParts = {
  status: true,
  branches: true,
  log: true,
  undo: true,
  stash: true,
  tags: true,
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
  // 適用は作業ツリーへ取り出すだけ（退避は一覧に残る）。コンフリクト時も
  // status を取り直して ConflictWizard を検出できるようにする。
  stash_apply: { status: true },
  // 取り出し（pop）は作業ツリーに戻す。コンフリクトが無ければ退避一覧からも消える
  // （コンフリクト時は退避を残すので stash も取り直して一覧の状態を反映する）。
  stash_pop: { status: true, stash: true },
  // 退避の削除は一覧だけが変わる。
  stash_drop: { stash: true },
  // 作成はブランチ一覧だけ。HEAD も作業ツリーも動かさない。
  create_branch: { branches: true, undo: true },
  // 切り替えは HEAD が動くので作業ツリー・ブランチ・履歴すべてが変わりうる。
  switch_branch: FULL_REFRESH,
  // 退避 → 切り替え → 復元。HEAD・作業ツリー・退避一覧が変わる。
  switch_branch_with_stash: FULL_REFRESH,
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
  // cherry-pick は HEAD に新しいコミットを積む。status・log・ブランチ関係が変わり、undo も積まれる。
  cherry_pick: { status: true, branches: true, log: true, undo: true },
  // revert も HEAD に新しいコミット（打ち消しコミット）を積む。cherry-pick と同じ再取得が要る。
  revert: { status: true, branches: true, log: true, undo: true },
  // タグ作成・削除はタグ一覧だけを取り直す。削除は undo も積まれる。
  create_tag: { tags: true, undo: true },
  delete_tag: { tags: true, undo: true },
  // リベース（squash / reword）は HEAD のコミットを作り直す。status・log・ブランチ関係が
  // 変わり、undo も積まれる。
  rebase: { status: true, branches: true, log: true, undo: true },
  // マージは HEAD を動かすので status・log・ブランチ関係が変わる。undo も積まれる。
  // コンフリクト時も status を取り直してコンフリクトを検出する。
  merge: FULL_REFRESH,
  // リモート削除はリモート一覧の再取得をアクション内で行うため空にする。
  remove_remote: {},
  // ファイル復元はステージ済みの状態と undo 履歴が変わる。
  restore_file: { status: true, undo: true },
  // #184 Bisect: 開始・終了のどちらも HEAD が detached ⇔ ブランチの間で動き、
  // 作業ツリーの中身も入れ替わる。status・log・ブランチ関係すべてを取り直す。
  // undo も bisect_start では積まれる（bisect_reset 自体は記録しない）。
  bisect_start: FULL_REFRESH,
  bisect_reset: FULL_REFRESH,
  // クローンはタブの外（WelcomeScreen）で完結する操作で、このタブの状態には影響しない。
  clone: {},
  // #197 detached HEAD の救出は HEAD がブランチへ移るので status・ブランチ関係を取り直す。
  // undo も積まれる。
  rescue_detached_head: { status: true, branches: true, log: true, undo: true },
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
  // #269 マージ済みブランチの一括削除時のみ設定。ConfirmDialog に削除対象のブランチ名一覧を渡す。
  affectedBranches?: string[];
}

// RepoWorkspace の props。App.tsx（タブ管理）から渡される。
export interface RepoWorkspaceProps {
  /** このタブがアクティブか。false の間はグローバルショートカット類を無効化する。 */
  active: boolean;
  /** マウント時に自動で開くパス。null なら初期画面（新しいタブ）から始める。 */
  initialPath: string | null;
  /**
   * 開いているリポジトリが変わったときの通知（開いたらパス、閉じたら null）。
   * タブのラベル表示とセッション保存に使う。
   */
  onOpenedRepoChange: (path: string | null) => void;
}

export function RepoWorkspace({
  active,
  initialPath,
  onOpenedRepoChange,
}: RepoWorkspaceProps) {
  // 表示言語の翻訳関数。UI 文言は段階的に t(key) 経由へ移行していく。
  const { t } = useLanguage();
  const [repoPath, setRepoPath] = useState(initialPath ?? "");
  const [opened, setOpened] = useState(false);
  // リポジトリの初期読み込み中フラグ。true の間は各パネルをスケルトンで表示する。
  const [repoLoading, setRepoLoading] = useState(false);
  // #171 delayed start: ロードが 100ms を超えて続くときだけスケルトンを表示し、
  // 高速なロードではちらつかせない。
  const showSkeleton = useDelayedFlag(repoLoading, 100);

  const [status, setStatus] = useState<RepoStatus | null>(null);
  const [branches, setBranches] = useState<BranchInfo[]>([]);
  const [branchGraph, setBranchGraph] = useState<BranchGraph | null>(null);
  // #269 ブランチクリーンアップ: マージ済み（保護ブランチ・現在ブランチを除く）ローカルブランチ。
  const [mergedBranches, setMergedBranches] = useState<MergedBranchInfo[]>([]);
  // #169 保護ブランチの設定一覧（git config `noobgit.protectedBranches`）。
  const [protectedBranches, setProtectedBranches] = useState<string[]>([]);
  const [commits, setCommits] = useState<CommitInfo[]>([]);
  const [hasMoreCommits, setHasMoreCommits] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [undoInfo, setUndoInfo] = useState<UndoEntry | null>(null);
  const [undoJournal, setUndoJournal] = useState<UndoEntry[]>([]); // #48 Undo タイムライン
  const [stashes, setStashes] = useState<StashInfo[]>([]);
  const [tags, setTags] = useState<TagInfo[]>([]);
  const [remotes, setRemotes] = useState<RemoteInfo[]>([]); // #71 リモート管理

  // #274 操作トリガーボタンの危険度カラー: guarded() を通る操作群をまとめて
  // 事前評価し、クリック前からボタン自体で Safe/Caution/Destructive を伝える。
  // 対象（ブランチ名）に依存しない操作は1件、push・delete_branch はブランチ
  // ごとに評価する（保護ブランチかどうかで結果が変わるため）。
  // リスク判定ロジックはここには無く、すべて core の assess_operation の結果。
  const localBranchNames = branches
    .filter((b) => !b.is_remote)
    .map((b) => b.name);
  const nonHeadLocalBranchNames = branches
    .filter((b) => !b.is_remote && !b.is_head)
    .map((b) => b.name);
  // 状態（作業ツリー・履歴・ブランチ）を再取得するたびに危険度も評価し直す。
  // 例: コミットを送信すると amend_commit が destructive → caution に変わる。
  const riskRefreshToken = useMemo(() => ({}), [status, commits, branches]);
  // 表示中のタブだけ評価する（隠れているタブのボタンの色は見えないため）。
  // タブを切り替えたときに評価し直す。
  const riskLevels = useRiskLevels(opened && active ? repoPath : null, [
    // 対象非依存（常に同じ判定になる操作）。
    { op: "discard" },
    { op: "reset_hard" },
    { op: "cherry_pick" },
    { op: "revert" },
    { op: "merge" },
    { op: "switch_branch" },
    { op: "force_push" },
    { op: "delete_tag" },
    { op: "remove_remote" },
    { op: "stash_apply" },
    { op: "stash_pop" },
    { op: "restore_file" },
    { op: "amend_commit" },
    { op: "pull" },
    // 対象（ブランチ）依存: 保護ブランチかどうかで危険度が変わる。
    ...localBranchNames.map((name) => ({ op: "push" as const, target: name })),
    ...nonHeadLocalBranchNames.map((name) => ({
      op: "delete_branch" as const,
      target: name,
    })),
  ], riskRefreshToken);

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

  // #277 履歴のカーソルベースページング用。直前に取得したページが返した
  // カーソル ID（続きが無ければ null）。loadMore() が「続きから」取得するのに使う。
  const logCursorRef = useRef<string | null>(null);
  // 使い終わった（先頭から取り直す・検索条件を変える等で不要になった）カーソルを
  // 手放す。失敗しても致命的ではない（core 側の上限超過での自動立ち退きが
  // 安全網になる）ので、エラーは無視する。
  const releaseLogCursor = useCallback(() => {
    const id = logCursorRef.current;
    if (id) {
      logCursorRef.current = null;
      void api.closeLogCursor(id).catch(() => {});
    }
  }, []);
  // タブを閉じる（アンマウントする）ときも、握ったままのカーソルを手放す。
  useEffect(() => {
    return () => releaseLogCursor();
  }, [releaseLogCursor]);

  const [commitMsg, setCommitMsg] = useState("");
  // コミット入力欄への参照。履歴が空のときの「コミットへ」誘導でフォーカスする。
  const commitInput = useRef<HTMLTextAreaElement>(null);
  // #172 件名 50 文字ガイドラインの「初回のみ」ツールチップの表示状態とタイマー。
  const [showSubjectHint, setShowSubjectHint] = useState(false);
  const subjectHintTimer = useRef<number | null>(null);
  useEffect(() => {
    return () => {
      if (subjectHintTimer.current != null) {
        window.clearTimeout(subjectHintTimer.current);
      }
    };
  }, []);
  // コミット入力欄に初めてフォーカスしたときだけ、50 文字ガイドラインの
  // ツールチップを 3 秒間表示する。
  function handleCommitFocus() {
    if (hasSeenSubjectHint()) return;
    markSubjectHintSeen();
    setShowSubjectHint(true);
    if (subjectHintTimer.current != null) {
      window.clearTimeout(subjectHintTimer.current);
    }
    subjectHintTimer.current = window.setTimeout(() => {
      setShowSubjectHint(false);
    }, 3000);
  }
  // 件名行（1行目）で Enter を押したとき、Git の慣習（件名 → 空行 → 本文）に
  // 沿って空行を自動挿入し、本文エリアへ自然に移行できるようにする。
  // 既に空行がある場合や本文入力中はブラウザ標準の Enter 動作に任せる。
  function handleCommitKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    // IME 変換中のキー入力は無視する（日本語入力で Enter が変換確定と誤って
    // 衝突しないように #185）。
    if (e.nativeEvent.isComposing) return;

    // 補完ドロップダウンが開いているときは、まずそちらのキー操作を優先する。
    // Ctrl/Cmd+Enter は既存のコミットショートカットを壊さないよう素通しする。
    if (showSuggestDropdown && subjectSuggestions.length > 0) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSuggestIndex((i) => (i + 1) % subjectSuggestions.length);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setSuggestIndex(
          (i) => (i - 1 + subjectSuggestions.length) % subjectSuggestions.length,
        );
        return;
      }
      if (e.key === "Tab" || (e.key === "Enter" && !e.ctrlKey && !e.metaKey)) {
        e.preventDefault();
        confirmSuggestion(suggestIndex);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setShowSuggestDropdown(false);
        return;
      }
    }

    // ドロップダウンが開いていないときの Enter は、これまでどおりの挙動。
    if (e.key !== "Enter") return;
    const el = e.currentTarget;
    const cursor = el.selectionStart ?? el.value.length;
    const firstNewline = el.value.indexOf("\n");
    const onSubjectLine = firstNewline === -1 || cursor <= firstNewline;
    if (!onSubjectLine) return;
    if (firstNewline !== -1 && el.value[firstNewline + 1] === "\n") return;
    e.preventDefault();
    const nextValue = `${el.value.slice(0, cursor)}\n\n${el.value.slice(cursor)}`;
    setCommitMsg(nextValue);
    const nextCursor = cursor + 2;
    requestAnimationFrame(() => {
      el.selectionStart = el.selectionEnd = nextCursor;
    });
  }

  // #185 コミットメッセージ件名の補完（過去の履歴 + Conventional Commits）。
  const [subjectSuggestions, setSubjectSuggestions] = useState<
    CommitSuggestion[]
  >([]);
  const [showSuggestDropdown, setShowSuggestDropdown] = useState(false);
  const [suggestIndex, setSuggestIndex] = useState(0);
  // 直近のリクエストの通し番号。古いリクエストが後から解決しても、新しい入力の
  // 結果を上書きしないための競合対策。
  const suggestRequestId = useRef(0);

  const commitSubject = getCommitSubject(commitMsg);
  useEffect(() => {
    if (!opened || !repoPath || commitSubject.trim() === "") {
      setShowSuggestDropdown(false);
      setSubjectSuggestions([]);
      return;
    }
    const requestId = ++suggestRequestId.current;
    const handle = window.setTimeout(() => {
      void (async () => {
        let historyMatches: string[] = [];
        try {
          historyMatches = await api.suggestCommitMessages(
            repoPath,
            commitSubject,
            COMMIT_SUGGEST_MAX,
          );
        } catch {
          // 補完の取得に失敗してもコミット操作自体は妨げない。
          // Conventional Commits のフォールバックだけで表示を続ける。
        }
        if (requestId !== suggestRequestId.current) return; // 古いリクエストは無視
        // 入力欄にフォーカスが無いとき（amend でメッセージを読み込んだ直後など、
        // プログラムから件名が変わったとき）は候補を開かない。
        if (document.activeElement !== commitInput.current) return;
        const items = buildCommitSuggestions(
          historyMatches,
          commitSubject,
          COMMIT_SUGGEST_MAX,
        );
        setSubjectSuggestions(items);
        setSuggestIndex(0);
        setShowSuggestDropdown(items.length > 0);
      })();
    }, COMMIT_SUGGEST_DEBOUNCE_MS);
    return () => window.clearTimeout(handle);
  }, [commitSubject, opened, repoPath]);

  // 選択中の候補を件名へ確定する（Tab / Enter / クリック）。
  function confirmSuggestion(index: number) {
    const item = subjectSuggestions[index];
    if (!item) return;
    const { text, cursor } = applyCommitSuggestion(commitMsg, item.text);
    setCommitMsg(text);
    setShowSuggestDropdown(false);
    const el = commitInput.current;
    requestAnimationFrame(() => {
      el?.focus();
      if (el) el.selectionStart = el.selectionEnd = cursor;
    });
  }

  const [error, setError] = useState<string | null>(null);
  // ネットワーク操作（fetch / pull / push）の実行中フラグ。
  // true の間は fetch / pull / push ボタンを無効化して二重実行を防ぐ。
  const [isNetworkBusy, setIsNetworkBusy] = useState(false);
  // #167 進捗フィードバック: fetch / pull / push 実行中の進捗バー状態。
  const networkProgress = useNetworkProgress();
  const [notice, setNotice] = useState<string | null>(null);
  const [guard, setGuard] = useState<Guard | null>(null);

  // #203 サブモジュール検出: サブモジュール未対応バナーを閉じたか。
  // リポジトリを開き直すたびにリセットする（新しいリポジトリでは改めて案内する）。
  const [submoduleBannerDismissed, setSubmoduleBannerDismissed] =
    useState(false);

  // #126 ネットワーク診断: ネットワーク操作のエラー種別と生メッセージ。
  // null のときはダイアログ非表示。
  const [networkErrorInfo, setNetworkErrorInfo] = useState<{
    kind: NetworkErrorKind;
    raw: string;
  } | null>(null);

  // #69 機密ファイル検出: 機密ファイルが見つかった場合の警告と、「それでもステージする」
  // ときに呼ぶ続行コールバックを保持する。null のときはダイアログ非表示。
  const [sensitiveGuard, setSensitiveGuard] = useState<{
    warnings: SensitiveWarning[];
    proceedAction: () => void;
  } | null>(null);

  // #81 LFS ガイド: 大容量・バイナリファイルが見つかった場合の候補一覧と、
  // 「それでもステージする」ときに呼ぶ続行コールバックを保持する。null のときはダイアログ非表示。
  const [lfsGuard, setLfsGuard] = useState<{
    candidates: LfsCandidate[];
    proceedAction: () => void;
  } | null>(null);

  // コンフリクト中ファイルの詳細（解消ウィザード用）。status.conflicted を補う形で
  // has_ancestor 等の情報を持つ。status の再取得に合わせて取り直す。
  const [conflicts, setConflicts] = useState<ConflictFile[]>([]);

  // #156 stash_pop がコンフリクトで退避を一覧に残したときの追跡情報。
  // コンフリクトウィザードで全ファイルを解消し終えたタイミングで、
  // 「退避を削除する / 残す」を選べる案内を出すために使う。
  // id は退避の ID（番号は新しい退避でずれるため、削除は ID で指定する）。
  // seenConflicts は、pop 後にコンフリクト一覧を実際に取得できたか。取得前の
  // 一瞬（conflicts がまだ空）に「解消しました」を出さないために使う。
  const [stashPopConflict, setStashPopConflict] = useState<{
    id: string;
    message: string;
    seenConflicts: boolean;
  } | null>(null);

  // pop 後にコンフリクト一覧が実際に届いたら「観測済み」にする。これ以降に一覧が
  // 空になったときだけ、解消完了として後片付けの案内を出す。
  useEffect(() => {
    if (conflicts.length > 0) {
      setStashPopConflict((prev) =>
        prev && !prev.seenConflicts ? { ...prev, seenConflicts: true } : prev,
      );
    }
  }, [conflicts]);

  // リベース（squash / reword）で選択中のコミット id 集合と、ウィザードの表示状態。
  const [selectedCommitIds, setSelectedCommitIds] = useState<Set<string>>(
    new Set(),
  );
  const [showRebase, setShowRebase] = useState(false);

  // #184 Bisect: 現在のセッション状態（null = 未開始）と、ウィザードの表示状態。
  // ウィザードを閉じても bisectStatus は保持し、バナーから再度開けるようにする。
  const [bisectStatus, setBisectStatus] = useState<BisectStatus | null>(null);
  const [showBisectWizard, setShowBisectWizard] = useState(false);

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

  // #63 ショートカット: ヘルプダイアログの表示状態。
  const [showShortcuts, setShowShortcuts] = useState(false);

  // 設定ダイアログ（表示言語の切り替えなど）の表示状態。
  const [showSettings, setShowSettings] = useState(false);

  // #70 .gitignore 管理: 閲覧モーダルの表示と、その時点の .gitignore 内容（null = 未作成）。
  const [gitignore, setGitignore] = useState<{ content: string | null } | null>(
    null,
  );

  // #105 コマンドパレット: Ctrl+K / ⌘K で開くパレットの表示状態。
  const [paletteOpen, setPaletteOpen] = useState(false);

  // SourceTree 風レイアウト: メイン領域に表示するビュー。サイドバーで切り替える。
  const [view, setView] = useState<MainView>("status");

  // コミット入力欄へ移動してフォーカスする（ツールバーの「コミット」や履歴の
  // Empty State から呼ばれる）。入力欄はファイルステータスビューにだけあるので、
  // ビューを切り替えてから次フレームでフォーカスする。
  const goToCommitBox = useCallback(() => {
    setView("status");
    setTimeout(() => {
      commitInput.current?.focus();
      commitInput.current?.scrollIntoView({ behavior: "smooth", block: "center" });
    }, 0);
  }, []);

  const refresh = useCallback(
    async (parts: RefreshParts = FULL_REFRESH): Promise<boolean> => {
      if (!repoPath) return false;
      try {
        const tasks: Promise<unknown>[] = [];
        if (parts.status) tasks.push(api.getStatus(repoPath).then(setStatus));
        if (parts.branches) {
          tasks.push(api.getBranches(repoPath).then(setBranches));
          tasks.push(api.getBranchGraph(repoPath).then(setBranchGraph));
          // #269 ブランチクリーンアップ導線の候補一覧。
          tasks.push(
            api.getMergedBranches(repoPath).then(setMergedBranches),
          );
          tasks.push(
            api.getProtectedBranches(repoPath).then(setProtectedBranches),
          );
        }
        if (parts.log) {
          // すでに「もっと見る」で広げていれば、その件数を保ったまま先頭から取り直す。
          const want = Math.max(LOG_PAGE_SIZE, loadedCount.current);
          // 検索条件があればそれを渡す（無ければ未指定で全件＝従来動作）。
          const filter = logFilterRef.current;
          const arg = hasFilter(filter) ? filter : undefined;
          // 先頭から取り直すので、直前のカーソルはもう使わない。
          releaseLogCursor();
          tasks.push(
            api.getLogPage(repoPath, want, arg).then((page) => {
              setCommits(page.commits);
              setHasMoreCommits(page.has_more);
              logCursorRef.current = page.cursor;
            }),
          );
        }
        if (parts.undo) {
          tasks.push(api.peekUndo(repoPath).then(setUndoInfo));
          tasks.push(api.getUndoJournal(repoPath).then(setUndoJournal)); // #48 Undo タイムライン
        }
        if (parts.stash) tasks.push(api.getStashes(repoPath).then(setStashes));
        if (parts.tags) tasks.push(api.listTags(repoPath).then(setTags));
        await Promise.all(tasks);
        setError(null);
        return true;
      } catch (e) {
        setError(String(e));
        return false;
      }
    },
    [repoPath, releaseLogCursor],
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

  // #71 リモート一覧の取得。失敗しても画面表示は止めない。
  const loadRemotes = useCallback(async () => {
    if (!repoPath) return;
    try {
      setRemotes(await api.listRemotes(repoPath));
    } catch {
      setRemotes([]);
    }
  }, [repoPath]);

  // #184 Bisect: 進行中のセッションがあれば復元する（アプリ再起動・タブ再表示対応）。
  // 失敗してもバナーが出ないだけなので、画面表示は止めない。
  const loadBisectStatus = useCallback(async () => {
    if (!repoPath) return;
    try {
      setBisectStatus(await api.bisectStatus(repoPath));
    } catch {
      setBisectStatus(null);
    }
  }, [repoPath]);

  // 開いた直後の補助情報の取得。状態・履歴などの本体（refresh）はここでは
  // 呼ばない — openRepo() がすでに refresh() を待っているので、ここでも呼ぶと
  // 起動直後に同じ重い読み込みが 2 回走ってしまう。
  useEffect(() => {
    if (opened) {
      void loadIdentity();
      void loadRemotes(); // #71 リモート一覧
      void loadBisectStatus(); // #184 Bisect セッションの復元
    }
  }, [opened, loadIdentity, loadRemotes, loadBisectStatus]);

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

  // status にコンフリクトがあれば、その詳細（has_ancestor 等）を取り直す。
  // 取得失敗はベストエフォートで無視（ウィザードを出さないだけ）。
  useEffect(() => {
    if (!repoPath || !status || status.conflicted.length === 0) {
      setConflicts([]);
      return;
    }
    let cancelled = false;
    void api
      .getConflicts(repoPath)
      .then((cs) => {
        if (!cancelled) setConflicts(cs);
      })
      .catch(() => {
        if (!cancelled) setConflicts([]);
      });
    return () => {
      cancelled = true;
    };
  }, [repoPath, status]);

  // コンフリクトが発生したら、解消ウィザードのあるファイルステータスビューへ
  // 自動で切り替える（他のビューを見ていて気づかない事故を防ぐ）。
  const hasConflicts = conflicts.length > 0;
  useEffect(() => {
    if (hasConflicts) setView("status");
  }, [hasConflicts]);

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

  // #262/#263 自動オープン: App.tsx（タブ管理）が initialPath を渡してきた
  // タブ（前回セッションの復元）は、初期画面を挟まずにそのリポジトリを開く。自動で
  // 開くのは 1 回だけで、「別のリポジトリ」で初期画面へ戻ったあとは通常の導線に
  // 任せる。開けなかった場合（フォルダ削除など）は openRepo が初期画面へ戻すので、
  // エラーはそこで案内される。
  //
  // 開くのは、そのタブが初めて表示（active）されたとき。復元したタブをすべて
  // 起動直後に読み込むと、見えていないタブの分まで重い処理が走って、表示中の
  // タブが操作できるようになるまでが遅くなるため。まだ開いていない間も、
  // タブ名とセッション保存には initialPath を使う（下の通知を参照）。
  // autoOpenPath（ref）は二重オープン防止用（開発時の StrictMode ではエフェクトが
  // 2 回走るため、state だけだと 2 回開いてしまう）。autoOpenPending（state）は
  // 「まだ開いていない」ことを画面・タブ管理へ反映するためのもの。
  const autoOpenPath = useRef<string | null>(initialPath);
  const [autoOpenPending, setAutoOpenPending] = useState(initialPath !== null);
  useEffect(() => {
    if (!active || autoOpenPath.current === null) return;
    const path = autoOpenPath.current;
    autoOpenPath.current = null;
    setAutoOpenPending(false);
    if (repoPath === path) void openRepo();
  }, [active, repoPath]);

  // 開いているリポジトリの変化をタブ管理（App.tsx）へ通知する。コールバックの同一性に
  // 依存しないよう、ref 経由で常に最新の関数を呼ぶ。
  const onOpenedRepoChangeRef = useRef(onOpenedRepoChange);
  useEffect(() => {
    onOpenedRepoChangeRef.current = onOpenedRepoChange;
  }, [onOpenedRepoChange]);
  // まだ自動オープン待ち（一度も表示されていない復元タブ）のときは、これから開く
  // initialPath を知らせる。タブ名が「新しいタブ」にならず、表示しないまま
  // アプリを閉じてもセッションから消えないようにするため。
  useEffect(() => {
    onOpenedRepoChangeRef.current(
      opened ? repoPath : autoOpenPending ? initialPath : null,
    );
  }, [opened, repoPath, autoOpenPending, initialPath]);

  async function openRepo() {
    if (!repoPath.trim()) return;
    setNotice(null);
    // #203 サブモジュール検出: 新しいリポジトリでは改めてバナーを案内する。
    setSubmoduleBannerDismissed(false);
    // #184 Bisect: 前のリポジトリのセッション表示を引きずらない
    // （実際の状態は下の loadBisectStatus が改めて取得する）。
    setBisectStatus(null);
    setShowBisectWizard(false);
    // 先に開いた状態にしてスケルトンを表示し、その裏で初期読み込みを行う。
    setRepoLoading(true);
    setOpened(true);
    const ok = await refresh();
    // 開けなかった場合は元の入力画面に戻す（エラーはバナーで表示済み）。
    if (!ok) {
      setOpened(false);
    } else {
      // 成功したら最近使ったリポジトリ一覧に記録する。
      rememberRepo(repoPath);
    }
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

  // #169 保護ブランチの設定: 新しい一覧を丸ごと保存し、ブランチ表示を更新する。
  // set_protected_branches は core 側で正規化・検証する（空配列で既定値に戻る）。
  async function saveProtectedBranches(names: string[]) {
    try {
      await api.setProtectedBranches(repoPath, names);
      await refresh({ branches: true });
      setError(null);
    } catch (e) {
      const msg = String(e);
      setError(msg);
      showToast(msg, "error");
    }
  }

  function addProtectedBranch(name: string) {
    if (protectedBranches.includes(name)) return;
    void saveProtectedBranches([...protectedBranches, name]);
  }

  function removeProtectedBranch(name: string) {
    void saveProtectedBranches(protectedBranches.filter((n) => n !== name));
  }

  // 安全な操作はそのまま実行し、結果を更新する。
  // refresh を省略した場合は全件再取得（取り消しなど影響範囲が読めない操作向け）。
  // networkOp: true を渡すと実行中に isNetworkBusy を立て、完了・失敗時に必ず下ろす。
  // 成功・失敗の結果はトースト通知でも伝える（既存バナーと併用）。
  // #126 ネットワーク診断: networkOp が true のとき、失敗したらエラーを種別分類して
  // NetworkErrorDialog を表示する（バナーは出さずダイアログ優先にする）。
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
        if (opts.networkOp) {
          // #126 ネットワーク診断: ネットワーク操作の失敗はダイアログで種別ごとに案内する。
          // バナーは出さず、ダイアログだけ表示する（二重表示を避ける）。
          try {
            const kind = await api.classifyNetworkError(msg);
            setNetworkErrorInfo({ kind, raw: msg });
          } catch {
            // 分類自体が失敗した場合は従来のエラーバナーにフォールバックする。
            setError(msg);
            showToast(msg, "error");
          }
        } else {
          setError(msg);
          showToast(msg, "error");
        }
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
    // #269 マージ済みブランチの一括削除時に、確認ダイアログへそのまま渡す削除対象一覧。
    affectedBranches?: string[],
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
        setGuard({
          title,
          assessment,
          explanation,
          action,
          refresh: parts,
          networkOp,
          affectedFiles,
          affectedBranches,
        });
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
    const run = async () => {
      await api.commit(repoPath, msg);
      setCommitMsg("");
    };
    // #197 detached HEAD 中のコミットはブランチに属さず見失いやすいので、警告を挟む。
    if (status?.head_detached) {
      void guarded("コミット（detached HEAD）", "commit", async () => {
        await run();
        showToast("コミットしました。", "success");
      });
      return;
    }
    void exec(run, {
      successMsg: "コミットしました。",
      refresh: REFRESH_BY_OP.commit,
    });
  }

  // #197 detached HEAD: 今の位置に新しいブランチを作って安全にする（Safe 判定なので直接実行）。
  function doRescueDetachedHead(name: string) {
    void guarded(
      "ここから新しいブランチを作る",
      "rescue_detached_head",
      async () => {
        await api.rescueDetachedHead(repoPath, name);
        showToast(`ブランチ「${name}」を作って安全にしました。`, "success");
      },
      name,
    );
  }

  // 「もっと見る」: 末尾から次のページを読み、現在の一覧に追記する。
  // 検索条件があれば同じ条件で続きを取得する（条件と無関係なコミットが混ざらない）。
  //
  // #277 カーソルベースのページング: 直前のページが返したカーソルを渡すことで、
  // core 側が revwalk の続きから読む。ページを重ねても各回のコストは
  // 「すでに読んだ件数」に依存しない（skip を渡す従来方式は O(表示済み件数) を
  // 毎回払うため、無限スクロールで N ページ捲ると合計 O(N^2) になっていた）。
  // `commits.length` は、カーソルが失効していた場合にだけ core 側のフォール
  // バック（従来の skip ベース取得）で使われる。
  function loadMore() {
    if (loadingMore || !repoPath) return;
    setLoadingMore(true);
    void (async () => {
      try {
        const filter = logFilterRef.current;
        const arg = hasFilter(filter) ? filter : undefined;
        const page = await api.getLogPage(
          repoPath,
          LOG_PAGE_SIZE,
          arg,
          logCursorRef.current ?? undefined,
          commits.length,
        );
        setCommits((prev) => [...prev, ...page.commits]);
        setHasMoreCommits(page.has_more);
        logCursorRef.current = page.cursor;
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
        // ページングはリセットし、先頭ページから取り直す（＝新しいカーソルを開く）。
        // 直前のカーソルはもう使わないので手放す。
        releaseLogCursor();
        const arg = hasFilter(filter) ? filter : undefined;
        try {
          const page = await api.getLogPage(repoPath, LOG_PAGE_SIZE, arg);
          setCommits(page.commits);
          setHasMoreCommits(page.has_more);
          logCursorRef.current = page.cursor;
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
    [repoPath, releaseLogCursor],
  );

  function doUndo() {
    void exec(async () => {
      const desc = await api.undoLast(repoPath);
      // 取り消し完了はトーストで通知（exec の successMsg 経路を使わず直接呼ぶ）。
      showToast(`取り消しました: ${desc}`, "success");
    });
  }

  // #63 ショートカット: Ctrl+P で現在ブランチをプッシュする。
  function doPushCurrentBranch() {
    const name = status?.branch;
    if (!name) return;
    void guarded(
      `ブランチ「${name}」を送信`,
      "push",
      () =>
        // #167 進捗フィードバック: 送信オブジェクト数などを流す。
        networkProgress.run("プッシュ", (onProgress) =>
          api.push(
            repoPath,
            "origin",
            `refs/heads/${name}:refs/heads/${name}`,
            false,
            onProgress,
          ),
        ),
      name,
      true, // networkOp
    );
  }

  // #63 ショートカット: グローバルキーボードショートカットを登録する。
  // #263 複数タブがマウントされたままになるため、アクティブなタブだけを有効にする。
  useGlobalShortcuts(active, {
    onCommit: doCommit,
    onStageAll: () => {
      const allPaths = status
        ? [
            ...status.unstaged.map((f) => f.path),
            ...status.untracked,
          ]
        : [];
      void stageWithSensitiveCheck(allPaths, () =>
        exec(() => api.stageAll(repoPath), {
          refresh: REFRESH_BY_OP.stage,
        }),
      );
    },
    onUndo: () => {
      if (undoInfo) doUndo();
    },
    onRefresh: () => void refresh(),
    onPush: doPushCurrentBranch,
    onHelp: () => setShowShortcuts(true),
  });

  // #105 コマンドパレット: Ctrl+K / ⌘K でパレットを開く。
  // テキスト入力中でも開いてよい（issue 要件）ため、inText チェックは行わない。
  // #263 非アクティブなタブはリスナー自体を登録しない（多重発火防止）。
  useEffect(() => {
    if (!active) return;
    function onKeyDown(e: KeyboardEvent): void {
      if (isPaletteShortcut(e)) {
        e.preventDefault();
        setPaletteOpen(true);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [active]);

  // 取得（fetch）: リモートの最新情報だけを取り込む安全操作。確認なしで実行する。
  function doFetch() {
    void exec(
      async () => {
        // #167 進捗フィードバック: 受信オブジェクト数などをプログレスバーへ流す。
        const outcome = await networkProgress.run("フェッチ", (onProgress) =>
          api.fetch(repoPath, DEFAULT_REMOTE, onProgress),
        );
        const msg =
          outcome.updated_refs > 0
            ? `リモート「${outcome.remote}」から最新情報を取得しました（追跡ブランチ ${outcome.updated_refs} 件を更新）。`
            : `リモート「${outcome.remote}」を確認しました。新しい変更はありませんでした。`;
        showToast(msg, "info");
        // #268 fetch のプルーニング対応: リモートで削除済みのブランチに対応する追跡
        // ブランチを自動で整理したら、それを別トーストで伝える（ローカルブランチ本体は
        // 削除していないので info 扱い）。
        if (outcome.pruned.length > 0) {
          const names = outcome.pruned.slice(0, 3).join("、");
          const more = outcome.pruned.length > 3 ? " など" : "";
          showToast(
            `リモートで削除済みの追跡ブランチ ${outcome.pruned.length} 件（${names}${more}）を整理しました。`,
            "info",
          );
        }
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
        // #167 進捗フィードバック: fetch 部分の受信オブジェクト数などを流す。
        const outcome = await networkProgress.run("プル", (onProgress) =>
          api.pull(repoPath, DEFAULT_REMOTE, branch, onProgress),
        );
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

  // コンフリクトを「解消済み」としてマークする（解消した内容をステージ）。
  // ステージ相当の安全操作なので確認ダイアログは挟まず、status を取り直す。
  function doMarkResolved(path: string) {
    void exec(
      async () => {
        await api.markResolved(repoPath, path);
        showToast(`「${path}」を解消済みにしました。`, "success");
      },
      { refresh: REFRESH_BY_OP.stage },
    );
  }

  // ブランチのマージ。コンフリクトの可能性があるため guarded を通す。
  // コンフリクト時は FULL_REFRESH で status が更新され、ConflictWizard が自動表示される。
  function doMergeBranch(name: string) {
    void guarded(
      `ブランチ「${name}」をマージ`,
      "merge",
      async () => {
        const outcome = await api.mergeBranch(repoPath, name);
        if (outcome.kind === "up_to_date") {
          showToast(
            `「${name}」はすでにこのブランチに統合済みです。`,
            "success",
          );
        } else if (outcome.kind === "fast_forwarded") {
          showToast(
            `「${name}」をマージしました（${outcome.commit.short_id} まで前進）。`,
            "success",
          );
        } else if (outcome.kind === "merged") {
          showToast(
            `「${name}」をマージしました（マージコミット ${outcome.commit.short_id}）。`,
            "success",
          );
        } else {
          showToast(
            `「${name}」のマージ中にコンフリクトが発生しました。コンフリクト解消ウィザードで対処してください。`,
            "warning",
          );
        }
      },
      undefined,
    );
  }

  // #269 マージ済みブランチの一括削除。core 側が削除直前に再検証するため、選んだ
  // ブランチの一部だけが削除されることがある（結果はトーストで件数を案内する）。
  // 既存の delete_branch のリスク評価（Caution）をそのまま流用する
  // （候補は常に非保護・現在ブランチ以外なので Destructive にはならない）。
  function doBulkDeleteMerged(names: string[]) {
    if (names.length === 0) return;
    void guarded(
      `マージ済みブランチを削除（${names.length}件）`,
      "delete_branch",
      async () => {
        const outcome = await api.deleteBranches(repoPath, names);
        if (outcome.skipped.length === 0) {
          showToast(
            `${outcome.deleted.length}件のブランチを削除しました。`,
            "success",
          );
        } else if (outcome.deleted.length === 0) {
          showToast(
            "選んだブランチを削除できませんでした（条件を満たさないためスキップされました）。",
            "warning",
          );
        } else {
          showToast(
            `${outcome.deleted.length}件を削除、${outcome.skipped.length}件はスキップしました。`,
            "warning",
          );
        }
      },
      undefined,
      undefined,
      names,
    );
  }

  // コミットのコピー（cherry-pick）。コンフリクトの可能性があるため guarded を通す。
  function doCherryPick(commit: CommitInfo) {
    void guarded(
      "コミットをコピー（cherry-pick）",
      "cherry_pick",
      async () => {
        await api.cherryPick(repoPath, commit.id);
        showToast(`コミット ${commit.short_id} をコピーしました`, "success");
      },
      undefined,
    );
  }

  // コミットの打ち消し（revert）。履歴は書き換えず、逆向きの変更を新しいコミットとして積む。
  function doRevert(commit: CommitInfo) {
    void guarded(
      `「${commit.short_id}」を打ち消す（revert）`,
      "revert",
      async () => {
        await api.revertCommit(repoPath, commit.id);
        showToast(`コミット ${commit.short_id} を打ち消しました`, "success");
      },
    );
  }

  // #184 Bisect: 開始。detached HEAD になり作業ツリーが入れ替わるので guarded を通す。
  // 成功したら返ってきた状態をそのまま保持し、ウィザードは「進行中」画面に自動で切り替わる。
  function doBisectStart(bad: string, good: string) {
    void guarded("Bisect を開始", "bisect_start", async () => {
      const s = await api.bisectStart(repoPath, bad, good);
      setBisectStatus(s);
      showToast("Bisect を開始しました。", "success");
    });
  }

  // #184 Bisect: 判定（はい/いいえ）。ステージ相当の軽い操作として確認ダイアログは
  // 挟まず（ConflictWizard の doMarkResolved と同じ方針）、exec で直接実行する。
  function doBisectMark(commitId: string, isGood: boolean) {
    void exec(
      async () => {
        const s = await api.bisectMark(repoPath, commitId, isGood);
        setBisectStatus(s);
        if (s.is_done) {
          showToast("原因コミットが見つかりました。", "success");
        }
      },
      { refresh: REFRESH_BY_OP.bisect_start },
    );
  }

  // #184 Bisect: 終了。開始前のブランチへ戻すので guarded を通す。
  function doBisectReset() {
    void guarded("Bisect を終了", "bisect_reset", async () => {
      await api.bisectReset(repoPath);
      setBisectStatus(null);
      setShowBisectWizard(false);
      showToast("Bisect を終了し、元のブランチへ戻りました。", "success");
    });
  }

  // 変更の破棄。元に戻せない破壊的操作なので必ず guarded を通す。
  function doDiscard(path: string) {
    void guarded(`「${path}」の変更を破棄`, "discard", () =>
      api.discardPath(repoPath, path),
    );
  }

  // #81 LFS ガイド: ステージ前に LFS 候補ファイルを検査し、見つかれば LFS ガイドダイアログを挟む。
  //
  // `pathsToCheck` はステージ対象パスの一覧。`proceedAction` は警告を無視して続行するときに
  // 呼ぶ本来のステージ処理。検出されなければ `proceedAction` を直接呼ぶ。
  async function lfsCheck(
    pathsToCheck: string[],
    proceedAction: () => void,
  ) {
    if (!repoPath || pathsToCheck.length === 0) {
      proceedAction();
      return;
    }
    try {
      const candidates = await api.checkLfsCandidates(repoPath, pathsToCheck);
      if (candidates.length > 0) {
        setLfsGuard({ candidates, proceedAction });
      } else {
        proceedAction();
      }
    } catch {
      // 検出失敗はベストエフォートで無視し、従来どおりステージを続ける。
      proceedAction();
    }
  }

  // #69 + #81 ステージ前ガード: 機密チェック → LFS チェック → 実ステージ の順に走る。
  //
  // `pathsToCheck` はステージ対象パスの一覧。`proceedAction` はすべてのガードを
  // 通過したとき（または「それでもステージする」が選ばれたとき）に呼ぶ本来のステージ処理。
  // どのステージ経路でも必ずこの関数を通すこと。
  async function stageWithGuards(
    pathsToCheck: string[],
    proceedAction: () => void,
  ) {
    if (!repoPath || pathsToCheck.length === 0) {
      proceedAction();
      return;
    }
    // 機密チェック。見つかれば sensitiveGuard ダイアログを表示し、
    // 「それでもステージする」が選ばれたら LFS チェックに進む。
    try {
      const warnings = await api.checkSensitive(repoPath, pathsToCheck);
      if (warnings.length > 0) {
        // 機密ダイアログの「それでもステージする」は LFS チェックを通してから実ステージ。
        setSensitiveGuard({
          warnings,
          proceedAction: () => {
            void lfsCheck(pathsToCheck, proceedAction);
          },
        });
        return;
      }
    } catch {
      // 検出失敗はベストエフォートで無視。
    }
    // 機密なし → LFS チェック。
    await lfsCheck(pathsToCheck, proceedAction);
  }

  // 後方互換エイリアス: 既存の呼び出し箇所の名前を変えずに移行できるよう残す。
  const stageWithSensitiveCheck = stageWithGuards;

  // #70 .gitignore 管理: ファイルを無視リスト（.gitignore）に追加する。
  // .gitignore への追記だけの安全操作なので guarded は通さず exec で直接実行する。
  // 追記後に status を更新すると、未追跡ファイルは無視されて一覧から消える。
  function doIgnore(path: string) {
    void exec(() => api.addToGitignore(repoPath, path), {
      successMsg: `.gitignore に追加しました: ${path}`,
      refresh: REFRESH_BY_OP.stage,
    });
  }

  // #70 .gitignore 管理: 現在の .gitignore の内容を取得して閲覧モーダルを開く。
  async function doShowGitignore() {
    try {
      const content = await api.getGitignore(repoPath);
      setGitignore({ content });
    } catch (e) {
      showToast(String(e), "error");
    }
  }

  // #173 .gitignore バリデーション: モーダルの手入力欄から任意のパターンを追加する。
  // バリデーション・重複チェックはモーダル側で（入力中に）済ませたうえで呼ばれる想定。
  // 追加後はモーダルの表示内容を最新化し、無視によって未追跡ファイルが一覧から
  // 消えることがあるので status も更新する。
  function doAddGitignorePattern(pattern: string): Promise<void> {
    return exec(
      async () => {
        await api.addToGitignore(repoPath, pattern);
        const content = await api.getGitignore(repoPath);
        setGitignore({ content });
      },
      {
        successMsg: `.gitignore に追加しました: ${pattern}`,
        refresh: REFRESH_BY_OP.stage,
      },
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
  // コンフリクトが起きた場合は success トーストを出さず、warning でウィザードへの
  // 誘導を伝える（status の再取得は REFRESH_BY_OP.stash_apply が担うので、
  // conflicts が検出されると ConflictWizard が自動表示される）。
  function doStashApply(index: number) {
    void guarded("退避を適用", "stash_apply", async () => {
      const outcome = await api.stashApply(repoPath, index);
      if (outcome.conflicted) {
        showToast(
          "退避の取り出し中にコンフリクトが発生しました。コンフリクト解消ウィザードで対処してください。",
          "warning",
        );
      } else {
        showToast("退避した変更を取り出しました（退避は一覧に残しています）。", "success");
      }
    });
  }

  // ブランチ切り替え。未コミットの変更が邪魔で切り替えできなかった（core が Blocked を
  // 返した）ときは、エラーにせず「退避して切り替える」提案（確認ダイアログ）を出す。
  // 変更は core 側で退避 → 切り替え → 復元され、やめる（キャンセル）なら何も変わらない。
  function doSwitchBranch(name: string) {
    void guarded(
      `ブランチ「${name}」へ切り替え`,
      "switch_branch",
      async () => {
        try {
          await api.switchBranch(repoPath, name);
        } catch (e) {
          if (isSwitchBlockedByChanges(e)) {
            offerSwitchWithStash(name);
            return;
          }
          throw e;
        }
      },
      name,
    );
  }

  // 「退避して切り替える」の確認。コンフリクトしたときは退避が一覧に残るので、
  // stash_pop と同じく stashPopConflict へ記録して既存の後片付け導線に乗せる。
  function offerSwitchWithStash(name: string) {
    void guarded(
      `変更を退避してブランチ「${name}」へ切り替え`,
      "switch_branch_with_stash",
      async () => {
        let outcome;
        try {
          outcome = await api.switchBranchWithStash(repoPath, name);
        } catch (e) {
          // 「切り替えは済んだが戻せなかった」場合もあるので、状態を取り直してから伝える。
          await refresh(FULL_REFRESH);
          throw e;
        }
        if (!outcome.stashed) {
          showToast(`ブランチ「${name}」へ切り替えました。`, "success");
        } else if (outcome.conflicted) {
          // 退避は今作ったばかりで先頭（index 0）にある。
          const list = await api.getStashes(repoPath);
          if (list[0]) {
            setStashPopConflict({
              id: list[0].id,
              message: list[0].message,
              seenConflicts: false,
            });
          }
          showToast(
            "切り替え後、退避した変更を戻すときにコンフリクトが発生しました。コンフリクト解消ウィザードで対処してください（退避は一覧に残しています）。",
            "warning",
          );
        } else {
          showToast(
            `変更を退避してブランチ「${name}」へ切り替え、変更を戻しました。`,
            "success",
          );
        }
      },
      name,
    );
  }

  // 退避の取り出し（pop・コンフリクトが無ければ一覧から削除）。
  // コンフリクトの可能性があるため guarded を通す。コンフリクト時は退避を
  // 一覧に残すので、解消し終えたら「退避を削除する / 残す」を選べるように
  // stashPopConflict へ記録する（#156）。
  function doStashPop(index: number) {
    const target = stashes.find((s) => s.index === index);
    const message = target?.message ?? "";
    void guarded("退避を取り出す", "stash_pop", async () => {
      const outcome = await api.stashPop(repoPath, index);
      if (outcome.conflicted && target) {
        setStashPopConflict({ id: target.id, message, seenConflicts: false });
        showToast(
          "退避の取り出し中にコンフリクトが発生しました。コンフリクト解消ウィザードで対処してください（退避はいったん一覧に残しています）。",
          "warning",
        );
      } else {
        showToast("退避した変更を取り出し、一覧から取り除きました。", "success");
      }
    });
  }

  // #156 stash_pop のコンフリクトを解消し終えたあと、不要になった退避を削除する。
  // 破壊的（元に戻せない）ので guarded を通す（stash_drop は Caution）。
  function doStashDrop(stashId: string) {
    void guarded("退避を削除", "stash_drop", async () => {
      await api.stashDrop(repoPath, stashId);
      showToast("退避を削除しました。", "success");
      setStashPopConflict(null);
    });
  }

  // 退避の差分プレビュー。退避を適用しない安全な読み取り操作なので guarded は通さない。
  async function loadStashDiff(index: number): Promise<FileChange[]> {
    try {
      return await api.stashDiff(repoPath, index);
    } catch (e) {
      showToast(`退避の差分を取得できませんでした: ${String(e)}`, "error");
      throw e;
    }
  }

  // タグの作成。安全操作なので guarded はダイアログを出さずそのまま実行する。
  function doCreateTag(name: string, message?: string) {
    void guarded("タグを作成", "create_tag", async () => {
      await api.createTag(repoPath, name, undefined, message);
      showToast(`タグ「${name}」を作成しました。`, "success");
    });
  }

  // タグの削除。注意操作なので guarded を通す（直後に Undo で復元できる）。
  function doDeleteTag(name: string) {
    void guarded(`タグ「${name}」の削除`, "delete_tag", async () => {
      await api.deleteTag(repoPath, name);
      showToast(`タグ「${name}」を削除しました。`, "success");
    });
  }

  // #71 リモート管理: 追加・URL変更・削除。
  function doAddRemote(name: string, url: string) {
    void exec(
      async () => {
        await api.addRemote(repoPath, name, url);
        await loadRemotes();
      },
      { successMsg: `リモート「${name}」を追加しました。`, refresh: {} },
    );
  }

  function doSetRemoteUrl(name: string, url: string) {
    void exec(
      async () => {
        await api.setRemoteUrl(repoPath, name, url);
        await loadRemotes();
      },
      { successMsg: `リモート「${name}」の URL を変更しました。`, refresh: {} },
    );
  }

  function doRemoveRemote(name: string) {
    void guarded(`リモート「${name}」の削除`, "remove_remote", async () => {
      await api.removeRemote(repoPath, name);
      showToast(`リモート「${name}」を削除しました。`, "success");
      await loadRemotes();
    });
  }

  // リベース対象のチェックボックスを切り替える。
  function toggleCommitSelect(id: string) {
    setSelectedCommitIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  // 選択を解除する（ウィザードを閉じる・実行後に呼ぶ）。
  function clearCommitSelection() {
    setSelectedCommitIds(new Set());
  }

  // 履歴の表示順（新しい順）で選択中のコミットを取り出す。
  const selectedCommits = commits.filter((c) => selectedCommitIds.has(c.id));

  // squash: 選んだ連続コミットを1つにまとめる。破壊的なので guarded を通す。
  function doSquash(commitOids: string[], message: string) {
    setShowRebase(false);
    void guarded("コミット履歴の整理（リベース）", "rebase", async () => {
      await api.squashCommits(repoPath, commitOids, message);
      clearCommitSelection();
      showToast("選んだコミットを1つにまとめました。", "success");
    });
  }

  // reword: 最新コミットのメッセージを書き換える。破壊的なので guarded を通す。
  function doReword(message: string) {
    setShowRebase(false);
    void guarded("コミット履歴の整理（リベース）", "rebase", async () => {
      await api.rewordCommit(repoPath, message);
      clearCommitSelection();
      showToast("コミットメッセージを書き換えました。", "success");
    });
  }

  // #105 コマンドパレット: リポジトリが開かれているときだけ使えるコマンド一覧。
  // 既存の exec / guarded を通すことで安全チェックを二重化しない。
  const paletteCommands: PaletteCommand[] = opened
    ? [
        {
          id: "stage-all",
          label: "すべてステージ",
          description: "変更ファイルをすべてステージエリアに追加する",
          run: () => {
            const allPaths = status
              ? [
                  ...status.unstaged.map((f) => f.path),
                  ...status.untracked,
                ]
              : [];
            void stageWithSensitiveCheck(allPaths, () =>
              exec(() => api.stageAll(repoPath), {
                refresh: REFRESH_BY_OP.stage,
              }),
            );
          },
        },
        {
          id: "commit",
          label: "コミットする",
          description: "ステージ済みの変更をコミットする",
          run: doCommit,
        },
        {
          id: "amend",
          label: "直前を修正（amend）",
          description: "直前のコミットに変更を追加・メッセージを書き換える",
          run: doAmend,
        },
        {
          id: "undo",
          label: "取り消す（undo）",
          description: "直前の操作を Undo する",
          run: () => {
            if (undoInfo) doUndo();
          },
        },
        {
          id: "fetch",
          label: "取得（fetch）",
          description: "リモートの最新情報だけを取得する（作業中ファイルは変わらない）",
          run: doFetch,
        },
        {
          id: "pull",
          label: "取り込む（pull）",
          description: "リモートの変更を安全に取り込む（fast-forward のみ）",
          run: doPull,
        },
        {
          id: "refresh",
          label: "履歴を更新",
          description: "ステータス・ブランチ・履歴を再取得する",
          run: () => void refresh(),
        },
      ]
    : [];

  // タイトルバーに出すリポジトリ名（パスの末尾のフォルダ名）。
  const repoName =
    repoPath.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || repoPath;

  if (!opened) {
    return (
      <WelcomeScreen
        repoPath={repoPath}
        setRepoPath={setRepoPath}
        onOpen={openRepo}
        error={error}
      />
    );
  }

  return (
    <div className="app">
      {/* #64 オンボーディング */}
      <OnboardingWizard onClose={() => {}} />
      <header className="topbar">
        <div className="repo-info">
          <strong>noobGit</strong>
          <span className="repo-name" title={repoPath}>
            <Icon name="repo" /> {repoName}
          </span>
          <span className="current-branch">
            {status?.branch ? (
              <>
                <Icon name="branch" /> {status.branch}
              </>
            ) : (
              "(ブランチ不明)"
            )}
          </span>
        </div>
        <div className="topbar-actions">
          <button
            className="btn btn-small"
            onClick={() => setShowIdentity(true)}
            title="コミット作者の名前とメールアドレスを設定します"
          >
            <Icon name="identity" /> 名前/メール
          </button>
          <ThemeToggle />
          {/* 設定（表示言語など）を開くボタン */}
          <button
            className="btn btn-small"
            onClick={() => setShowSettings(true)}
            title={t("settings.open")}
            aria-label={t("settings.open")}
          >
            <Icon name="settings" />
          </button>
          {/* #63 ショートカット: ヘルプを開くボタン */}
          <button
            className="btn btn-small"
            onClick={() => setShowShortcuts(true)}
            title="キーボードショートカット一覧 [? / F1]"
            aria-label="キーボードショートカット一覧"
          >
            <Icon name="help" />
          </button>
          <button
            className="btn btn-small"
            onClick={() => {
              setOpened(false);
              setStatus(null);
              setIdentity(null);
              // #203 サブモジュール検出: 次のリポジトリでは改めて案内する。
              setSubmoduleBannerDismissed(false);
              // 次に開くリポジトリは初期件数から軽く表示し直す。
              setCommits([]);
              setHasMoreCommits(false);
              // 履歴の絞り込みもリセットする。
              setLogFilter({});
              logFilterRef.current = {};
              setStashes([]);
              setTags([]);
              setSelectedFile(null);
              setDiff(null);
              setCompareBase(null);
              setCompareTarget(null);
              setCommitDiffs(null);
              // 次のリポジトリはファイルステータスから見せる。
              setView("status");
            }}
          >
            別のリポジトリ
          </button>
        </div>
      </header>

      {/* SourceTree 風の大型ツールバー: 主要操作をアイコン＋ラベルで横並びにする */}
      <div className="toolbar" role="toolbar" aria-label="主要操作">
        {/* #104 操作説明ツールチップ */}
        <ExplainTooltip op="commit">
          <button
            className="toolbar-btn"
            onClick={goToCommitBox}
            title="コミット入力欄へ移動します [Ctrl+Enter でコミット]"
          >
            <span className="toolbar-btn-icon">
              <Icon name="commit" />
            </span>
            <span className="toolbar-btn-label">コミット</span>
          </button>
        </ExplainTooltip>
        <span className="toolbar-sep" />
        {/* #104 操作説明ツールチップ */}
        <ExplainTooltip op="pull">
          <button
            // #274 危険度カラー: pull は常に注意（安全に進められるときだけ取り込むが、
            // 分岐時は中断する可能性があるため）。
            className={`toolbar-btn ${riskTriggerClassFor(riskLevels, "pull")}`}
            onClick={doPull}
            disabled={isNetworkBusy}
            title="リモートの変更を取り込みます（安全に進められるときだけ取り込みます）"
          >
            <span className="toolbar-btn-icon">
              {isNetworkBusy ? (
                <span className="network-spinner">
                  <Icon name="pull" />
                </span>
              ) : (
                <Icon name="pull" />
              )}
            </span>
            <span className="toolbar-btn-label">プル</span>
          </button>
        </ExplainTooltip>
        {/* #104 操作説明ツールチップ */}
        <ExplainTooltip op="push">
          <button
            // #274 危険度カラー: 保護ブランチ（main/master等）への push だけ注意色になる。
            className={`toolbar-btn ${riskTriggerClassFor(riskLevels, "push", status?.branch ?? undefined)}`}
            onClick={doPushCurrentBranch}
            disabled={isNetworkBusy || !status?.branch}
            title="現在のブランチをリモートへ送信します [Ctrl+P]"
          >
            <span className="toolbar-btn-icon">
              {isNetworkBusy ? (
                <span className="network-spinner">
                  <Icon name="push" />
                </span>
              ) : (
                <Icon name="push" />
              )}
            </span>
            <span className="toolbar-btn-label">プッシュ</span>
          </button>
        </ExplainTooltip>
        {/* #104 操作説明ツールチップ */}
        <ExplainTooltip op="fetch">
          <button
            className="toolbar-btn"
            onClick={doFetch}
            disabled={isNetworkBusy}
            title="リモートの最新情報だけを取得します（作業中のファイルは変わりません）"
          >
            <span className="toolbar-btn-icon">
              {isNetworkBusy ? (
                <span className="network-spinner">
                  <Icon name="fetch" />
                </span>
              ) : (
                <Icon name="fetch" />
              )}
            </span>
            <span className="toolbar-btn-label">フェッチ</span>
          </button>
        </ExplainTooltip>
        <span className="toolbar-sep" />
        <button
          className="toolbar-btn"
          onClick={() => setView("branches")}
          title="ブランチの作成・切り替え・マージを行います"
        >
          <span className="toolbar-btn-icon">
            <Icon name="branch" />
          </span>
          <span className="toolbar-btn-label">ブランチ</span>
        </button>
        <button
          className="toolbar-btn"
          onClick={() => setView("stashes")}
          title="作業中の変更を一時退避（スタッシュ）します"
        >
          <span className="toolbar-btn-icon">
            <Icon name="stash" />
          </span>
          <span className="toolbar-btn-label">スタッシュ</span>
        </button>
        <button
          className="toolbar-btn"
          onClick={() => setView("tags")}
          title="タグの作成・削除を行います"
        >
          <span className="toolbar-btn-icon">
            <Icon name="tag" />
          </span>
          <span className="toolbar-btn-label">タグ</span>
        </button>
        <span className="toolbar-spacer" />
        {undoInfo && (
          // #104 操作説明ツールチップ: undoInfo.op を使って対応する説明を表示する。
          <ExplainTooltip op={undoInfo.op}>
            <button
              className="toolbar-btn toolbar-btn-undo"
              onClick={doUndo}
              title={`直前の操作を取り消します: ${undoInfo.description} [Ctrl+Z]`}
            >
              <span className="toolbar-btn-icon">
                <Icon name="undo" />
              </span>
              <span className="toolbar-btn-label">取り消す</span>
            </button>
          </ExplainTooltip>
        )}
        <button
          className="toolbar-btn"
          onClick={() => void refresh()}
          title="ステータスを再取得します [Ctrl+R]"
        >
          <span className="toolbar-btn-icon">
            <Icon name="refresh" />
          </span>
          <span className="toolbar-btn-label">更新</span>
        </button>
      </div>

      {/* #167 進捗フィードバック: fetch / pull / push 実行中の進捗バー。 */}
      <NetworkProgressBar state={networkProgress.state} />

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
            <Icon name="greeting" /> コミットには「名前」と「メールアドレス」の設定が必要です。
          </span>
          <button
            className="btn btn-small"
            onClick={() => setShowIdentity(true)}
          >
            設定する
          </button>
        </div>
      )}

      {/* #203 サブモジュール検出: 未対応であることと、見慣れない表示の意味を説明する */}
      {status?.has_submodules && !submoduleBannerDismissed && (
        <div className="banner setup">
          <span>
            <Icon name="submodule" /> このリポジトリは「リポジトリの中に別のリポジトリ」（サブモジュール）を含んでいます。
            noobGit では中身の操作（クローン・更新・コミットなど）はできません。
            サブモジュールの変更は、ターミナルや他の Git ツールで扱ってください。
          </span>
          <button
            className="btn btn-small"
            onClick={() => setSubmoduleBannerDismissed(true)}
          >
            閉じる
          </button>
        </div>
      )}

      {/* #197 detached HEAD: 状態の説明と復帰アクション。Bisect 中は detached が正常なので出さない */}
      {status?.head_detached && !bisectStatus && (
        <DetachedHeadBanner
          info={status.detached_info}
          onRescue={doRescueDetachedHead}
          onReturn={(name) =>
            void guarded(
              `ブランチ「${name}」へ戻る`,
              "switch_branch",
              () => api.switchBranch(repoPath, name),
              name,
            )
          }
        />
      )}

      {/* #184 Bisect: セッション進行中であることをタブ上で常に分かるようにするバナー。
          ウィザードを閉じていてもここから再度開ける。 */}
      {bisectStatus && !showBisectWizard && (
        <div className="banner setup bisect-banner">
          <span>
            <Icon name="bisect" />{" "}
            {bisectStatus.is_done
              ? "Bisect: 原因コミットが見つかっています。"
              : `Bisect 進行中（detached HEAD） ・ 残り約 ${bisectStatus.remaining_steps} 回`}
          </span>
          <button
            className="btn btn-small"
            onClick={() => setShowBisectWizard(true)}
          >
            続ける
          </button>
        </div>
      )}

      {/* SourceTree 風レイアウト: 左サイドバー + メインビュー */}
      <div className="workspace">
        <Sidebar
          view={view}
          onSelectView={setView}
          changeCount={
            status
              ? status.staged.length +
                status.unstaged.length +
                status.untracked.length
              : 0
          }
          conflictCount={status?.conflicted.length ?? 0}
          branches={branches}
          tags={tags}
          remotes={remotes}
          stashes={stashes}
          undoCount={undoJournal.length}
          onSwitchBranch={(name) => doSwitchBranch(name)}
        />

        <main className="main-view">
          {/* ファイルステータス: 変更一覧 + 差分（スクロール）と、下部固定のコミット欄 */}
          {view === "status" && (
          <div className="view-status">
          <div className="view-scroll">
          <AnimatePresence mode="wait">
            {showSkeleton ? (
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
                    repoPath={repoPath}
                    discardRiskClass={riskTriggerClassFor(riskLevels, "discard")}
                    onSelect={selectFile}
                    onStageAll={() => {
                      // stage_all の対象: 未ステージ（追跡済み）+ 未追跡ファイル全て。
                      const allPaths = status
                        ? [
                            ...status.unstaged.map((f) => f.path),
                            ...status.untracked,
                          ]
                        : [];
                      void stageWithSensitiveCheck(allPaths, () =>
                        exec(() => api.stageAll(repoPath), {
                          refresh: REFRESH_BY_OP.stage,
                        }),
                      );
                    }}
                    onStagePath={(p) => {
                      void stageWithSensitiveCheck([p], () =>
                        exec(() => api.stagePath(repoPath, p), {
                          refresh: REFRESH_BY_OP.stage,
                        }),
                      );
                    }}
                    onUnstage={(p) =>
                      void exec(() => api.unstage(repoPath, p), {
                        refresh: REFRESH_BY_OP.unstage,
                      })
                    }
                    onDiscard={doDiscard}
                    onShowHistory={(p) => setHistoryPath(p)}
                    onBlame={(p) => void openBlame(p)}
                    onStagePaths={/* #127 マルチ選択 */(paths) => {
                      void stageWithSensitiveCheck(paths, () =>
                        exec(
                          async () => {
                            for (const p of paths) await api.stagePath(repoPath, p);
                          },
                          { refresh: REFRESH_BY_OP.stage },
                        ),
                      );
                    }}
                    onUnstagePaths={(paths) =>
                      void exec(
                        async () => {
                          for (const p of paths) await api.unstage(repoPath, p);
                        },
                        { refresh: REFRESH_BY_OP.unstage },
                      )
                    }
                    onDiscardPaths={(paths) =>
                      void guarded(
                        `選択した ${paths.length} 件の変更を破棄`,
                        "discard",
                        async () => {
                          for (const p of paths) await api.discardPath(repoPath, p);
                        },
                      )
                    }
                    // #125 hunk ステージ
                    onStageHunk={(p, h) =>
                      void exec(() => api.stageHunk(repoPath, p, h), {
                        refresh: REFRESH_BY_OP.stage,
                      })
                    }
                    // #158 hunk アンステージ
                    onUnstageHunk={(p, h) =>
                      void exec(() => api.unstageHunk(repoPath, p, h), {
                        refresh: REFRESH_BY_OP.unstage,
                      })
                    }
                    // #70 .gitignore 管理
                    onIgnore={doIgnore}
                    onShowGitignore={() => void doShowGitignore()}
                  />
                </motion.div>
              )
            )}
          </AnimatePresence>

          {conflicts.length > 0 && (
            <ConflictWizard
              conflicts={conflicts}
              selectedPath={
                selectedFile?.source === "conflicted"
                  ? selectedFile.path
                  : null
              }
              onSelect={(p) => selectFile(p, "conflicted")}
              onMarkResolved={doMarkResolved}
            />
          )}

          {/* #156 stash_pop がコンフリクトで残した退避の後片付け。
              コンフリクトが（このタブで）すべて解消された時点でだけ表示する。 */}
          {stashPopConflict?.seenConflicts && conflicts.length === 0 && (
            <StashPopFollowUp
              message={stashPopConflict.message}
              onDelete={() => doStashDrop(stashPopConflict.id)}
              onKeep={() => setStashPopConflict(null)}
            />
          )}

          <DiffPanel
            selection={selectedFile}
            diff={diff}
            loading={diffLoading}
            onStageHunk={(hunkHeader) =>
              void exec(
                () => api.stageHunk(repoPath, selectedFile!.path, hunkHeader),
                { refresh: REFRESH_BY_OP.stage },
              )
            }
            onUnstageHunk={(hunkHeader) =>
              void exec(
                () =>
                  api.unstageHunk(repoPath, selectedFile!.path, hunkHeader),
                { refresh: REFRESH_BY_OP.unstage },
              )
            }
          />

          </div>

          {/* SourceTree 風にコミット欄はビュー下部へ固定する */}
          <div className="commit-dock">
            <div className="commit-dock-head">コミットメッセージ</div>
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
            <div className="commit-body-wrap">
              {/* #172 初回フォーカス時のみ表示する 50 文字ガイドラインの案内 */}
              <AnimatePresence>
                {showSubjectHint && (
                  <motion.div
                    className="commit-subject-hint"
                    role="tooltip"
                    initial={{ opacity: 0, y: 4 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: 4 }}
                    transition={transitions.fast}
                  >
                    <Icon name="hint" /> 件名は 50 文字以内が推奨です
                  </motion.div>
                )}
              </AnimatePresence>
              <textarea
                ref={commitInput}
                value={commitMsg}
                placeholder="このコミットで何をしたか書きましょう（例: ログイン画面を追加）"
                onChange={(e) => setCommitMsg(e.target.value)}
                onFocus={handleCommitFocus}
                onKeyDown={handleCommitKeyDown}
                // #185 フォーカスが外れたら候補を閉じる。候補クリックは onMouseDown
                // 側で preventDefault しているので blur は発生せず、選択操作は壊れない。
                onBlur={() => setShowSuggestDropdown(false)}
                role="combobox"
                aria-expanded={showSuggestDropdown}
                aria-controls="commit-suggest-listbox"
                aria-autocomplete="list"
              />
              {/* #172 本文の 72 文字折り返し目安（薄い縦線）。可変幅フォントのため
                  厳密な位置ではなくあくまで目安。 */}
              <div className="commit-col-guide" aria-hidden="true" />
              {/* #185 コミットメッセージ件名の補完候補（過去の履歴 + Conventional Commits）。
                  ↑↓ で選択、Tab / Enter で確定、Esc で閉じる。 */}
              <AnimatePresence>
                {showSuggestDropdown && subjectSuggestions.length > 0 && (
                  <motion.ul
                    id="commit-suggest-listbox"
                    className="commit-suggest-dropdown"
                    role="listbox"
                    initial={{ opacity: 0, y: -4 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: -4 }}
                    transition={transitions.fast}
                  >
                    {subjectSuggestions.map((item, i) => (
                      <li
                        key={`${item.text}-${i}`}
                        role="option"
                        aria-selected={i === suggestIndex}
                        className={
                          "commit-suggest-item" +
                          (i === suggestIndex ? " active" : "")
                        }
                        // mousedown で確定する: textarea の blur（それに伴う
                        // ドロップダウンの消滅）より先に発火させ、フォーカスを
                        // 保ったままクリックで確定できるようにする。
                        onMouseDown={(e) => {
                          e.preventDefault();
                          confirmSuggestion(i);
                        }}
                        onMouseEnter={() => setSuggestIndex(i)}
                      >
                        <span className="commit-suggest-text">{item.text}</span>
                        {item.desc && (
                          <span className="commit-suggest-desc">{item.desc}</span>
                        )}
                      </li>
                    ))}
                  </motion.ul>
                )}
              </AnimatePresence>
            </div>
            {/* 文字数カウンター (#77, #172: コードポイント単位で数え 50/72 の目安を表示) */}
            {commitMsg.length > 0 && (() => {
              const len = getSubjectLength(commitMsg);
              const color =
                len <= SUBJECT_LIMIT
                  ? "var(--safe)"
                  : len <= BODY_WRAP_LIMIT
                    ? "var(--caution)"
                    : "var(--destructive)";
              const hint =
                len > BODY_WRAP_LIMIT
                  ? "短くまとめると見やすくなります"
                  : len > SUBJECT_LIMIT
                    ? "本文への移動を検討してください"
                    : null;
              return (
                <div className="char-count">
                  {/* 50 文字超過で警告色へなめらかに変化する (framer motion) */}
                  <motion.span
                    animate={{ color }}
                    transition={transitions.normal}
                    style={{ fontWeight: 600 }}
                  >
                    {len}
                  </motion.span>
                  <span className="char-limit">/ {SUBJECT_LIMIT} 字推奨</span>
                  {hint && <span className="char-hint">{hint}</span>}
                </div>
              );
            })()}
            <div className="commit-actions">
              {/* #104 操作説明ツールチップ */}
              <ExplainTooltip op="commit">
                <button
                  className="btn"
                  onClick={doCommit}
                  disabled={!commitMsg.trim()}
                  title="変更をコミットします [Ctrl+Enter]"
                >
                  コミットする
                </button>
              </ExplainTooltip>
              {/* #104 操作説明ツールチップ */}
              <ExplainTooltip op="amend_commit">
                <button
                  // #274 危険度カラー: 直前のコミットが送信(push)済みなら destructive になる。
                  className={`btn btn-small ${riskTriggerClassFor(riskLevels, "amend_commit")}`}
                  onClick={doAmend}
                  disabled={commits.length === 0}
                  title="直前のコミットを書き換えます。メッセージ欄が空ならメッセージはそのまま、ステージした変更を取り込みます。"
                >
                  直前を修正
                </button>
              </ExplainTooltip>
            </div>
          </div>

          </div>
          )}

          {/* 履歴: コミット一覧とコミット間差分 */}
          {view === "history" && (
          <div className="view-scroll">
          <AnimatePresence mode="wait">
            {showSkeleton ? (
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
                  onGoToCommit={goToCommitBox}
                  onReset={(c) =>
                    void guarded(
                      `「${c.short_id}」までハードリセット`,
                      "reset_hard",
                      () => api.resetHard(repoPath, c.id),
                    )
                  }
                  onCompareSelect={onCompareSelect}
                  compareBaseId={compareBase?.id ?? null}
                  onCherryPick={doCherryPick}
                  onRevert={doRevert}
                  selectedIds={selectedCommitIds}
                  onToggleSelect={toggleCommitSelect}
                  onStartRebase={() => setShowRebase(true)}
                  repoPath={repoPath}
                  onResetTo={(newOid) =>
                    void guarded(
                      `reflog: コミット ${newOid.slice(0, 7)} までハードリセット`,
                      "reset_hard",
                      () => api.resetHard(repoPath, newOid),
                    )
                  }
                  onStartBisect={() => setShowBisectWizard(true)}
                  // #274 危険度カラー
                  resetRiskClass={riskTriggerClassFor(riskLevels, "reset_hard")}
                  cherryPickRiskClass={riskTriggerClassFor(riskLevels, "cherry_pick")}
                  revertRiskClass={riskTriggerClassFor(riskLevels, "revert")}
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
          </div>
          )}

          {/* ブランチ: 作成・切り替え・マージ・送信 */}
          {view === "branches" && (
          <div className="view-scroll">
          <AnimatePresence mode="wait">
            {showSkeleton ? (
              <motion.div
                key="branches-skeleton"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.15 }}
              >
                <BranchPanelSkeleton />
              </motion.div>
            ) : (
              <motion.div
                key="branches-content"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                transition={{ duration: 0.2 }}
              >
              <BranchPanel
                branches={branches}
                graph={branchGraph}
                mergedBranches={mergedBranches}
                onBulkDeleteMerged={doBulkDeleteMerged}
                networkBusy={isNetworkBusy}
                protectedBranches={protectedBranches}
                onAddProtected={addProtectedBranch}
                onRemoveProtected={removeProtectedBranch}
                riskLevels={riskLevels}
                onCreate={(name) =>
                  void guarded("ブランチを作成", "create_branch", () =>
                    api.createBranch(repoPath, name),
                  )
                }
                onSwitch={(name) => doSwitchBranch(name)}
                onDelete={(name) =>
                  void guarded(
                    `ブランチ「${name}」を削除`,
                    "delete_branch",
                    () => api.deleteBranch(repoPath, name),
                    name,
                  )
                }
                onMerge={(name) => doMergeBranch(name)}
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
          </div>
          )}

          {/* タグ: 作成・削除 */}
          {view === "tags" && (
          <div className="view-scroll">
          <AnimatePresence mode="wait">
            {showSkeleton ? (
              <motion.div
                key="tags-skeleton"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.15 }}
              >
                <TagPanelSkeleton />
              </motion.div>
            ) : (
              <motion.div
                key="tags-content"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                transition={{ duration: 0.2 }}
              >
                <TagPanel
                  tags={tags}
                  canTag={commits.length > 0}
                  onCreate={doCreateTag}
                  onDelete={doDeleteTag}
                  deleteRiskClass={riskTriggerClassFor(riskLevels, "delete_tag")}
                />
              </motion.div>
            )}
          </AnimatePresence>
          </div>
          )}

          {/* #71 リモート管理: リモートの一覧・追加・URL変更・削除。 */}
          {view === "remotes" && (
          <div className="view-scroll">
            <RemotePanel
              remotes={remotes}
              onAdd={doAddRemote}
              onSetUrl={doSetRemoteUrl}
              onRemove={doRemoveRemote}
              removeRiskClass={riskTriggerClassFor(riskLevels, "remove_remote")}
            />
          </div>
          )}

          {/* スタッシュ（退避）: 保存・適用・取り出し */}
          {view === "stashes" && (
          <div className="view-scroll">
          <AnimatePresence mode="wait">
            {showSkeleton ? (
              <motion.div
                key="stashes-skeleton"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.15 }}
              >
                <StashPanelSkeleton />
              </motion.div>
            ) : (
              <motion.div
                key="stashes-content"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                transition={{ duration: 0.2 }}
              >
                <StashPanel
                  stashes={stashes}
                  canStash={!!status && !status.is_clean}
                  onSave={doStashSave}
                  onApply={doStashApply}
                  onPop={doStashPop}
                  onLoadDiff={loadStashDiff}
                  applyRiskClass={riskTriggerClassFor(riskLevels, "stash_apply")}
                  popRiskClass={riskTriggerClassFor(riskLevels, "stash_pop")}
                />
              </motion.div>
            )}
          </AnimatePresence>
          </div>
          )}

          {/* #48 Undo タイムライン: 取り消し履歴を新しい順で表示する。 */}
          {view === "undo" && (
          <div className="view-scroll">
            <UndoTimeline entries={[...undoJournal].reverse()} />
          </div>
          )}
        </main>
      </div>

      {guard && (
        <ConfirmDialog
          title={guard.title}
          assessment={guard.assessment}
          explanation={guard.explanation}
          affectedFiles={guard.affectedFiles}
          affectedBranches={guard.affectedBranches}
          onConfirm={() => void confirmGuard()}
          onCancel={() => setGuard(null)}
        />
      )}

      {historyPath && (
        <FileHistoryView
          repoPath={repoPath}
          path={historyPath}
          onClose={() => setHistoryPath(null)}
          onRestore={(commitId) => {
            const path = historyPath;
            void guarded(
              `「${path}」をこのコミット時点に復元`,
              "restore_file",
              async () => {
                await api.restoreFileFromCommit(repoPath, commitId, path);
                showToast(`「${path}」を過去のコミット時点に復元してステージしました。`, "success");
              },
            );
          }}
          restoreRiskClass={riskTriggerClassFor(riskLevels, "restore_file")}
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

      {showRebase && (
        <RebaseWizard
          selected={selectedCommits}
          onSquash={doSquash}
          onReword={doReword}
          onCancel={() => setShowRebase(false)}
        />
      )}

      {/* #184 Bisect: バグ混入コミットの二分探索ウィザード。バナーからも開閉できる
          ので、bisectStatus の有無とは無関係に showBisectWizard だけで表示を決める。 */}
      {showBisectWizard && (
        <BisectWizard
          repoPath={repoPath}
          status={bisectStatus}
          commits={commits}
          onStart={doBisectStart}
          onMark={doBisectMark}
          onReset={doBisectReset}
          onClose={() => setShowBisectWizard(false)}
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

      {/* #63 ショートカット: ヘルプダイアログ */}
      {showShortcuts && (
        <ShortcutHelpDialog onClose={() => setShowShortcuts(false)} />
      )}

      {/* 設定（表示言語など）ダイアログ */}
      {showSettings && (
        <SettingsDialog onClose={() => setShowSettings(false)} />
      )}

      {/* #70 .gitignore 管理: 閲覧モーダル */}
      {gitignore && (
        <GitignoreModal
          content={gitignore.content}
          repoPath={repoPath}
          onAdd={doAddGitignorePattern}
          onClose={() => setGitignore(null)}
        />
      )}

      {/* #105 コマンドパレット */}
      <CommandPalette
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        commands={paletteCommands}
      />

      {/* #126 ネットワーク診断: fetch / pull / push のエラー種別ダイアログ */}
      {networkErrorInfo && (
        <NetworkErrorDialog
          kind={networkErrorInfo.kind}
          raw={networkErrorInfo.raw}
          onClose={() => setNetworkErrorInfo(null)}
        />
      )}

      {/* #69 機密ファイル検出: ステージ前に機密ファイルが見つかった場合の警告ダイアログ */}
      {sensitiveGuard && (
        <SensitiveWarningDialog
          warnings={sensitiveGuard.warnings}
          onStageAnyway={() => {
            const proceed = sensitiveGuard.proceedAction;
            setSensitiveGuard(null);
            proceed();
          }}
          onAddToGitignore={(pattern) => {
            doIgnore(pattern);
            // .gitignore に追加したらダイアログは閉じる（残りのファイルは再チェックせず
            // ユーザーに委ねる）。ステージもキャンセルする。
            setSensitiveGuard(null);
          }}
          onCancel={() => setSensitiveGuard(null)}
        />
      )}

      {/* #81 LFS ガイド: ステージ前に大容量・バイナリファイルが見つかった場合のガイドダイアログ */}
      {lfsGuard && (
        <LfsGuideDialog
          candidates={lfsGuard.candidates}
          onStageAnyway={() => {
            const proceed = lfsGuard.proceedAction;
            setLfsGuard(null);
            proceed();
          }}
          onAddToGitignore={(pattern) => {
            doIgnore(pattern);
            // .gitignore に追加したらダイアログは閉じる。ステージもキャンセルする。
            setLfsGuard(null);
          }}
          onCancel={() => setLfsGuard(null)}
        />
      )}
    </div>
  );
}
