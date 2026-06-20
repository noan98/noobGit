import { invoke } from "@tauri-apps/api/core";

// --- core の serde 型に対応する TypeScript 型 -----------------------------

export type ChangeKind =
  | "added"
  | "modified"
  | "deleted"
  | "renamed"
  | "type_change"
  | "untracked"
  | "conflicted";

export interface FileChange {
  path: string;
  kind: ChangeKind;
}

export interface RepoStatus {
  branch: string | null;
  staged: FileChange[];
  unstaged: FileChange[];
  untracked: string[];
  conflicted: string[];
  is_clean: boolean;
}

export interface BranchInfo {
  name: string;
  is_head: boolean;
  is_remote: boolean;
  upstream: string | null;
  is_protected: boolean;
}

export interface CommitInfo {
  id: string;
  short_id: string;
  summary: string;
  author_name: string;
  author_email: string;
  time: number;
  /** 親コミットの完全な oid 文字列の一覧。マージコミットは複数、最初のコミットは空配列。 */
  parent_ids: string[];
}

// コミット履歴の絞り込み条件。すべて任意で、未指定の項目は条件として使わない。
// message はメッセージ（件名・本文）への部分一致、author は作者名/メールへの
// 部分一致（どちらも大文字小文字を無視）、since/until はコミット時刻
// （Unix エポック秒）の下限・上限（両端を含む）。
export interface LogFilter {
  message?: string;
  author?: string;
  since?: number;
  until?: number;
}

export type DiffLineKind = "context" | "addition" | "deletion" | "hunk";

export interface DiffLine {
  kind: DiffLineKind;
  old_lineno: number | null;
  new_lineno: number | null;
  content: string;
}

export interface FileDiff {
  path: string;
  is_binary: boolean;
  truncated: boolean;
  is_conflicted: boolean;
  lines: DiffLine[];
}

// blame（行ごとの最終変更コミット）の1かたまり。
// lines_start は1始まりの行番号で、そこから lines_count 行ぶんが対象。
export interface BlameHunk {
  lines_start: number;
  lines_count: number;
  commit_id: string;
  short_id: string;
  message_short: string;
  author_name: string;
  time: number;
}

// コンフリクト中のファイル1件（解消ウィザード用）。
// has_ancestor は共通祖先側エントリの有無（3-way マージか否かの簡易情報）。
export interface ConflictFile {
  path: string;
  has_ancestor: boolean;
}

export interface BranchRelation {
  name: string;
  is_current: boolean;
  merged_into_current: boolean;
  ahead: number;
  behind: number;
}

export interface LikelyBase {
  name: string;
  ambiguous: boolean;
  ahead: number;
  behind: number;
}

export interface BranchGraph {
  current: string | null;
  likely_base: LikelyBase | null;
  relations: BranchRelation[];
}

export type OperationKind =
  | "stage"
  | "unstage"
  | "commit"
  | "amend_commit"
  | "discard"
  | "stash_save"
  | "stash_apply"
  | "stash_pop"
  | "create_branch"
  | "switch_branch"
  | "delete_branch"
  | "reset_hard"
  | "fetch"
  | "pull"
  | "push"
  | "force_push"
  | "cherry_pick"
  | "create_tag"
  | "delete_tag"
  | "rebase"
  | "merge"
  | "remove_remote"
  | "restore_file";

export type RiskLevel = "safe" | "caution" | "destructive";

export interface RiskAssessment {
  level: RiskLevel;
  reasons: string[];
  reversible: boolean;
  permanent_data_loss: boolean;
  recommended_alternative: string | null;
}

export interface Explanation {
  title: string;
  what: string;
  why: string;
  on_trouble: string;
}

export interface UndoEntry {
  op: OperationKind;
  description: string;
}

// 退避（stash）1件の情報。index は一覧での位置（0 が最新）。
export interface StashInfo {
  index: number;
  message: string;
  id: string;
  // この退避に含まれる変更ファイル数（一覧表示用の概要）。
  file_count: number;
}

// リモートリポジトリ1件の情報。push_url は fetch と異なる場合のみ文字列、同じか未設定なら null。
export interface RemoteInfo {
  name: string;
  fetch_url: string;
  push_url: string | null;
}

// タグ1件の情報。message は注釈付きタグのときだけ文字列、軽量タグは null。
export interface TagInfo {
  name: string;
  target_id: string;
  target_short_id: string;
  message: string | null;
}

// fetch（取得）の結果。リモート追跡ブランチを更新するだけの安全操作。
export interface FetchOutcome {
  remote: string;
  // 今回更新（前進・新規取得）された追跡ブランチ数。0 ならリモートにも新着なし。
  updated_refs: number;
}

// pull（取り込み）の結果。fast-forward でのみ取り込む。
// 分岐して取り込めない場合は invoke が reject する（kind は返らない）。
export type PullOutcome =
  | { kind: "up_to_date" }
  | { kind: "fast_forwarded"; commit: CommitInfo };

// merge（ブランチ統合）の結果。
export type MergeOutcome =
  | { kind: "up_to_date" }
  | { kind: "fast_forwarded"; commit: CommitInfo }
  | { kind: "merged"; commit: CommitInfo }
  | { kind: "conflicted" };

// identity の保存先。"local" は今のリポジトリだけ、"global" はこのPC全体。
export type IdentityScope = "local" | "global";

// ネットワーク操作（fetch / pull / push）のエラー種別（core の NetworkErrorKind に対応）。
// snake_case のリテラルで届く（serde rename_all = "snake_case" による）。
export type NetworkErrorKind =
  | "auth_failed"
  | "remote_not_found"
  | "ssh_key_not_found"
  | "non_fast_forward"
  | "timeout"
  | "other";

export interface Identity {
  name: string | null;
  email: string | null;
}

// #69 機密ファイル検出: ステージしようとしたファイルが機密性の高いものだった場合の警告1件。
// path はリポジトリルートからの相対パス、reason はなぜ危険かの日本語説明。
export interface SensitiveWarning {
  path: string;
  reason: string;
}

// #81 LFS ガイド: ステージしようとしたファイルが Git LFS 移行候補（大容量・バイナリ）だった場合の情報1件。
// path はリポジトリルートからの相対パス、size_bytes は実ファイルサイズ（取得失敗時は 0）、
// reason はなぜ候補かの日本語説明。
export interface LfsCandidate {
  path: string;
  size_bytes: number;
  reason: string;
}

// --- ラベル -----------------------------------------------------------------

export const changeKindLabel: Record<ChangeKind, string> = {
  added: "追加",
  modified: "変更",
  deleted: "削除",
  renamed: "リネーム",
  type_change: "種別変更",
  untracked: "未追跡",
  conflicted: "コンフリクト",
};

// --- Tauri コマンドのラッパ --------------------------------------------------

export const api = {
  getStatus: (repoPath: string) =>
    invoke<RepoStatus>("get_status", { repoPath }),
  getBranches: (repoPath: string) =>
    invoke<BranchInfo[]>("get_branches", { repoPath }),
  // filter を省略すると従来どおり全件を対象にする（後方互換）。
  getLog: (repoPath: string, skip: number, max: number, filter?: LogFilter) =>
    invoke<CommitInfo[]>("get_log", {
      repoPath,
      skip,
      max,
      filter: filter ?? null,
    }),
  getFileLog: (repoPath: string, path: string, max: number) =>
    invoke<CommitInfo[]>("get_file_log", { repoPath, path, max }),
  getDiffUnstaged: (repoPath: string, path: string) =>
    invoke<FileDiff>("get_diff_unstaged", { repoPath, path }),
  getDiffStaged: (repoPath: string, path: string) =>
    invoke<FileDiff>("get_diff_staged", { repoPath, path }),
  getDiffConflict: (repoPath: string, path: string) =>
    invoke<FileDiff>("get_diff_conflict", { repoPath, path }),
  // 2 つのコミット間の差分。fromOid が null なら toOid の第1親との比較になる。
  getDiffBetween: (repoPath: string, fromOid: string | null, toOid: string) =>
    invoke<FileDiff[]>("get_diff_between", { repoPath, fromOid, toOid }),
  getBlame: (repoPath: string, path: string) =>
    invoke<BlameHunk[]>("get_blame", { repoPath, path }),
  getConflicts: (repoPath: string) =>
    invoke<ConflictFile[]>("get_conflicts", { repoPath }),
  markResolved: (repoPath: string, path: string) =>
    invoke<void>("mark_resolved", { repoPath, path }),
  getBranchGraph: (repoPath: string) =>
    invoke<BranchGraph>("get_branch_graph", { repoPath }),

  explain: (op: OperationKind) =>
    invoke<Explanation>("explain_operation", { op }),
  assess: (repoPath: string, op: OperationKind, targetBranch?: string) =>
    invoke<RiskAssessment>("assess_operation", {
      repoPath,
      op,
      targetBranch: targetBranch ?? null,
    }),

  stageAll: (repoPath: string) => invoke<void>("stage_all", { repoPath }),
  stagePath: (repoPath: string, path: string) =>
    invoke<void>("stage_path", { repoPath, path }),
  stageHunk: (repoPath: string, filePath: string, hunkHeader: string) =>
    invoke<void>("stage_hunk", { repoPath, filePath, hunkHeader }),
  unstage: (repoPath: string, path: string) =>
    invoke<void>("unstage", { repoPath, path }),
  commit: (repoPath: string, message: string) =>
    invoke<CommitInfo>("commit", { repoPath, message }),
  amendCommit: (repoPath: string, message: string) =>
    invoke<CommitInfo>("amend_commit", { repoPath, message }),
  squashCommits: (repoPath: string, commitOids: string[], message: string) =>
    invoke<void>("squash_commits", { repoPath, commitOids, message }),
  rewordCommit: (repoPath: string, message: string) =>
    invoke<CommitInfo>("reword_commit", { repoPath, message }),
  discardPath: (repoPath: string, path: string) =>
    invoke<void>("discard_path", { repoPath, path }),

  // #70 .gitignore 管理: 現在の .gitignore の内容を取得する（無ければ null）。
  getGitignore: (repoPath: string) =>
    invoke<string | null>("get_gitignore", { repoPath }),
  // #70 .gitignore 管理: パターンを .gitignore の末尾に 1 行追記する（無ければ新規作成）。
  addToGitignore: (repoPath: string, pattern: string) =>
    invoke<void>("add_to_gitignore", { repoPath, pattern }),

  getStashes: (repoPath: string) =>
    invoke<StashInfo[]>("get_stashes", { repoPath }),
  stashSave: (repoPath: string, message: string) =>
    invoke<void>("stash_save", { repoPath, message }),
  stashApply: (repoPath: string, index: number) =>
    invoke<void>("stash_apply", { repoPath, index }),
  stashPop: (repoPath: string, index: number) =>
    invoke<void>("stash_pop", { repoPath, index }),
  // 指定退避の変更ファイル一覧を返す（退避は適用しない安全な操作）。
  stashDiff: (repoPath: string, index: number) =>
    invoke<FileChange[]>("stash_diff", { repoPath, index }),

  getIdentity: (repoPath: string) =>
    invoke<Identity>("get_identity", { repoPath }),
  setIdentity: (
    repoPath: string,
    name: string,
    email: string,
    scope: IdentityScope,
  ) => invoke<void>("set_identity", { repoPath, name, email, scope }),

  createBranch: (repoPath: string, name: string) =>
    invoke<void>("create_branch", { repoPath, name }),
  switchBranch: (repoPath: string, name: string) =>
    invoke<void>("switch_branch", { repoPath, name }),
  deleteBranch: (repoPath: string, name: string) =>
    invoke<void>("delete_branch", { repoPath, name }),
  fetch: (repoPath: string, remote: string) =>
    invoke<FetchOutcome>("fetch", { repoPath, remote }),
  pull: (repoPath: string, remote: string, branch: string) =>
    invoke<PullOutcome>("pull", { repoPath, remote, branch }),
  resetHard: (repoPath: string, revspec: string) =>
    invoke<void>("reset_hard", { repoPath, revspec }),
  push: (
    repoPath: string,
    remote: string,
    refspec: string,
    force: boolean,
  ) => invoke<void>("push", { repoPath, remote, refspec, force }),

  cherryPick: (repoPath: string, oid: string) =>
    invoke<CommitInfo>("cherry_pick", { repoPath, oid }),
  mergeBranch: (repoPath: string, branchName: string) =>
    invoke<MergeOutcome>("merge_branch", { repoPath, branchName }),
  listTags: (repoPath: string) => invoke<TagInfo[]>("list_tags", { repoPath }),
  createTag: (
    repoPath: string,
    name: string,
    target?: string,
    message?: string,
  ) =>
    invoke<void>("create_tag", {
      repoPath,
      name,
      target: target ?? null,
      message: message ?? null,
    }),
  deleteTag: (repoPath: string, name: string) =>
    invoke<void>("delete_tag", { repoPath, name }),

  // #71 リモート管理: リモートリポジトリの一覧・追加・URL変更・削除。
  listRemotes: (repoPath: string) =>
    invoke<RemoteInfo[]>("list_remotes", { repoPath }),
  addRemote: (repoPath: string, name: string, url: string) =>
    invoke<void>("add_remote", { repoPath, name, url }),
  removeRemote: (repoPath: string, name: string) =>
    invoke<void>("remove_remote", { repoPath, name }),
  setRemoteUrl: (repoPath: string, name: string, url: string) =>
    invoke<void>("set_remote_url", { repoPath, name, url }),

  // 取り消し履歴のすべてのエントリを古い順で返す（タイムライン表示用）。
  getUndoJournal: (repoPath: string) =>
    invoke<UndoEntry[]>("get_undo_journal", { repoPath }),
  peekUndo: (repoPath: string) =>
    invoke<UndoEntry | null>("peek_undo", { repoPath }),
  undoLast: (repoPath: string) => invoke<string>("undo_last", { repoPath }),

  // #126 ネットワーク診断: エラーメッセージを種別に分類する。
  // fetch / pull / push が reject されたとき、その文字列をここに渡して種別を得る。
  classifyNetworkError: (message: string) =>
    invoke<NetworkErrorKind>("classify_network_error_cmd", { message }),

  // #69 機密ファイル検出: 指定パスが機密性の高いファイルかどうかを検出する。
  // 機密ファイルが含まれる場合は SensitiveWarning の配列を返す（空なら問題なし）。
  checkSensitive: (repoPath: string, paths: string[]) =>
    invoke<SensitiveWarning[]>("check_sensitive", { repoPath, paths }),

  // #81 LFS ガイド: 指定パスが Git LFS 移行候補（大容量・バイナリ）かどうかを検出する。
  // 候補ファイルが含まれる場合は LfsCandidate の配列を返す（空なら問題なし）。
  checkLfsCandidates: (repoPath: string, paths: string[]) =>
    invoke<LfsCandidate[]>("check_lfs_candidates", { repoPath, paths }),

  // #130 ファイル復元: 指定コミット時点のファイル内容を作業ツリーに復元し、ステージする。
  // git restore --source <commitId> -- <filePath> に相当する。
  restoreFileFromCommit: (repoPath: string, commitId: string, filePath: string) =>
    invoke<void>("restore_file_from_commit", { repoPath, commitId, filePath }),
};
