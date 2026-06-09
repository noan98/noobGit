//! Tauriコマンド層。ここは薄く保ち、実処理は `noobgit-core` に委ねる。
//!
//! 各コマンドは `Result<T, String>` を返すので、フロントは `invoke().catch()` で
//! 日本語のエラーメッセージをそのまま表示できる。

use git2::Repository;

use noobgit_core::explain::{explain as explain_op, Explanation};
use noobgit_core::identity::{Identity, IdentityScope};
use noobgit_core::model::{
    BranchGraph, BranchInfo, CommitInfo, FetchOutcome, FileDiff, PullOutcome, RepoStatus, StashInfo,
};
use noobgit_core::safety::{assess, OperationKind, RiskAssessment, SafetyContext};
use noobgit_core::undo::UndoEntry;
use noobgit_core::{identity, ops, repo, undo};

fn open(repo_path: &str) -> Result<Repository, String> {
    repo::open(repo_path).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_status(repo_path: String) -> Result<RepoStatus, String> {
    let r = open(&repo_path)?;
    repo::status(&r).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_branches(repo_path: String) -> Result<Vec<BranchInfo>, String> {
    let r = open(&repo_path)?;
    repo::branches(&r, &[]).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_log(repo_path: String, skip: usize, max: usize) -> Result<Vec<CommitInfo>, String> {
    let r = open(&repo_path)?;
    repo::log_paged(&r, skip, max).map_err(|e| e.to_string())
}

/// 指定ファイルを変更したコミットを新しい順に最大 `max` 件返す（ファイル別履歴）。
#[tauri::command]
fn get_file_log(repo_path: String, path: String, max: usize) -> Result<Vec<CommitInfo>, String> {
    let r = open(&repo_path)?;
    repo::file_log(&r, &path, max).map_err(|e| e.to_string())
}

/// 指定ファイルの未ステージ差分（インデックス↔作業ツリー）を返す。
#[tauri::command]
fn get_diff_unstaged(repo_path: String, path: String) -> Result<FileDiff, String> {
    let r = open(&repo_path)?;
    repo::diff_unstaged(&r, &path).map_err(|e| e.to_string())
}

/// 指定ファイルのステージ済み差分（HEAD↔インデックス）を返す。
#[tauri::command]
fn get_diff_staged(repo_path: String, path: String) -> Result<FileDiff, String> {
    let r = open(&repo_path)?;
    repo::diff_staged(&r, &path).map_err(|e| e.to_string())
}

/// コンフリクト中ファイルの作業ツリーの内容（競合の目印を含む）を返す。
#[tauri::command]
fn get_diff_conflict(repo_path: String, path: String) -> Result<FileDiff, String> {
    let r = open(&repo_path)?;
    repo::diff_conflict(&r, &path).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_branch_graph(repo_path: String) -> Result<BranchGraph, String> {
    let r = open(&repo_path)?;
    repo::branch_graph(&r).map_err(|e| e.to_string())
}

#[tauri::command]
fn explain_operation(op: OperationKind) -> Explanation {
    explain_op(op)
}

/// 操作のリスクを評価する。未コミット変更の有無はリポジトリから自動判定する。
#[tauri::command]
fn assess_operation(
    repo_path: String,
    op: OperationKind,
    target_branch: Option<String>,
) -> Result<RiskAssessment, String> {
    let r = open(&repo_path)?;
    let working_dir_dirty = repo::is_dirty(&r).map_err(|e| e.to_string())?;
    // amend のときだけ、HEAD が公開（push）済みかを判定する（危険度の引き上げに使う）。
    let head_published = if matches!(op, OperationKind::AmendCommit) {
        repo::head_is_published(&r).unwrap_or(false)
    } else {
        false
    };
    let ctx = SafetyContext {
        target_branch,
        working_dir_dirty,
        protected_branches: Vec::new(),
        head_published,
    };
    Ok(assess(op, &ctx))
}

#[tauri::command]
fn stage_all(repo_path: String) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::stage_all(&r).map_err(|e| e.to_string())
}

#[tauri::command]
fn stage_path(repo_path: String, path: String) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::stage_path(&r, &path).map_err(|e| e.to_string())
}

#[tauri::command]
fn unstage(repo_path: String, path: String) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::unstage(&r, &path).map_err(|e| e.to_string())
}

#[tauri::command]
fn commit(repo_path: String, message: String) -> Result<CommitInfo, String> {
    let r = open(&repo_path)?;
    ops::commit(&r, &message).map_err(|e| e.to_string())
}

/// 直前のコミットを書き換える（amend）。メッセージが空ならもとのメッセージを保つ。
#[tauri::command]
fn amend_commit(repo_path: String, message: String) -> Result<CommitInfo, String> {
    let r = open(&repo_path)?;
    ops::amend_commit(&r, &message).map_err(|e| e.to_string())
}

/// 指定パスの、まだコミットしていない変更を捨てる（破棄）。元に戻せない破壊的操作。
#[tauri::command]
fn discard_path(repo_path: String, path: String) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::discard_path(&r, &path).map_err(|e| e.to_string())
}

/// 現在の変更を一時的にしまう（stash 退避）。未追跡ファイルも含めて退避する。
#[tauri::command]
fn stash_save(repo_path: String, message: String) -> Result<(), String> {
    let mut r = open(&repo_path)?;
    ops::stash_save(&mut r, &message).map_err(|e| e.to_string())
}

/// 退避を作業ツリーに取り出す（一覧には残す）。
#[tauri::command]
fn stash_apply(repo_path: String, index: usize) -> Result<(), String> {
    let mut r = open(&repo_path)?;
    ops::stash_apply(&mut r, index).map_err(|e| e.to_string())
}

/// 退避を作業ツリーに取り出し、一覧から取り除く（pop）。
#[tauri::command]
fn stash_pop(repo_path: String, index: usize) -> Result<(), String> {
    let mut r = open(&repo_path)?;
    ops::stash_pop(&mut r, index).map_err(|e| e.to_string())
}

/// 退避の一覧を返す（0 がいちばん新しい退避）。
#[tauri::command]
fn get_stashes(repo_path: String) -> Result<Vec<StashInfo>, String> {
    let mut r = open(&repo_path)?;
    ops::stash_list(&mut r).map_err(|e| e.to_string())
}

/// 現在の identity（user.name / user.email）を取得する。初回セットアップ案内に使う。
#[tauri::command]
fn get_identity(repo_path: String) -> Result<Identity, String> {
    let r = open(&repo_path)?;
    identity::get_identity(&r).map_err(|e| e.to_string())
}

/// identity を保存する。`scope` で保存先（ローカル/グローバル）を選ぶ。
#[tauri::command]
fn set_identity(
    repo_path: String,
    name: String,
    email: String,
    scope: IdentityScope,
) -> Result<(), String> {
    let r = open(&repo_path)?;
    identity::set_identity(&r, &name, &email, scope).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_branch(repo_path: String, name: String) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::create_branch(&r, &name).map_err(|e| e.to_string())
}

#[tauri::command]
fn switch_branch(repo_path: String, name: String) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::switch_branch(&r, &name).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_branch(repo_path: String, name: String) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::delete_branch(&r, &name).map_err(|e| e.to_string())
}

/// リモートから最新を取得し、リモート追跡ブランチを更新する（作業ツリーは変えない）。
#[tauri::command]
fn fetch(repo_path: String, remote: String) -> Result<FetchOutcome, String> {
    let r = open(&repo_path)?;
    ops::fetch(&r, &remote).map_err(|e| e.to_string())
}

/// fetch 後、安全に進められるとき（fast-forward）だけ取り込む。分岐時は中断する。
#[tauri::command]
fn pull(repo_path: String, remote: String, branch: String) -> Result<PullOutcome, String> {
    let r = open(&repo_path)?;
    ops::pull(&r, &remote, &branch).map_err(|e| e.to_string())
}

#[tauri::command]
fn reset_hard(repo_path: String, revspec: String) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::reset_hard(&r, &revspec).map_err(|e| e.to_string())
}

/// ローカルのコミットをリモートへ送信する。`force` が真なら強制 push。
#[tauri::command]
fn push(repo_path: String, remote: String, refspec: String, force: bool) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::push(&r, &remote, &refspec, force).map_err(|e| e.to_string())
}

#[tauri::command]
fn peek_undo(repo_path: String) -> Result<Option<UndoEntry>, String> {
    let r = open(&repo_path)?;
    Ok(undo::peek(&r).ok().flatten())
}

#[tauri::command]
fn undo_last(repo_path: String) -> Result<String, String> {
    let r = open(&repo_path)?;
    undo::undo_last(&r).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_branches,
            get_log,
            get_file_log,
            get_diff_unstaged,
            get_diff_staged,
            get_diff_conflict,
            get_branch_graph,
            explain_operation,
            assess_operation,
            stage_all,
            stage_path,
            unstage,
            commit,
            amend_commit,
            discard_path,
            stash_save,
            stash_apply,
            stash_pop,
            get_stashes,
            get_identity,
            set_identity,
            create_branch,
            switch_branch,
            delete_branch,
            fetch,
            pull,
            reset_hard,
            push,
            peek_undo,
            undo_last,
        ])
        .run(tauri::generate_context!())
        .expect("noobGit の起動に失敗しました");
}
