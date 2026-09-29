import { Channel, invoke } from "@tauri-apps/api/core";

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
  // #203 サブモジュール検出: このパスがサブモジュール（リポジトリの中の別リポジトリ）か。
  // true のとき、差分は「中身の変更」ではなく「参照先コミットの変更」を意味する。
  is_submodule: boolean;
}

export interface RepoStatus {
  branch: string | null;
  staged: FileChange[];
  unstaged: FileChange[];
  untracked: string[];
  conflicted: string[];
  is_clean: boolean;
  // #203 サブモジュール検出: リポジトリが .gitmodules を含むか。
  // true のとき noobGit は中身を操作できないことを説明するバナーを表示する。
  has_submodules: boolean;
  // #197 detached HEAD: HEAD がブランチを指さずコミットを直接指しているか。
  head_detached: boolean;
  // detached のときだけ非 null。復帰ガイド（バナー）用の補足情報。
  detached_info: DetachedHeadInfo | null;
}

export interface DetachedHeadInfo {
  // 直前までいたブランチ名（reflog からの推定）。特定できなければ null。
  previous_branch: string | null;
  // どのブランチ・タグ・リモート追跡ブランチにも属していないコミット数。
  unsaved_commits: number;
}

export interface BranchInfo {
  name: string;
  is_head: boolean;
  is_remote: boolean;
  upstream: string | null;
  is_protected: boolean;
  // #268 fetch のプルーニング対応: upstream の設定はあるが、その追跡ブランチ
  // （refs/remotes/...）がもう存在しない（リモートで削除された）か。
  // ローカルブランチ自体は消えていない。リモートブランチや upstream 未設定では常に false。
  upstream_gone: boolean;
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

// コミット履歴をカーソルベースでページングしたときの1ページ分の結果
// （Issue #277）。`cursor` は「続きがあるときに次回の getLogPage へそのまま
// 渡すオペークな ID」で、中身に意味は無い。`has_more` が true なら `cursor` は
// 必ず non-null。
export interface LogPage {
  commits: CommitInfo[];
  cursor: string | null;
  has_more: boolean;
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
  // このファイルの変更種別。コンフリクト中は常に "conflicted"。リネーム検出は
  // コミット間の差分（get_diff_between）でのみ有効。
  kind: ChangeKind;
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

// #269 ブランチクリーンアップ: マージ済みローカルブランチ1件の情報（一括削除候補）。
// 保護ブランチ自身・現在チェックアウト中のブランチは含まれない。
export interface MergedBranchInfo {
  name: string;
  // 取り込み済みと判定された保護ブランチ名。
  merged_into: string;
  short_id: string;
}

// #269 ブランチクリーンアップ: 一括削除でスキップされた1件（理由付き）。
export interface SkippedBranch {
  name: string;
  reason: string;
}

// #269 ブランチクリーンアップ: 一括削除の結果。
export interface BulkDeleteBranchesOutcome {
  deleted: string[];
  skipped: SkippedBranch[];
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
  | "stash_drop"
  | "create_branch"
  | "switch_branch"
  | "switch_branch_with_stash"
  | "delete_branch"
  | "reset_hard"
  | "fetch"
  | "pull"
  | "push"
  | "force_push"
  | "cherry_pick"
  | "revert"
  | "create_tag"
  | "delete_tag"
  | "rebase"
  | "merge"
  | "remove_remote"
  | "restore_file"
  | "bisect_start"
  | "bisect_reset"
  | "clone"
  | "rescue_detached_head";

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
  // 記録時点の HEAD コミット id。旧形式のジャーナルには無い（#201）。
  head_at_record?: string | null;
}

// #201 undo エントリを今のリポジトリ状態で適用してよいかの検証結果。
// core の UndoApplicability（serde tag = "status"）のミラー。
export type UndoApplicability =
  | { status: "applicable" }
  | { status: "already_undone" }
  | { status: "unresolvable"; reason: string }
  | { status: "risky"; reason: string };

// 退避（stash）1件の情報。index は一覧での位置（0 が最新）。
export interface StashInfo {
  index: number;
  message: string;
  id: string;
  // この退避に含まれる変更ファイル数（一覧表示用の概要）。
  file_count: number;
}

// 退避の取り出し（stash_apply / stash_pop）の結果。
// conflicted が true のときは作業ツリー・インデックスがコンフリクト状態のまま
// 返る（status.conflicted にも反映される）。stash_pop の場合、conflicted が
// true の間は退避を一覧から取り除かない。
export interface StashRestoreOutcome {
  conflicted: boolean;
}

// 破棄する 1 ファイルぶんの差分（ImpactPreview の discarded_diffs の要素）。
// ステージ済み・未ステージのどちらも失われる。差分が無い側は null。
export interface DiscardedDiff {
  path: string;
  staged: FileDiff | null;
  unstaged: FileDiff | null;
}

// 操作の実行前に見せる「影響プレビュー」（#196）。kind で判別する tagged union。
// core/src/model.rs の ImpactPreview と一致させること。
export type ImpactPreview =
  // reset_hard: 失われる未コミットの変更ファイル。
  | { kind: "lost_changes"; files: FileChange[] }
  // discard: 失われる差分そのもの（多いときは omitted_files に省略数）。
  | { kind: "discarded_diffs"; diffs: DiscardedDiff[]; omitted_files: number }
  // delete_branch: そのブランチにしかないコミット。
  | {
      kind: "unique_commits";
      branch: string;
      commits: CommitInfo[];
      truncated: boolean;
    }
  // force push: リモート上で上書きされて消えるコミット（remote_ref は最後の fetch 時点）。
  | {
      kind: "overwritten_commits";
      remote_ref: string;
      commits: CommitInfo[];
      truncated: boolean;
    }
  // stash_apply / stash_pop: 退避と今の作業ツリーで重なる（衝突しうる）ファイル。
  | {
      kind: "stash_overlap";
      stash_file_count: number;
      overlapping: FileChange[];
    }
  // squash / reword: 書き換わるコミット（新しい順）と公開済みか。
  | { kind: "rewritten_commits"; commits: CommitInfo[]; published: boolean }
  // リベースプラン（並べ替え・削除など）: 変更前/後の履歴（新しい順）と消えるコミット。
  // after の id / short_id は元コミットのもの、summary は reword 反映後。
  | {
      kind: "rebase_plan";
      before: CommitInfo[];
      after: CommitInfo[];
      dropped: CommitInfo[];
      published: boolean;
    };

// リベースプランの 1 ステップ。core/src/model.rs の RebaseStep と一致させること。
// プランは古い順（適用する順）。squash は直前に残るステップへ取り込む。
export type RebaseStep =
  | { action: "pick"; oid: string }
  | { action: "drop"; oid: string }
  | { action: "reword"; oid: string; message: string }
  | { action: "squash"; oid: string };

// 影響プレビューの計算依頼。core/src/model.rs の ImpactRequest と一致させること。
export type ImpactRequest =
  | { op: "reset_hard" }
  | { op: "discard"; paths: string[] }
  | { op: "delete_branch"; name: string }
  | { op: "force_push"; remote: string; branch: string }
  | { op: "stash_apply"; index: number }
  | { op: "stash_pop"; index: number }
  // commit_ids が空なら HEAD の 1 件（reword）。
  | { op: "rebase"; commit_ids: string[] }
  // リベースプラン（古い順）。
  | { op: "rebase_plan"; plan: RebaseStep[] };

// 「退避して切り替える」（switch_branch_with_stash）の結果。変更は失われない。
// stashed: 実際に退避したか（変更が無ければ false）。
// conflicted: 戻すときにコンフリクトが起きたか。true の間は退避を一覧に残す。
export interface SwitchWithStashOutcome {
  stashed: boolean;
  conflicted: boolean;
}

// switch_branch が「未コミットの変更のため切り替えできない」で失敗したか。
// Tauri 境界ではエラーが日本語メッセージの文字列になるため、core の
// ops::switch_branch の Blocked メッセージ（この一節）で判定する。
// メッセージを変えたらここも合わせること。
export function isSwitchBlockedByChanges(error: unknown): boolean {
  return String(error).includes("未コミットの変更があるため切り替えできません");
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
  // #268 fetch のプルーニング対応: リモートで削除されて整理（削除）された追跡ブランチ名
  // （例: "origin/feature-x"）の一覧。ローカルブランチ本体はここには含まれない
  // （削除されないため）。
  pruned: string[];
}

// clone（新規取得）の結果。クローンは成功か失敗の二択なので、結果は保存先パスだけ持つ。
export interface CloneOutcome {
  path: string;
}

// pull（取り込み）の結果。fast-forward でのみ取り込む。
// 分岐して取り込めない場合は invoke が reject する（kind は返らない）。
export type PullOutcome =
  | { kind: "up_to_date" }
  | { kind: "fast_forwarded"; commit: CommitInfo };

// #167 進捗フィードバック: fetch / pull / push の通信段階（core の
// NetworkProgressStage に対応）。この型は check_type_contract.py の自動検証
// 対象ではない（対象は OperationKind / RiskLevel / ChangeKind / DiffLineKind /
// NetworkErrorKind / LocalErrorKind のみ）ため、core/src/model.rs の enum を変更したら
// ここも必ず手動で同期すること。
export type NetworkProgressStage =
  | "connecting"
  | "receiving_objects"
  | "resolving_deltas"
  | "sending_objects";

// #167 進捗フィードバック: fetch / pull / push の進捗1件（core の NetworkProgress
// に対応）。Tauri の Channel でストリーミング配信される。total_objects が 0 の間は
// まだ総数が分かっていない（stage が "connecting" のときなど）ので、パーセント表示は
// total_objects > 0 のときだけ行うこと。
export interface NetworkProgress {
  stage: NetworkProgressStage;
  received_objects: number;
  total_objects: number;
  received_bytes: number;
  indexed_deltas: number;
  total_deltas: number;
}

// Bisect（バグ混入コミットの二分探索）セッションの状態。
// bisect_start / bisect_mark の返り値、および bisect_status での復元にも使う
// （アプリ再起動やタブの再表示のあいだも進行状況を追えるようにするため）。
export interface BisectStatus {
  current_commit: CommitInfo | null;
  remaining_steps: number;
  is_done: boolean;
  found_commit: CommitInfo | null;
  tested_count: number;
}

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

// ローカル操作（ステージ・コミット・チェックアウトなど）のエラー種別
// （core の LocalErrorKind に対応。#204）。snake_case のリテラルで届く。
export type LocalErrorKind =
  | "lock_busy"
  | "permission_denied"
  | "repo_corrupted"
  | "disk_full"
  | "other";

// ローカル操作エラーの初心者向け解説（core の LocalErrorExplanation に対応。#204）。
// steps は上から順に試す解決手順。
export interface LocalErrorExplanation {
  kind: LocalErrorKind;
  title: string;
  what: string;
  why: string;
  steps: string[];
}

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

// #173 .gitignore バリデーション: 1 パターンの検証結果。
// valid が false のとき error に平易な日本語の理由が入る。duplicate は構文として
// 正しいが既存の .gitignore に同じ行がすでにあることを示す（エラーではなく警告）。
export interface GitignorePatternCheck {
  valid: boolean;
  error: string | null;
  duplicate: boolean;
}

// #173 .gitignore 提案: ファイルパスから生成した無視パターンの候補1件。
// pattern が実際に追記する文字列、label が選択肢の短いラベル、description が説明。
export interface GitignoreSuggestion {
  pattern: string;
  label: string;
  description: string;
}

// #131 reflog の可視化: HEAD の移動履歴の1エントリ。
// old_oid は移動前、new_oid は移動後のコミット OID（40桁）。
// short_id は new_oid の先頭7桁。message は生のメッセージ、short_message は日本語化した説明。
// timestamp は Unix エポック秒。
export interface ReflogEntry {
  old_oid: string;
  new_oid: string;
  short_id: string;
  message: string;
  short_message: string;
  timestamp: number;
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
  // 保護ブランチ一覧（未設定なら既定値の main/master）。リポジトリごとに独立する。
  getProtectedBranches: (repoPath: string) =>
    invoke<string[]>("get_protected_branches", { repoPath }),
  // 保護ブランチ一覧を保存する。空配列を渡すと既定値（main/master）に戻る。
  setProtectedBranches: (repoPath: string, names: string[]) =>
    invoke<void>("set_protected_branches", { repoPath, names }),
  // filter を省略すると従来どおり全件を対象にする（後方互換）。
  getLog: (repoPath: string, skip: number, max: number, filter?: LogFilter) =>
    invoke<CommitInfo[]>("get_log", {
      repoPath,
      skip,
      max,
      filter: filter ?? null,
    }),
  // カーソルベースのページング（Issue #277）。`cursor` を省略すると先頭ページ、
  // 渡すとその続きを取得する。各回のコストは「すでに読んだ件数」に依存しない
  // （詳しくは core 側の `LogCursorStore` を参照）。`fallbackSkip` は、渡した
  // カーソルが失効していた場合にだけ使われる「現在表示済みの件数」。
  getLogPage: (
    repoPath: string,
    max: number,
    filter?: LogFilter,
    cursor?: string,
    fallbackSkip = 0,
  ) =>
    invoke<LogPage>("get_log_page", {
      repoPath,
      max,
      filter: filter ?? null,
      cursor: cursor ?? null,
      fallbackSkip,
    }),
  // 使い終わったログカーソルを手放す（検索条件の変更・リフレッシュ・タブを
  // 閉じる等）。呼び忘れてもキャッシュ側の上限超過で自動的に立ち退くが、
  // すぐに手放したほうがリポジトリのハンドルを長く握り続けずに済む。
  closeLogCursor: (cursor: string) =>
    invoke<void>("close_log_cursor", { cursor }),
  getFileLog: (repoPath: string, path: string, max: number) =>
    invoke<CommitInfo[]>("get_file_log", { repoPath, path, max }),
  // 過去のコミット履歴から件名（1行目）が prefix に前方一致する候補を頻度順で返す (#185)。
  suggestCommitMessages: (repoPath: string, prefix: string, max: number) =>
    invoke<string[]>("suggest_commit_messages", { repoPath, prefix, max }),
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
  // 複数の操作の危険度をまとめて評価する（ボタンの危険度カラー用, #274）。
  // リポジトリの状態を 1 回だけ調べるので、assess を件数分呼ぶより大幅に軽い。
  // 結果は requests と同じ順序・同じ件数で返る。
  assessMany: (
    repoPath: string,
    requests: { op: OperationKind; targetBranch?: string }[],
  ) =>
    invoke<RiskAssessment[]>("assess_operations", {
      repoPath,
      requests: requests.map((r) => ({
        op: r.op,
        target_branch: r.targetBranch ?? null,
      })),
    }),

  stageAll: (repoPath: string) => invoke<void>("stage_all", { repoPath }),
  stagePath: (repoPath: string, path: string) =>
    invoke<void>("stage_path", { repoPath, path }),
  stageHunk: (repoPath: string, filePath: string, hunkHeader: string) =>
    invoke<void>("stage_hunk", { repoPath, filePath, hunkHeader }),
  unstage: (repoPath: string, path: string) =>
    invoke<void>("unstage", { repoPath, path }),
  unstageHunk: (repoPath: string, filePath: string, hunkHeader: string) =>
    invoke<void>("unstage_hunk", { repoPath, filePath, hunkHeader }),
  commit: (repoPath: string, message: string) =>
    invoke<CommitInfo>("commit", { repoPath, message }),
  amendCommit: (repoPath: string, message: string) =>
    invoke<CommitInfo>("amend_commit", { repoPath, message }),
  squashCommits: (repoPath: string, commitOids: string[], message: string) =>
    invoke<void>("squash_commits", { repoPath, commitOids, message }),
  rebasePlan: (repoPath: string, plan: RebaseStep[]) =>
    invoke<void>("rebase_plan", { repoPath, plan }),
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
  // #173 .gitignore バリデーション: glob 構文チェックと重複チェックをまとめて行う。
  checkGitignorePattern: (repoPath: string, pattern: string) =>
    invoke<GitignorePatternCheck>("check_gitignore_pattern", { repoPath, pattern }),
  // #173 .gitignore 提案: ファイルパスから無視パターンの候補を生成する（リポジトリの
  // 状態には依存しないので repoPath は渡さない）。
  suggestGitignorePatterns: (path: string) =>
    invoke<GitignoreSuggestion[]>("suggest_gitignore_patterns", { path }),

  // #196 操作の影響プレビュー。読み取り専用。失敗しても操作をブロックしないよう、
  // 呼び出し側は失敗を「プレビューなし」として扱うこと。
  getImpactPreview: (repoPath: string, request: ImpactRequest) =>
    invoke<ImpactPreview>("get_impact_preview", { repoPath, request }),

  getStashes: (repoPath: string) =>
    invoke<StashInfo[]>("get_stashes", { repoPath }),
  stashSave: (repoPath: string, message: string) =>
    invoke<void>("stash_save", { repoPath, message }),
  stashApply: (repoPath: string, index: number) =>
    invoke<StashRestoreOutcome>("stash_apply", { repoPath, index }),
  stashPop: (repoPath: string, index: number) =>
    invoke<StashRestoreOutcome>("stash_pop", { repoPath, index }),
  // 退避を一覧から取り除く（中身は復元できない。undo は記録されない）。
  // 番号は新しい退避でずれるため、StashInfo.id で指定する。
  stashDrop: (repoPath: string, stashId: string) =>
    invoke<void>("stash_drop", { repoPath, stashId }),
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
  // #197 detached HEAD: 今の位置に新しいブランチを作って乗り換え、コミットを安全にする。
  rescueDetachedHead: (repoPath: string, name: string) =>
    invoke<void>("rescue_detached_head", { repoPath, name }),
  switchBranch: (repoPath: string, name: string) =>
    invoke<void>("switch_branch", { repoPath, name }),
  switchBranchWithStash: (repoPath: string, name: string) =>
    invoke<SwitchWithStashOutcome>("switch_branch_with_stash", {
      repoPath,
      name,
    }),
  deleteBranch: (repoPath: string, name: string) =>
    invoke<void>("delete_branch", { repoPath, name }),
  // #269 ブランチクリーンアップ: マージ済みローカルブランチの一覧を返す。
  getMergedBranches: (repoPath: string) =>
    invoke<MergedBranchInfo[]>("get_merged_branches", { repoPath }),
  // #269 ブランチクリーンアップ: マージ済みブランチを一括削除する。
  // core 側で削除直前に再検証するため、渡した一覧の一部だけが削除されることがある。
  deleteBranches: (repoPath: string, names: string[]) =>
    invoke<BulkDeleteBranchesOutcome>("delete_branches", { repoPath, names }),
  // #167 進捗フィードバック: onProgress を渡すと、受信オブジェクト数などの進捗を
  // 都度呼び出す（Tauri の Channel でストリーミング配信される）。省略可能で、
  // 省略時は何もしない Channel を渡すだけで動作は変わらない
  // （Rust 側の `Channel<T>` 引数は IPC 参照型のため Option にできない）。
  fetch: (
    repoPath: string,
    remote: string,
    onProgress?: (progress: NetworkProgress) => void,
  ) => {
    const progress = new Channel<NetworkProgress>(onProgress ?? (() => {}));
    return invoke<FetchOutcome>("fetch", { repoPath, remote, progress });
  },
  pull: (
    repoPath: string,
    remote: string,
    branch: string,
    onProgress?: (progress: NetworkProgress) => void,
  ) => {
    const progress = new Channel<NetworkProgress>(onProgress ?? (() => {}));
    return invoke<PullOutcome>("pull", { repoPath, remote, branch, progress });
  },
  resetHard: (repoPath: string, revspec: string) =>
    invoke<void>("reset_hard", { repoPath, revspec }),
  push: (
    repoPath: string,
    remote: string,
    refspec: string,
    force: boolean,
    onProgress?: (progress: NetworkProgress) => void,
  ) => {
    const progress = new Channel<NetworkProgress>(onProgress ?? (() => {}));
    return invoke<void>("push", {
      repoPath,
      remote,
      refspec,
      force,
      progress,
    });
  },
  // リモートリポジトリを新規にクローンする。他のコマンドと違い、まだリポジトリが
  // 存在しないため repoPath は取らない（destPath が保存先）。onProgress は
  // fetch/pull/push と同じく省略可能。
  cloneRepo: (
    url: string,
    destPath: string,
    onProgress?: (progress: NetworkProgress) => void,
  ) => {
    const progress = new Channel<NetworkProgress>(onProgress ?? (() => {}));
    return invoke<CloneOutcome>("clone_repo", { url, destPath, progress });
  },

  cherryPick: (repoPath: string, oid: string) =>
    invoke<CommitInfo>("cherry_pick", { repoPath, oid }),
  revertCommit: (repoPath: string, oid: string) =>
    invoke<CommitInfo>("revert_commit", { repoPath, oid }),
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
  // confirmRisky: 履歴が進んでいて新しい作業も巻き戻る場合に、確認済みとして進める（#201）。
  undoLast: (repoPath: string, confirmRisky = false) =>
    invoke<string>("undo_last", { repoPath, confirmRisky }),
  // 各エントリの適用可否（getUndoJournal と同じ古い順）。
  getUndoApplicability: (repoPath: string) =>
    invoke<UndoApplicability[]>("get_undo_applicability", { repoPath }),
  // 適用不能な履歴を整理し、取り除いた件数を返す。
  pruneUndoJournal: (repoPath: string) =>
    invoke<number>("prune_undo_journal", { repoPath }),

  // #126 ネットワーク診断: エラーメッセージを種別に分類する。
  // fetch / pull / push が reject されたとき、その文字列をここに渡して種別を得る。
  classifyNetworkError: (message: string) =>
    invoke<NetworkErrorKind>("classify_network_error_cmd", { message }),

  // #204 ローカルエラーの日本語化: 操作が reject されたときの文字列を渡すと、
  // noobGit が日本語に包んだローカルエラー（ロック競合・権限・破損・ディスク満杯・
  // その他）なら解説を返す。それ以外（ネットワーク系など）は null。
  explainLocalError: (message: string) =>
    invoke<LocalErrorExplanation | null>("explain_local_error_cmd", { message }),

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

  // #131 reflog の可視化: HEAD の移動履歴を新しい順に最大 max 件返す。
  // reflog が存在しないリポジトリでは空の配列を返す。
  getReflog: (repoPath: string, max: number) =>
    invoke<ReflogEntry[]>("get_reflog", { repoPath, max }),

  // #184 Bisect: バグ混入コミットの二分探索。
  // bad は「壊れている」コミット、good は「動いていた」コミット（どちらも revspec）。
  bisectStart: (repoPath: string, bad: string, good: string) =>
    invoke<BisectStatus>("bisect_start", { repoPath, bad, good }),
  // いま Bisect が調べているコミットについて good/bad を記録し、次の候補へ進める。
  bisectMark: (repoPath: string, commit: string, isGood: boolean) =>
    invoke<BisectStatus>("bisect_mark", { repoPath, commit, isGood }),
  // Bisect セッションを終了し、開始前のブランチ（または元のコミット）へ戻す。
  bisectReset: (repoPath: string) =>
    invoke<void>("bisect_reset", { repoPath }),
  // 現在の Bisect セッションの状態を返す（無ければ null）。タブの再表示やアプリ
  // 再起動後の復元に使う読み取り専用コマンド。
  bisectStatus: (repoPath: string) =>
    invoke<BisectStatus | null>("bisect_status", { repoPath }),
};
