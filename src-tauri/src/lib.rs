//! Tauriコマンド層。ここは薄く保ち、実処理は `noobgit-core` に委ねる。
//!
//! 各コマンドは `Result<T, String>` を返すので、フロントは `invoke().catch()` で
//! 日本語のエラーメッセージをそのまま表示できる。

use git2::Repository;

use noobgit_core::error::{classify_network_error, NetworkErrorKind};
use noobgit_core::explain::{explain as explain_op, Explanation};
use noobgit_core::identity::{Identity, IdentityScope};
use noobgit_core::model::{
    BlameHunk, BranchGraph, BranchInfo, CommitInfo, ConflictFile, FetchOutcome, FileChange,
    FileDiff, LfsCandidate, MergeOutcome, PullOutcome, ReflogEntry, RemoteInfo, RepoStatus,
    SensitiveWarning, StashInfo, TagInfo,
};
use noobgit_core::repo::LogFilter;
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

/// コミット履歴をページングして返す。`filter` を渡すとメッセージ・作者・日付範囲で
/// 絞り込む。`filter` が `null`（未指定）のときは従来どおり全件を対象にする。
#[tauri::command]
fn get_log(
    repo_path: String,
    skip: usize,
    max: usize,
    filter: Option<LogFilter>,
) -> Result<Vec<CommitInfo>, String> {
    let r = open(&repo_path)?;
    match filter {
        Some(f) => repo::log_filtered(&r, skip, max, &f).map_err(|e| e.to_string()),
        None => repo::log_paged(&r, skip, max).map_err(|e| e.to_string()),
    }
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

/// 2 つのコミット間（または親コミット↔指定コミット）の全変更ファイルの差分を返す。
///
/// `from_oid` が `null` のときは `to_oid` の第1親との比較になる。
#[tauri::command]
fn get_diff_between(
    repo_path: String,
    from_oid: Option<String>,
    to_oid: String,
) -> Result<Vec<FileDiff>, String> {
    let r = open(&repo_path)?;
    repo::diff_commits(&r, from_oid.as_deref(), &to_oid).map_err(|e| e.to_string())
}

/// 指定ファイルの blame（各行を最後に変更したコミット）を返す。
#[tauri::command]
fn get_blame(repo_path: String, path: String) -> Result<Vec<BlameHunk>, String> {
    let r = open(&repo_path)?;
    repo::blame_file(&r, &path).map_err(|e| e.to_string())
}

/// コンフリクト中のファイル一覧を返す（解消ウィザード用）。
#[tauri::command]
fn get_conflicts(repo_path: String) -> Result<Vec<ConflictFile>, String> {
    let r = open(&repo_path)?;
    repo::get_conflicts(&r).map_err(|e| e.to_string())
}

/// 指定ファイルのコンフリクトを「解消済み」としてマークする（解消した内容をステージ）。
#[tauri::command]
fn mark_resolved(repo_path: String, path: String) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::mark_resolved(&r, &path).map_err(|e| e.to_string())
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
    // amend / rebase のときだけ、HEAD が公開（push）済みかを判定する（危険度の引き上げに使う）。
    let head_published = if matches!(op, OperationKind::AmendCommit | OperationKind::Rebase) {
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

/// 指定ファイルの差分のうち、`hunk_header` に一致する塊（hunk）だけをステージする。
#[tauri::command]
fn stage_hunk(repo_path: String, file_path: String, hunk_header: String) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::stage_hunk(&r, &file_path, &hunk_header).map_err(|e| e.to_string())
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

/// HEAD から連続する複数のコミットを1つにまとめる（squash）。
///
/// `commit_oids` は HEAD から連続する範囲を新しい順（先頭が HEAD）で渡す。
#[tauri::command]
fn squash_commits(
    repo_path: String,
    commit_oids: Vec<String>,
    message: String,
) -> Result<(), String> {
    let r = open(&repo_path)?;
    let refs: Vec<&str> = commit_oids.iter().map(|s| s.as_str()).collect();
    ops::squash_commits(&r, &refs, &message).map_err(|e| e.to_string())
}

/// 最新のコミット（HEAD）のメッセージだけを書き換える（reword）。
#[tauri::command]
fn reword_commit(repo_path: String, message: String) -> Result<CommitInfo, String> {
    let r = open(&repo_path)?;
    ops::reword_commit(&r, &message).map_err(|e| e.to_string())
}

/// 指定パスの、まだコミットしていない変更を捨てる（破棄）。元に戻せない破壊的操作。
#[tauri::command]
fn discard_path(repo_path: String, path: String) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::discard_path(&r, &path).map_err(|e| e.to_string())
}

/// リポジトリ直下の `.gitignore` の内容を返す（ファイルが無ければ null）。
#[tauri::command]
fn get_gitignore(repo_path: String) -> Result<Option<String>, String> {
    let r = open(&repo_path)?;
    repo::read_gitignore(&r).map_err(|e| e.to_string())
}

/// `.gitignore` の末尾にパターンを 1 行追記する（ファイルが無ければ新規作成）。
#[tauri::command]
fn add_to_gitignore(repo_path: String, pattern: String) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::add_to_gitignore(&r, &pattern).map_err(|e| e.to_string())
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

/// 指定 index の退避に含まれる変更ファイル一覧を返す（退避は適用しない安全な操作）。
#[tauri::command]
fn stash_diff(repo_path: String, index: usize) -> Result<Vec<FileChange>, String> {
    let mut r = open(&repo_path)?;
    ops::stash_diff(&mut r, index).map_err(|e| e.to_string())
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

/// 指定したローカルブランチを現在のブランチにマージする。
/// コンフリクトが発生した場合は `Conflicted` を返し、リポジトリをマージ中の状態にする。
#[tauri::command]
fn merge_branch(repo_path: String, branch_name: String) -> Result<MergeOutcome, String> {
    let r = open(&repo_path)?;
    ops::merge_branch(&r, &branch_name).map_err(|e| e.to_string())
}

/// 指定したコミットの変更を、いまのブランチの先頭にコピーする（cherry-pick）。
#[tauri::command]
fn cherry_pick(repo_path: String, oid: String) -> Result<CommitInfo, String> {
    let r = open(&repo_path)?;
    ops::cherry_pick(&r, &oid).map_err(|e| e.to_string())
}

/// タグの一覧を返す（名前順）。
#[tauri::command]
fn list_tags(repo_path: String) -> Result<Vec<TagInfo>, String> {
    let r = open(&repo_path)?;
    repo::list_tags(&r).map_err(|e| e.to_string())
}

/// コミットに目印（タグ）を付ける。`target` 省略時は HEAD、`message` 省略時は軽量タグ。
#[tauri::command]
fn create_tag(
    repo_path: String,
    name: String,
    target: Option<String>,
    message: Option<String>,
) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::create_tag(&r, &name, target.as_deref(), message.as_deref()).map_err(|e| e.to_string())
}

/// タグ（目印）を削除する。直後に Undo で復元できる。
#[tauri::command]
fn delete_tag(repo_path: String, name: String) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::delete_tag(&r, &name).map_err(|e| e.to_string())
}

/// リモートリポジトリの一覧を返す（名前順）。
#[tauri::command]
fn list_remotes(repo_path: String) -> Result<Vec<RemoteInfo>, String> {
    let r = open(&repo_path)?;
    repo::list_remotes(&r).map_err(|e| e.to_string())
}

/// リモートリポジトリを追加する。
#[tauri::command]
fn add_remote(repo_path: String, name: String, url: String) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::add_remote(&r, &name, &url).map_err(|e| e.to_string())
}

/// リモートリポジトリを削除する。
#[tauri::command]
fn remove_remote(repo_path: String, name: String) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::remove_remote(&r, &name).map_err(|e| e.to_string())
}

/// リモートリポジトリの fetch URL を変更する。
#[tauri::command]
fn set_remote_url(repo_path: String, name: String, url: String) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::set_remote_url(&r, &name, &url).map_err(|e| e.to_string())
}

/// ネットワーク操作のエラーメッセージを種別に分類する。
///
/// フロントエンドが fetch / pull / push の失敗時にエラー文字列をここに渡すと、
/// [`NetworkErrorKind`] が返る。それを使って種別ごとの日本語ガイドダイアログを表示できる。
/// リポジトリ不要の純粋関数なので `repo_path` は取らない。
#[tauri::command]
fn classify_network_error_cmd(message: String) -> NetworkErrorKind {
    classify_network_error(&message)
}

/// 取り消し履歴のすべてのエントリを古い順で返す（タイムライン表示用）。
#[tauri::command]
fn get_undo_journal(repo_path: String) -> Result<Vec<UndoEntry>, String> {
    let r = open(&repo_path)?;
    undo::list(&r).map_err(|e| e.to_string())
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

/// 指定したコミット時点のファイル内容を作業ツリーに復元し、ステージする。
///
/// `commit_id` は復元元コミットのハッシュ（短縮形可）。`file_path` はリポジトリルートからの
/// 相対パス。指定コミットに対象ファイルが存在しない場合は日本語エラーを返す。
#[tauri::command]
fn restore_file_from_commit(
    repo_path: String,
    commit_id: String,
    file_path: String,
) -> Result<(), String> {
    let r = open(&repo_path)?;
    ops::restore_file_from_commit(&r, &commit_id, &file_path).map_err(|e| e.to_string())
}

/// HEAD の reflog（移動履歴）を新しい順に最大 `max` 件返す。
///
/// 各エントリには移動前後の OID・短縮形・生メッセージ・日本語化した操作説明・
/// タイムスタンプを含む。reflog が存在しないリポジトリでは空の配列を返す。
#[tauri::command]
fn get_reflog(repo_path: String, max: usize) -> Result<Vec<ReflogEntry>, String> {
    let r = open(&repo_path)?;
    repo::read_reflog(&r, max).map_err(|e| e.to_string())
}

/// ステージしようとしているファイルが機密性の高いものかを検出する。
///
/// `paths` はリポジトリルートからの相対パス（スラッシュ区切り）の一覧。
/// 機密ファイルが見つかった場合、その理由を日本語で説明した [`SensitiveWarning`] の一覧を返す。
/// 何も見つからなければ空の配列を返す。
#[tauri::command]
fn check_sensitive(repo_path: String, paths: Vec<String>) -> Result<Vec<SensitiveWarning>, String> {
    let r = open(&repo_path)?;
    // リポジトリの作業ツリーのルートパスを使う。bare の場合は repo_path をそのまま使う。
    let workdir = r
        .workdir()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from(&repo_path));
    let path_refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
    Ok(noobgit_core::safety::check_sensitive_files(
        &path_refs, &workdir,
    ))
}

/// ステージしようとしているファイルが Git LFS 移行候補（大容量・バイナリ）かを検出する。
///
/// `paths` はリポジトリルートからの相対パス（スラッシュ区切り）の一覧。
/// 候補ファイルが見つかった場合、情報を [`LfsCandidate`] の一覧で返す。
/// 何も見つからなければ空の配列を返す。
#[tauri::command]
fn check_lfs_candidates(
    repo_path: String,
    paths: Vec<String>,
) -> Result<Vec<LfsCandidate>, String> {
    let r = open(&repo_path)?;
    let workdir = r
        .workdir()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from(&repo_path));
    let path_refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
    Ok(noobgit_core::safety::check_lfs_candidates(
        &path_refs, &workdir,
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // フォルダ選択ダイアログ（参照ボタン）のためにダイアログプラグインを登録する。
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_branches,
            get_log,
            get_file_log,
            get_diff_unstaged,
            get_diff_staged,
            get_diff_conflict,
            get_diff_between,
            get_blame,
            get_conflicts,
            mark_resolved,
            get_branch_graph,
            explain_operation,
            assess_operation,
            stage_all,
            stage_path,
            stage_hunk,
            unstage,
            commit,
            amend_commit,
            squash_commits,
            reword_commit,
            discard_path,
            get_gitignore,
            add_to_gitignore,
            stash_save,
            stash_apply,
            stash_pop,
            get_stashes,
            stash_diff,
            get_identity,
            set_identity,
            create_branch,
            switch_branch,
            delete_branch,
            fetch,
            pull,
            reset_hard,
            push,
            cherry_pick,
            merge_branch,
            list_tags,
            create_tag,
            delete_tag,
            list_remotes,
            add_remote,
            remove_remote,
            set_remote_url,
            classify_network_error_cmd,
            get_undo_journal,
            peek_undo,
            undo_last,
            check_sensitive,
            check_lfs_candidates,
            restore_file_from_commit,
            get_reflog,
        ])
        .run(tauri::generate_context!())
        .expect("noobGit の起動に失敗しました");
}
