//! Tauriコマンド層。ここは薄く保ち、実処理は `noobgit-core` に委ねる。
//!
//! 各コマンドは `Result<T, String>` を返すので、フロントは `invoke().catch()` で
//! 日本語のエラーメッセージをそのまま表示できる。

use git2::Repository;

use noobgit_core::explain::{explain as explain_op, Explanation};
use noobgit_core::model::{BranchInfo, CommitInfo, RepoStatus};
use noobgit_core::safety::{assess, OperationKind, RiskAssessment, SafetyContext};
use noobgit_core::undo::UndoEntry;
use noobgit_core::{ops, repo, undo};

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
fn get_log(repo_path: String, max: usize) -> Result<Vec<CommitInfo>, String> {
    let r = open(&repo_path)?;
    repo::log(&r, max).map_err(|e| e.to_string())
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
    let ctx = SafetyContext {
        target_branch,
        working_dir_dirty,
        protected_branches: Vec::new(),
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

#[tauri::command]
fn reset_hard(repo_path: String, revspec: String) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::reset_hard(&r, &revspec).map_err(|e| e.to_string())
}

#[tauri::command]
fn can_undo(repo_path: String) -> Result<bool, String> {
    let r = open(&repo_path)?;
    undo::can_undo(&r).map_err(|e| e.to_string())
}

#[tauri::command]
fn peek_undo(repo_path: String) -> Result<Option<UndoEntry>, String> {
    let r = open(&repo_path)?;
    undo::peek(&r).map_err(|e| e.to_string())
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
            explain_operation,
            assess_operation,
            stage_all,
            stage_path,
            unstage,
            commit,
            create_branch,
            switch_branch,
            delete_branch,
            reset_hard,
            can_undo,
            peek_undo,
            undo_last,
        ])
        .run(tauri::generate_context!())
        .expect("noobGit の起動に失敗しました");
}
