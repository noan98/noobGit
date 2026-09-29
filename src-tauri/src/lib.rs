//! Tauriコマンド層。ここは薄く保ち、実処理は `noobgit-core` に委ねる。
//!
//! 各コマンドは `Result<T, String>` を返すので、フロントは `invoke().catch()` で
//! 日本語のエラーメッセージをそのまま表示できる。

use std::sync::Mutex;

use git2::Repository;
use tauri::ipc::Channel;

use noobgit_core::error::{classify_network_error, NetworkErrorKind};
use noobgit_core::explain::{explain as explain_op, Explanation};
use noobgit_core::identity::{Identity, IdentityScope};
use noobgit_core::model::{
    BisectStatus, BlameHunk, BranchGraph, BranchInfo, BulkDeleteBranchesOutcome, CloneOutcome,
    CommitInfo, ConflictFile, FetchOutcome, FileChange, FileDiff, GitignorePatternCheck,
    GitignoreSuggestion, LfsCandidate, LogPage, MergeOutcome, MergedBranchInfo, NetworkProgress,
    PullOutcome, ReflogEntry, RemoteInfo, RepoStatus, SensitiveWarning, StashInfo,
    StashRestoreOutcome, TagInfo,
};
use noobgit_core::repo::{LogCursorStore, LogFilter};
use noobgit_core::safety::{assess, OperationKind, RiskAssessment, SafetyContext};
use noobgit_core::undo::{UndoApplicability, UndoEntry};
use noobgit_core::{bisect, identity, ops, repo, undo};

/// 書き込み系コマンドを 1 つずつ順番に実行するためのロック。
///
/// コマンドは `#[tauri::command(async)]` でメインスレッド（画面の描画・入力を
/// 処理するスレッド）の外で動かしている。これにより Git の処理中も画面が固まら
/// ないが、そのままだと複数の書き込み（例: ステージとコミット）が同時に走り、
/// index のロック競合や undo ジャーナルの書き込みが交錯しうる。以前は全コマンドが
/// メインスレッドで順番に実行されていたので、書き込み系だけはこのロックで同じ
/// 「1 つずつ」の性質を保つ。読み取り系はロックを取らず、並行して実行してよい。
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// [`WRITE_LOCK`] を取得する。前の書き込みがパニックしてロックが汚染されて
/// いても、以後の操作をすべて失敗させないよう、そのまま使い続ける
/// （守っているデータは無く、順番に実行することだけが目的のため）。
fn write_lock() -> std::sync::MutexGuard<'static, ()> {
    WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn open(repo_path: &str) -> Result<Repository, String> {
    repo::open(repo_path).map_err(|e| e.to_string())
}

/// 保護ブランチ一覧を返す。読み込みに失敗した場合は安全側に倒し、既定値
/// （main/master）にフォールバックする。
fn protected_branches_or_default(r: &Repository) -> Vec<String> {
    repo::load_protected_branches(r).unwrap_or_else(|_| {
        noobgit_core::safety::DEFAULT_PROTECTED_BRANCHES
            .iter()
            .map(|s| s.to_string())
            .collect()
    })
}

#[tauri::command(async)]
fn get_status(repo_path: String) -> Result<RepoStatus, String> {
    let r = open(&repo_path)?;
    repo::status(&r).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn get_branches(repo_path: String) -> Result<Vec<BranchInfo>, String> {
    let r = open(&repo_path)?;
    let protected = protected_branches_or_default(&r);
    repo::branches(&r, &protected).map_err(|e| e.to_string())
}

/// 保護ブランチ一覧を返す（未設定なら既定値の main/master）。
#[tauri::command(async)]
fn get_protected_branches(repo_path: String) -> Result<Vec<String>, String> {
    let r = open(&repo_path)?;
    Ok(protected_branches_or_default(&r))
}

/// 保護ブランチ一覧を保存する。空配列を渡すと既定値（main/master）に戻る。
#[tauri::command(async)]
fn set_protected_branches(repo_path: String, names: Vec<String>) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::save_protected_branches(&r, &names).map_err(|e| e.to_string())
}

/// コミット履歴をページングして返す。`filter` を渡すとメッセージ・作者・日付範囲で
/// 絞り込む。`filter` が `null`（未指定）のときは従来どおり全件を対象にする。
#[tauri::command(async)]
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

/// コミット履歴をカーソルベースでページングして返す（Issue #277）。
///
/// `cursor` が `null`（未指定）なら先頭ページを新しく開く。`cursor` を渡すと
/// （直前の呼び出しが返した ID をそのまま渡せば）その続きを取得する —
/// `noobgit-core` 側で revwalk の走査状態そのものを保持し続けるため、
/// `skip` を渡す従来の `get_log` と違い、ページ数を重ねても各回のコストが
/// 「すでに読んだ件数」に依存しない（詳しい設計意図は
/// `noobgit_core::repo::LogCursorStore` のドキュメントを参照）。
///
/// カーソルが失効している場合（プロセス再起動や、閉じ忘れの蓄積によるキャッシュの
/// 立ち退きなど、通常運用ではまず起きない）は、フロントエンドが渡す
/// `fallback_skip`（現在表示済みのコミット数）を使って、従来の skip ベース取得
/// （[`repo::log_filtered`]）に一度だけ自動でフォールバックする。フロントエンドは
/// カーソル切れを個別に扱う必要はない。
///
/// ページの続きを使い切った・検索条件を変えた・タブを閉じたときは
/// [`close_log_cursor`] でカーソルを手放すこと。
#[tauri::command(async)]
fn get_log_page(
    repo_path: String,
    max: usize,
    filter: Option<LogFilter>,
    cursor: Option<String>,
    fallback_skip: usize,
    cursors: tauri::State<'_, Mutex<LogCursorStore>>,
) -> Result<LogPage, String> {
    let filter = filter.unwrap_or_default();
    // ロック中のパニックで以後ずっと使えなくなるのを避け、汚染されていても
    // 中身（キャッシュの中身）はそのまま使い続ける。
    let mut store = cursors.lock().unwrap_or_else(|e| e.into_inner());

    let Some(id) = cursor else {
        return store
            .first_page(&repo_path, filter, max)
            .map_err(|e| e.to_string());
    };

    match store.next_page(&id, max).map_err(|e| e.to_string())? {
        Some(page) => Ok(page),
        None => {
            // カーソルが見つからない（失効済み）。フロントエンドが持っている
            // 表示済み件数を skip として渡し、従来の方式で一度だけ取り直す。
            let r = open(&repo_path)?;
            let commits =
                repo::log_filtered(&r, fallback_skip, max, &filter).map_err(|e| e.to_string())?;
            let has_more = commits.len() == max;
            Ok(LogPage {
                commits,
                cursor: None,
                has_more,
            })
        }
    }
}

/// 使い終わったログカーソルを手放す（検索条件の変更・リフレッシュ・タブを閉じる等）。
/// 存在しない ID を渡しても何も起きない。
#[tauri::command]
fn close_log_cursor(cursor: String, cursors: tauri::State<'_, Mutex<LogCursorStore>>) {
    let mut store = cursors.lock().unwrap_or_else(|e| e.into_inner());
    store.close(&cursor);
}

/// 指定ファイルを変更したコミットを新しい順に最大 `max` 件返す（ファイル別履歴）。
#[tauri::command(async)]
fn get_file_log(repo_path: String, path: String, max: usize) -> Result<Vec<CommitInfo>, String> {
    let r = open(&repo_path)?;
    repo::file_log(&r, &path, max).map_err(|e| e.to_string())
}

/// 過去のコミット履歴から `prefix` に前方一致する件名候補を頻度順で返す
/// （コミットメッセージのインライン補完 #185）。
#[tauri::command(async)]
fn suggest_commit_messages(
    repo_path: String,
    prefix: String,
    max: usize,
) -> Result<Vec<String>, String> {
    let r = open(&repo_path)?;
    repo::suggest_commit_messages(&r, &prefix, max).map_err(|e| e.to_string())
}

/// 指定ファイルの未ステージ差分（インデックス↔作業ツリー）を返す。
#[tauri::command(async)]
fn get_diff_unstaged(repo_path: String, path: String) -> Result<FileDiff, String> {
    let r = open(&repo_path)?;
    repo::diff_unstaged(&r, &path).map_err(|e| e.to_string())
}

/// 指定ファイルのステージ済み差分（HEAD↔インデックス）を返す。
#[tauri::command(async)]
fn get_diff_staged(repo_path: String, path: String) -> Result<FileDiff, String> {
    let r = open(&repo_path)?;
    repo::diff_staged(&r, &path).map_err(|e| e.to_string())
}

/// コンフリクト中ファイルの作業ツリーの内容（競合の目印を含む）を返す。
#[tauri::command(async)]
fn get_diff_conflict(repo_path: String, path: String) -> Result<FileDiff, String> {
    let r = open(&repo_path)?;
    repo::diff_conflict(&r, &path).map_err(|e| e.to_string())
}

/// 2 つのコミット間（または親コミット↔指定コミット）の全変更ファイルの差分を返す。
///
/// `from_oid` が `null` のときは `to_oid` の第1親との比較になる。
#[tauri::command(async)]
fn get_diff_between(
    repo_path: String,
    from_oid: Option<String>,
    to_oid: String,
) -> Result<Vec<FileDiff>, String> {
    let r = open(&repo_path)?;
    repo::diff_commits(&r, from_oid.as_deref(), &to_oid).map_err(|e| e.to_string())
}

/// 指定ファイルの blame（各行を最後に変更したコミット）を返す。
#[tauri::command(async)]
fn get_blame(repo_path: String, path: String) -> Result<Vec<BlameHunk>, String> {
    let r = open(&repo_path)?;
    repo::blame_file(&r, &path).map_err(|e| e.to_string())
}

/// コンフリクト中のファイル一覧を返す（解消ウィザード用）。
#[tauri::command(async)]
fn get_conflicts(repo_path: String) -> Result<Vec<ConflictFile>, String> {
    let r = open(&repo_path)?;
    repo::get_conflicts(&r).map_err(|e| e.to_string())
}

/// 指定ファイルのコンフリクトを「解消済み」としてマークする（解消した内容をステージ）。
#[tauri::command(async)]
fn mark_resolved(repo_path: String, path: String) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::mark_resolved(&r, &path).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn get_branch_graph(repo_path: String) -> Result<BranchGraph, String> {
    let r = open(&repo_path)?;
    repo::branch_graph(&r).map_err(|e| e.to_string())
}

#[tauri::command]
fn explain_operation(op: OperationKind) -> Explanation {
    explain_op(op)
}

/// 危険度の評価に必要なリポジトリの状態。1 回だけ調べて、複数の評価で使い回す。
struct AssessState {
    working_dir_dirty: bool,
    protected_branches: Vec<String>,
    /// HEAD が公開（push）済みか。amend / rebase の評価が 1 件も無ければ調べない。
    head_published: bool,
}

impl AssessState {
    fn load(r: &Repository, needs_head_published: bool) -> Result<Self, String> {
        Ok(Self {
            working_dir_dirty: repo::is_dirty(r).map_err(|e| e.to_string())?,
            protected_branches: protected_branches_or_default(r),
            head_published: needs_head_published && repo::head_is_published(r).unwrap_or(false),
        })
    }

    fn assess(&self, op: OperationKind, target_branch: Option<String>) -> RiskAssessment {
        // amend / rebase のときだけ、HEAD が公開済みかを危険度の引き上げに使う。
        let head_published =
            matches!(op, OperationKind::AmendCommit | OperationKind::Rebase) && self.head_published;
        let ctx = SafetyContext {
            target_branch,
            working_dir_dirty: self.working_dir_dirty,
            protected_branches: self.protected_branches.clone(),
            head_published,
        };
        assess(op, &ctx)
    }
}

fn needs_head_published(op: OperationKind) -> bool {
    matches!(op, OperationKind::AmendCommit | OperationKind::Rebase)
}

/// 操作のリスクを評価する。未コミット変更の有無はリポジトリから自動判定する。
#[tauri::command(async)]
fn assess_operation(
    repo_path: String,
    op: OperationKind,
    target_branch: Option<String>,
) -> Result<RiskAssessment, String> {
    let r = open(&repo_path)?;
    let state = AssessState::load(&r, needs_head_published(op))?;
    Ok(state.assess(op, target_branch))
}

/// [`assess_operations`] の 1 件分の依頼。
#[derive(serde::Deserialize)]
struct AssessRequest {
    op: OperationKind,
    target_branch: Option<String>,
}

/// 複数の操作のリスクをまとめて評価する（ボタンの危険度カラー用, #274）。
///
/// 1 件ずつ [`assess_operation`] を呼ぶと、そのたびにリポジトリを開き直し、
/// 作業ツリー全体を調べ直すことになる（ブランチが多いと数十回）。ここでは
/// リポジトリの状態を 1 回だけ調べて、すべての評価で使い回す。
/// 結果は `requests` と同じ順序・同じ件数で返す。
#[tauri::command(async)]
fn assess_operations(
    repo_path: String,
    requests: Vec<AssessRequest>,
) -> Result<Vec<RiskAssessment>, String> {
    let r = open(&repo_path)?;
    let needs = requests.iter().any(|q| needs_head_published(q.op));
    let state = AssessState::load(&r, needs)?;
    Ok(requests
        .into_iter()
        .map(|q| state.assess(q.op, q.target_branch))
        .collect())
}

#[tauri::command(async)]
fn stage_all(repo_path: String) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::stage_all(&r).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn stage_path(repo_path: String, path: String) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::stage_path(&r, &path).map_err(|e| e.to_string())
}

/// 指定ファイルの差分のうち、`hunk_header` に一致する塊（hunk）だけをステージする。
#[tauri::command(async)]
fn stage_hunk(repo_path: String, file_path: String, hunk_header: String) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::stage_hunk(&r, &file_path, &hunk_header).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn unstage(repo_path: String, path: String) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::unstage(&r, &path).map_err(|e| e.to_string())
}

/// 指定ファイルのステージ済み差分のうち、`hunk_header` に一致する塊（hunk）だけを
/// アンステージする。作業ツリーは変わらない。
#[tauri::command(async)]
fn unstage_hunk(repo_path: String, file_path: String, hunk_header: String) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::unstage_hunk(&r, &file_path, &hunk_header).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn commit(repo_path: String, message: String) -> Result<CommitInfo, String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::commit(&r, &message).map_err(|e| e.to_string())
}

/// 直前のコミットを書き換える（amend）。メッセージが空ならもとのメッセージを保つ。
#[tauri::command(async)]
fn amend_commit(repo_path: String, message: String) -> Result<CommitInfo, String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::amend_commit(&r, &message).map_err(|e| e.to_string())
}

/// HEAD から連続する複数のコミットを1つにまとめる（squash）。
///
/// `commit_oids` は HEAD から連続する範囲を新しい順（先頭が HEAD）で渡す。
#[tauri::command(async)]
fn squash_commits(
    repo_path: String,
    commit_oids: Vec<String>,
    message: String,
) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    let refs: Vec<&str> = commit_oids.iter().map(|s| s.as_str()).collect();
    ops::squash_commits(&r, &refs, &message).map_err(|e| e.to_string())
}

/// 最新のコミット（HEAD）のメッセージだけを書き換える（reword）。
#[tauri::command(async)]
fn reword_commit(repo_path: String, message: String) -> Result<CommitInfo, String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::reword_commit(&r, &message).map_err(|e| e.to_string())
}

/// 指定パスの、まだコミットしていない変更を捨てる（破棄）。元に戻せない破壊的操作。
#[tauri::command(async)]
fn discard_path(repo_path: String, path: String) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::discard_path(&r, &path).map_err(|e| e.to_string())
}

/// リポジトリ直下の `.gitignore` の内容を返す（ファイルが無ければ null）。
#[tauri::command(async)]
fn get_gitignore(repo_path: String) -> Result<Option<String>, String> {
    let r = open(&repo_path)?;
    repo::read_gitignore(&r).map_err(|e| e.to_string())
}

/// `.gitignore` の末尾にパターンを 1 行追記する（ファイルが無ければ新規作成）。
#[tauri::command(async)]
fn add_to_gitignore(repo_path: String, pattern: String) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::add_to_gitignore(&r, &pattern).map_err(|e| e.to_string())
}

/// `.gitignore` の 1 パターンを、glob 構文チェックと重複チェックの両方込みで検証する。
///
/// 入力中のリアルタイムバリデーションと、追加ボタン押下前の重複確認の両方に使う。
#[tauri::command(async)]
fn check_gitignore_pattern(
    repo_path: String,
    pattern: String,
) -> Result<GitignorePatternCheck, String> {
    let r = open(&repo_path)?;
    ops::check_gitignore_pattern(&r, &pattern).map_err(|e| e.to_string())
}

/// ファイルパスから `.gitignore` パターンの候補（このファイルのみ／同じ拡張子／
/// ディレクトリ全体）を生成する。リポジトリの状態には依存しない純粋な変換。
#[tauri::command]
fn suggest_gitignore_patterns(path: String) -> Vec<GitignoreSuggestion> {
    ops::suggest_gitignore_patterns(&path)
}

/// 現在の変更を一時的にしまう（stash 退避）。未追跡ファイルも含めて退避する。
#[tauri::command(async)]
fn stash_save(repo_path: String, message: String) -> Result<(), String> {
    let _write = write_lock();
    let mut r = open(&repo_path)?;
    ops::stash_save(&mut r, &message).map_err(|e| e.to_string())
}

/// 退避を作業ツリーに取り出す（一覧には残す）。コンフリクトが起きた場合も
/// エラーにはせず、`StashRestoreOutcome.conflicted` で伝える（フロントの
/// コンフリクト解消ウィザードへ自然につなげるため）。
#[tauri::command(async)]
fn stash_apply(repo_path: String, index: usize) -> Result<StashRestoreOutcome, String> {
    let _write = write_lock();
    let mut r = open(&repo_path)?;
    ops::stash_apply(&mut r, index).map_err(|e| e.to_string())
}

/// 退避を作業ツリーに取り出し、コンフリクトが無ければ一覧から取り除く（pop）。
/// コンフリクトが起きた場合は退避を一覧に残す（`StashRestoreOutcome.conflicted` で伝える）。
#[tauri::command(async)]
fn stash_pop(repo_path: String, index: usize) -> Result<StashRestoreOutcome, String> {
    let _write = write_lock();
    let mut r = open(&repo_path)?;
    ops::stash_pop(&mut r, index).map_err(|e| e.to_string())
}

/// 退避を一覧から取り除く（中身は復元できない）。undo は記録しない。
#[tauri::command(async)]
fn stash_drop(repo_path: String, stash_id: String) -> Result<(), String> {
    let _write = write_lock();
    let mut r = open(&repo_path)?;
    ops::stash_drop(&mut r, &stash_id).map_err(|e| e.to_string())
}

/// 退避の一覧を返す（0 がいちばん新しい退避）。
#[tauri::command(async)]
fn get_stashes(repo_path: String) -> Result<Vec<StashInfo>, String> {
    let mut r = open(&repo_path)?;
    ops::stash_list(&mut r).map_err(|e| e.to_string())
}

/// 指定 index の退避に含まれる変更ファイル一覧を返す（退避は適用しない安全な操作）。
#[tauri::command(async)]
fn stash_diff(repo_path: String, index: usize) -> Result<Vec<FileChange>, String> {
    let mut r = open(&repo_path)?;
    ops::stash_diff(&mut r, index).map_err(|e| e.to_string())
}

/// 現在の identity（user.name / user.email）を取得する。初回セットアップ案内に使う。
#[tauri::command(async)]
fn get_identity(repo_path: String) -> Result<Identity, String> {
    let r = open(&repo_path)?;
    identity::get_identity(&r).map_err(|e| e.to_string())
}

/// identity を保存する。`scope` で保存先（ローカル/グローバル）を選ぶ。
#[tauri::command(async)]
fn set_identity(
    repo_path: String,
    name: String,
    email: String,
    scope: IdentityScope,
) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    identity::set_identity(&r, &name, &email, scope).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn create_branch(repo_path: String, name: String) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::create_branch(&r, &name).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn switch_branch(repo_path: String, name: String) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::switch_branch(&r, &name).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn delete_branch(repo_path: String, name: String) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::delete_branch(&r, &name).map_err(|e| e.to_string())
}

/// マージ済みローカルブランチ（保護ブランチのいずれかに取り込み済み）の一覧を返す。
///
/// 保護ブランチ自身・現在チェックアウト中のブランチは含まれない。保護ブランチが
/// ローカルに1つも無い場合は空の配列を返す（ブランチクリーンアップ導線 #269）。
#[tauri::command(async)]
fn get_merged_branches(repo_path: String) -> Result<Vec<MergedBranchInfo>, String> {
    let r = open(&repo_path)?;
    let protected = protected_branches_or_default(&r);
    repo::merged_branches(&r, &protected).map_err(|e| e.to_string())
}

/// マージ済みブランチを一括削除する（ブランチクリーンアップ導線 #269）。
///
/// フロントから渡された `names` はそのまま信用せず、core 側（[`ops::delete_branches`]）が
/// 削除直前に再検証する。条件を満たさないブランチは削除せずスキップし、理由と合わせて返す。
#[tauri::command(async)]
fn delete_branches(
    repo_path: String,
    names: Vec<String>,
) -> Result<BulkDeleteBranchesOutcome, String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    let protected = protected_branches_or_default(&r);
    ops::delete_branches(&r, &names, &protected).map_err(|e| e.to_string())
}

/// リモートから最新を取得し、リモート追跡ブランチを更新する（作業ツリーは変えない）。
///
/// `progress` へ受信オブジェクト数などの進捗を Tauri の Channel 経由で
/// フロントエンドへ都度ストリーミング送信する（#167 進捗フィードバック）。
/// 送信自体が失敗しても fetch は継続する（進捗表示はベストエフォート）。
/// `Channel<T>` は Tauri の IPC 参照型で `Option` にはできないため、フロントエンドは
/// 進捗を使わないときも（何もしない onmessage の）Channel を渡す（`src/api.ts` 参照）。
#[tauri::command(async)]
fn fetch(
    repo_path: String,
    remote: String,
    progress: Channel<NetworkProgress>,
) -> Result<FetchOutcome, String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    let mut on_progress = move |p: NetworkProgress| {
        let _ = progress.send(p);
    };
    ops::fetch_with_progress(&r, &remote, &mut on_progress).map_err(|e| e.to_string())
}

/// fetch 後、安全に進められるとき（fast-forward）だけ取り込む。分岐時は中断する。
///
/// `progress` は fetch 部分（データ受信）の進捗を通知する。fast-forward 自体は
/// ローカルの作業なので進捗イベントは発生しない。
#[tauri::command(async)]
fn pull(
    repo_path: String,
    remote: String,
    branch: String,
    progress: Channel<NetworkProgress>,
) -> Result<PullOutcome, String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    let mut on_progress = move |p: NetworkProgress| {
        let _ = progress.send(p);
    };
    ops::pull_with_progress(&r, &remote, &branch, &mut on_progress).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn reset_hard(repo_path: String, revspec: String) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::reset_hard(&r, &revspec).map_err(|e| e.to_string())
}

/// ローカルのコミットをリモートへ送信する。`force` が真なら強制 push。
///
/// `progress` を渡すと、送信オブジェクト数などの進捗を Tauri の Channel 経由で
/// フロントエンドへ都度ストリーミング送信する（#167 進捗フィードバック）。
#[tauri::command(async)]
fn push(
    repo_path: String,
    remote: String,
    refspec: String,
    force: bool,
    progress: Channel<NetworkProgress>,
) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    let mut on_progress = move |p: NetworkProgress| {
        let _ = progress.send(p);
    };
    ops::push_with_progress(&r, &remote, &refspec, force, &mut on_progress)
        .map_err(|e| e.to_string())
}

/// リモートリポジトリを `dest_path` へ新規にクローンする。
///
/// クローンはまだリポジトリが存在しない状態から始めるので、他のコマンドと違い
/// `open()` を経由しない（`repo_path` を取らない）。`progress` へ受信オブジェクト数
/// などの進捗を Tauri の Channel 経由でフロントエンドへ都度ストリーミング送信する
/// （fetch / pull / push と同じ方式。#167 進捗フィードバック）。
#[tauri::command(async)]
fn clone_repo(
    url: String,
    dest_path: String,
    progress: Channel<NetworkProgress>,
) -> Result<CloneOutcome, String> {
    let mut on_progress = move |p: NetworkProgress| {
        let _ = progress.send(p);
    };
    ops::clone_with_progress(&url, std::path::Path::new(&dest_path), &mut on_progress)
        .map_err(|e| e.to_string())
}

/// 指定したローカルブランチを現在のブランチにマージする。
/// コンフリクトが発生した場合は `Conflicted` を返し、リポジトリをマージ中の状態にする。
#[tauri::command(async)]
fn merge_branch(repo_path: String, branch_name: String) -> Result<MergeOutcome, String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::merge_branch(&r, &branch_name).map_err(|e| e.to_string())
}

/// 指定したコミットの変更を、いまのブランチの先頭にコピーする（cherry-pick）。
#[tauri::command(async)]
fn cherry_pick(repo_path: String, oid: String) -> Result<CommitInfo, String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::cherry_pick(&r, &oid).map_err(|e| e.to_string())
}

/// タグの一覧を返す（名前順）。
#[tauri::command(async)]
fn list_tags(repo_path: String) -> Result<Vec<TagInfo>, String> {
    let r = open(&repo_path)?;
    repo::list_tags(&r).map_err(|e| e.to_string())
}

/// コミットに目印（タグ）を付ける。`target` 省略時は HEAD、`message` 省略時は軽量タグ。
#[tauri::command(async)]
fn create_tag(
    repo_path: String,
    name: String,
    target: Option<String>,
    message: Option<String>,
) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::create_tag(&r, &name, target.as_deref(), message.as_deref()).map_err(|e| e.to_string())
}

/// タグ（目印）を削除する。直後に Undo で復元できる。
#[tauri::command(async)]
fn delete_tag(repo_path: String, name: String) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::delete_tag(&r, &name).map_err(|e| e.to_string())
}

/// リモートリポジトリの一覧を返す（名前順）。
#[tauri::command(async)]
fn list_remotes(repo_path: String) -> Result<Vec<RemoteInfo>, String> {
    let r = open(&repo_path)?;
    repo::list_remotes(&r).map_err(|e| e.to_string())
}

/// リモートリポジトリを追加する。
#[tauri::command(async)]
fn add_remote(repo_path: String, name: String, url: String) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::add_remote(&r, &name, &url).map_err(|e| e.to_string())
}

/// リモートリポジトリを削除する。
#[tauri::command(async)]
fn remove_remote(repo_path: String, name: String) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::remove_remote(&r, &name).map_err(|e| e.to_string())
}

/// リモートリポジトリの fetch URL を変更する。
#[tauri::command(async)]
fn set_remote_url(repo_path: String, name: String, url: String) -> Result<(), String> {
    let _write = write_lock();
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
#[tauri::command(async)]
fn get_undo_journal(repo_path: String) -> Result<Vec<UndoEntry>, String> {
    let r = open(&repo_path)?;
    undo::list(&r).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn peek_undo(repo_path: String) -> Result<Option<UndoEntry>, String> {
    let r = open(&repo_path)?;
    Ok(undo::peek(&r).ok().flatten())
}

/// 直前の操作を取り消す。履歴が進んでいて新しい作業も巻き戻る恐れがある場合は、
/// `confirm_risky` が true でない限り何も変えずにエラーを返す（#201）。
#[tauri::command(async)]
fn undo_last(repo_path: String, confirm_risky: Option<bool>) -> Result<String, String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    undo::undo_last_confirmed(&r, confirm_risky.unwrap_or(false)).map_err(|e| e.to_string())
}

/// 取り消し履歴の各エントリが今のリポジトリ状態で適用できるかを検証する
/// （`get_undo_journal` と同じ古い順）。読み取り専用（#201）。
#[tauri::command(async)]
fn get_undo_applicability(repo_path: String) -> Result<Vec<UndoApplicability>, String> {
    let r = open(&repo_path)?;
    undo::validate_journal(&r).map_err(|e| e.to_string())
}

/// 適用不能になった取り消し履歴を整理し、取り除いた件数を返す（#201）。
#[tauri::command(async)]
fn prune_undo_journal(repo_path: String) -> Result<usize, String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    undo::prune_unresolvable(&r).map_err(|e| e.to_string())
}

/// 指定したコミット時点のファイル内容を作業ツリーに復元し、ステージする。
///
/// `commit_id` は復元元コミットのハッシュ（短縮形可）。`file_path` はリポジトリルートからの
/// 相対パス。指定コミットに対象ファイルが存在しない場合は日本語エラーを返す。
#[tauri::command(async)]
fn restore_file_from_commit(
    repo_path: String,
    commit_id: String,
    file_path: String,
) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    ops::restore_file_from_commit(&r, &commit_id, &file_path).map_err(|e| e.to_string())
}

/// HEAD の reflog（移動履歴）を新しい順に最大 `max` 件返す。
///
/// 各エントリには移動前後の OID・短縮形・生メッセージ・日本語化した操作説明・
/// タイムスタンプを含む。reflog が存在しないリポジトリでは空の配列を返す。
#[tauri::command(async)]
fn get_reflog(repo_path: String, max: usize) -> Result<Vec<ReflogEntry>, String> {
    let r = open(&repo_path)?;
    repo::read_reflog(&r, max).map_err(|e| e.to_string())
}

/// ステージしようとしているファイルが機密性の高いものかを検出する。
///
/// `paths` はリポジトリルートからの相対パス（スラッシュ区切り）の一覧。
/// 機密ファイルが見つかった場合、その理由を日本語で説明した [`SensitiveWarning`] の一覧を返す。
/// 何も見つからなければ空の配列を返す。
#[tauri::command(async)]
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
#[tauri::command(async)]
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

/// Bisect（バグ混入コミットの二分探索）を開始する。
/// `bad` は「壊れている」コミット、`good` は「動いていた」コミット（どちらも revspec）。
#[tauri::command(async)]
fn bisect_start(repo_path: String, bad: String, good: String) -> Result<BisectStatus, String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    bisect::bisect_start(&r, &bad, &good).map_err(|e| e.to_string())
}

/// いま Bisect が調べているコミットについて good/bad を記録し、次の候補へ進める。
#[tauri::command(async)]
fn bisect_mark(repo_path: String, commit: String, is_good: bool) -> Result<BisectStatus, String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    bisect::bisect_mark(&r, &commit, is_good).map_err(|e| e.to_string())
}

/// Bisect セッションを終了し、開始前のブランチ（または元のコミット）へ戻す。
#[tauri::command(async)]
fn bisect_reset(repo_path: String) -> Result<(), String> {
    let _write = write_lock();
    let r = open(&repo_path)?;
    bisect::bisect_reset(&r).map_err(|e| e.to_string())
}

/// 現在の Bisect セッションの状態を返す（無ければ null）。タブの再表示やアプリ再起動後の
/// 復元に使う読み取り専用コマンド。
#[tauri::command(async)]
fn bisect_status(repo_path: String) -> Result<Option<BisectStatus>, String> {
    let r = open(&repo_path)?;
    bisect::bisect_status(&r).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // フォルダ選択ダイアログ（参照ボタン）のためにダイアログプラグインを登録する。
        .plugin(tauri_plugin_dialog::init())
        // コミット履歴のカーソルベースページング（Issue #277）用のキャッシュ。
        // Git ロジックは `noobgit-core::repo::LogCursorStore` 側に閉じており、
        // ここでは `Mutex` に包んでプロセス内で保持するだけ。
        .manage(Mutex::new(LogCursorStore::new()))
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_branches,
            get_protected_branches,
            set_protected_branches,
            get_log,
            get_log_page,
            close_log_cursor,
            get_file_log,
            suggest_commit_messages,
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
            assess_operations,
            stage_all,
            stage_path,
            stage_hunk,
            unstage,
            unstage_hunk,
            commit,
            amend_commit,
            squash_commits,
            reword_commit,
            discard_path,
            get_gitignore,
            add_to_gitignore,
            check_gitignore_pattern,
            suggest_gitignore_patterns,
            stash_save,
            stash_apply,
            stash_pop,
            stash_drop,
            get_stashes,
            stash_diff,
            get_identity,
            set_identity,
            create_branch,
            switch_branch,
            delete_branch,
            get_merged_branches,
            delete_branches,
            fetch,
            pull,
            reset_hard,
            push,
            clone_repo,
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
            get_undo_applicability,
            prune_undo_journal,
            check_sensitive,
            check_lfs_candidates,
            restore_file_from_commit,
            get_reflog,
            bisect_start,
            bisect_mark,
            bisect_reset,
            bisect_status,
        ])
        .run(tauri::generate_context!())
        .expect("noobGit の起動に失敗しました");
}
