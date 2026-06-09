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
  | "cherry_pick";

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

// identity の保存先。"local" は今のリポジトリだけ、"global" はこのPC全体。
export type IdentityScope = "local" | "global";

export interface Identity {
  name: string | null;
  email: string | null;
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
  getLog: (repoPath: string, skip: number, max: number) =>
    invoke<CommitInfo[]>("get_log", { repoPath, skip, max }),
  getDiffUnstaged: (repoPath: string, path: string) =>
    invoke<FileDiff>("get_diff_unstaged", { repoPath, path }),
  getDiffStaged: (repoPath: string, path: string) =>
    invoke<FileDiff>("get_diff_staged", { repoPath, path }),
  getDiffConflict: (repoPath: string, path: string) =>
    invoke<FileDiff>("get_diff_conflict", { repoPath, path }),
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
  unstage: (repoPath: string, path: string) =>
    invoke<void>("unstage", { repoPath, path }),
  commit: (repoPath: string, message: string) =>
    invoke<CommitInfo>("commit", { repoPath, message }),
  amendCommit: (repoPath: string, message: string) =>
    invoke<CommitInfo>("amend_commit", { repoPath, message }),
  discardPath: (repoPath: string, path: string) =>
    invoke<void>("discard_path", { repoPath, path }),

  getStashes: (repoPath: string) =>
    invoke<StashInfo[]>("get_stashes", { repoPath }),
  stashSave: (repoPath: string, message: string) =>
    invoke<void>("stash_save", { repoPath, message }),
  stashApply: (repoPath: string, index: number) =>
    invoke<void>("stash_apply", { repoPath, index }),
  stashPop: (repoPath: string, index: number) =>
    invoke<void>("stash_pop", { repoPath, index }),

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

  peekUndo: (repoPath: string) =>
    invoke<UndoEntry | null>("peek_undo", { repoPath }),
  undoLast: (repoPath: string) => invoke<string>("undo_last", { repoPath }),
};
