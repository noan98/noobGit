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
  | "create_branch"
  | "switch_branch"
  | "delete_branch"
  | "reset_hard"
  | "pull"
  | "push"
  | "force_push"
  | "merge";

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

  createBranch: (repoPath: string, name: string) =>
    invoke<void>("create_branch", { repoPath, name }),
  switchBranch: (repoPath: string, name: string) =>
    invoke<void>("switch_branch", { repoPath, name }),
  deleteBranch: (repoPath: string, name: string) =>
    invoke<void>("delete_branch", { repoPath, name }),
  resetHard: (repoPath: string, revspec: string) =>
    invoke<void>("reset_hard", { repoPath, revspec }),

  canUndo: (repoPath: string) => invoke<boolean>("can_undo", { repoPath }),
  peekUndo: (repoPath: string) =>
    invoke<UndoEntry | null>("peek_undo", { repoPath }),
  undoLast: (repoPath: string) => invoke<string>("undo_last", { repoPath }),
};
