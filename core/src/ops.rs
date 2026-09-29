use std::cell::{Cell, RefCell};
use std::path::{Component, Path};

use git2::build::{CheckoutBuilder, RepoBuilder};
use git2::{
    BranchType, Commit, Cred, CredentialType, FetchOptions, IndexAddOption, PushOptions,
    RemoteCallbacks, Repository, ResetType, StashFlags,
};

use crate::error::{
    describe_git2_error, describe_git2_error_keep_unknown, describe_io_error, CoreError, Result,
};
use crate::model::{
    BulkDeleteBranchesOutcome, ChangeKind, CloneOutcome, CommitInfo, FetchOutcome, FileChange,
    GitignorePatternCheck, GitignoreSuggestion, MergeOutcome, NetworkProgress,
    NetworkProgressStage, PullOutcome, SkippedBranch, StashInfo, StashRestoreOutcome,
    SwitchWithStashOutcome,
};
use crate::repo::{current_branch, is_submodule_path, merged_branches, read_gitignore};
use crate::safety::OperationKind;
use crate::undo::{self, UndoAction, UndoEntry};

/// 作業ツリーの全変更（追加・変更・削除）をインデックスに載せる。
pub fn stage_all(repo: &Repository) -> Result<()> {
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None)?;
    // 追跡中ファイルの削除も拾う。
    index.update_all(["*"].iter(), None)?;
    index.write()?;
    Ok(())
}

/// サブモジュール（`.gitmodules` に登録された「リポジトリの中の別リポジトリ」）のパスに
/// 対する書き込み操作を拒否するときの共通メッセージ。
///
/// noobGit はサブモジュールの中身（クローン・更新・コミット等）を一切サポートしない。
/// 中途半端に操作すると壊れた中間状態を作りかねないので、検出したら安全に拒否し、
/// 代わりにターミナルや他の Git ツールでの操作を案内する。
fn submodule_blocked_message(path: &str) -> String {
    format!(
        "「{path}」はサブモジュール（リポジトリの中に埋め込まれた別のGitリポジトリ）です。\
         noobGit はサブモジュールの中身を操作できません。\
         ターミナルや他のGitツールで操作してください。"
    )
}

/// リポジトリ内を指す相対パスであることを検証する。
///
/// 絶対パスや `..` を含むパスは作業ツリーの外に出られてしまうため、パスを受け取る
/// 書き込み系の操作では一律拒否する（libgit2 の生エラーに任せず、先に平易に断る）。
fn ensure_repo_relative_path(path: &str) -> Result<()> {
    let rel = Path::new(path);
    if rel.as_os_str().is_empty()
        || rel.is_absolute()
        || rel.components().any(|c| matches!(c, Component::ParentDir))
    {
        return Err(CoreError::InvalidInput(format!("不正なパスです: {path}")));
    }
    Ok(())
}

/// 指定パスをステージする。ファイルが消えていれば削除としてステージする。
///
/// サブモジュールのパスが指定された場合は、意図しない挙動を避けるため安全に拒否する。
pub fn stage_path(repo: &Repository, path: &str) -> Result<()> {
    ensure_repo_relative_path(path)?;
    if is_submodule_path(repo, path) {
        return Err(CoreError::Blocked(submodule_blocked_message(path)));
    }
    let mut index = repo.index()?;
    let exists = repo
        .workdir()
        .map(|w| w.join(path).exists())
        .unwrap_or(false);
    if exists {
        index.add_path(Path::new(path))?;
    } else {
        index.remove_path(Path::new(path))?;
    }
    index.write()?;
    Ok(())
}

/// コンフリクトを解消したファイルを「解消済み」としてマークする。
///
/// 解消した内容（作業ツリーの当該ファイル）をインデックスに載せると、libgit2 は
/// そのパスの conflict エントリ（stage 1/2/3）を取り除いて通常のステージ済み
/// （stage 0）に置き換える。これがコンフリクト解消マークの実体。ファイルが消えて
/// いる（削除で解消した）場合はインデックスから取り除く。マーク後はそのまま
/// コミットへ進める。undo は通常のステージと同じ扱いなので記録しない。
pub fn mark_resolved(repo: &Repository, path: &str) -> Result<()> {
    ensure_repo_relative_path(path)?;
    let mut index = repo.index()?;
    let exists = repo
        .workdir()
        .map(|w| w.join(path).exists())
        .unwrap_or(false);
    if exists {
        index.add_path(Path::new(path))?;
    } else {
        index.remove_path(Path::new(path))?;
    }
    index.write()?;
    Ok(())
}

/// 指定パスのステージを解除する（変更内容は保持）。
pub fn unstage(repo: &Repository, path: &str) -> Result<()> {
    ensure_repo_relative_path(path)?;
    match repo.head() {
        Ok(head) => {
            let commit = head.peel_to_commit()?;
            repo.reset_default(Some(commit.as_object()), [Path::new(path)])?;
        }
        Err(_) => {
            // まだコミットが無い（未誕生ブランチ）。インデックスから外すだけ。
            let mut index = repo.index()?;
            index.remove_path(Path::new(path))?;
            index.write()?;
        }
    }
    Ok(())
}

/// 指定ファイルの差分のうち、`hunk_header` に一致する hunk（変更の塊）だけをステージする。
///
/// `file_path` の未ステージ差分（index と作業ツリーの差分）を取り、`hunk_header`
/// （例 `@@ -1,3 +1,4 @@`）に一致する hunk だけをインデックスへ適用する。ほかの hunk は
/// 未ステージのまま残る。該当 hunk が見つからなければ入力エラーにする。
///
/// 取り消し用に、そのパスのステージ解除（`UnstagePath`）を undo に記録する。
pub fn stage_hunk(repo: &Repository, file_path: &str, hunk_header: &str) -> Result<()> {
    let file_path = file_path.trim();
    let hunk_header = hunk_header.trim();
    if file_path.is_empty() {
        return Err(CoreError::InvalidInput(
            "ステージするファイルを指定してください。".to_string(),
        ));
    }
    if hunk_header.is_empty() {
        return Err(CoreError::InvalidInput(
            "ステージする変更の塊（hunk）を指定してください。".to_string(),
        ));
    }
    ensure_repo_relative_path(file_path)?;
    if is_submodule_path(repo, file_path) {
        return Err(CoreError::Blocked(submodule_blocked_message(file_path)));
    }

    // 対象パスだけの未ステージ差分（index → 作業ツリー）を取る。
    let mut diff_opts = git2::DiffOptions::new();
    diff_opts.pathspec(file_path).context_lines(3);
    let diff = repo.diff_index_to_workdir(None, Some(&mut diff_opts))?;

    // 指定された hunk が差分に含まれるか先に確認する（無ければ入力エラー）。
    let matched = Cell::new(false);
    diff.foreach(
        &mut |_delta, _progress| true,
        None,
        Some(&mut |_delta, hunk| {
            if normalize_hunk_header(hunk.header()) == hunk_header {
                matched.set(true);
            }
            true
        }),
        None,
    )?;
    if !matched.get() {
        return Err(CoreError::InvalidInput(format!(
            "指定した変更の塊（hunk）が見つかりませんでした: {hunk_header}"
        )));
    }

    // 一致する hunk だけを index へ適用する。
    let mut apply_opts = git2::ApplyOptions::new();
    // 対象パス以外は触らない。
    apply_opts.delta_callback(move |delta| {
        delta
            .and_then(|d| d.new_file().path())
            .map(|p| p.to_string_lossy() == file_path)
            .unwrap_or(false)
    });
    // ヘッダーが一致する hunk だけ true を返して選択適用する。
    apply_opts.hunk_callback(move |hunk| {
        hunk.map(|h| normalize_hunk_header(h.header()) == hunk_header)
            .unwrap_or(false)
    });

    repo.apply(&diff, git2::ApplyLocation::Index, Some(&mut apply_opts))
        .map_err(|e| {
            CoreError::Git(format!(
                "変更の塊（hunk）のステージに失敗しました: {}",
                describe_git2_error(&e)
            ))
        })?;

    record_undo(
        repo,
        UndoEntry {
            op: OperationKind::Stage,
            description: format!("「{file_path}」の一部（hunk）のステージを取り消す"),
            action: UndoAction::UnstagePath {
                path: file_path.to_string(),
            },
        },
    );
    Ok(())
}

/// hunk ヘッダー文字列を比較用に整える（末尾の改行を落とす）。
///
/// git2 の hunk ヘッダーは `@@ -1,3 +1,4 @@\n` のように末尾に改行を含むことがあるため、
/// 呼び出し側から渡される `@@ -1,3 +1,4 @@`（改行なし）と比較できるよう揃える。
fn normalize_hunk_header(header: &[u8]) -> String {
    String::from_utf8_lossy(header).trim_end().to_string()
}

/// 指定ファイルのステージ済み差分のうち、`hunk_header` に一致する hunk（変更の塊）だけを
/// アンステージする（＝ index を HEAD 側へ部分的に戻す）。作業ツリーは一切変更しない。
///
/// `file_path` のステージ済み差分（HEAD と index の差分）を取り、`hunk_header`
/// （例 `@@ -1,3 +1,4 @@`。ステージ済み差分表示で使われているのと同じヘッダー文字列）に
/// 一致する hunk だけを index から取り除く。ほかの hunk はステージされたまま残る。
/// 該当 hunk が見つからなければ入力エラーにする。`stage_hunk` と対になる操作。
///
/// 取り消し用に、操作前のこのパスのインデックスエントリ（blob と実行モード。無ければ
/// 「無かった」こと）を記録する（`RestoreIndexEntry`）。取り消すと hunk が再びステージ
/// された状態に戻る（＝再ステージ）。
pub fn unstage_hunk(repo: &Repository, file_path: &str, hunk_header: &str) -> Result<()> {
    let file_path = file_path.trim();
    let hunk_header = hunk_header.trim();
    if file_path.is_empty() {
        return Err(CoreError::InvalidInput(
            "アンステージするファイルを指定してください。".to_string(),
        ));
    }
    if hunk_header.is_empty() {
        return Err(CoreError::InvalidInput(
            "アンステージする変更の塊（hunk）を指定してください。".to_string(),
        ));
    }
    ensure_repo_relative_path(file_path)?;
    if is_submodule_path(repo, file_path) {
        return Err(CoreError::Blocked(submodule_blocked_message(file_path)));
    }

    let index = repo.index()?;
    let head_tree = match repo.head() {
        Ok(h) => Some(h.peel_to_tree()?),
        Err(_) => None,
    };

    // ステージ済み差分（HEAD → index）の中で、対象 hunk が何番目（0始まり）かを求める。
    // 対象パスだけに絞っているので、この差分には多くとも1ファイル分の hunk しか出ない。
    let mut diff_opts = git2::DiffOptions::new();
    diff_opts.pathspec(file_path).context_lines(3);
    let diff = repo.diff_tree_to_index(head_tree.as_ref(), Some(&index), Some(&mut diff_opts))?;

    let position = Cell::new(None::<usize>);
    let counter = Cell::new(0usize);
    diff.foreach(
        &mut |_delta, _progress| true,
        None,
        Some(&mut |_delta, hunk| {
            let idx = counter.get();
            counter.set(idx + 1);
            if position.get().is_none() && normalize_hunk_header(hunk.header()) == hunk_header {
                position.set(Some(idx));
            }
            true
        }),
        None,
    )?;
    let target_position = position.get().ok_or_else(|| {
        CoreError::InvalidInput(format!(
            "指定した変更の塊（hunk）が見つかりませんでした: {hunk_header}"
        ))
    })?;

    // 取り消し用に、操作前のインデックスエントリ（blob と実行モード）を記録しておく。
    // 存在しない（＝新規ファイルがこの hunk しか持たず、この後インデックスから消える）
    // 場合は None のまま記録し、undo 側でそれに合わせて「無かった」状態へ戻す。
    let before = index
        .get_path(Path::new(file_path), 0)
        .map(|e| (e.id.to_string(), e.mode));

    // 同じ内容を反転させた差分（old側=index、new側=HEAD）を作り、位置が一致する hunk だけを
    // index に適用する。index は現在この反転差分の old 側と一致しているので、適用すると
    // その hunk の範囲だけが HEAD の内容に戻る（＝アンステージ）。作業ツリーには触れない。
    let mut rev_opts = git2::DiffOptions::new();
    rev_opts.pathspec(file_path).context_lines(3).reverse(true);
    let reversed_diff =
        repo.diff_tree_to_index(head_tree.as_ref(), Some(&index), Some(&mut rev_opts))?;

    let path_for_delta = file_path.to_string();
    let mut apply_opts = git2::ApplyOptions::new();
    // 対象パス以外は触らない。
    apply_opts.delta_callback(move |delta| {
        delta
            .and_then(|d| d.new_file().path())
            .map(|p| p.to_string_lossy() == path_for_delta)
            .unwrap_or(false)
    });
    // 反転差分の中で、先ほど数えた位置と同じ hunk だけを選択適用する。
    let apply_counter = Cell::new(0usize);
    apply_opts.hunk_callback(move |_hunk| {
        let idx = apply_counter.get();
        apply_counter.set(idx + 1);
        idx == target_position
    });

    repo.apply(
        &reversed_diff,
        git2::ApplyLocation::Index,
        Some(&mut apply_opts),
    )
    .map_err(|e| {
        CoreError::Git(format!(
            "変更の塊（hunk）のアンステージに失敗しました: {}",
            describe_git2_error(&e)
        ))
    })?;

    record_undo(
        repo,
        UndoEntry {
            op: OperationKind::Unstage,
            description: format!("「{file_path}」の一部（hunk）のアンステージを取り消す"),
            action: UndoAction::RestoreIndexEntry {
                path: file_path.to_string(),
                blob: before.as_ref().map(|(id, _)| id.clone()),
                mode: before.map(|(_, m)| m).unwrap_or(0),
            },
        },
    );
    Ok(())
}

/// ステージされた変更をコミットする。直後に Undo で取り消せる。
///
/// マージ中（コンフリクト解消後）にコミットすると、取り込み元（MERGE_HEAD）を第2親に
/// 加えたマージコミットを作り、マージ中の状態を片付けてマージを完了させる。
/// コンフリクトが未解消のファイルが残っている間は [`CoreError::Blocked`] で中断する。
pub fn commit(repo: &Repository, message: &str) -> Result<CommitInfo> {
    if message.trim().is_empty() {
        return Err(CoreError::InvalidInput(
            "コミットメッセージを入力してください。".to_string(),
        ));
    }

    let sig = repo.signature().map_err(|_| {
        CoreError::InvalidInput(
            "コミットには名前とメールの設定が必要です（git config user.name / user.email）。"
                .to_string(),
        )
    })?;

    let mut index = repo.index()?;
    // コンフリクトが未解消のまま進むと libgit2 の生エラーになるため、先に平易な日本語で案内する。
    if index.has_conflicts() {
        return Err(CoreError::Blocked(
            "コンフリクト（競合）が解消されていないファイルがあります。すべてのファイルを直して「解消済みとしてマーク」してから、もう一度コミットしてください。"
                .to_string(),
        ));
    }
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    let prev = repo.head().ok().and_then(|h| h.target());
    let branch = current_branch(repo).unwrap_or_else(|| "main".to_string());

    // マージ中（コンフリクト解消後）のコミットなら、取り込み元（MERGE_HEAD）を第2親に
    // 加えて正しいマージコミットを作る。これが無いと、コンフリクト解消後のコミットが
    // 取り込み元との親子関係を持たない普通のコミットになり、マージが履歴に残らないうえ
    // MERGE_HEAD が残ってリポジトリが「マージ中」のまま取り残される。
    // mergehead_foreach は &mut Repository を要するため、同じパスで開き直す。
    let mut merge_parents: Vec<git2::Oid> = Vec::new();
    if repo.state() == git2::RepositoryState::Merge {
        let mut r = Repository::open(repo.path())?;
        r.mergehead_foreach(|oid| {
            merge_parents.push(*oid);
            true
        })?;
    }

    // コミットする変更があるか確認する。マージの締めくくりのコミットは、片方の内容を
    // 全面採用してツリーが変わらなくても意味がある（親子関係を記録する）ので確認しない。
    if merge_parents.is_empty() {
        match prev {
            Some(p) => {
                let parent_tree = repo.find_commit(p)?.tree()?.id();
                if parent_tree == tree_id {
                    return Err(CoreError::InvalidInput(
                        "コミットする変更がありません。先に変更をステージしてください。"
                            .to_string(),
                    ));
                }
            }
            None => {
                if index.is_empty() {
                    return Err(CoreError::InvalidInput(
                        "コミットする変更がありません。先に変更をステージしてください。"
                            .to_string(),
                    ));
                }
            }
        }
    }

    let mut parents: Vec<Commit> = match prev {
        Some(p) => vec![repo.find_commit(p)?],
        None => vec![],
    };
    for oid in &merge_parents {
        if Some(*oid) != prev {
            parents.push(repo.find_commit(*oid)?);
        }
    }
    let parent_refs: Vec<&Commit> = parents.iter().collect();

    let oid = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)?;

    // マージ中の状態（MERGE_HEAD 等）を片付けて、マージを完了させる。
    if !merge_parents.is_empty() {
        let _ = repo.cleanup_state();
    }

    let action = match prev {
        Some(p) => UndoAction::SoftResetTo {
            previous: p.to_string(),
        },
        None => UndoAction::UncommitInitial { branch },
    };
    record_undo(
        repo,
        UndoEntry {
            op: OperationKind::Commit,
            description: format!("コミット「{}」を取り消す", first_line(message)),
            action,
        },
    );

    let commit = repo.find_commit(oid)?;
    Ok(commit_info(&commit))
}

/// `git2::Commit` を serde 可能な [`CommitInfo`] に変換する。
fn commit_info(commit: &Commit) -> CommitInfo {
    let id = commit.id();
    let author = commit.author();
    CommitInfo {
        id: id.to_string(),
        short_id: id.to_string().chars().take(7).collect(),
        summary: commit.summary().ok().flatten().unwrap_or("").to_string(),
        author_name: author.name().unwrap_or("").to_string(),
        author_email: author.email().unwrap_or("").to_string(),
        time: commit.time().seconds(),
        parent_ids: commit.parent_ids().map(|p| p.to_string()).collect(),
    }
}

/// 直前のコミット（HEAD）を書き換える（amend）。
///
/// 現在のインデックスからツリーを作るので、ステージ済みの変更があれば取り込まれる。
/// `new_message` が空ならもとのメッセージを引き継ぐ（＝入れ忘れたファイルの追加だけ）。
/// author はもとのまま、committer を現在の identity に更新する（git の amend と同じ）。
/// 取り消し用に、修正前のコミットへ戻す soft reset を記録する。
pub fn amend_commit(repo: &Repository, new_message: &str) -> Result<CommitInfo> {
    let head_commit = repo.head().and_then(|h| h.peel_to_commit()).map_err(|_| {
        CoreError::Blocked(
            "まだコミットが無いため、修正（amend）できません。先に最初のコミットをしてください。"
                .to_string(),
        )
    })?;
    let original = head_commit.id();

    let sig = repo.signature().map_err(|_| {
        CoreError::InvalidInput(
            "コミットの修正には名前とメールの設定が必要です（git config user.name / user.email）。"
                .to_string(),
        )
    })?;

    // 現在のインデックスからツリーを作る。ステージ済みの変更があれば取り込まれる。
    let mut index = repo.index()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    // メッセージが空ならもとのメッセージを引き継ぐ。
    let message = if new_message.trim().is_empty() {
        head_commit.message().unwrap_or("").to_string()
    } else {
        new_message.to_string()
    };
    if message.trim().is_empty() {
        return Err(CoreError::InvalidInput(
            "コミットメッセージを入力してください。".to_string(),
        ));
    }

    let new_oid = head_commit.amend(
        Some("HEAD"),
        None,
        Some(&sig),
        None,
        Some(&message),
        Some(&tree),
    )?;

    record_undo(
        repo,
        UndoEntry {
            op: OperationKind::AmendCommit,
            description: "直前のコミットの修正（amend）を取り消す".to_string(),
            action: UndoAction::SoftResetTo {
                previous: original.to_string(),
            },
        },
    );

    let commit = repo.find_commit(new_oid)?;
    Ok(commit_info(&commit))
}

/// HEAD から連続する複数のコミットを1つにまとめる（squash / リベースの一種）。
///
/// `commit_oids` は **HEAD から連続する範囲**を **新しい順**（先頭が HEAD、末尾が最古）で渡す。
/// 例: 履歴が `c3(HEAD) → c2 → c1` のとき `["c3", "c2"]` を渡すと c3 と c2 が1つにまとまり、
/// 履歴は `(まとめたコミット) → c1` になる。離れたコミットの並び替えはこの関数では扱わない。
///
/// 実装方針: 範囲の最古コミット（末尾）の「親」を新しいベースとし、範囲の最新コミット（先頭＝HEAD）の
/// ツリーをそのまま使って単一のコミットを作り、現在のブランチをそれに向ける。ツリーをそのまま使うため
/// 通常コンフリクトは起きない。`message` が新しいコミットのメッセージになる。
///
/// 取り消し用に、元の HEAD への hard reset を記録する。
pub fn squash_commits(repo: &Repository, commit_oids: &[&str], message: &str) -> Result<()> {
    if commit_oids.len() < 2 {
        return Err(CoreError::InvalidInput(
            "まとめる（squash）には2つ以上のコミットを選んでください。".to_string(),
        ));
    }
    if message.trim().is_empty() {
        return Err(CoreError::InvalidInput(
            "まとめた後のコミットメッセージを入力してください。".to_string(),
        ));
    }

    let sig = repo.signature().map_err(|_| {
        CoreError::InvalidInput(
            "履歴の整理には名前とメールの設定が必要です（git config user.name / user.email）。"
                .to_string(),
        )
    })?;

    // 渡された各 oid を解析する。
    let mut oids = Vec::with_capacity(commit_oids.len());
    for s in commit_oids {
        let oid = git2::Oid::from_str(s.trim())
            .map_err(|_| CoreError::InvalidInput(format!("コミットを特定できません: {s}")))?;
        oids.push(oid);
    }

    // 範囲が HEAD から連続していることを検証する。
    // commit_oids は新しい順なので、HEAD から親をたどった列と一致しなければならない。
    let head_commit = repo.head().and_then(|h| h.peel_to_commit()).map_err(|_| {
        CoreError::Blocked(
            "まだコミットが無いため、履歴を整理できません。先にコミットをしてください。"
                .to_string(),
        )
    })?;
    let original_head = head_commit.id();

    let mut walker = head_commit.clone();
    for (i, expected) in oids.iter().enumerate() {
        if walker.id() != *expected {
            return Err(CoreError::Blocked(
                "選んだコミットが HEAD から連続していません。まとめられるのは、最新のコミットから続いた範囲だけです。"
                    .to_string(),
            ));
        }
        if i + 1 < oids.len() {
            // 次の親へ進む。マージコミット（親が複数）は扱わない。
            if walker.parent_count() != 1 {
                return Err(CoreError::Blocked(
                    "マージコミットを含む範囲はまとめられません。".to_string(),
                ));
            }
            walker = walker.parent(0)?;
        }
    }

    // 範囲の最古コミット（oids の末尾 = いま walker が指すコミット）の親を新しいベースにする。
    let oldest = walker;
    let new_parents: Vec<Commit> = if oldest.parent_count() == 0 {
        // 範囲が最初のコミットまで含む場合、ベースは無し（root コミットを作り直す）。
        Vec::new()
    } else if oldest.parent_count() == 1 {
        vec![oldest.parent(0)?]
    } else {
        return Err(CoreError::Blocked(
            "マージコミットを含む範囲はまとめられません。".to_string(),
        ));
    };
    let parent_refs: Vec<&Commit> = new_parents.iter().collect();

    // まとめツリー = 範囲の最新コミット（HEAD）のツリー。中身はそのまま保たれる。
    let tree = head_commit.tree()?;

    // 単一コミットを作る。参照は update_ref=None で更新せずに作り（libgit2 は HEAD 直更新時に
    // 「新コミットの第1親が現在の tip であること」を要求するため）、その後で現在ブランチの
    // 参照を手動で新コミットへ向ける。
    let new_oid = repo.commit(None, &sig, &sig, message, &tree, &parent_refs)?;

    // HEAD が指すブランチ参照（例: refs/heads/main）を新コミットへ進める。
    // detached HEAD（ブランチを指していない）の場合は HEAD 自体を直接向ける。
    match repo.head().ok().and_then(|h| {
        if h.is_branch() {
            h.name().ok().map(|s| s.to_string())
        } else {
            None
        }
    }) {
        Some(refname) => {
            repo.reference(&refname, new_oid, true, "noobgit: squash commits")?;
        }
        None => {
            repo.set_head_detached(new_oid)?;
        }
    }

    record_undo(
        repo,
        UndoEntry {
            op: OperationKind::Rebase,
            description: format!(
                "コミットの統合（squash）を取り消す（{} 個を1つにまとめる前へ）",
                oids.len()
            ),
            action: UndoAction::HardResetTo {
                previous: original_head.to_string(),
            },
        },
    );

    Ok(())
}

/// 最新のコミット（HEAD）のメッセージだけを書き換える（reword / リベースの一種）。
///
/// ツリーは現在の HEAD のツリーをそのまま使い、内容は一切変えない。author は据え置き、committer を
/// 現在の identity に更新する（[`amend_commit`] のメッセージ特化版）。`message` は非空であること。
///
/// 取り消し用に、書き換え前のコミットへ戻す soft reset を記録する。
pub fn reword_commit(repo: &Repository, message: &str) -> Result<CommitInfo> {
    if message.trim().is_empty() {
        return Err(CoreError::InvalidInput(
            "コミットメッセージを入力してください。".to_string(),
        ));
    }

    let head_commit = repo.head().and_then(|h| h.peel_to_commit()).map_err(|_| {
        CoreError::Blocked(
            "まだコミットが無いため、メッセージを書き換えられません。先にコミットをしてください。"
                .to_string(),
        )
    })?;
    let original = head_commit.id();

    let sig = repo.signature().map_err(|_| {
        CoreError::InvalidInput(
            "コミットの書き換えには名前とメールの設定が必要です（git config user.name / user.email）。"
                .to_string(),
        )
    })?;

    // ツリーは HEAD のものをそのまま使う（内容は変えない）。author は据え置き、committer を更新。
    let tree = head_commit.tree()?;
    let new_oid = head_commit.amend(
        Some("HEAD"),
        None,
        Some(&sig),
        None,
        Some(message),
        Some(&tree),
    )?;

    record_undo(
        repo,
        UndoEntry {
            op: OperationKind::Rebase,
            description: "コミットメッセージの書き換え（reword）を取り消す".to_string(),
            action: UndoAction::SoftResetTo {
                previous: original.to_string(),
            },
        },
    );

    let commit = repo.find_commit(new_oid)?;
    Ok(commit_info(&commit))
}

/// 指定パスの、まだコミットしていない変更を捨てる（破棄）。
///
/// - HEAD にあるファイル: 最後にコミットした状態へ強制的に戻す（ステージ済み・未ステージの
///   変更をいずれも捨てる）。
/// - HEAD に無いファイル（新規）: インデックスから外し、作業ツリーから削除する。
///
/// 捨てた内容は元に戻せない破壊的操作。安全な代替は stash（退避）。undo は記録しない。
///
/// サブモジュールのパスが指定された場合は、中の別リポジトリを壊しかねないため
/// 実行せず安全に拒否する。
pub fn discard_path(repo: &Repository, path: &str) -> Result<()> {
    // 作業ツリー外を指すパスは扱わない（安全のため）。
    ensure_repo_relative_path(path)?;
    if is_submodule_path(repo, path) {
        return Err(CoreError::Blocked(submodule_blocked_message(path)));
    }

    let workdir = repo
        .workdir()
        .ok_or_else(|| CoreError::Git("作業ツリーがありません。".to_string()))?;

    let rel = Path::new(path);

    // HEAD のツリーに当該パスがあるか（＝コミット済みのファイルか）。
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    let in_head = head_tree
        .as_ref()
        .map(|t| t.get_path(rel).is_ok())
        .unwrap_or(false);

    if in_head {
        // コミット済み: HEAD の内容へ強制的に戻す（インデックスも合わせる）。
        let tree = head_tree.expect("in_head が真ならツリーは存在する");
        let mut co = CheckoutBuilder::new();
        co.force().update_index(true).path(path);
        repo.checkout_tree(tree.as_object(), Some(&mut co))?;
    } else {
        // 新規ファイル: ステージされていれば外し、作業ツリーから削除する。
        let mut index = repo.index()?;
        if index.get_path(rel, 0).is_some() {
            index.remove_path(rel)?;
            index.write()?;
        }
        let full = workdir.join(rel);
        if full.exists() {
            std::fs::remove_file(&full).map_err(|e| {
                CoreError::Git(format!(
                    "ファイルを削除できませんでした: {}",
                    describe_io_error(&e)
                ))
            })?;
        }
    }
    Ok(())
}

/// `.gitignore` の末尾にパターンを 1 行追記する（ファイルが無ければ新規作成）。
///
/// `.gitignore` は Git に無視させたいファイルを指定するテキストファイル。ここへ追記する
/// だけで Git のインデックス・履歴は変えないため、undo は記録しない（変更は通常の
/// ファイル編集として status に現れ、ユーザー自身が確認・コミットできる）。
///
/// 安全とおせっかい防止のための約束:
/// - `pattern` が glob 構文として不正なら（[`validate_gitignore_pattern`] 参照）
///   [`CoreError::InvalidInput`]。
/// - すでに同じパターンが（コメント・空行を除く行として）書かれていれば何もしない
///   （冪等・重複防止）。
/// - 既存内容の末尾に改行が無ければ補ってから追記し、行が混ざらないようにする。
pub fn add_to_gitignore(repo: &Repository, pattern: &str) -> Result<()> {
    let pattern = pattern.trim();
    let check = validate_gitignore_pattern(pattern);
    if !check.valid {
        return Err(CoreError::InvalidInput(
            check
                .error
                .unwrap_or_else(|| "無視するパターンが不正です。".to_string()),
        ));
    }

    let workdir = repo
        .workdir()
        .ok_or_else(|| CoreError::Git("作業ツリーがありません。".to_string()))?;
    let path = workdir.join(".gitignore");

    // 既存の内容を読む（無ければ空から始める）。
    let existing = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(CoreError::Git(format!(
                ".gitignore を読み込めませんでした: {}",
                describe_io_error(&e)
            )))
        }
    };

    // すでに同じパターンが行として存在すれば重複追記を避ける。
    if gitignore_has_pattern(&existing, pattern) {
        return Ok(());
    }

    // 末尾に改行が無ければ補ってから追記し、最後も改行で終える。
    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(pattern);
    next.push('\n');

    std::fs::write(&path, next).map_err(|e| {
        CoreError::Git(format!(
            ".gitignore に書き込めませんでした: {}",
            describe_io_error(&e)
        ))
    })?;
    Ok(())
}

/// `existing`（`.gitignore` の全内容）の中に、`pattern` と同じ行がすでにあるかを判定する。
///
/// コメント行（`#` で始まる）と空行は比較対象から除く。各行・`pattern` とも
/// 前後の空白を取り除いてから比較する（正規化）。
fn gitignore_has_pattern(existing: &str, pattern: &str) -> bool {
    let pattern = pattern.trim();
    existing.lines().any(|line| {
        let line = line.trim();
        !line.is_empty() && !line.starts_with('#') && line == pattern
    })
}

/// `.gitignore` の 1 パターンが glob 構文として意味をなすかを検証する（#173）。
///
/// `git2` やファイルシステムには依存しない純粋な関数で、フロントエンドの入力中
/// リアルタイムバリデーションにも使える。ここでの「不正」は Git 自体が
/// エラーにするわけではない書き方（Git は不正な行があっても他の行は無視しつつ
/// 動き続けてしまう）を、初心者に分かるよう先回りして拒否するもの。
///
/// チェックする内容:
/// - 空文字列（前後の空白のみ・否定 `!` の後が空、を含む）
/// - 改行を含む（1行に複数パターンを書こうとしている）
/// - `#` から始まる（コメント行になり、無視パターンとして機能しない）
/// - 末尾が単独の `\`（エスケープする文字が続いていない）
/// - `[` が閉じられていない（文字クラスの表記が壊れている）
/// - `**` がディレクトリ区切り以外の位置にある（例: `foo**bar` は無効。
///   `**/foo` `foo/**` `a/**/b` は有効）
pub fn validate_gitignore_pattern(pattern: &str) -> GitignorePatternCheck {
    fn invalid(msg: impl Into<String>) -> GitignorePatternCheck {
        GitignorePatternCheck {
            valid: false,
            error: Some(msg.into()),
            duplicate: false,
        }
    }

    if pattern.contains('\n') || pattern.contains('\r') {
        return invalid("パターンに改行を含めることはできません（1行に1パターンです）。");
    }
    if pattern.starts_with('#') {
        return invalid("「#」で始まる行はコメントとして扱われ、無視パターンとして機能しません。");
    }

    // 否定パターン（"!foo" = foo を無視対象から除外する）の "!" の後が空でないか。
    let negated = pattern.starts_with('!');
    let body = if negated { &pattern[1..] } else { pattern };
    // 末尾の（エスケープされていない）空白は Git 側で無視されるので、実質空なら拒否する。
    if body.trim_end_matches(' ').is_empty() {
        return invalid(if negated {
            "「!」の後にパターンを入力してください。"
        } else {
            "無視するパターンを入力してください。"
        });
    }

    // 末尾が単独の "\"（エスケープ対象の文字が続いていない）。
    let trailing_backslashes = pattern.chars().rev().take_while(|&c| c == '\\').count();
    if trailing_backslashes % 2 == 1 {
        return invalid(
            "末尾の「\\」でエスケープする文字が続いていません。「\\」を削除するか、続けて文字を入力してください。",
        );
    }

    // 閉じていない "[" （文字クラスの表記）。
    let mut in_bracket = false;
    let mut escaped = false;
    for c in pattern.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '[' if !in_bracket => in_bracket = true,
            ']' if in_bracket => in_bracket = false,
            _ => {}
        }
    }
    if in_bracket {
        return invalid(
            "「[」が閉じられていません。文字クラスを使うときは「]」で閉じてください（例: 「*.[oa]」）。",
        );
    }

    // "**" はディレクトリ区切り（"/"）で囲まれた位置でだけ特別な意味を持つ。
    // それ以外（例: "foo**", "**bar", "foo**bar"）は Git の仕様上「無効」とされる。
    for segment in pattern.split('/') {
        if segment.contains("**") && segment != "**" {
            return invalid(
                "「**」が特別な意味（何階層でも一致）を持つのは「**/foo」「foo/**」「a/**/b」の形だけです。「foo**」のように他の文字と続けると普通の「*」と同じ扱いになり、意図どおりに無視されない可能性があります。",
            );
        }
    }

    GitignorePatternCheck {
        valid: true,
        error: None,
        duplicate: false,
    }
}

/// `.gitignore` の 1 パターンを、構文チェックと重複チェックの両方込みで検証する（#173）。
///
/// [`validate_gitignore_pattern`] に加えて、既存の `.gitignore`（あれば）と同じ行が
/// すでにあるかを見る。構文が不正な場合は `duplicate` は常に false（構文チェックが
/// 先に失敗するので重複の判定に意味が無いため）。
pub fn check_gitignore_pattern(repo: &Repository, pattern: &str) -> Result<GitignorePatternCheck> {
    let pattern = pattern.trim();
    let check = validate_gitignore_pattern(pattern);
    if !check.valid {
        return Ok(check);
    }

    let existing = read_gitignore(repo)?.unwrap_or_default();
    Ok(GitignorePatternCheck {
        duplicate: gitignore_has_pattern(&existing, pattern),
        ..check
    })
}

/// パスの一部を `.gitignore` のパターン中で「文字どおり」に一致させるためにエスケープする。
///
/// ファイル名に含まれる `*` `?` `[` `]` `\` は glob の特殊文字として解釈されてしまうため
/// `\` を前置する。行頭の `!`（否定）/ `#`（コメント）と末尾の空白（無視される）も
/// エスケープして、そのファイル名そのものに一致するパターンにする。
fn escape_gitignore_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.chars().enumerate() {
        match c {
            '*' | '?' | '[' | ']' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '!' | '#' if i == 0 => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    // 末尾の空白は Git に無視されるので、最後の 1 つをエスケープして残す。
    if out.ends_with(' ') {
        out.pop();
        out.push_str("\\ ");
    }
    out
}

/// ファイルパスから `.gitignore` パターンの候補を生成する（#173）。
///
/// 3 種類の候補を、可能な範囲で生成する:
/// 1. このファイルのみ（ルートからの絶対パスで固定するので、同名の別ファイルには
///    影響しない）。
/// 2. 同じ拡張子のファイルをすべて無視（拡張子が無いファイル・ドットファイル
///    （`.env` など）では意味が無いので省く）。
/// 3. このファイルが入っているディレクトリ全体を無視（リポジトリ直下のファイルには
///    親ディレクトリが無いので省く）。
///
/// `path` はリポジトリルートからの相対パス（先頭の `/` は取り除いてから使う）。
/// 空文字列を渡した場合は空の一覧を返す。
pub fn suggest_gitignore_patterns(path: &str) -> Vec<GitignoreSuggestion> {
    let mut out = Vec::new();
    let normalized = path.trim().trim_start_matches('/');
    if normalized.is_empty() {
        return out;
    }
    let p = Path::new(normalized);
    // パスの各要素を文字どおりに一致させる（区切りの `/` はそのまま）。
    let escape_path = |s: &str| {
        s.split('/')
            .map(escape_gitignore_literal)
            .collect::<Vec<_>>()
            .join("/")
    };

    // 1. このファイルのみ。
    out.push(GitignoreSuggestion {
        pattern: format!("/{}", escape_path(normalized)),
        label: "このファイルだけを無視".to_string(),
        description: format!(
            "「/{normalized}」を .gitignore に追加します。同じ名前の別の場所にあるファイルには影響しません。"
        ),
    });

    // 2. 同じ拡張子のファイルをすべて無視（拡張子が無い・ドットファイルは省く）。
    if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
        let pattern = format!("*.{}", escape_gitignore_literal(ext));
        out.push(GitignoreSuggestion {
            label: "同じ拡張子のファイルをすべて無視".to_string(),
            description: format!(
                "「{pattern}」を .gitignore に追加します。拡張子が .{ext} のファイルは、リポジトリ内のどこにあっても無視されます。"
            ),
            pattern,
        });
    }

    // 3. このディレクトリ全体を無視（ルート直下のファイルは親ディレクトリが無いので省く）。
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() {
            let dir = parent.to_string_lossy().replace('\\', "/");
            let pattern = format!("{}/", escape_path(&dir));
            out.push(GitignoreSuggestion {
                label: "このディレクトリ全体を無視".to_string(),
                description: format!(
                    "「{pattern}」を .gitignore に追加します。「{dir}」ディレクトリの中身がすべて無視されます。"
                ),
                pattern,
            });
        }
    }

    out
}

/// 現在の変更を一時的にしまう（stash 退避）。未追跡ファイルも含めて退避し、作業ツリーを
/// きれいな状態に戻す。`message` が空なら libgit2 が既定のメッセージを付ける。
///
/// 退避は変更を消さない安全操作。直後に取り出せるよう、PopStash の undo を記録する。
pub fn stash_save(repo: &mut Repository, message: &str) -> Result<()> {
    let stash_oid = stash_save_unrecorded(repo, message)?
        .ok_or_else(|| CoreError::Blocked("退避できる変更がありません。".to_string()))?;

    record_undo(
        repo,
        UndoEntry {
            op: OperationKind::StashSave,
            description: "退避（stash）を取り消す（しまった変更を作業ツリーに戻す）".to_string(),
            action: UndoAction::PopStash {
                id: stash_oid.to_string(),
            },
        },
    );
    Ok(())
}

/// [`stash_save`] の本体（undo を記録しない版）。退避する変更が無ければ `Ok(None)`。
///
/// [`switch_branch_with_stash`] のように「退避 → 別の操作 → 取り出し」を 1 つの操作として
/// 合成する呼び出し側は、途中の退避に対する PopStash undo を残したくない（取り出し済みの
/// 退避を指す古い undo が残ってしまうため）ので、記録の有無を呼び出し側が選べるように
/// 分けている。
fn stash_save_unrecorded(repo: &mut Repository, message: &str) -> Result<Option<git2::Oid>> {
    let sig = repo.signature().map_err(|_| {
        CoreError::InvalidInput(
            "退避（stash）には名前とメールの設定が必要です（git config user.name / user.email）。"
                .to_string(),
        )
    })?;

    let msg = message.trim();
    let msg = if msg.is_empty() { None } else { Some(msg) };
    let flags = StashFlags::INCLUDE_UNTRACKED;

    match repo.stash_save2(&sig, msg, Some(flags)) {
        Ok(oid) => Ok(Some(oid)),
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(e) => Err(CoreError::Git(format!(
            "退避（stash）に失敗しました: {}",
            describe_git2_error(&e)
        ))),
    }
}

/// 退避を作業ツリーに取り出す（一覧には残す）。
///
/// 作業ツリーに、退避内容と同じ箇所への未コミットの変更があり、それを上書きして
/// しまう場合は、何も変えずに [`CoreError::Blocked`] で中断する（従来どおり）。
/// 一方、作業ツリーはクリーンだが退避内容と HEAD 側の変更が中身で競合する場合、
/// libgit2 はエラーにせず、コンフリクトの目印（`<<<<<<<` 等）を作業ツリーへ書き込み、
/// index にコンフリクトエントリを残して成功する。この場合は
/// [`StashRestoreOutcome::conflicted`] を true にして返し、呼び出し側（フロント）が
/// 既存のコンフリクト解消ウィザードへ自然につなげられるようにする。
/// 退避は成功・コンフリクトいずれの場合も一覧に残る（apply はそういう操作）。
pub fn stash_apply(repo: &mut Repository, index: usize) -> Result<StashRestoreOutcome> {
    repo.stash_apply(index, None)
        .map_err(map_stash_restore_err)?;
    Ok(StashRestoreOutcome {
        conflicted: repo.index()?.has_conflicts(),
    })
}

/// 退避を作業ツリーに取り出し、コンフリクトが無ければ一覧から取り除く（pop）。
///
/// [`stash_apply`] と同じ理由で、中身が競合する場合はエラーにせずコンフリクトの
/// 目印を書き込んで成功を返す。ただし libgit2 の生の `stash_pop`（`git_stash_pop`）は
/// 「apply がエラーにならなければ」退避を drop してしまうため、コンフリクトが
/// 残ったまま退避が一覧から消えてしまう（`git stash pop` の「コンフリクト時は退避を
/// 残す」という挙動と食い違う）。そのため、ここでは `apply` → コンフリクト確認 →
/// 問題なければ `drop` という手順に分解し、コンフリクト発生時は退避を一覧に残す
/// （解消後、ユーザーが改めて「退避を削除する」を選べるようにするため）。
pub fn stash_pop(repo: &mut Repository, index: usize) -> Result<StashRestoreOutcome> {
    repo.stash_apply(index, None)
        .map_err(map_stash_restore_err)?;
    let conflicted = repo.index()?.has_conflicts();
    if !conflicted {
        repo.stash_drop(index)?;
    }
    Ok(StashRestoreOutcome { conflicted })
}

/// 退避を一覧から取り除く（内容は破棄する）。
///
/// おもに `stash_pop` がコンフリクトで退避を一覧に残したあと、コンフリクトを
/// 手で解消し終えたユーザーが「もう要らないので消す」ために使う（もちろん、
/// 単に不要になった退避を消す用途にも使える）。
///
/// 破棄した退避の中身（元のコミット・作業ツリー・未追跡ファイルへのスナップショット）
/// を復元する簡単な方法が libgit2 には無いため、**undo は記録しない**（他の破壊的操作と
/// 違い、直後の「取り消し」ボタンでは戻せない）。呼び出し側は必ず確認ダイアログ
/// （`guarded()`）を経由すること。
///
/// 退避は番号（index）ではなく ID（退避コミットの oid。[`StashInfo::id`]）で指定する。
/// 番号は新しい退避を作るたびにずれるため、画面を開いたあとに別の退避が作られていると
/// 「別の退避を消してしまう」事故になる。ID が一覧に見つからなければ何もせず
/// [`CoreError::InvalidInput`] を返す。
pub fn stash_drop(repo: &mut Repository, stash_id: &str) -> Result<()> {
    let target = git2::Oid::from_str(stash_id.trim())
        .map_err(|_| CoreError::InvalidInput("退避の指定が正しくありません。".to_string()))?;
    let mut found = None;
    repo.stash_foreach(|index, _message, id| {
        if *id == target {
            found = Some(index);
            false
        } else {
            true
        }
    })?;
    let index = found.ok_or_else(|| {
        CoreError::InvalidInput(
            "指定した退避が見つかりませんでした（すでに削除されている可能性があります）。"
                .to_string(),
        )
    })?;
    repo.stash_drop(index).map_err(|e| {
        CoreError::Git(format!(
            "退避の削除に失敗しました: {}",
            describe_git2_error(&e)
        ))
    })
}

/// 退避の一覧を返す（0 がいちばん新しい退避）。各退避の変更ファイル数も付ける。
pub fn stash_list(repo: &mut Repository) -> Result<Vec<StashInfo>> {
    // stash_foreach の最中は repo を借用するため、まず (index, message, id) を集める。
    let mut raw = Vec::new();
    repo.stash_foreach(|index, message, id| {
        raw.push((index, message.to_string(), *id));
        true
    })?;

    // 退避ごとに、退避コミットと base（第1親）のツリーを比較して変更ファイル数を数える。
    let mut out = Vec::with_capacity(raw.len());
    for (index, message, id) in raw {
        let file_count = stash_changed_files(repo, id)?.len();
        out.push(StashInfo {
            index,
            message,
            id: id.to_string(),
            file_count,
        });
    }
    Ok(out)
}

/// 指定 index の退避に含まれる変更ファイルの一覧（パスと変更種別）を返す。
///
/// 退避コミットのツリーと base（第1親）のツリーを比較して求めるだけで、退避を作業ツリーへ
/// 適用しない非破壊・安全な操作。
pub fn stash_diff(repo: &mut Repository, stash_index: usize) -> Result<Vec<FileChange>> {
    // index から退避コミットの OID を引く。
    let mut found: Option<git2::Oid> = None;
    repo.stash_foreach(|index, _message, id| {
        if index == stash_index {
            found = Some(*id);
            false // 見つかったので走査を止める。
        } else {
            true
        }
    })?;
    let oid = found.ok_or_else(|| {
        CoreError::InvalidInput("指定した退避が見つかりませんでした。".to_string())
    })?;

    stash_changed_files(repo, oid)
}

/// 退避コミット（`oid`）の変更ファイル一覧を返す。
///
/// stash コミットの第1親が base（退避時点の HEAD）。退避コミットのツリーと base のツリーを
/// 比較して、追跡ファイルの変更を求める。未追跡ファイルを含めて退避した場合は、それらは
/// 第3親（untracked コミット）のツリーに収まっているので、空ツリーとの比較で「追加」として
/// 拾う。いずれもツリー比較だけで求め、退避を作業ツリーへ適用しない非破壊な操作。
fn stash_changed_files(repo: &Repository, oid: git2::Oid) -> Result<Vec<FileChange>> {
    let stash_commit = repo.find_commit(oid)?;
    let stash_tree = stash_commit.tree()?;
    // 第1親が base（退避時点の HEAD）。親が無い（未誕生 base）場合は空ツリーと比較する。
    let base_tree = match stash_commit.parent(0) {
        Ok(parent) => Some(parent.tree()?),
        Err(_) => None,
    };

    let mut out = Vec::new();

    // 追跡ファイルの変更（base ↔ 退避ツリー）。
    let diff = repo.diff_tree_to_tree(base_tree.as_ref(), Some(&stash_tree), None)?;
    for delta in diff.deltas() {
        out.push(delta_to_file_change(&delta));
    }

    // 未追跡ファイル: INCLUDE_UNTRACKED で退避すると第3親（index 2）に untracked コミットが
    // 付く。その内容（空ツリーとの差分＝すべて追加）を拾う。第3親が無ければ未追跡は無い。
    if let Ok(untracked_commit) = stash_commit.parent(2) {
        let untracked_tree = untracked_commit.tree()?;
        let diff = repo.diff_tree_to_tree(None, Some(&untracked_tree), None)?;
        for delta in diff.deltas() {
            out.push(delta_to_file_change(&delta));
        }
    }

    Ok(out)
}

/// diff の1デルタを [`FileChange`] に変換する（新パス優先、無ければ旧パス）。
///
/// stash の変更ファイル一覧（プレビュー専用・非破壊）で使うため、サブモジュール
/// 判定はここでは行わない（常に `is_submodule: false`）。
fn delta_to_file_change(delta: &git2::DiffDelta) -> FileChange {
    let path = delta
        .new_file()
        .path()
        .or_else(|| delta.old_file().path())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    FileChange {
        path,
        kind: delta_change_kind(delta.status()),
        is_submodule: false,
    }
}

/// `git2::Delta` を [`ChangeKind`] に変換する。
fn delta_change_kind(status: git2::Delta) -> ChangeKind {
    use git2::Delta;
    match status {
        Delta::Added | Delta::Untracked | Delta::Copied => ChangeKind::Added,
        Delta::Deleted => ChangeKind::Deleted,
        Delta::Renamed => ChangeKind::Renamed,
        Delta::Typechange => ChangeKind::TypeChange,
        _ => ChangeKind::Modified,
    }
}

/// stash の取り出し（apply / pop）のエラーを初学者向けの日本語に変換する。
fn map_stash_restore_err(e: git2::Error) -> CoreError {
    use git2::ErrorCode;
    match e.code() {
        ErrorCode::NotFound => {
            CoreError::InvalidInput("指定した退避が見つかりませんでした。".to_string())
        }
        ErrorCode::Conflict | ErrorCode::MergeConflict => CoreError::Blocked(
            "退避を取り出すとコンフリクト（競合）が起きるため、安全のため中断しました。先にいまの変更を整理してから取り出してください。"
                .to_string(),
        ),
        _ => CoreError::Git(format!("退避の取り出しに失敗しました: {}", describe_git2_error(&e))),
    }
}

/// HEAD を起点に新しいブランチを作る。
pub fn create_branch(repo: &Repository, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CoreError::InvalidInput(
            "ブランチ名を入力してください。".to_string(),
        ));
    }
    let head_commit = repo.head().and_then(|h| h.peel_to_commit()).map_err(|_| {
        CoreError::Blocked(
            "まだコミットが無いため、ブランチを作成できません。先に最初のコミットをしてください。"
                .to_string(),
        )
    })?;

    if repo.find_branch(name, BranchType::Local).is_ok() {
        return Err(CoreError::InvalidInput(format!(
            "ブランチ「{name}」はすでに存在します。別の名前を使ってください。"
        )));
    }

    repo.branch(name, &head_commit, false)?;
    record_undo(
        repo,
        UndoEntry {
            op: OperationKind::CreateBranch,
            description: format!("ブランチ「{name}」の作成を取り消す"),
            action: UndoAction::DeleteBranch {
                name: name.to_string(),
            },
        },
    );
    Ok(())
}

/// 既存ブランチへ切り替える。未コミット変更と衝突する場合は安全のため失敗する。
pub fn switch_branch(repo: &Repository, name: &str) -> Result<()> {
    let name = name.trim();
    repo.find_branch(name, BranchType::Local)
        .map_err(|_| CoreError::InvalidInput(format!("ブランチ「{name}」が見つかりません。")))?;

    let refname = format!("refs/heads/{name}");
    let obj = repo.revparse_single(&refname)?;

    // 既定（safe）チェックアウト: 未コミット変更を上書きせず、衝突時はエラーにする。
    let mut co = CheckoutBuilder::new();
    repo.checkout_tree(&obj, Some(&mut co)).map_err(|_| {
        CoreError::Blocked(
            "未コミットの変更があるため切り替えできません。先にコミットか退避(stash)をしてください。"
                .to_string(),
        )
    })?;
    repo.set_head(&refname)?;
    Ok(())
}

/// 未コミットの変更を退避（stash）してからブランチを切り替え、切り替え後に変更を戻す
/// （`git switch` の autostash 相当）。手順・失敗時のロールバック・undo の扱いは
/// [`switch_with_stash_impl`] を参照。
pub fn switch_branch_with_stash(
    repo: &mut Repository,
    name: &str,
) -> Result<SwitchWithStashOutcome> {
    switch_with_stash_impl(repo, name, switch_branch)
}

/// [`switch_branch_with_stash`] の本体。切り替え処理を差し替えられるようにしてあるのは、
/// 「切り替えが途中で失敗しても変更が失われない」ことをテストで確かめるため
/// （通常は [`switch_branch`] を渡す）。
///
/// 手順と失敗時の扱い:
///
/// 1. 切り替え先が存在するか先に確かめる（無ければ何も退避せず `InvalidInput`）。
/// 2. 未コミット変更（未追跡ファイル含む）を「〈元ブランチ〉から〈先ブランチ〉への切り替え時に
///    自動退避」という名前で退避する。退避する変更が無ければ、退避せず普通に切り替える
///    （`stashed: false`）。
/// 3. 切り替える。**失敗したら元のブランチのまま、退避した変更を作業ツリーへ戻し**
///    （ロールバック）、元のエラーをそのまま返す。戻す処理自体が失敗・コンフリクトした場合も
///    退避は一覧に残るので変更は失われない（その旨をエラーメッセージに加える）。
/// 4. 切り替え後、退避を取り出す（[`stash_pop`] と同じ）。
///    - 成功: 退避は一覧から取り除かれる（`conflicted: false`）。
///    - コンフリクト: 目印を書き込んで成功を返し（`conflicted: true`）、**退避は一覧に残す**。
///      フロントは既存のコンフリクト解消ウィザードと退避パネルへつなげられる。
///    - 取り出し自体の失敗: 切り替えは済んでいるが、退避は一覧に残るので、その旨を
///      エラーメッセージで伝える。
///
/// 退避は番号ではなく退避コミットの ID で探す（途中で別の退避が増えても取り違えない）。
///
/// undo: この操作は undo を記録しない。[`stash_save`] が記録する PopStash は、直後に
/// この関数自身が取り出してしまう退避を指す古い undo になるため記録しない（内部では
/// undo を記録しない版の退避を使う）。また [`switch_branch`] 自体も undo を記録しない
/// ので、既存操作と一貫している。元のブランチへは、もう一度ブランチ切り替えで戻れる。
fn switch_with_stash_impl<F>(
    repo: &mut Repository,
    name: &str,
    switch: F,
) -> Result<SwitchWithStashOutcome>
where
    F: FnOnce(&Repository, &str) -> Result<()>,
{
    let name = name.trim();
    repo.find_branch(name, BranchType::Local)
        .map_err(|_| CoreError::InvalidInput(format!("ブランチ「{name}」が見つかりません。")))?;

    let from = match repo.head() {
        Ok(h) if h.is_branch() => h.shorthand().unwrap_or("(不明)").to_string(),
        Ok(h) => h
            .target()
            .map(|o| format!("{:.7}", o.to_string()))
            .unwrap_or_else(|| "(不明)".to_string()),
        Err(_) => "(コミット前)".to_string(),
    };
    let message = format!("{from}から{name}への切り替え時に自動退避");

    let Some(stash_oid) = stash_save_unrecorded(repo, &message)? else {
        switch(repo, name)?;
        return Ok(SwitchWithStashOutcome {
            stashed: false,
            conflicted: false,
        });
    };

    if let Err(switch_err) = switch(repo, name) {
        // ロールバック: 切り替えが途中まで進んでいた場合に備え、作業ツリーを HEAD に揃えて
        // から（退避済みなので失われる未コミット変更は無い）、退避を元のブランチへ戻す。
        let _ = repo.checkout_head(Some(CheckoutBuilder::new().force()));
        let restored = match find_stash_index(repo, stash_oid) {
            Ok(Some(index)) => stash_pop(repo, index),
            Ok(None) => Err(CoreError::InvalidInput(
                "退避した変更が一覧に見つかりませんでした。".to_string(),
            )),
            Err(e) => Err(e),
        };
        return match restored {
            Ok(o) if !o.conflicted => Err(switch_err),
            _ => Err(CoreError::Git(format!(
                "{switch_err} 退避した変更を元に戻す処理も完了しませんでしたが、変更は退避（stash）一覧に残っているので失われていません。退避パネルから取り出してください。"
            ))),
        };
    }

    let index = find_stash_index(repo, stash_oid)?.ok_or_else(|| {
        CoreError::Git(format!(
            "ブランチ「{name}」へ切り替えましたが、退避した変更が一覧に見つかりませんでした。"
        ))
    })?;
    match stash_pop(repo, index) {
        Ok(o) => Ok(SwitchWithStashOutcome {
            stashed: true,
            conflicted: o.conflicted,
        }),
        Err(e) => Err(CoreError::Git(format!(
            "ブランチ「{name}」へは切り替えましたが、退避した変更を戻せませんでした（{e}）。変更は退避（stash）一覧に残っているので失われていません。退避パネルから取り出してください。"
        ))),
    }
}

/// 退避コミットの ID から、退避一覧での現在の番号を探す。
fn find_stash_index(repo: &mut Repository, id: git2::Oid) -> Result<Option<usize>> {
    let mut found = None;
    repo.stash_foreach(|index, _message, oid| {
        if *oid == id {
            found = Some(index);
            false
        } else {
            true
        }
    })?;
    Ok(found)
}

/// ブランチを削除する。直後に Undo で復元できる。
pub fn delete_branch(repo: &Repository, name: &str) -> Result<()> {
    let name = name.trim();
    let mut branch = repo
        .find_branch(name, BranchType::Local)
        .map_err(|_| CoreError::InvalidInput(format!("ブランチ「{name}」が見つかりません。")))?;

    if branch.is_head() {
        return Err(CoreError::Blocked(
            "今チェックアウト中のブランチは削除できません。先に別のブランチへ切り替えてください。"
                .to_string(),
        ));
    }

    let target = branch
        .get()
        .target()
        .ok_or_else(|| CoreError::Git("ブランチの参照先を取得できませんでした。".to_string()))?;

    branch.delete()?;
    record_undo(
        repo,
        UndoEntry {
            op: OperationKind::DeleteBranch,
            description: format!("ブランチ「{name}」の削除を取り消す"),
            action: UndoAction::RecreateBranch {
                name: name.to_string(),
                target: target.to_string(),
            },
        },
    );
    Ok(())
}

/// 保護ブランチ一覧をリポジトリローカルの git config `noobgit.protectedBranches`
/// に保存する（カンマ区切り文字列。例: `main,master,release`）。
///
/// グローバル設定（`~/.gitconfig`）には書き込まない — 必ず
/// `ConfigLevel::Local`（`.git/config`）を明示的に開くので、この設定はリポジトリ
/// ごとに独立する（読み込みは [`crate::repo::load_protected_branches`]）。
///
/// `names` は前後の空白を取り除き、空文字・重複を除去したうえで、それぞれが
/// Git のブランチ名として有効かを検証する。不正な名前が1つでもあれば何も
/// 書き込まず `InvalidInput` を返す。正規化した結果が空リストになる場合は、
/// キー自体を削除して既定値（`main`/`master`）に戻す。
pub fn save_protected_branches(repo: &Repository, names: &[String]) -> Result<()> {
    let normalized =
        crate::safety::normalize_protected_branch_names(names.iter().map(|s| s.as_str()));

    for name in &normalized {
        let valid = git2::Branch::name_is_valid(name).unwrap_or(false);
        if !valid {
            return Err(CoreError::InvalidInput(format!(
                "「{name}」は有効なブランチ名ではありません。"
            )));
        }
    }

    let mut local = repo
        .config()?
        .open_level(git2::ConfigLevel::Local)
        .map_err(CoreError::from)?;
    if normalized.is_empty() {
        // 空にする = 既定値（main/master）に戻す。キーが元々無い場合のエラーは無視する。
        let _ = local.remove("noobgit.protectedBranches");
    } else {
        local.set_str("noobgit.protectedBranches", &normalized.join(","))?;
    }
    Ok(())
}

/// マージ済みブランチ（Issue #269: ブランチクリーンアップ）を一括で削除する。
///
/// フロントから渡された `names` をそのまま信用せず、削除の直前に
/// [`crate::repo::merged_branches`] で候補を計算し直し、各ブランチがそこに
/// 含まれているか（＝保護されていない・現在ブランチでない・いずれかの保護
/// ブランチに取り込み済み）を再検証する。含まれないブランチは理由付きで
/// スキップし、削除は行わない。
///
/// 削除は既存の [`delete_branch`] をそのまま使うため、削除したブランチは
/// 1件ごとに `RecreateBranch` の undo エントリが積まれ、個別に元へ戻せる。
/// 途中の1件が失敗（他プロセスによる同時削除など）しても、それ以降のブランチの
/// 削除は続行する。
pub fn delete_branches(
    repo: &Repository,
    names: &[String],
    protected: &[String],
) -> Result<BulkDeleteBranchesOutcome> {
    let candidates = merged_branches(repo, protected)?;

    let mut deleted = Vec::new();
    let mut skipped = Vec::new();

    for raw_name in names {
        let name = raw_name.trim();
        if name.is_empty() {
            continue;
        }

        if !candidates.iter().any(|c| c.name == name) {
            skipped.push(SkippedBranch {
                name: name.to_string(),
                reason:
                    "マージ済みブランチの一覧に含まれていないため、安全のためスキップしました。"
                        .to_string(),
            });
            continue;
        }

        match delete_branch(repo, name) {
            Ok(()) => deleted.push(name.to_string()),
            Err(e) => skipped.push(SkippedBranch {
                name: name.to_string(),
                reason: e.to_string(),
            }),
        }
    }

    Ok(BulkDeleteBranchesOutcome { deleted, skipped })
}

/// コミットに目印（タグ）を付ける。
///
/// `target` が `None` なら HEAD のコミットに付ける。`Some` なら revparse で解決した対象に
/// 付ける（コミットの短縮 oid やブランチ名など）。`message` が空でなければ注釈付きタグ
/// （作成者・メッセージを持つ）、空または `None` なら軽量タグ（参照だけ）を作る。
/// 同名タグが既にあれば日本語エラーで案内する。作成したタグは `DeleteTag` の undo を記録する
/// （`create_branch` / `DeleteBranch` と同じパターン）。
pub fn create_tag(
    repo: &Repository,
    name: &str,
    target: Option<&str>,
    message: Option<&str>,
) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CoreError::InvalidInput(
            "タグ名を入力してください（例: v1.0.0）。".to_string(),
        ));
    }

    // 既に同名タグがあれば案内する。
    if repo.find_reference(&format!("refs/tags/{name}")).is_ok() {
        return Err(CoreError::InvalidInput(format!(
            "タグ「{name}」はすでに存在します。別の名前を使ってください。"
        )));
    }

    // 付ける対象（オブジェクト）を決める。
    let obj = match target {
        Some(rev) => repo.revparse_single(rev.trim()).map_err(|_| {
            CoreError::InvalidInput(format!("対象「{rev}」を特定できませんでした。"))
        })?,
        None => repo
            .head()
            .and_then(|h| h.peel_to_commit())
            .map_err(|_| {
                CoreError::Blocked(
                    "まだコミットが無いため、タグを付けられません。先に最初のコミットをしてください。"
                        .to_string(),
                )
            })?
            .into_object(),
    };

    let annotated = message.map(|m| m.trim()).filter(|m| !m.is_empty());
    match annotated {
        Some(msg) => {
            let sig = repo.signature().map_err(|_| {
                CoreError::InvalidInput(
                    "注釈付きタグには名前とメールの設定が必要です（git config user.name / user.email）。"
                        .to_string(),
                )
            })?;
            repo.tag(name, &obj, &sig, msg, false)?;
        }
        None => {
            repo.tag_lightweight(name, &obj, false)?;
        }
    }

    record_undo(
        repo,
        UndoEntry {
            op: OperationKind::CreateTag,
            description: format!("タグ「{name}」の作成を取り消す"),
            action: UndoAction::DeleteTag {
                name: name.to_string(),
            },
        },
    );

    Ok(())
}

/// タグ（目印）を削除する。直後に Undo で同じタグを作り直して復元できる。
///
/// 削除前に対象 oid と（注釈付きなら）メッセージを控え、`RecreateTag` の undo を記録する。
pub fn delete_tag(repo: &Repository, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CoreError::InvalidInput(
            "タグ名を入力してください。".to_string(),
        ));
    }

    let refname = format!("refs/tags/{name}");
    let reference = repo
        .find_reference(&refname)
        .map_err(|_| CoreError::InvalidInput(format!("タグ「{name}」が見つかりません。")))?;
    let ref_oid = reference
        .target()
        .ok_or_else(|| CoreError::Git("タグの参照先を取得できませんでした。".to_string()))?;

    // 注釈付きタグなら対象 oid とメッセージを控える。軽量タグは参照 oid が対象。
    let (target_oid, message) = match repo.find_tag(ref_oid) {
        Ok(tag) => (
            tag.target_id(),
            tag.message()
                .ok()
                .flatten()
                .map(|m| m.trim_end().to_string()),
        ),
        Err(_) => (ref_oid, None),
    };

    repo.tag_delete(name)?;
    record_undo(
        repo,
        UndoEntry {
            op: OperationKind::DeleteTag,
            description: format!("タグ「{name}」の削除を取り消す"),
            action: UndoAction::RecreateTag {
                name: name.to_string(),
                target: target_oid.to_string(),
                message,
            },
        },
    );
    Ok(())
}

/// リモートリポジトリを追加する。
///
/// `name` はリモート名（例: "origin"）、`url` は fetch 用 URL。
/// 同名リモートが既にある場合や名前が空の場合は日本語エラーを返す。
/// undo は記録しない（再追加で復元できるため）。
pub fn add_remote(repo: &Repository, name: &str, url: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CoreError::InvalidInput(
            "リモート名を入力してください（例: origin）。".to_string(),
        ));
    }
    let url = url.trim();
    if url.is_empty() {
        return Err(CoreError::InvalidInput(
            "URL を入力してください。".to_string(),
        ));
    }
    repo.remote(name, url).map_err(|e| {
        CoreError::InvalidInput(format!(
            "リモート「{name}」を追加できませんでした: {}",
            describe_git2_error(&e)
        ))
    })?;
    Ok(())
}

/// リモートリポジトリを削除する。
///
/// 指定した名前のリモートが無い場合は日本語エラーを返す。
/// undo は記録しない（再追加で復元できるため）。
pub fn remove_remote(repo: &Repository, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CoreError::InvalidInput(
            "リモート名を入力してください。".to_string(),
        ));
    }
    repo.remote_delete(name).map_err(|e| {
        CoreError::InvalidInput(format!(
            "リモート「{name}」を削除できませんでした: {}",
            describe_git2_error(&e)
        ))
    })
}

/// リモートリポジトリの fetch URL を変更する。
///
/// 指定した名前のリモートが無い場合は日本語エラーを返す。
/// undo は記録しない（再変更で復元できるため）。
pub fn set_remote_url(repo: &Repository, name: &str, url: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CoreError::InvalidInput(
            "リモート名を入力してください。".to_string(),
        ));
    }
    let url = url.trim();
    if url.is_empty() {
        return Err(CoreError::InvalidInput(
            "URL を入力してください。".to_string(),
        ));
    }
    repo.remote_set_url(name, url).map_err(|e| {
        CoreError::InvalidInput(format!(
            "リモート「{name}」の URL を変更できませんでした: {}",
            describe_git2_error(&e)
        ))
    })
}

/// リモートから最新を取得し、リモート追跡ブランチ（例: `origin/main`）を更新する。
///
/// 作業ツリー・インデックス・現在ブランチには一切触れない安全操作。取り込む前に
/// 「何が来ているか」を確認するために使う。更新された追跡ブランチ数を返す。
/// リモートで削除されたブランチのプルーニング（追跡ブランチの整理）は既定で有効。
pub fn fetch(repo: &Repository, remote_name: &str) -> Result<FetchOutcome> {
    fetch_with_progress(repo, remote_name, &mut |_| {})
}

/// [`fetch`] と同じ処理を行いつつ、通信の進捗を `on_progress` へ都度通知する。
///
/// `on_progress` は UI スレッドをブロックしないよう軽量に保つこと（例: Tauri の
/// Channel へ送るだけ）。`fetch` はこの関数を何もしないコールバックで呼ぶ薄いラッパー。
/// プルーニングは既定で有効（[`fetch_with_options`] 参照）。
pub fn fetch_with_progress(
    repo: &Repository,
    remote_name: &str,
    on_progress: &mut dyn FnMut(NetworkProgress),
) -> Result<FetchOutcome> {
    fetch_with_options(repo, remote_name, true, on_progress)
}

/// [`fetch_with_progress`] のオプション付き版。
///
/// `prune` が `true`（既定）なら、リモートで削除されたブランチに対応する
/// `refs/remotes/<remote_name>/...` の追跡ブランチも一緒に削除して手元を整理する
/// （`git fetch --prune` 相当。`git2::FetchOptions::prune`）。`false` にすると、
/// 追跡ブランチはリモートで削除されても手元に残り続ける（旧来の挙動）。
/// いまの呼び出し元（[`fetch`] / [`fetch_with_progress`]）はすべて `true` 固定だが、
/// 将来 UI 側でオフにできる余地として引数を残してある。
///
/// **ローカルブランチ本体は、prune の対象であっても絶対に削除しない** — 対象は
/// `refs/remotes/` 以下の追跡ブランチのみ。整理された追跡ブランチ名（例:
/// `origin/feature-x`）は [`FetchOutcome::pruned`] に入れて返す。取りこぼしが
/// 無いよう、fetch 前後の `refs/remotes/<remote_name>/*` の実際の差分を突き合わせて
/// 求める（libgit2 の prune コールバック頼みにしない）。
pub fn fetch_with_options(
    repo: &Repository,
    remote_name: &str,
    prune: bool,
    on_progress: &mut dyn FnMut(NetworkProgress),
) -> Result<FetchOutcome> {
    let remote_name = remote_name.trim();
    if remote_name.is_empty() {
        return Err(CoreError::InvalidInput(
            "リモート名を指定してください（例: origin）。".to_string(),
        ));
    }
    let mut remote = repo.find_remote(remote_name).map_err(|_| {
        CoreError::InvalidInput(format!(
            "リモート「{remote_name}」が見つかりません。取得先の名前を確認してください。"
        ))
    })?;

    notify_connecting(on_progress);

    // プルーニングで消える追跡ブランチを取りこぼしなく求めるため、fetch 前の
    // `refs/remotes/<remote_name>/*` を控えておく。
    let before = remote_tracking_ref_names(repo, remote_name);

    // 更新（前進・新規取得）された追跡ブランチ数を update_tips コールバックで数える。
    let updated = Cell::new(0usize);
    {
        let mut cb = RemoteCallbacks::new();
        // HTTPS / SSH の認証は OS の認証ヘルパや SSH エージェントに委ねる。
        cb.credentials(|url, username_from_url, allowed| {
            credentials(repo, url, username_from_url, allowed)
        });
        cb.update_tips(|_refname, old, new| {
            if old != new {
                updated.set(updated.get() + 1);
            }
            true
        });
        cb.transfer_progress(|stats| {
            let stage = if stats.indexed_deltas() > 0 || stats.total_deltas() > 0 {
                NetworkProgressStage::ResolvingDeltas
            } else {
                NetworkProgressStage::ReceivingObjects
            };
            on_progress(NetworkProgress {
                stage,
                received_objects: stats.received_objects(),
                total_objects: stats.total_objects(),
                received_bytes: stats.received_bytes(),
                indexed_deltas: stats.indexed_deltas(),
                total_deltas: stats.total_deltas(),
            });
            true
        });

        let mut fo = FetchOptions::new();
        fo.remote_callbacks(cb);
        fo.prune(if prune {
            git2::FetchPrune::On
        } else {
            git2::FetchPrune::Off
        });

        // リモートに設定された取得 refspec（例: +refs/heads/*:refs/remotes/origin/*）で取得する。
        let refspecs: Vec<String> = remote
            .fetch_refspecs()?
            .iter()
            .filter_map(|r| r.ok().flatten())
            .map(|s| s.to_string())
            .collect();
        // refspec が空のリモートでは libgit2 が既定の refspec を補う。
        remote.fetch(&refspecs, Some(&mut fo), None).map_err(|e| {
            CoreError::Git(format!(
                "取得（fetch）に失敗しました: {}",
                describe_git2_error_keep_unknown(&e)
            ))
        })?;
    }

    // fetch 後の `refs/remotes/<remote_name>/*` との差分が、実際に整理された追跡ブランチ。
    let after = remote_tracking_ref_names(repo, remote_name);
    let pruned: Vec<String> = before.difference(&after).cloned().collect();

    Ok(FetchOutcome {
        remote: remote_name.to_string(),
        updated_refs: updated.get(),
        pruned,
    })
}

/// `refs/remotes/<remote_name>/*` にある追跡ブランチの表示名（例: `origin/main`）の集合。
///
/// fetch のプルーニングで実際に消えた追跡ブランチを、前後の差分から確実に求めるための
/// 補助。列挙に失敗しても（通常起きない）fetch 全体を失敗させたくないので空集合にする。
fn remote_tracking_ref_names(
    repo: &Repository,
    remote_name: &str,
) -> std::collections::BTreeSet<String> {
    let glob = format!("refs/remotes/{remote_name}/*");
    let mut out = std::collections::BTreeSet::new();
    if let Ok(iter) = repo.references_glob(&glob) {
        for r in iter.flatten() {
            if let Ok(name) = r.name() {
                if let Some(short) = name.strip_prefix("refs/remotes/") {
                    out.insert(short.to_string());
                }
            }
        }
    }
    out
}

/// リモートから取得したうえで、安全に進められるとき（fast-forward）だけ取り込む。
///
/// まず [`fetch`] でリモート追跡ブランチを最新化し、`merge_analysis` で取り込み方を判定する。
/// - すでに最新: 何もしない。
/// - fast-forward 可能: 履歴を一直線に保ったまま前進させる（マージコミットは作らない）。
/// - 分岐していて fast-forward できない: マージが必要だが、コンフリクトでの事故を避けるため
///   **何も変更せずに中断** する（[`CoreError::Blocked`]）。マージと解決は別途のコンフリクト
///   解決 UI に委ねる。これによりデータ消失が起きないことを保証する。
pub fn pull(repo: &Repository, remote_name: &str, branch: &str) -> Result<PullOutcome> {
    pull_with_progress(repo, remote_name, branch, &mut |_| {})
}

/// [`pull`] と同じ処理を行いつつ、fetch 部分の通信進捗を `on_progress` へ都度通知する。
/// `pull` はこの関数を何もしないコールバックで呼ぶ薄いラッパー。
pub fn pull_with_progress(
    repo: &Repository,
    remote_name: &str,
    branch: &str,
    on_progress: &mut dyn FnMut(NetworkProgress),
) -> Result<PullOutcome> {
    let branch = branch.trim();
    if branch.is_empty() {
        return Err(CoreError::InvalidInput(
            "取り込むブランチ名を指定してください。".to_string(),
        ));
    }

    // 1. まずリモートの最新を取得する（ネットワーク操作はここだけ）。
    fetch_with_progress(repo, remote_name, on_progress)?;
    let remote_name = remote_name.trim();

    // 2. 取り込み元（例: refs/remotes/origin/main）の先端コミットを得る。
    let tracking = format!("refs/remotes/{remote_name}/{branch}");
    let their_commit = repo
        .find_reference(&tracking)
        .map_err(|_| {
            CoreError::InvalidInput(format!(
                "リモート「{remote_name}」にブランチ「{branch}」が見つかりませんでした。ブランチ名を確認してください。"
            ))
        })?
        .peel_to_commit()?;
    let annotated = repo.find_annotated_commit(their_commit.id())?;

    // 3. 取り込み方を判定する。
    let (analysis, _pref) = repo.merge_analysis(&[&annotated])?;

    // まだ1つもコミットが無い（未誕生）ブランチ: 取り込み先を作って前進させる（FF 相当）。
    if analysis.is_unborn() {
        return fast_forward_unborn(repo, &their_commit);
    }
    if analysis.is_up_to_date() {
        return Ok(PullOutcome::UpToDate);
    }
    if analysis.is_fast_forward() {
        return fast_forward(repo, &their_commit);
    }

    // 4. 分岐あり（FF 不可）。安全のため何も変えずに中断する。
    Err(CoreError::Blocked(
        "リモートとローカルそれぞれに別の変更があり、自動では安全に取り込めません（fast-forward できません）。\
         取り込むにはマージが必要です。変更を失わないよう、ここでは何も変更せずに中断しました。"
            .to_string(),
    ))
}

/// 現在ブランチを `target` まで fast-forward する。
///
/// 安全チェックアウトで作業ツリー・インデックスを `target` に合わせてから、現在ブランチの
/// 参照を `target` へ進める。未コミットのローカル変更と衝突する場合は libgit2 が
/// チェックアウトを失敗させるので、上書きによるデータ消失は起きない。
fn fast_forward(repo: &Repository, target: &Commit) -> Result<PullOutcome> {
    let mut co = CheckoutBuilder::new();
    repo.checkout_tree(target.as_object(), Some(&mut co))
        .map_err(|_| {
            CoreError::Blocked(
                "未コミットの変更があるため取り込めません。先に変更をコミットするか退避(stash)してください。"
                    .to_string(),
            )
        })?;

    // 現在ブランチ（HEAD が指す参照）を target へ進める。HEAD はブランチを指したまま。
    let mut head_ref = repo.head()?;
    head_ref.set_target(target.id(), "noobgit: fast-forward pull")?;

    Ok(PullOutcome::FastForwarded {
        commit: commit_info(target),
    })
}

/// 未誕生（コミット0件）の現在ブランチへ取り込む。HEAD が指すブランチ参照を作る。
fn fast_forward_unborn(repo: &Repository, target: &Commit) -> Result<PullOutcome> {
    // HEAD が指しているブランチ名（例: refs/heads/main）を取り出す。
    let head_ref_name = repo
        .find_reference("HEAD")?
        .symbolic_target()?
        .ok_or_else(|| CoreError::Git("現在のブランチを特定できませんでした。".to_string()))?
        .to_string();

    let mut co = CheckoutBuilder::new();
    repo.checkout_tree(target.as_object(), Some(&mut co))
        .map_err(|_| {
            CoreError::Blocked(
                "作業フォルダの内容と衝突するため取り込めません。先に退避してください。"
                    .to_string(),
            )
        })?;
    repo.reference(
        &head_ref_name,
        target.id(),
        true,
        "noobgit: pull into unborn branch",
    )?;

    Ok(PullOutcome::FastForwarded {
        commit: commit_info(target),
    })
}

/// fetch / pull の認証情報を解決する。HTTPS は OS の認証ヘルパ、SSH はエージェントに委ねる。
fn credentials(
    repo: &Repository,
    url: &str,
    username_from_url: Option<&str>,
    allowed: CredentialType,
) -> std::result::Result<Cred, git2::Error> {
    // SSH: サーバがまずユーザ名だけを要求する2段階のことがある。
    if allowed.contains(CredentialType::USERNAME) {
        if let Some(user) = username_from_url {
            return Cred::username(user);
        }
    }
    // SSH 鍵はエージェントから取り出す。
    if allowed.contains(CredentialType::SSH_KEY) {
        if let Some(user) = username_from_url {
            return Cred::ssh_key_from_agent(user);
        }
    }
    // HTTPS など: Git の認証ヘルパ（資格情報マネージャ）に委ねる。
    if allowed.contains(CredentialType::USER_PASS_PLAINTEXT) {
        if let Ok(cfg) = repo.config() {
            if let Ok(cred) = Cred::credential_helper(&cfg, url, username_from_url) {
                return Ok(cred);
            }
        }
    }
    if allowed.contains(CredentialType::DEFAULT) {
        return Cred::default();
    }
    Err(git2::Error::from_str(
        "認証情報が見つかりませんでした。Git の認証設定（資格情報マネージャや SSH エージェント）を確認してください。",
    ))
}

/// push / clone の認証情報を解決する。
///
/// [`credentials`]（fetch / pull 用）は既に開いているリポジトリの設定
/// （`repo.config()`）を見るが、push は書き込み用に OS の既定設定を直接見ており、
/// clone は（これから作るリポジトリなので）まだ `Repository` を持たない。この2つは
/// 同じ「既定設定を見る」方式で足りるため、この関数へ共通化する。
fn default_remote_credentials(
    url: &str,
    username_from_url: Option<&str>,
    allowed: CredentialType,
) -> std::result::Result<Cred, git2::Error> {
    // SSH 鍵はエージェントから取り出す。ユーザ名が不明なら Git サーバの慣例 "git" を使う。
    if allowed.contains(CredentialType::SSH_KEY) {
        return Cred::ssh_key_from_agent(username_from_url.unwrap_or("git"));
    }
    // HTTPS など: Git の認証ヘルパ（資格情報マネージャ）に委ねる。
    if allowed.contains(CredentialType::USER_PASS_PLAINTEXT) {
        if let Ok(config) = git2::Config::open_default() {
            if let Ok(cred) = Cred::credential_helper(&config, url, username_from_url) {
                return Ok(cred);
            }
        }
    }
    if allowed.contains(CredentialType::DEFAULT) {
        return Cred::default();
    }
    Err(git2::Error::from_str(
        "利用できる認証情報が見つかりませんでした。",
    ))
}

/// fetch / pull / push / clone 共通: まだ何も届いていない「接続待ち」の段階を
/// 一度だけ通知する。transfer_progress 系のコールバックは接続確立後にしか呼ばれない
/// ため、これが無いと接続に時間がかかるとき UI が完全に無反応に見えてしまう。
fn notify_connecting(on_progress: &mut dyn FnMut(NetworkProgress)) {
    on_progress(NetworkProgress {
        stage: NetworkProgressStage::Connecting,
        received_objects: 0,
        total_objects: 0,
        received_bytes: 0,
        indexed_deltas: 0,
        total_deltas: 0,
    });
}

/// 指定地点までハードリセットする。破壊的操作。直後にコミット位置を Undo で戻せる。
pub fn reset_hard(repo: &Repository, revspec: &str) -> Result<()> {
    let prev = repo.head().ok().and_then(|h| h.target()).ok_or_else(|| {
        CoreError::Blocked("まだコミットが無いためリセットできません。".to_string())
    })?;

    let obj = repo.revparse_single(revspec)?;
    let commit = obj
        .peel_to_commit()
        .map_err(|_| CoreError::InvalidInput(format!("コミットを特定できません: {revspec}")))?;

    repo.reset(commit.as_object(), ResetType::Hard, None)?;
    record_undo(
        repo,
        UndoEntry {
            op: OperationKind::ResetHard,
            description: "ハードリセットを取り消す（リセット前の位置に戻す）".to_string(),
            action: UndoAction::HardResetTo {
                previous: prev.to_string(),
            },
        },
    );
    Ok(())
}

/// 指定したコミットの変更を、いまのブランチの先頭にコピーする（cherry-pick）。
///
/// `oid` はコピー元コミットのハッシュ。元のコミットはそのまま残り、現在ブランチに
/// 同じ変更を持つ新しいコミットを 1 つ積む。author は元コミットを引き継ぎ、committer は
/// 現在の identity に更新する（git の cherry-pick と同じ）。メッセージも元コミットを引き継ぐ。
///
/// 未コミットの変更を黙って消さないため、次の場合は**何も変えずに** [`CoreError::Blocked`]
/// で中断する: ステージ済みの変更があるとき（git と同じ）、コンフリクト（競合）が起きた
/// とき、コピー内容が未ステージの変更と同じファイルに触れているとき。作業ツリーへの反映は
/// 安全（safe）チェックアウトで行い、無関係なファイルのローカル変更は保たれる。
/// 成功時は、コピー直前の HEAD への soft reset を undo に記録する。
pub fn cherry_pick(repo: &Repository, oid: &str) -> Result<CommitInfo> {
    let target = git2::Oid::from_str(oid.trim())
        .map_err(|_| CoreError::InvalidInput(format!("コミットの指定が不正です: {oid}")))?;
    let commit = repo.find_commit(target).map_err(|_| {
        CoreError::InvalidInput(format!("指定したコミットが見つかりませんでした: {oid}"))
    })?;

    // コピー先となる現在の HEAD コミット。これが無ければまだ何もコミットしていない。
    let head_commit = repo.head().and_then(|h| h.peel_to_commit()).map_err(|_| {
        CoreError::Blocked(
            "まだコミットが無いため、コピー（cherry-pick）できません。先に最初のコミットをしてください。"
                .to_string(),
        )
    })?;
    let previous = head_commit.id();

    let sig = repo.signature().map_err(|_| {
        CoreError::InvalidInput(
            "コピー（cherry-pick）には名前とメールの設定が必要です（git config user.name / user.email）。"
                .to_string(),
        )
    })?;

    // git と同様、ステージ済みの変更があるときは実行しない。コピーの結果と混ざって
    // 後から区別できなくなるため、何も変えずに中断して先に整理してもらう。
    let head_tree = head_commit.tree()?;
    let staged = repo.diff_tree_to_index(Some(&head_tree), None, None)?;
    if staged.deltas().len() > 0 {
        return Err(CoreError::Blocked(
            "ステージ済みの変更があるため、コピー（cherry-pick）できません。先にコミットするか退避(stash)してください。"
                .to_string(),
        ));
    }

    // HEAD を土台に、コピー元コミットの変更を当てたインデックスをメモリ上に作る
    // （作業ツリー・実インデックスにはまだ触れない）。
    let mut merged = repo
        .cherrypick_commit(&commit, &head_commit, 0, None)
        .map_err(|e| {
            CoreError::Git(format!(
                "コピー（cherry-pick）に失敗しました: {}",
                describe_git2_error(&e)
            ))
        })?;

    // コンフリクトがあれば、何も変えずに中断する（作業ツリーは元から触れていない）。
    if merged.has_conflicts() {
        return Err(CoreError::Blocked(
            "コンフリクト（競合）のため取り込めませんでした。状態は元に戻しました。先にいまの変更を整理してから、もう一度お試しください。"
                .to_string(),
        ));
    }

    // コンフリクトなし: 合成したインデックスからツリーを作る。
    let tree_id = merged.write_tree_to(repo)?;
    let tree = repo.find_tree(tree_id)?;
    let message = commit.message().unwrap_or("");

    // コミットの前に、作業ツリー・インデックスを安全（safe）チェックアウトで新しい内容へ
    // 合わせる。コピー内容が未コミットの変更と同じファイルに触れている場合はここで失敗し、
    // 何も変えずに中断する。force だと、無関係なファイルの未コミット変更まで黙って
    // 消えてしまう（データ消失）ため使わない。
    let mut co = CheckoutBuilder::new();
    repo.checkout_tree(tree.as_object(), Some(&mut co)).map_err(|_| {
        CoreError::Blocked(
            "未コミットの変更とコピー内容が同じファイルに触れているため、コピー（cherry-pick）を中断しました。先にコミットか退避(stash)をしてください。"
                .to_string(),
        )
    })?;

    // author は元コミットのまま、committer を現在の identity にして新コミットを作る。
    let new_oid = repo.commit(
        Some("HEAD"),
        &commit.author(),
        &sig,
        message,
        &tree,
        &[&head_commit],
    )?;

    // 念のため CHERRY_PICK_HEAD 等の途中状態を片付ける（メモリ index 方式では通常付かない）。
    let _ = repo.cleanup_state();

    record_undo(
        repo,
        UndoEntry {
            op: OperationKind::CherryPick,
            description: format!(
                "コミット「{}」のコピー（cherry-pick）を取り消す",
                first_line(message)
            ),
            action: UndoAction::SoftResetTo {
                previous: previous.to_string(),
            },
        },
    );

    let new_commit = repo.find_commit(new_oid)?;
    Ok(commit_info(&new_commit))
}

/// 指定したコミットの変更を打ち消す新しいコミットを、いまのブランチの先頭に積む（revert）。
///
/// `oid` は打ち消したいコミットのハッシュ。履歴は書き換えず、逆向きの変更を持つ
/// コミットを 1 つ**追加**するだけなので、すでに push 済みのコミットにも安全に使える
/// （reset や amend のような履歴書き換えとの最大の違い）。メッセージは git の
/// `git revert` と同じ形式（`Revert "元の件名"` と `This reverts commit <id>.`）にする。
///
/// 次の場合は**何も変えずに** [`CoreError::Blocked`] で中断する: マージコミット
/// （親が 2 つ以上。どちらの親側へ戻すか選ぶ必要があり、v1 では非対応）、ステージ済みの
/// 変更があるとき、コンフリクト（競合）が起きたとき、打ち消し内容が未コミットの変更と
/// 同じファイルに触れているとき。作業ツリーへの反映は安全（safe）チェックアウトで行い、
/// 無関係なファイルのローカル変更は保たれる（[`cherry_pick`] と同じ方針）。
/// 成功時は、revert 直前の HEAD への soft reset を undo に記録する。
pub fn revert_commit(repo: &Repository, oid: &str) -> Result<CommitInfo> {
    let target = git2::Oid::from_str(oid.trim())
        .map_err(|_| CoreError::InvalidInput(format!("コミットの指定が不正です: {oid}")))?;
    let commit = repo.find_commit(target).map_err(|_| {
        CoreError::InvalidInput(format!("指定したコミットが見つかりませんでした: {oid}"))
    })?;

    if commit.parent_count() > 1 {
        return Err(CoreError::Blocked(
            "マージコミットは、どちらの側へ戻すかを選ぶ必要があるため、まだ打ち消せません。"
                .to_string(),
        ));
    }

    let head_commit = repo.head().and_then(|h| h.peel_to_commit()).map_err(|_| {
        CoreError::Blocked(
            "まだコミットが無いため、打ち消し（revert）できません。先に最初のコミットをしてください。"
                .to_string(),
        )
    })?;
    let previous = head_commit.id();

    let sig = repo.signature().map_err(|_| {
        CoreError::InvalidInput(
            "打ち消し（revert）には名前とメールの設定が必要です（git config user.name / user.email）。"
                .to_string(),
        )
    })?;

    // git と同様、ステージ済みの変更があるときは実行しない（打ち消しの結果と混ざるため）。
    let head_tree = head_commit.tree()?;
    let staged = repo.diff_tree_to_index(Some(&head_tree), None, None)?;
    if staged.deltas().len() > 0 {
        return Err(CoreError::Blocked(
            "ステージ済みの変更があるため、打ち消し（revert）できません。先にコミットするか退避(stash)してください。"
                .to_string(),
        ));
    }

    // HEAD を土台に、対象コミットの逆向きの変更を当てたインデックスをメモリ上に作る
    // （作業ツリー・実インデックスにはまだ触れない）。
    let mut merged = repo
        .revert_commit(&commit, &head_commit, 0, None)
        .map_err(|e| {
            CoreError::Git(format!(
                "打ち消し（revert）に失敗しました: {}",
                describe_git2_error(&e)
            ))
        })?;

    if merged.has_conflicts() {
        return Err(CoreError::Blocked(
            "いまの内容と打ち消したい変更が同じ箇所に触れていて、コンフリクト（競合）のため打ち消せませんでした。状態は元のままです。"
                .to_string(),
        ));
    }

    let tree_id = merged.write_tree_to(repo)?;
    let tree = repo.find_tree(tree_id)?;

    // 未コミットの変更を黙って消さないため、force ではなく safe チェックアウトで反映する。
    let mut co = CheckoutBuilder::new();
    repo.checkout_tree(tree.as_object(), Some(&mut co)).map_err(|_| {
        CoreError::Blocked(
            "未コミットの変更と打ち消し内容が同じファイルに触れているため、打ち消し（revert）を中断しました。先にコミットか退避(stash)をしてください。"
                .to_string(),
        )
    })?;

    let summary = first_line(commit.message().unwrap_or("")).to_string();
    let message = format!(
        "Revert \"{}\"\n\nThis reverts commit {}.\n",
        summary,
        commit.id()
    );
    let new_oid = repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[&head_commit])?;

    let _ = repo.cleanup_state();

    record_undo(
        repo,
        UndoEntry {
            op: OperationKind::Revert,
            description: format!("コミット「{summary}」の打ち消し（revert）を取り消す"),
            action: UndoAction::SoftResetTo {
                previous: previous.to_string(),
            },
        },
    );

    let new_commit = repo.find_commit(new_oid)?;
    Ok(commit_info(&new_commit))
}

/// ローカルのコミットをリモートへ送信（push）する。
///
/// `remote` はリモート名（例: `origin`）、`refspec` は送信するブランチの指定
/// （例: `refs/heads/main:refs/heads/main`）。`force` が真のときは強制 push（リモートの
/// 履歴を上書き）を行う。push はローカルだけでは取り消せないため undo は記録しない。
pub fn push(repo: &Repository, remote: &str, refspec: &str, force: bool) -> Result<()> {
    push_with_progress(repo, remote, refspec, force, &mut |_| {})
}

/// [`push`] と同じ処理を行いつつ、通信の進捗を `on_progress` へ都度通知する。
/// `push` はこの関数を何もしないコールバックで呼ぶ薄いラッパー。
pub fn push_with_progress(
    repo: &Repository,
    remote: &str,
    refspec: &str,
    force: bool,
    on_progress: &mut dyn FnMut(NetworkProgress),
) -> Result<()> {
    let remote = remote.trim();
    let refspec = refspec.trim();
    if remote.is_empty() {
        return Err(CoreError::InvalidInput(
            "送信先のリモート名を指定してください（例: origin）。".to_string(),
        ));
    }
    if refspec.is_empty() {
        return Err(CoreError::InvalidInput(
            "送信するブランチを指定してください。".to_string(),
        ));
    }

    let mut remote_obj = repo.find_remote(remote).map_err(|_| {
        CoreError::InvalidInput(format!(
            "リモート「{remote}」が見つかりません。リモートの設定を確認してください。"
        ))
    })?;

    // 強制 push のときだけ refspec の先頭に '+' を付けて、上書き（非fast-forward）を許可する。
    let effective = if force && !refspec.starts_with('+') {
        format!("+{refspec}")
    } else {
        refspec.to_string()
    };

    // リモートが個々の参照を拒否した理由（非fast-forward 等）を callback から拾う。
    // push() 自体は成功(Ok)を返しつつ、拒否はこの callback の status で通知されることがある。
    let rejection: RefCell<Option<String>> = RefCell::new(None);

    // fetch と同様、接続確立前は push_transfer_progress が一度も呼ばれないため、
    // まず「接続待ち」を一度通知しておく。
    notify_connecting(on_progress);

    {
        let mut callbacks = RemoteCallbacks::new();
        // 認証は SSH エージェント → 資格情報ヘルパ（トークン等）→ 既定 の順で試す。
        // ローカルパスのリモートでは認証は不要で、この callback は呼ばれない。
        callbacks.credentials(default_remote_credentials);
        // 各参照の更新結果。status が Some なら、その参照はリモートに拒否されている。
        callbacks.push_update_reference(|refname, status| {
            if let Some(msg) = status {
                *rejection.borrow_mut() = Some(format!("{refname}: {msg}"));
            }
            Ok(())
        });
        // 送信オブジェクトの進捗。(current, total, bytes) の順で渡される。
        callbacks.push_transfer_progress(|current, total, bytes| {
            on_progress(NetworkProgress {
                stage: NetworkProgressStage::SendingObjects,
                received_objects: current,
                total_objects: total,
                received_bytes: bytes,
                indexed_deltas: 0,
                total_deltas: 0,
            });
        });

        let mut opts = PushOptions::new();
        opts.remote_callbacks(callbacks);

        remote_obj
            .push(&[effective.as_str()], Some(&mut opts))
            .map_err(map_push_error)?;
    }

    if let Some(reason) = rejection.into_inner() {
        return Err(CoreError::Blocked(format!(
            "リモートへの送信が拒否されました。リモートに自分の手元には無い変更があるかもしれません。先に取り込み（pull）をしてから、もう一度送信してください。（詳細: {reason}）"
        )));
    }

    Ok(())
}

/// push の git2 エラーを初心者向けの日本語 [`CoreError`] に変換する。
fn map_push_error(e: git2::Error) -> CoreError {
    use git2::ErrorCode;
    match e.code() {
        ErrorCode::Auth => CoreError::Blocked(
            "リモートの認証に失敗しました。SSH鍵やトークンの設定を確認してください。".to_string(),
        ),
        ErrorCode::NotFastForward => CoreError::Blocked(
            "リモートへの送信が拒否されました（非fast-forward）。先に取り込み（pull）をしてから、もう一度送信してください。"
                .to_string(),
        ),
        _ => CoreError::Git(format!("リモートへの送信に失敗しました: {}", describe_git2_error_keep_unknown(&e))),
    }
}

/// リモートリポジトリを `dest_path` へ新規にクローンする（進捗通知なし版）。
///
/// 詳細は [`clone_with_progress`] を参照。undo は記録しない（ネットワーク操作）。
pub fn clone_repo(url: &str, dest_path: &Path) -> Result<CloneOutcome> {
    clone_with_progress(url, dest_path, &mut |_| {})
}

/// リモートリポジトリを `dest_path` へ新規にクローンし、通信の進捗を `on_progress` へ
/// 都度通知する。`clone_repo` はこの関数を何もしないコールバックで呼ぶ薄いラッパー。
///
/// # 安全のための約束
/// - `url` が空、または明らかに URL の形をしていない場合は [`CoreError::InvalidInput`]。
/// - `dest_path` が既に存在し、かつ中身が空でないディレクトリなら、**既存データを一切
///   変更・削除せず** [`CoreError::Blocked`] で中断する（空のディレクトリへは通常の
///   `git clone` と同様にクローンできる）。
/// - クローンが失敗した場合、**このクローンのために新規作成したディレクトリだけ**を
///   後片付けする。呼び出し前から存在していた（空の）ディレクトリは、中身だけ片付けて
///   ディレクトリ自体は残す。どちらの場合も、呼び出し前から存在していたファイルを
///   消すことは無い。
/// - 認証・進捗の通知は [`fetch_with_progress`] / [`push_with_progress`] と同じ基盤
///   （[`default_remote_credentials`] / [`notify_connecting`]）を再利用する。
/// - ネットワーク操作のため undo は記録しない。
pub fn clone_with_progress(
    url: &str,
    dest_path: &Path,
    on_progress: &mut dyn FnMut(NetworkProgress),
) -> Result<CloneOutcome> {
    let url = url.trim();
    if url.is_empty() {
        return Err(CoreError::InvalidInput(
            "クローン元の URL を入力してください。".to_string(),
        ));
    }
    if url.chars().any(|c| c.is_whitespace()) {
        return Err(CoreError::InvalidInput(
            "URL に空白を含めることはできません。".to_string(),
        ));
    }
    // 明らかに URL/パスの形をしていないものを早めに弾く。
    // 対応する形: "scheme://..."、scp形式 "user@host:path"、絶対/相対パス。
    let looks_like_source = url.contains("://")
        || url.contains('@')
        || url.starts_with('/')
        || url.starts_with('.')
        || url.starts_with('~')
        // Windows のドライブレター形式 (C:\ や C:/)。
        || (url.len() >= 3
            && url.as_bytes()[0].is_ascii_alphabetic()
            && url.as_bytes()[1] == b':'
            && matches!(url.as_bytes()[2], b'\\' | b'/'));
    if !looks_like_source {
        return Err(CoreError::InvalidInput(format!(
            "「{url}」は有効な URL に見えません。https://... や git@... の形式で指定してください。"
        )));
    }

    if dest_path.as_os_str().is_empty() {
        return Err(CoreError::InvalidInput(
            "保存先のフォルダを指定してください。".to_string(),
        ));
    }
    // 相対パスだとアプリの作業フォルダ基準になり、意図しない場所に作られてしまう。
    if !dest_path.is_absolute() {
        return Err(CoreError::InvalidInput(format!(
            "保存先「{}」は完全なパスで指定してください（例: C:\\Users\\you\\projects\\repo）。「参照…」からフォルダを選ぶと確実です。",
            dest_path.display()
        )));
    }

    // 保存先の既存状態を確認する。既に存在して中身があれば、既存データを守るため
    // 何もせず拒否する。
    let pre_existed = dest_path.exists();
    if pre_existed {
        if !dest_path.is_dir() {
            return Err(CoreError::InvalidInput(format!(
                "保存先「{}」はフォルダではありません。別の保存先を指定してください。",
                dest_path.display()
            )));
        }
        let has_entries = std::fs::read_dir(dest_path)
            .map_err(|e| {
                CoreError::Git(format!(
                    "保存先フォルダを確認できませんでした: {}",
                    describe_io_error(&e)
                ))
            })?
            .next()
            .is_some();
        if has_entries {
            return Err(CoreError::Blocked(format!(
                "保存先「{}」には既にファイルがあります。空のフォルダか、新しいフォルダ名を指定してください。",
                dest_path.display()
            )));
        }
    }

    notify_connecting(on_progress);

    let mut cb = RemoteCallbacks::new();
    // push と同じ基盤（OS 既定設定を見る）を再利用する。clone はまだリポジトリを
    // 開いていないため、fetch/pull のようにリポジトリの設定を見る方式は使えない。
    cb.credentials(default_remote_credentials);
    cb.transfer_progress(|stats| {
        let stage = if stats.indexed_deltas() > 0 || stats.total_deltas() > 0 {
            NetworkProgressStage::ResolvingDeltas
        } else {
            NetworkProgressStage::ReceivingObjects
        };
        on_progress(NetworkProgress {
            stage,
            received_objects: stats.received_objects(),
            total_objects: stats.total_objects(),
            received_bytes: stats.received_bytes(),
            indexed_deltas: stats.indexed_deltas(),
            total_deltas: stats.total_deltas(),
        });
        true
    });

    let mut fo = FetchOptions::new();
    fo.remote_callbacks(cb);

    let mut builder = RepoBuilder::new();
    builder.fetch_options(fo);

    match builder.clone(url, dest_path) {
        Ok(_repo) => Ok(CloneOutcome {
            path: dest_path.to_string_lossy().into_owned(),
        }),
        Err(e) => {
            // 失敗したら、このクローンのために作った分だけ後片付けする。
            cleanup_failed_clone(dest_path, pre_existed);
            Err(map_clone_error(e))
        }
    }
}

/// クローン失敗後の後片付け。
///
/// 呼び出し前から `dest_path` が存在していた（`pre_existed`）場合は、そのディレクトリ
/// 自体は残して中身だけを片付ける（もともと空だったはずなので、消すのはクローンが
/// 部分的に作った分だけになる）。存在していなかった（今回のクローンのために新規作成
/// した）場合は、ディレクトリごと削除する。いずれの経路でも、呼び出し前から存在して
/// いたファイルを消すことは無い。後片付け自体の失敗はベストエフォートで無視する
/// （クローン失敗というエラーの伝達を優先し、後片付けの失敗で上書きしない）。
fn cleanup_failed_clone(dest_path: &Path, pre_existed: bool) {
    if pre_existed {
        if let Ok(entries) = std::fs::read_dir(dest_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                let _ = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
            }
        }
    } else {
        let _ = std::fs::remove_dir_all(dest_path);
    }
}

/// clone の git2 エラーを初心者向けの日本語 [`CoreError`] に変換する。
fn map_clone_error(e: git2::Error) -> CoreError {
    use git2::ErrorCode;
    match e.code() {
        ErrorCode::Auth => CoreError::Blocked(
            "認証に失敗しました。URL やアクセス権、SSH鍵・トークンの設定を確認してください。"
                .to_string(),
        ),
        ErrorCode::NotFound => CoreError::InvalidInput(
            "指定したリポジトリが見つかりませんでした。URL を確認してください。".to_string(),
        ),
        _ => CoreError::Git(format!(
            "クローンに失敗しました: {}",
            describe_git2_error_keep_unknown(&e)
        )),
    }
}

/// 指定したローカルブランチを現在のブランチにマージする。
///
/// - すでに統合済み（up-to-date）: 何もせず [`MergeOutcome::UpToDate`] を返す。
/// - fast-forward 可能: マージコミットを作らず履歴を一直線に前進させ、
///   [`MergeOutcome::FastForwarded`] を返す。undo は `HardResetTo` で記録。
/// - 通常マージ（コンフリクトなし）: マージコミットを作成して [`MergeOutcome::Merged`] を返す。
///   undo は `SoftResetTo` で記録。
/// - コンフリクトあり: リポジトリをマージ中の状態のまま [`MergeOutcome::Conflicted`] を返す。
///   フロントエンドの ConflictWizard でコンフリクト解消を行う。undo は記録しない（解消後に
///   コミットして初めて状態が確定する）。
pub fn merge_branch(repo: &Repository, branch_name: &str) -> Result<MergeOutcome> {
    let branch_name = branch_name.trim();
    if branch_name.is_empty() {
        return Err(CoreError::InvalidInput(
            "マージするブランチ名を指定してください。".to_string(),
        ));
    }

    // マージ対象ブランチの先端コミットを取得する。
    let branch = repo
        .find_branch(branch_name, BranchType::Local)
        .map_err(|_| {
            CoreError::InvalidInput(format!(
                "ブランチ「{branch_name}」が見つかりませんでした。ブランチ名を確認してください。"
            ))
        })?;
    let their_commit = branch.get().peel_to_commit()?;
    let annotated = repo.find_annotated_commit(their_commit.id())?;

    // 現在の HEAD を記録する（undo 用）。
    let head_commit = repo.head().and_then(|h| h.peel_to_commit()).map_err(|_| {
        CoreError::Blocked(
            "まだコミットがないため、マージできません。先に最初のコミットをしてください。"
                .to_string(),
        )
    })?;
    let previous = head_commit.id();

    // マージ方法を判定する。
    let (analysis, _pref) = repo.merge_analysis(&[&annotated])?;

    if analysis.is_up_to_date() {
        return Ok(MergeOutcome::UpToDate);
    }

    if analysis.is_fast_forward() {
        // fast-forward: 作業ツリー・インデックスを対象コミットへ合わせ、HEAD を前進させる。
        let mut co = CheckoutBuilder::new();
        repo.checkout_tree(their_commit.as_object(), Some(&mut co))
            .map_err(|_| {
                CoreError::Blocked(
                    "未コミットの変更があるためマージできません。先に変更をコミットするか退避(stash)してください。"
                        .to_string(),
                )
            })?;
        let mut head_ref = repo.head()?;
        head_ref.set_target(their_commit.id(), "noobgit: fast-forward merge")?;

        record_undo(
            repo,
            UndoEntry {
                op: OperationKind::Merge,
                description: format!("ブランチ「{branch_name}」のマージ（fast-forward）を取り消す"),
                action: UndoAction::HardResetTo {
                    previous: previous.to_string(),
                },
            },
        );

        let updated = repo.find_commit(their_commit.id())?;
        return Ok(MergeOutcome::FastForwarded {
            commit: commit_info(&updated),
        });
    }

    // 通常マージ: インデックスと作業ツリーにマージ結果を適用する。
    repo.merge(&[&annotated], None, None).map_err(|e| {
        CoreError::Git(format!("マージに失敗しました: {}", describe_git2_error(&e)))
    })?;

    // コンフリクトがあれば、リポジトリをマージ中の状態のまま返す。
    // フロントエンドは status を取り直して ConflictWizard に誘導する。
    let mut index = repo.index()?;
    if index.has_conflicts() {
        return Ok(MergeOutcome::Conflicted);
    }

    // コンフリクトなし: マージコミットを作成する。
    let sig = repo.signature().map_err(|_| {
        CoreError::InvalidInput(
            "マージには名前とメールの設定が必要です（git config user.name / user.email）。"
                .to_string(),
        )
    })?;
    let tree_id = index.write_tree_to(repo)?;
    let tree = repo.find_tree(tree_id)?;
    let message = format!("Merge branch '{branch_name}'");

    let new_oid = repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        &message,
        &tree,
        &[&head_commit, &their_commit],
    )?;

    // インデックスをディスクに書き出し、MERGE_HEAD などの中間状態を片付ける。
    index.write()?;
    let _ = repo.cleanup_state();

    record_undo(
        repo,
        UndoEntry {
            op: OperationKind::Merge,
            description: format!("ブランチ「{branch_name}」のマージを取り消す"),
            action: UndoAction::SoftResetTo {
                previous: previous.to_string(),
            },
        },
    );

    let new_commit = repo.find_commit(new_oid)?;
    Ok(MergeOutcome::Merged {
        commit: commit_info(&new_commit),
    })
}

/// 指定したコミット時点のファイル内容を作業ツリーに復元し、ステージする。
///
/// `commit_id` は復元元コミットのハッシュ（短縮形可）。`file_path` はリポジトリルートからの
/// 相対パス。`git restore --source <commit> -- <path>` に相当する。
///
/// - 指定コミットのツリーから対象 blob を取り出し、作業ツリーへ書き込む（上書き）。
/// - その内容をインデックスにもステージする。
/// - 指定コミットに対象ファイルが存在しなければ日本語エラーを返す。
/// - undo: ステージした分を戻せるよう `UnstagePath` を記録する（ベストエフォート）。
///   上書き自体は不可逆であることは explain.rs と confirm ダイアログで伝える。
pub fn restore_file_from_commit(repo: &Repository, commit_id: &str, file_path: &str) -> Result<()> {
    let commit_id = commit_id.trim();
    let file_path = file_path.trim();

    if commit_id.is_empty() {
        return Err(CoreError::InvalidInput(
            "復元元のコミットを指定してください。".to_string(),
        ));
    }
    if file_path.is_empty() {
        return Err(CoreError::InvalidInput(
            "復元するファイルのパスを指定してください。".to_string(),
        ));
    }

    // 作業ツリー外を指すパスは拒否する（安全のため）。
    ensure_repo_relative_path(file_path)?;
    let rel = Path::new(file_path);

    // コミット ID を解決してコミットオブジェクトを得る。
    let obj = repo.revparse_single(commit_id).map_err(|_| {
        CoreError::InvalidInput(format!(
            "指定したコミット「{commit_id}」が見つかりませんでした。コミット ID を確認してください。"
        ))
    })?;
    let commit = obj.peel_to_commit().map_err(|_| {
        CoreError::InvalidInput(format!("「{commit_id}」はコミットではありません。"))
    })?;

    // コミットのツリーから対象パスのエントリを取得する。
    let tree = commit.tree()?;
    let entry = tree.get_path(rel).map_err(|_| {
        CoreError::InvalidInput(format!(
            "コミット「{commit_id}」にファイル「{file_path}」が見つかりませんでした。\
             そのコミット時点では存在しないファイルです。"
        ))
    })?;

    // blob を取り出してバイト列を得る。
    let blob = repo.find_blob(entry.id()).map_err(|_| {
        CoreError::InvalidInput(format!(
            "コミット「{commit_id}」の「{file_path}」の内容を取得できませんでした。"
        ))
    })?;
    let content = blob.content();

    // 作業ツリーへ書き込む（親ディレクトリが無ければ作成する）。
    let workdir = repo
        .workdir()
        .ok_or_else(|| CoreError::Git("作業ツリーがありません。".to_string()))?;
    let dest = workdir.join(rel);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            CoreError::Git(format!(
                "ディレクトリを作成できませんでした: {}",
                describe_io_error(&e)
            ))
        })?;
    }
    std::fs::write(&dest, content).map_err(|e| {
        CoreError::Git(format!(
            "ファイルを書き込めませんでした: {}",
            describe_io_error(&e)
        ))
    })?;

    // インデックスにもステージする。
    let mut index = repo.index()?;
    index.add_path(rel).map_err(|e| {
        CoreError::Git(format!(
            "ステージに失敗しました: {}",
            describe_git2_error(&e)
        ))
    })?;
    index.write()?;

    // undo: ステージを戻せるよう UnstagePath を記録する（ベストエフォート）。
    record_undo(
        repo,
        UndoEntry {
            op: OperationKind::RestoreFile,
            description: format!("「{file_path}」のコミット時点への復元を取り消す（アンステージ）"),
            action: UndoAction::UnstagePath {
                path: file_path.to_string(),
            },
        },
    );

    Ok(())
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("").trim()
}

/// Undo履歴への記録はベストエフォートで行う。
///
/// 呼び出し時点でGit操作自体は既に成功している。履歴ファイル(.git内)の書き込みが
/// ディスク満杯やファイルロック（Windowsの同期/アンチウイルス等）で失敗しても、
/// 操作を「失敗」扱いにはしない（再実行による二次事故を避けるため）。
/// この場合、その操作のワンクリックUndoだけが使えなくなる。
fn record_undo(repo: &Repository, entry: UndoEntry) {
    let _ = undo::push(repo, entry);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{log, status};
    use crate::test_support::TestRepo;
    use crate::undo::undo_last;

    #[test]
    fn stage_and_commit_then_undo_restores_changes() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "hello");

        let repo = fx.open();
        stage_all(&repo).unwrap();
        let info = commit(&repo, "最初のコミット").unwrap();
        assert_eq!(info.summary, "最初のコミット");
        assert_eq!(log(&repo, 10).unwrap().len(), 1);

        // Undo: 最初のコミットを取り消すと未誕生に戻り、変更はステージに残る。
        let desc = undo_last(&repo).unwrap();
        assert!(desc.contains("最初のコミット"));
        let repo = fx.open();
        assert_eq!(log(&repo, 10).unwrap().len(), 0);
        // 変更内容は失われていない。
        let st = status(&repo).unwrap();
        assert!(!st.is_clean);
    }

    #[test]
    fn second_commit_undo_keeps_changes_staged() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        fx.write_file("a.txt", "2");
        stage_all(&repo).unwrap();
        commit(&repo, "c2").unwrap();
        assert_eq!(log(&repo, 10).unwrap().len(), 2);

        undo_last(&repo).unwrap();
        let repo = fx.open();
        assert_eq!(log(&repo, 10).unwrap().len(), 1);
        // soft reset なので変更はステージに残る。
        let st = status(&repo).unwrap();
        assert_eq!(st.staged.len(), 1);
    }

    /// undo ジャーナルの tmp 書き込み先（`noobgit_undo.json.tmp`）にディレクトリを
    /// 作っておくと、`fs::write` が「ディレクトリには書き込めない」で必ず失敗する。
    /// これはパーミッションではなくファイルシステムの制約なので、root で実行される
    /// このコンテナでも、Windows でも確実に失敗する（chmod と違って環境に左右されない）。
    fn make_undo_journal_write_fail(repo: &Repository) {
        let tmp = repo.path().join("noobgit_undo.json.tmp");
        std::fs::create_dir_all(&tmp).unwrap();
    }

    // CLAUDE.md: 「undo の記録（record_undo）は、根底の Git 操作を絶対に失敗させては
    // ならない」。ジャーナル書き込みが実際に失敗する状況を作ったうえで、それでも
    // commit が成功しリポジトリ状態が変わることを検証する（常に通るだけの
    // テストにしないため、undo::push を直接呼んで失敗パスに到達することも確認する）。
    #[test]
    fn commit_succeeds_even_if_undo_journal_write_fails() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        fx.write_file("a.txt", "2");
        let repo = fx.open();
        stage_all(&repo).unwrap();

        make_undo_journal_write_fail(&repo);

        // 前提の確認: この状態で undo::push を直接呼ぶと本当に失敗する
        // （＝以降の commit で失敗パスに実際に到達することの裏付け）。
        let probe = undo::push(
            &repo,
            UndoEntry {
                op: OperationKind::Commit,
                description: "probe".into(),
                action: UndoAction::SoftResetTo {
                    previous: fx.head_oid().to_string(),
                },
            },
        );
        assert!(
            probe.is_err(),
            "テスト前提が崩れている: この状態では journal 書き込みが失敗するはずだった"
        );
        // ジャーナル本体（rename 先）はまだ作られていない。
        assert!(!repo.path().join("noobgit_undo.json").exists());

        // ジャーナルへの記録が失敗しても、コミット自体は成功しリポジトリ状態は変わる。
        let info = commit(&repo, "c2").unwrap();
        assert_eq!(info.summary, "c2");
        assert_eq!(log(&repo, 10).unwrap().len(), 2);

        // 記録は失敗し続けているので、ジャーナルは依然として作られていない
        // （＝ record_undo が失敗を握りつぶしていることの確認）。
        assert!(!repo.path().join("noobgit_undo.json").exists());
    }

    // create_branch でも同様に、undo 記録の失敗が操作の成功を妨げないこと。
    #[test]
    fn create_branch_succeeds_even_if_undo_journal_write_fails() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        make_undo_journal_write_fail(&repo);

        let probe = undo::push(
            &repo,
            UndoEntry {
                op: OperationKind::CreateBranch,
                description: "probe".into(),
                action: UndoAction::DeleteBranch {
                    name: "probe-branch".into(),
                },
            },
        );
        assert!(
            probe.is_err(),
            "テスト前提が崩れている: この状態では journal 書き込みが失敗するはずだった"
        );
        assert!(!repo.path().join("noobgit_undo.json").exists());

        create_branch(&repo, "feature").unwrap();
        assert!(repo.find_branch("feature", BranchType::Local).is_ok());
        assert!(!repo.path().join("noobgit_undo.json").exists());
    }

    #[test]
    fn empty_commit_is_rejected() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        let err = commit(&repo, "empty").unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput(_)));
    }

    #[test]
    fn empty_message_is_rejected() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        let repo = fx.open();
        stage_all(&repo).unwrap();
        assert!(matches!(
            commit(&repo, "   ").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
    }

    #[test]
    fn create_switch_and_delete_branch_with_undo() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        create_branch(&repo, "feature").unwrap();
        switch_branch(&repo, "feature").unwrap();
        assert_eq!(
            crate::repo::current_branch(&repo).as_deref(),
            Some("feature")
        );

        // feature にいる間は feature を削除できない。
        assert!(matches!(
            delete_branch(&repo, "feature").unwrap_err(),
            CoreError::Blocked(_)
        ));

        switch_branch(&repo, "main").unwrap();
        delete_branch(&repo, "feature").unwrap();
        assert!(repo.find_branch("feature", BranchType::Local).is_err());

        // Undo で feature を復元。
        undo_last(&repo).unwrap();
        assert!(repo.find_branch("feature", BranchType::Local).is_ok());
    }

    // Issue #269: マージ済みブランチの一括削除。
    #[test]
    fn delete_branches_deletes_only_merged_and_records_individual_undo() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1"); // main: c1

        let repo = fx.open();
        // merged1 / merged2 は c1 のまま据え置く（main が進むので取り込み済みになる）。
        create_branch(&repo, "merged1").unwrap();
        create_branch(&repo, "merged2").unwrap();
        // feature は独自コミットを持たせて未取り込みにする。
        create_branch(&repo, "feature").unwrap();

        switch_branch(&repo, "feature").unwrap();
        fx.write_file("b.txt", "x");
        fx.stage_all();
        fx.commit("feature-c2");

        let repo = fx.open();
        switch_branch(&repo, "main").unwrap();
        fx.write_file("a.txt", "2");
        fx.stage_all();
        fx.commit("main-c2"); // main: c1 -> main-c2

        let repo = fx.open();
        // "feature"（未マージ）と "main"（保護ブランチ・現在ブランチ）を混ぜて渡しても、
        // core 側の再検証でスキップされ、削除されるのは merged1 / merged2 だけ。
        let outcome = delete_branches(
            &repo,
            &[
                "merged1".to_string(),
                "merged2".to_string(),
                "feature".to_string(),
                "main".to_string(),
            ],
            &[],
        )
        .unwrap();

        let mut deleted = outcome.deleted.clone();
        deleted.sort();
        assert_eq!(deleted, vec!["merged1".to_string(), "merged2".to_string()]);

        let skipped_names: Vec<&str> = outcome.skipped.iter().map(|s| s.name.as_str()).collect();
        assert!(skipped_names.contains(&"feature"));
        assert!(skipped_names.contains(&"main"));
        assert!(outcome.skipped.iter().all(|s| !s.reason.is_empty()));

        // 実際にブランチが消えている。
        assert!(repo.find_branch("merged1", BranchType::Local).is_err());
        assert!(repo.find_branch("merged2", BranchType::Local).is_err());
        // 未マージ・保護ブランチは手を付けられていない。
        assert!(repo.find_branch("feature", BranchType::Local).is_ok());
        assert!(repo.find_branch("main", BranchType::Local).is_ok());

        // 削除した2件は、それぞれ個別に undo できる（1回の undo_last で1件だけ戻る）。
        let desc1 = undo_last(&repo).unwrap();
        let restored_after_first = ["merged1", "merged2"]
            .iter()
            .filter(|n| repo.find_branch(n, BranchType::Local).is_ok())
            .count();
        assert_eq!(
            restored_after_first, 1,
            "1回目の undo で2件のうち1件だけ復元されるはず: {desc1}"
        );

        let desc2 = undo_last(&repo).unwrap();
        assert!(repo.find_branch("merged1", BranchType::Local).is_ok());
        assert!(repo.find_branch("merged2", BranchType::Local).is_ok());
        assert_ne!(desc1, desc2);
    }

    #[test]
    fn delete_branches_with_empty_candidate_list_skips_everything() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        create_branch(&repo, "feature").unwrap();

        // feature はまだ main に取り込まれていない独自コミットが無い（main と同じ）ため、
        // 実は merged 扱いになりうる。ここでは意図的に未マージにしておく。
        switch_branch(&repo, "feature").unwrap();
        fx.write_file("b.txt", "x");
        fx.stage_all();
        fx.commit("feature-c2");
        switch_branch(&repo, "main").unwrap();

        let repo = fx.open();
        // 存在しないブランチ名を渡してもパニックせず、スキップ扱いになる。
        let outcome = delete_branches(&repo, &["no-such-branch".to_string()], &[]).unwrap();
        assert!(outcome.deleted.is_empty());
        assert_eq!(outcome.skipped.len(), 1);
        assert_eq!(outcome.skipped[0].name, "no-such-branch");
    }

    #[test]
    fn reset_hard_then_undo() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        fx.write_file("a.txt", "2");
        fx.stage_all();
        fx.commit("c2");

        let repo = fx.open();
        assert_eq!(log(&repo, 10).unwrap().len(), 2);
        reset_hard(&repo, "HEAD~1").unwrap();
        assert_eq!(log(&repo, 10).unwrap().len(), 1);

        undo_last(&repo).unwrap();
        let repo = fx.open();
        assert_eq!(log(&repo, 10).unwrap().len(), 2);
    }

    /// upstream をローカルにクローンし、(一時ディレクトリ, クローン先パス) を返す。
    /// クローン先には identity を設定しておく（分岐テストでローカルコミットを作れるように）。
    fn clone_local(upstream: &TestRepo) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let dest = dir.path().join("clone");
        let repo = git2::Repository::clone(upstream.path().to_str().unwrap(), &dest).unwrap();
        let mut cfg = repo.config().unwrap();
        cfg.set_str("user.name", "Clone User").unwrap();
        cfg.set_str("user.email", "clone@example.com").unwrap();
        (dir, dest)
    }

    // --- Issue #267: clone_with_progress / clone_repo のテスト ---

    #[test]
    fn clone_repo_creates_local_copy_of_upstream() {
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "hello");
        upstream.stage_all();
        upstream.commit("最初のコミット");

        let dir = tempfile::TempDir::new().unwrap();
        // まだ存在しないパス（親フォルダは存在する）へクローンする典型ケース。
        let dest = dir.path().join("cloned-repo");

        let outcome = clone_repo(upstream.path().to_str().unwrap(), &dest).unwrap();
        assert_eq!(outcome.path, dest.to_string_lossy());

        let repo = git2::Repository::open(&dest).unwrap();
        assert_eq!(log(&repo, 10).unwrap().len(), 1);
        assert!(dest.join("a.txt").exists());
        // クローン直後は作業ツリーがきれいな状態。
        assert!(status(&repo).unwrap().is_clean);
    }

    #[test]
    fn clone_into_existing_empty_directory_succeeds() {
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "hello");
        upstream.stage_all();
        upstream.commit("c1");

        let dir = tempfile::TempDir::new().unwrap();
        let dest = dir.path().join("empty-dest");
        std::fs::create_dir(&dest).unwrap();

        let outcome = clone_repo(upstream.path().to_str().unwrap(), &dest).unwrap();
        assert_eq!(outcome.path, dest.to_string_lossy());
        assert!(dest.join("a.txt").exists());
    }

    #[test]
    fn clone_rejects_relative_destination() {
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "hello");
        upstream.stage_all();
        upstream.commit("c1");

        let dest = Path::new("relative-dest-should-not-be-created");
        let err = clone_repo(upstream.path().to_str().unwrap(), dest).unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput(_)));
        assert!(!dest.exists());
    }

    #[test]
    fn clone_rejects_nonempty_existing_destination_and_keeps_existing_files() {
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "hello");
        upstream.stage_all();
        upstream.commit("c1");

        let dir = tempfile::TempDir::new().unwrap();
        let dest = dir.path().join("occupied");
        std::fs::create_dir(&dest).unwrap();
        // 既存の（無関係な）ファイルを置いておく。
        std::fs::write(dest.join("keep-me.txt"), "大事なファイル").unwrap();

        let err = clone_repo(upstream.path().to_str().unwrap(), &dest).unwrap_err();
        assert!(matches!(err, CoreError::Blocked(_)));

        // 既存ファイルは一切触れられていないはず。
        assert!(dest.join("keep-me.txt").exists());
        assert_eq!(
            std::fs::read_to_string(dest.join("keep-me.txt")).unwrap(),
            "大事なファイル"
        );
        // クローンは行われていない（.git が作られていない）。
        assert!(!dest.join(".git").exists());
    }

    #[test]
    fn clone_failure_removes_newly_created_directory() {
        let dir = tempfile::TempDir::new().unwrap();
        // 呼び出し前は存在しないパス。存在しないローカルパスを URL として渡すと、
        // クローンはネットワーク/Git レベルで失敗する。
        let dest = dir.path().join("will-not-exist");
        let bogus_source = dir.path().join("no-such-upstream-repo");

        let err = clone_repo(bogus_source.to_str().unwrap(), &dest).unwrap_err();
        assert!(matches!(
            err,
            CoreError::Git(_) | CoreError::InvalidInput(_)
        ));
        // 今回のクローンのために作ったディレクトリは残らない。
        assert!(!dest.exists());
    }

    #[test]
    fn clone_failure_keeps_preexisting_empty_directory_but_cleans_its_contents() {
        let dir = tempfile::TempDir::new().unwrap();
        let dest = dir.path().join("preexisting-empty");
        std::fs::create_dir(&dest).unwrap();
        let bogus_source = dir.path().join("no-such-upstream-repo");

        let err = clone_repo(bogus_source.to_str().unwrap(), &dest).unwrap_err();
        assert!(matches!(
            err,
            CoreError::Git(_) | CoreError::InvalidInput(_)
        ));
        // 呼び出し前から存在していたディレクトリ自体は消さない。
        assert!(dest.exists());
        assert!(dest.is_dir());
    }

    #[test]
    fn clone_rejects_empty_or_invalid_url() {
        let dir = tempfile::TempDir::new().unwrap();
        let dest = dir.path().join("dest");

        assert!(matches!(
            clone_repo("", &dest).unwrap_err(),
            CoreError::InvalidInput(_)
        ));
        assert!(matches!(
            clone_repo("   ", &dest).unwrap_err(),
            CoreError::InvalidInput(_)
        ));
        assert!(matches!(
            clone_repo("not a url with spaces", &dest).unwrap_err(),
            CoreError::InvalidInput(_)
        ));
        assert!(matches!(
            clone_repo("clearlynotaurl", &dest).unwrap_err(),
            CoreError::InvalidInput(_)
        ));
        // 入力エラーでは、そもそも保存先に触れない。
        assert!(!dest.exists());
    }

    /// #167 進捗フィードバック基盤の再利用: clone_with_progress も fetch と同様、
    /// 「接続待ち」を最初に必ず通知する。
    #[test]
    fn clone_with_progress_reports_connecting_first() {
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "1");
        upstream.stage_all();
        upstream.commit("c1");

        let dir = tempfile::TempDir::new().unwrap();
        let dest = dir.path().join("progress-dest");

        let mut events: Vec<NetworkProgress> = Vec::new();
        clone_with_progress(upstream.path().to_str().unwrap(), &dest, &mut |p| {
            events.push(p)
        })
        .unwrap();

        assert!(!events.is_empty());
        assert_eq!(events[0].stage, NetworkProgressStage::Connecting);
    }

    #[test]
    fn fetch_updates_remote_tracking_branch() {
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "1");
        upstream.stage_all();
        upstream.commit("c1");

        let (_keep, local_path) = clone_local(&upstream);

        // クローン直後はリモートと同じなので、再 fetch しても更新は 0 件。
        let repo = git2::Repository::open(&local_path).unwrap();
        assert_eq!(fetch(&repo, "origin").unwrap().updated_refs, 0);

        // upstream を進める。
        upstream.write_file("a.txt", "2");
        upstream.stage_all();
        upstream.commit("c2");

        let outcome = fetch(&repo, "origin").unwrap();
        assert_eq!(outcome.remote, "origin");
        // origin/main が 1 件前進する。
        assert_eq!(outcome.updated_refs, 1);
        // リモート追跡ブランチが upstream の先端まで更新されている。
        let tracking = repo
            .find_reference("refs/remotes/origin/main")
            .unwrap()
            .peel_to_commit()
            .unwrap();
        assert_eq!(tracking.id(), upstream.head_oid());
        // 作業ツリーは変わっていない（安全操作）。
        assert!(status(&repo).unwrap().is_clean);
    }

    /// #268 fetch のプルーニング: リモートでブランチが削除されたら、次の fetch で
    /// 対応する追跡ブランチ（`refs/remotes/origin/...`）が整理され、`pruned` に入る。
    /// 同名のローカルブランチ本体は一切削除されない。
    #[test]
    fn fetch_prunes_deleted_remote_tracking_branch_but_keeps_local_branch() {
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "1");
        upstream.stage_all();
        upstream.commit("c1");

        // upstream 側に feature ブランチを作る。
        {
            let repo = upstream.open();
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("feature", &head, false).unwrap();
        }

        let (_keep, local_path) = clone_local(&upstream);
        let repo = git2::Repository::open(&local_path).unwrap();

        // 最初の fetch で origin/feature が現れる。プルーニングは何も起きない。
        let outcome = fetch(&repo, "origin").unwrap();
        assert!(outcome.pruned.is_empty());
        assert!(repo.find_reference("refs/remotes/origin/feature").is_ok());

        // ローカルにも同名のブランチを作り、upstream として origin/feature を設定する
        // （switch_branch 相当のシナリオを避けて直接 git2 で組み立てる）。
        {
            let tracking_oid = repo
                .find_reference("refs/remotes/origin/feature")
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .id();
            let commit = repo.find_commit(tracking_oid).unwrap();
            let mut local_branch = repo.branch("feature", &commit, false).unwrap();
            local_branch.set_upstream(Some("origin/feature")).unwrap();
        }

        // upstream 側で feature ブランチを削除する。
        {
            let repo = upstream.open();
            let mut b = repo.find_branch("feature", BranchType::Local).unwrap();
            b.delete().unwrap();
        }

        // 再 fetch すると origin/feature の追跡ブランチが整理（prune）される。
        let outcome = fetch(&repo, "origin").unwrap();
        assert_eq!(outcome.pruned, vec!["origin/feature".to_string()]);
        assert!(repo.find_reference("refs/remotes/origin/feature").is_err());

        // ローカルの feature ブランチ本体は残っている（削除されない）。
        assert!(repo.find_branch("feature", BranchType::Local).is_ok());
    }

    /// #167 進捗フィードバック: fetch_with_progress が「接続待ち」を最初に必ず通知し、
    /// 通信中も進捗コールバックを呼ぶこと。
    #[test]
    fn fetch_with_progress_reports_connecting_first() {
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "1");
        upstream.stage_all();
        upstream.commit("c1");

        let (_keep, local_path) = clone_local(&upstream);
        upstream.write_file("a.txt", "2");
        upstream.stage_all();
        upstream.commit("c2");

        let repo = git2::Repository::open(&local_path).unwrap();
        let mut events: Vec<NetworkProgress> = Vec::new();
        let outcome = fetch_with_progress(&repo, "origin", &mut |p| events.push(p)).unwrap();

        assert_eq!(outcome.updated_refs, 1);
        // 最初のイベントは必ず「接続待ち」（オブジェクト数がまだ分からない段階）。
        assert_eq!(
            events.first().unwrap().stage,
            NetworkProgressStage::Connecting
        );
        assert_eq!(events.first().unwrap().total_objects, 0);
    }

    #[test]
    fn fetch_unknown_remote_is_rejected() {
        let fx = TestRepo::new();
        let repo = fx.open();
        assert!(matches!(
            fetch(&repo, "origin").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
    }

    /// remote 名・refspec が空なら入力エラーになる。
    #[test]
    fn push_rejects_empty_arguments() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        assert!(matches!(
            push(&repo, "  ", "refs/heads/main:refs/heads/main", false).unwrap_err(),
            CoreError::InvalidInput(_)
        ));
        assert!(matches!(
            push(&repo, "origin", "   ", false).unwrap_err(),
            CoreError::InvalidInput(_)
        ));
    }

    #[test]
    fn pull_up_to_date_when_nothing_new() {
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "1");
        upstream.stage_all();
        upstream.commit("c1");

        let (_keep, local_path) = clone_local(&upstream);
        let repo = git2::Repository::open(&local_path).unwrap();

        assert!(matches!(
            pull(&repo, "origin", "main").unwrap(),
            PullOutcome::UpToDate
        ));
    }

    #[test]
    fn pull_fast_forwards_working_tree() {
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "1\n");
        upstream.stage_all();
        upstream.commit("c1");

        let (_keep, local_path) = clone_local(&upstream);

        // upstream を 2 コミット進める（ファイル変更 + 新規ファイル）。
        upstream.write_file("a.txt", "2\n");
        upstream.stage_all();
        upstream.commit("c2");
        upstream.write_file("b.txt", "new\n");
        upstream.stage_all();
        upstream.commit("c3");

        let repo = git2::Repository::open(&local_path).unwrap();
        let outcome = pull(&repo, "origin", "main").unwrap();
        assert!(matches!(outcome, PullOutcome::FastForwarded { .. }));

        // 作業ツリーが前進している。
        assert_eq!(
            std::fs::read_to_string(local_path.join("a.txt")).unwrap(),
            "2\n"
        );
        assert!(local_path.join("b.txt").exists());

        // ローカルの現在ブランチが upstream の先端に追いついている。
        let repo = git2::Repository::open(&local_path).unwrap();
        assert_eq!(
            repo.head().unwrap().peel_to_commit().unwrap().id(),
            upstream.head_oid()
        );
        // 取り込み後はクリーンな状態。
        assert!(status(&repo).unwrap().is_clean);
    }

    /// #167 進捗フィードバック: pull_with_progress は内部の fetch 部分の進捗を通知する。
    #[test]
    fn pull_with_progress_reports_fetch_progress() {
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "1\n");
        upstream.stage_all();
        upstream.commit("c1");

        let (_keep, local_path) = clone_local(&upstream);
        upstream.write_file("a.txt", "2\n");
        upstream.stage_all();
        upstream.commit("c2");

        let repo = git2::Repository::open(&local_path).unwrap();
        let mut events: Vec<NetworkProgress> = Vec::new();
        let outcome = pull_with_progress(&repo, "origin", "main", &mut |p| events.push(p)).unwrap();

        assert!(matches!(outcome, PullOutcome::FastForwarded { .. }));
        assert_eq!(
            events.first().unwrap().stage,
            NetworkProgressStage::Connecting
        );
    }

    #[test]
    fn pull_aborts_safely_when_diverged_without_data_loss() {
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "base\n");
        upstream.stage_all();
        upstream.commit("c1");

        let (_keep, local_path) = clone_local(&upstream);

        // upstream 側の変更。
        upstream.write_file("a.txt", "remote\n");
        upstream.stage_all();
        upstream.commit("remote-c2");

        // ローカル側の別の変更（→ 分岐させる）。
        std::fs::write(local_path.join("a.txt"), "local\n").unwrap();
        let repo = git2::Repository::open(&local_path).unwrap();
        stage_all(&repo).unwrap();
        commit(&repo, "local-c2").unwrap();
        let local_before = repo.head().unwrap().peel_to_commit().unwrap().id();

        // FF できないので安全に中断する。
        let err = pull(&repo, "origin", "main").unwrap_err();
        assert!(matches!(err, CoreError::Blocked(_)));

        // データ消失なし: ローカルの先端も作業ツリーも変わっていない。
        let repo = git2::Repository::open(&local_path).unwrap();
        assert_eq!(
            repo.head().unwrap().peel_to_commit().unwrap().id(),
            local_before
        );
        assert_eq!(
            std::fs::read_to_string(local_path.join("a.txt")).unwrap(),
            "local\n"
        );
    }

    #[test]
    fn pull_into_unborn_branch_checks_out_files() {
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "hello\n");
        upstream.stage_all();
        upstream.commit("c1");

        // ローカルはコミット0件（未誕生 main）。origin を upstream に向ける。
        let local = TestRepo::new();
        let repo = local.open();
        repo.remote("origin", upstream.path().to_str().unwrap())
            .unwrap();

        // pull で未誕生ブランチへ取り込む（FF 相当）。
        let outcome = pull(&repo, "origin", "main").unwrap();
        assert!(matches!(outcome, PullOutcome::FastForwarded { .. }));

        // 作業ツリーにファイルが展開され、main が誕生して upstream に追いついている。
        assert_eq!(
            std::fs::read_to_string(local.path().join("a.txt")).unwrap(),
            "hello\n"
        );
        let repo = local.open();
        assert_eq!(
            repo.head().unwrap().peel_to_commit().unwrap().id(),
            upstream.head_oid()
        );
        assert!(status(&repo).unwrap().is_clean);
    }

    #[test]
    fn pull_unknown_branch_is_rejected() {
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "1");
        upstream.stage_all();
        upstream.commit("c1");

        let (_keep, local_path) = clone_local(&upstream);
        let repo = git2::Repository::open(&local_path).unwrap();

        assert!(matches!(
            pull(&repo, "origin", "no-such-branch").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
    }

    /// 設定されていないリモートへの push は入力エラーで案内する。
    #[test]
    fn push_to_unknown_remote_errors() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        let err = push(&repo, "origin", "refs/heads/main:refs/heads/main", false).unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput(_)));
    }

    /// 通常 push がローカルのベアリポジトリ（remote）に反映される。
    #[test]
    fn push_updates_remote_ref() {
        let bare = TestRepo::new_bare();
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        let oid = fx.commit("c1");
        fx.add_remote("origin", bare.path().to_str().unwrap());

        let repo = fx.open();
        push(&repo, "origin", "refs/heads/main:refs/heads/main", false).unwrap();

        let bare_repo = bare.open();
        assert_eq!(bare_repo.refname_to_id("refs/heads/main").unwrap(), oid);
    }

    /// #167 進捗フィードバック: push_with_progress が「接続待ち」を最初に通知し、
    /// 通常の push と同じくリモートに反映されること。
    #[test]
    fn push_with_progress_reports_connecting_first() {
        let bare = TestRepo::new_bare();
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        let oid = fx.commit("c1");
        fx.add_remote("origin", bare.path().to_str().unwrap());

        let repo = fx.open();
        let mut events: Vec<NetworkProgress> = Vec::new();
        push_with_progress(
            &repo,
            "origin",
            "refs/heads/main:refs/heads/main",
            false,
            &mut |p| events.push(p),
        )
        .unwrap();

        let bare_repo = bare.open();
        assert_eq!(bare_repo.refname_to_id("refs/heads/main").unwrap(), oid);
        assert_eq!(
            events.first().unwrap().stage,
            NetworkProgressStage::Connecting
        );
    }

    /// 非fast-forward の push は拒否され、日本語のブロックエラーになる。
    #[test]
    fn non_fast_forward_push_is_rejected() {
        let bare = TestRepo::new_bare();

        // 1人目: c1 を push して remote/main = c1 にする。
        let a = TestRepo::new();
        a.write_file("a.txt", "1");
        a.stage_all();
        a.commit("c1");
        a.add_remote("origin", bare.path().to_str().unwrap());
        push(
            &a.open(),
            "origin",
            "refs/heads/main:refs/heads/main",
            false,
        )
        .unwrap();

        // 2人目: remote を知らずに独自の d1 を作る → 非fast-forward。
        let b = TestRepo::new();
        b.write_file("b.txt", "x");
        b.stage_all();
        b.commit("d1");
        b.add_remote("origin", bare.path().to_str().unwrap());
        let err = push(
            &b.open(),
            "origin",
            "refs/heads/main:refs/heads/main",
            false,
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::Blocked(_)));

        // remote は c1 のままで上書きされていない。
        let bare_repo = bare.open();
        assert_eq!(
            bare_repo.refname_to_id("refs/heads/main").unwrap(),
            a.head_oid()
        );
    }

    /// 強制 push はリモートの履歴を上書きできる。
    #[test]
    fn force_push_overwrites_remote() {
        let bare = TestRepo::new_bare();

        let a = TestRepo::new();
        a.write_file("a.txt", "1");
        a.stage_all();
        a.commit("c1");
        a.add_remote("origin", bare.path().to_str().unwrap());
        push(
            &a.open(),
            "origin",
            "refs/heads/main:refs/heads/main",
            false,
        )
        .unwrap();

        let b = TestRepo::new();
        b.write_file("b.txt", "x");
        b.stage_all();
        let d1 = b.commit("d1");
        b.add_remote("origin", bare.path().to_str().unwrap());
        // force=true なら非fast-forward でも上書きできる。
        push(&b.open(), "origin", "refs/heads/main:refs/heads/main", true).unwrap();

        let bare_repo = bare.open();
        assert_eq!(bare_repo.refname_to_id("refs/heads/main").unwrap(), d1);
    }

    #[test]
    fn amend_changes_message_without_adding_commit() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("typo mesage");

        let repo = fx.open();
        let info = amend_commit(&repo, "fixed message").unwrap();
        assert_eq!(info.summary, "fixed message");
        // 履歴を書き換えただけなのでコミット数は増えない。
        assert_eq!(log(&repo, 10).unwrap().len(), 1);
    }

    #[test]
    fn amend_incorporates_staged_then_undo_restores_previous_commit() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        let original = fx.head_oid();

        // 入れ忘れたファイルをステージし、メッセージは空（もとのまま）で amend する。
        fx.write_file("b.txt", "new");
        let repo = fx.open();
        stage_all(&repo).unwrap();
        let info = amend_commit(&repo, "").unwrap();
        assert_eq!(info.summary, "c1"); // メッセージは引き継がれる
        assert_ne!(info.id, original.to_string()); // 別のコミットになっている

        // amend 後のコミットに b.txt が含まれている。
        let repo = fx.open();
        let tree = repo.head().unwrap().peel_to_tree().unwrap();
        assert!(tree.get_name("b.txt").is_some());

        // Undo で amend 前のコミットに戻る（変更はステージに残る）。
        undo_last(&repo).unwrap();
        let repo = fx.open();
        assert_eq!(repo.head().unwrap().target().unwrap(), original);
        assert_eq!(log(&repo, 10).unwrap().len(), 1);
        assert_eq!(status(&repo).unwrap().staged.len(), 1);
    }

    #[test]
    fn squash_combines_commits_and_undo_restores() {
        let fx = TestRepo::new();
        // 連続する3コミットを作る（c1 → c2 → c3）。
        fx.write_file("a.txt", "1\n");
        fx.stage_all();
        fx.commit("c1");
        fx.write_file("a.txt", "2\n");
        fx.stage_all();
        fx.commit("c2");
        fx.write_file("b.txt", "new\n");
        fx.stage_all();
        fx.commit("c3");

        let repo = fx.open();
        assert_eq!(log(&repo, 10).unwrap().len(), 3);
        let head_before = repo.head().unwrap().peel_to_commit().unwrap();
        let c3 = head_before.id();
        let c2 = head_before.parent(0).unwrap().id();
        // まとめ後のツリー内容（= HEAD のツリー）を控える。
        let tree_before = head_before.tree().unwrap().id();

        // 上位2つ（c3, c2）を1つにまとめる。
        squash_commits(&repo, &[&c3.to_string(), &c2.to_string()], "まとめた").unwrap();

        // 履歴は2件（まとめたコミット → c1）に減る。
        let repo = fx.open();
        let logged = log(&repo, 10).unwrap();
        assert_eq!(logged.len(), 2);
        assert_eq!(logged[0].summary, "まとめた");
        // ツリー内容（ファイルの中身）は保たれている。
        let head_after = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head_after.tree().unwrap().id(), tree_before);
        assert_eq!(
            std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
            "2\n"
        );
        assert!(fx.path().join("b.txt").exists());

        // Undo で元の3コミットに戻る。
        undo_last(&repo).unwrap();
        let repo = fx.open();
        assert_eq!(log(&repo, 10).unwrap().len(), 3);
        assert_eq!(repo.head().unwrap().peel_to_commit().unwrap().id(), c3);
    }

    #[test]
    fn squash_rejects_non_contiguous_or_too_few() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1\n");
        fx.stage_all();
        fx.commit("c1");
        fx.write_file("a.txt", "2\n");
        fx.stage_all();
        fx.commit("c2");

        let repo = fx.open();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let c2 = head.id();
        let c1 = head.parent(0).unwrap().id();

        // 1つだけでは squash できない。
        assert!(matches!(
            squash_commits(&repo, &[&c2.to_string()], "x").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
        // 空メッセージは拒否。
        assert!(matches!(
            squash_commits(&repo, &[&c2.to_string(), &c1.to_string()], "  ").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
        // HEAD から連続していない（先頭が HEAD でない）と拒否。
        assert!(matches!(
            squash_commits(&repo, &[&c1.to_string(), &c2.to_string()], "x").unwrap_err(),
            CoreError::Blocked(_)
        ));
    }

    #[test]
    fn reword_changes_message_and_undo_restores() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1\n");
        fx.stage_all();
        fx.commit("c1");
        fx.write_file("a.txt", "2\n");
        fx.stage_all();
        fx.commit("typo");

        let repo = fx.open();
        let before = repo.head().unwrap().peel_to_commit().unwrap();
        let original = before.id();
        let tree_before = before.tree().unwrap().id();

        let info = reword_commit(&repo, "fixed message").unwrap();
        assert_eq!(info.summary, "fixed message");

        // コミット数は変わらず、ツリー内容も保たれる（メッセージだけが変わる）。
        let repo = fx.open();
        assert_eq!(log(&repo, 10).unwrap().len(), 2);
        let after = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(after.tree().unwrap().id(), tree_before);
        assert_ne!(after.id(), original);

        // Undo で書き換え前のコミットに戻る。
        undo_last(&repo).unwrap();
        let repo = fx.open();
        assert_eq!(
            repo.head().unwrap().peel_to_commit().unwrap().id(),
            original
        );
    }

    #[test]
    fn reword_empty_message_is_rejected() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1\n");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        assert!(matches!(
            reword_commit(&repo, "   ").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
    }

    #[test]
    fn amend_without_commit_is_blocked() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        let repo = fx.open();
        stage_all(&repo).unwrap();
        // まだ1件もコミットが無ければ amend できない。
        assert!(matches!(
            amend_commit(&repo, "x").unwrap_err(),
            CoreError::Blocked(_)
        ));
    }

    #[test]
    fn discard_reverts_tracked_modification() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "original\n");
        fx.stage_all();
        fx.commit("c1");

        // ステージ済み・未ステージの両方の変更を作る。
        fx.write_file("a.txt", "changed\n");
        let repo = fx.open();
        stage_all(&repo).unwrap();
        fx.write_file("a.txt", "changed again\n");

        discard_path(&repo, "a.txt").unwrap();

        // 最後にコミットした内容へ戻り、作業ツリーはクリーンになる。
        assert_eq!(
            std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
            "original\n"
        );
        let repo = fx.open();
        assert!(status(&repo).unwrap().is_clean);
    }

    #[test]
    fn discard_deletes_untracked_file() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        fx.write_file("junk.txt", "delete me");
        let repo = fx.open();
        assert!(fx.path().join("junk.txt").exists());

        discard_path(&repo, "junk.txt").unwrap();
        assert!(!fx.path().join("junk.txt").exists());
        let repo = fx.open();
        assert!(status(&repo).unwrap().is_clean);
    }

    #[test]
    fn discard_staged_new_file_removes_from_index_and_disk() {
        // ケース A: `git add` 済みだが HEAD には無い新規ファイル。
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        // 新規ファイルを作ってステージする（index には stage 0 で載るが HEAD には無い）。
        fx.write_file("staged_new.txt", "draft");
        let repo = fx.open();
        stage_all(&repo).unwrap();
        assert_eq!(status(&repo).unwrap().staged.len(), 1);

        discard_path(&repo, "staged_new.txt").unwrap();

        // index からもディスクからも消える。
        assert!(!fx.path().join("staged_new.txt").exists());
        let repo = fx.open();
        let st = status(&repo).unwrap();
        assert!(st.is_clean);
        assert!(repo
            .index()
            .unwrap()
            .get_path(Path::new("staged_new.txt"), 0)
            .is_none());
    }

    #[test]
    fn discard_deleted_file_restores_from_head() {
        // ケース C: HEAD にあるファイルを削除した状態（ChangeKind::Deleted）から復元する。
        let fx = TestRepo::new();
        fx.write_file("keep.txt", "original\n");
        fx.stage_all();
        fx.commit("c1");

        // ファイルを削除し、その削除をステージする（INDEX_DELETED の状態を作る）。
        std::fs::remove_file(fx.path().join("keep.txt")).unwrap();
        let repo = fx.open();
        stage_all(&repo).unwrap();
        assert!(status(&repo)
            .unwrap()
            .staged
            .iter()
            .any(|c| c.path == "keep.txt" && c.kind == crate::model::ChangeKind::Deleted));

        // 破棄すると HEAD の内容へ復元され、作業ツリーはクリーンに戻る。
        discard_path(&repo, "keep.txt").unwrap();
        assert_eq!(
            std::fs::read_to_string(fx.path().join("keep.txt")).unwrap(),
            "original\n"
        );
        let repo = fx.open();
        assert!(status(&repo).unwrap().is_clean);
    }

    #[test]
    fn discard_renamed_file_handles_old_and_new_paths() {
        // ケース D: 名前変更（= 旧パスの削除 + 新パスの追加）の各パスへの破棄。
        // discard_path はリテラルなパスに対して動くので、両パスを独立に検証する。
        let fx = TestRepo::new();
        fx.write_file("old.txt", "content\n");
        fx.stage_all();
        fx.commit("c1");

        // old.txt -> new.txt の名前変更を作ってステージする。
        std::fs::remove_file(fx.path().join("old.txt")).unwrap();
        fx.write_file("new.txt", "content\n");
        let repo = fx.open();
        stage_all(&repo).unwrap();

        // 新パス（HEAD に無い）を破棄: index・ディスクから消える。
        discard_path(&repo, "new.txt").unwrap();
        assert!(!fx.path().join("new.txt").exists());

        // 旧パス（HEAD にある）を破棄: 削除を取り消して HEAD の内容へ復元される。
        let repo = fx.open();
        discard_path(&repo, "old.txt").unwrap();
        assert_eq!(
            std::fs::read_to_string(fx.path().join("old.txt")).unwrap(),
            "content\n"
        );

        // 両パスを破棄した結果、作業ツリーはクリーンに戻る。
        let repo = fx.open();
        assert!(status(&repo).unwrap().is_clean);
    }

    #[test]
    fn discard_rejects_path_traversal() {
        let fx = TestRepo::new();
        let repo = fx.open();
        assert!(matches!(
            discard_path(&repo, "../x.txt").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
        assert!(matches!(
            discard_path(&repo, "/etc/passwd").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
    }

    #[test]
    fn stash_save_cleans_tree_then_pop_restores() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        // 追跡ファイルの変更 + 未追跡ファイル。
        fx.write_file("a.txt", "2");
        fx.write_file("new.txt", "fresh");
        {
            let mut repo = fx.open();
            stash_save(&mut repo, "wip").unwrap();
        }

        // 退避後は作業ツリーがクリーンで、退避が1件ある。
        let repo = fx.open();
        assert!(status(&repo).unwrap().is_clean);
        {
            let mut repo = fx.open();
            let list = stash_list(&mut repo).unwrap();
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].index, 0);
        }

        // pop で変更が戻り、退避一覧が空になる。
        {
            let mut repo = fx.open();
            stash_pop(&mut repo, 0).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
            "2"
        );
        assert!(fx.path().join("new.txt").exists());
        {
            let mut repo = fx.open();
            assert!(stash_list(&mut repo).unwrap().is_empty());
        }
    }

    #[test]
    fn stash_apply_keeps_stash_in_list() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        fx.write_file("a.txt", "2");
        {
            let mut repo = fx.open();
            stash_save(&mut repo, "wip").unwrap();
        }
        {
            let mut repo = fx.open();
            stash_apply(&mut repo, 0).unwrap();
            // apply は退避を一覧に残す。
            assert_eq!(stash_list(&mut repo).unwrap().len(), 1);
        }
        assert_eq!(
            std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
            "2"
        );
    }

    // stash_save が PopStash の undo エントリを記録することを確認する（#73）。
    #[test]
    fn stash_save_records_undo_entry() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        fx.write_file("a.txt", "2");

        {
            let mut repo = fx.open();
            stash_save(&mut repo, "wip").unwrap();
        }

        let repo = fx.open();
        let entry = crate::undo::peek(&repo)
            .unwrap()
            .expect("stash_save は PopStash の undo エントリを記録すること");
        assert!(
            matches!(entry.action, crate::undo::UndoAction::PopStash { .. }),
            "PopStash エントリが記録されていること"
        );
        assert!(
            entry.description.contains("退避"),
            "説明に「退避」を含む日本語メッセージであること: {}",
            entry.description
        );
    }

    // stash_apply が「中身の競合」（作業ツリーはクリーンだが、退避内容と HEAD 側の
    // 変更が同じ箇所を触っている）の場合は、Blocked で中断せず、コンフリクトの目印
    // （<<<<<<< 等）を作業ツリーへ書き込んで成功を返すこと（#156）。
    //
    // これは libgit2 の実際の挙動を確認した結果に基づく: 作業ツリーが index/HEAD と
    // 一致している（＝上書きで消える未コミット変更が無い）限り、たとえ退避内容と
    // HEAD 側の変更が中身で競合していても、checkout はコンフリクトマーカー付きの
    // 内容を書き込んで成功する。`repo.state()` は（マージと違い）`Clean` のまま。
    #[test]
    fn stash_apply_content_conflict_succeeds_and_leaves_index_conflicted() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "base");
        fx.stage_all();
        fx.commit("c1");

        // 退避: base -> stash_change。
        fx.write_file("a.txt", "stash_change");
        {
            let mut repo = fx.open();
            stash_save(&mut repo, "wip").unwrap();
        }
        // 退避後の作業ツリーは HEAD と一致（"base"）。

        // 退避とは別に、HEAD 側でも同じファイルを変更するコミットを積む（中身の競合を作る）。
        fx.write_file("a.txt", "head_change");
        fx.stage_all();
        fx.commit("c2");
        // 作業ツリーはクリーン（HEAD の "head_change" と一致）。

        let outcome = {
            let mut repo = fx.open();
            stash_apply(&mut repo, 0).unwrap()
        };
        assert!(
            outcome.conflicted,
            "中身が競合しているのでコンフリクトになるはず"
        );

        let repo = fx.open();
        assert_eq!(
            repo.state(),
            git2::RepositoryState::Clean,
            "stash には MERGE_HEAD 相当の状態は無く、Clean のままのはず"
        );
        assert!(repo.index().unwrap().has_conflicts());
        // apply は常に退避を一覧に残す。
        let mut repo2 = fx.open();
        assert_eq!(stash_list(&mut repo2).unwrap().len(), 1);
    }

    // stash_pop が「中身の競合」の場合、libgit2 の素の stash_pop と違い、
    // 退避を一覧から取り除かないこと（#156）。
    //
    // libgit2 の `git_stash_pop` は apply が成功したとみなせば（＝Blocked にならなければ、
    // コンフリクトが起きていても）退避を drop してしまう。これは `git stash pop` が
    // コンフリクト時に退避を残す（"The stash entry is kept in case you need it again."）
    // という挙動と食い違うため、noobGit では apply → コンフリクト確認 → 問題なければ
    // drop、という手順に分解して安全側に倒す。
    #[test]
    fn stash_pop_content_conflict_keeps_stash_in_list() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "base");
        fx.stage_all();
        fx.commit("c1");

        fx.write_file("a.txt", "stash_change");
        {
            let mut repo = fx.open();
            stash_save(&mut repo, "wip").unwrap();
        }

        fx.write_file("a.txt", "head_change");
        fx.stage_all();
        fx.commit("c2");

        let outcome = {
            let mut repo = fx.open();
            stash_pop(&mut repo, 0).unwrap()
        };
        assert!(outcome.conflicted);

        let repo = fx.open();
        assert!(repo.index().unwrap().has_conflicts());
        let mut repo2 = fx.open();
        assert_eq!(
            stash_list(&mut repo2).unwrap().len(),
            1,
            "コンフリクト時は退避を一覧から取り除かないこと"
        );
    }

    // ---- switch_branch_with_stash（#198） ----

    /// main 相当（初期ブランチ）に a.txt / b.txt を持つコミットを作り、`other` ブランチで
    /// a.txt だけを変更したコミットを積んだうえで、初期ブランチに戻った状態を作る。
    /// 戻り値は初期ブランチ名。
    fn setup_two_branches(fx: &TestRepo) -> String {
        fx.write_file("a.txt", "base");
        fx.write_file("b.txt", "base-b");
        fx.stage_all();
        fx.commit("c1");
        let orig = current_branch(&fx.open()).unwrap();
        create_branch(&fx.open(), "other").unwrap();
        switch_branch(&fx.open(), "other").unwrap();
        fx.write_file("a.txt", "other-change");
        fx.stage_all();
        fx.commit("c2 on other");
        switch_branch(&fx.open(), &orig).unwrap();
        orig
    }

    fn read(fx: &TestRepo, rel: &str) -> String {
        std::fs::read_to_string(fx.path().join(rel)).unwrap()
    }

    // 成功パス: 退避 → 切り替え → 復元。変更（追跡・未追跡とも）が切り替え先へ持ち越され、
    // 退避は一覧に残らない。undo は記録しない。
    #[test]
    fn switch_with_stash_success_carries_changes_over() {
        let fx = TestRepo::new();
        let orig = setup_two_branches(&fx);
        fx.write_file("b.txt", "wip-b");
        fx.write_file("new.txt", "untracked");
        let undo_before = crate::undo::list(&fx.open()).unwrap().len();

        let outcome = {
            let mut repo = fx.open();
            switch_branch_with_stash(&mut repo, "other").unwrap()
        };
        assert_eq!(
            outcome,
            SwitchWithStashOutcome {
                stashed: true,
                conflicted: false
            }
        );

        let repo = fx.open();
        assert_eq!(current_branch(&repo).unwrap(), "other");
        assert_eq!(read(&fx, "a.txt"), "other-change");
        assert_eq!(read(&fx, "b.txt"), "wip-b");
        assert_eq!(read(&fx, "new.txt"), "untracked");
        let mut repo = fx.open();
        assert!(stash_list(&mut repo).unwrap().is_empty());
        assert_eq!(crate::undo::list(&repo).unwrap().len(), undo_before);
        // 元のブランチは動いていない。
        assert!(repo.find_branch(&orig, BranchType::Local).is_ok());
    }

    // 変更が無ければ退避せず、普通に切り替える。
    #[test]
    fn switch_with_stash_on_clean_tree_just_switches() {
        let fx = TestRepo::new();
        setup_two_branches(&fx);
        let outcome = {
            let mut repo = fx.open();
            switch_branch_with_stash(&mut repo, "other").unwrap()
        };
        assert!(!outcome.stashed && !outcome.conflicted);
        assert_eq!(read(&fx, "a.txt"), "other-change");
        let mut repo = fx.open();
        assert!(stash_list(&mut repo).unwrap().is_empty());
    }

    // pop コンフリクトパス: 素の switch_branch は Blocked になる変更でも、退避して切り替えられる。
    // 戻すときにコンフリクトするが、退避は一覧に残り変更は失われない。
    #[test]
    fn switch_with_stash_conflict_keeps_stash() {
        let fx = TestRepo::new();
        let orig = setup_two_branches(&fx);
        fx.write_file("a.txt", "wip-a");

        // 前提: 素の切り替えは Blocked。
        assert!(matches!(
            switch_branch(&fx.open(), "other"),
            Err(CoreError::Blocked(_))
        ));
        assert_eq!(read(&fx, "a.txt"), "wip-a");

        let outcome = {
            let mut repo = fx.open();
            switch_branch_with_stash(&mut repo, "other").unwrap()
        };
        assert!(outcome.stashed && outcome.conflicted);

        let mut repo = fx.open();
        assert_eq!(current_branch(&repo).unwrap(), "other");
        assert!(repo.index().unwrap().has_conflicts());
        let stashes = stash_list(&mut repo).unwrap();
        assert_eq!(stashes.len(), 1, "コンフリクト時は退避を残すこと");
        // 自動命名（退避メッセージに元ブランチ名と先ブランチ名が入る）。
        assert!(
            stashes[0]
                .message
                .contains(&format!("{orig}からotherへの切り替え時に自動退避")),
            "message = {}",
            stashes[0].message
        );
        // 作業ツリーには自分の変更がコンフリクトの目印付きで残っている。
        assert!(read(&fx, "a.txt").contains("wip-a"));
    }

    // 途中失敗のロールバック: 切り替えが失敗しても、元のブランチのまま未コミット変更
    // （追跡・未追跡）が作業ツリーに戻り、退避も残らない。元のエラーがそのまま返る。
    #[test]
    fn switch_with_stash_rolls_back_when_switch_fails() {
        let fx = TestRepo::new();
        let orig = setup_two_branches(&fx);
        fx.write_file("a.txt", "wip-a");
        fx.write_file("new.txt", "untracked");

        let err = {
            let mut repo = fx.open();
            switch_with_stash_impl(&mut repo, "other", |_r, _n| {
                Err(CoreError::Blocked("テスト用の切り替え失敗".to_string()))
            })
            .unwrap_err()
        };
        assert!(matches!(err, CoreError::Blocked(ref m) if m == "テスト用の切り替え失敗"));

        let mut repo = fx.open();
        assert_eq!(current_branch(&repo).unwrap(), orig);
        assert_eq!(read(&fx, "a.txt"), "wip-a");
        assert_eq!(read(&fx, "new.txt"), "untracked");
        assert!(stash_list(&mut repo).unwrap().is_empty());
    }

    // ロールバックで戻す処理自体がコンフリクトしても、変更は退避一覧に残って失われない。
    #[test]
    fn switch_with_stash_rollback_failure_keeps_changes_in_stash() {
        let fx = TestRepo::new();
        let orig = setup_two_branches(&fx);
        fx.write_file("a.txt", "wip-a");

        let err = {
            let mut repo = fx.open();
            // 「切り替え」の途中で HEAD 側のファイルが変わってしまった状況を模す
            // （戻すときに退避と競合する）。
            switch_with_stash_impl(&mut repo, "other", |r, _n| {
                let mut co = CheckoutBuilder::new();
                co.force();
                let obj = r.revparse_single("refs/heads/other")?;
                r.checkout_tree(&obj, Some(&mut co))?;
                r.set_head("refs/heads/other")?;
                r.set_head(&format!("refs/heads/{orig}"))?;
                // 作業ツリーだけ other の内容になったまま失敗する。
                Err(CoreError::Git("テスト用の中途半端な失敗".to_string()))
            })
            .unwrap_err()
        };
        assert!(err.to_string().contains("テスト用の中途半端な失敗"));

        // 変更は作業ツリーに戻っているか、少なくとも退避に残っている（どちらかは必ず満たす）。
        let mut repo = fx.open();
        let stashed = !stash_list(&mut repo).unwrap().is_empty();
        let in_tree = read(&fx, "a.txt").contains("wip-a");
        assert!(stashed || in_tree, "変更が失われた");
    }

    // 存在しないブランチ: 何も退避せず、未コミット変更もそのまま。
    #[test]
    fn switch_with_stash_unknown_branch_changes_nothing() {
        let fx = TestRepo::new();
        setup_two_branches(&fx);
        fx.write_file("a.txt", "wip-a");
        let err = {
            let mut repo = fx.open();
            switch_branch_with_stash(&mut repo, "nope").unwrap_err()
        };
        assert!(matches!(err, CoreError::InvalidInput(_)));
        assert_eq!(read(&fx, "a.txt"), "wip-a");
        let mut repo = fx.open();
        assert!(stash_list(&mut repo).unwrap().is_empty());
    }

    // stash_pop がコンフリクトなく成功する通常時は、従来どおり退避を一覧から
    // 取り除くこと（#156 のリグレッション防止）。
    #[test]
    fn stash_pop_without_conflict_drops_stash_and_reports_not_conflicted() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        fx.write_file("a.txt", "2");
        {
            let mut repo = fx.open();
            stash_save(&mut repo, "wip").unwrap();
        }

        let outcome = {
            let mut repo = fx.open();
            stash_pop(&mut repo, 0).unwrap()
        };
        assert!(!outcome.conflicted);
        assert_eq!(
            std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
            "2"
        );
        let mut repo = fx.open();
        assert!(stash_list(&mut repo).unwrap().is_empty());
    }

    // stash_apply がコンフリクト時にエラーを返し、作業ツリーの状態を保全すること（#73）。
    #[test]
    fn stash_apply_conflict_returns_error() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "base");
        fx.stage_all();
        fx.commit("c1");

        // a.txt = "stash_change" を退避する。
        fx.write_file("a.txt", "stash_change");
        {
            let mut repo = fx.open();
            stash_save(&mut repo, "wip").unwrap();
        }
        // 退避後の作業ツリーは a.txt = "base"（コミット状態）。

        // コンフリクトを起こす変更を作業ツリーに加える（ステージしない）。
        fx.write_file("a.txt", "local_change");

        // stash_apply は競合でエラーになる。
        {
            let mut repo = fx.open();
            let err = stash_apply(&mut repo, 0).unwrap_err();
            assert!(
                matches!(err, crate::error::CoreError::Blocked(_)),
                "コンフリクト時は Blocked エラーになること: {err:?}"
            );
        }

        // 作業ツリーの状態が保全されていること（a.txt は "local_change" のまま）。
        assert_eq!(
            std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
            "local_change"
        );
    }

    #[test]
    fn stash_save_with_clean_tree_is_blocked() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let mut repo = fx.open();
        // クリーンな状態では退避できる変更が無い。
        assert!(matches!(
            stash_save(&mut repo, "x").unwrap_err(),
            CoreError::Blocked(_)
        ));
    }

    // 名前付きで退避すると、そのメッセージが一覧に反映されること（#110）。
    #[test]
    fn stash_save_with_message_is_listed() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        fx.write_file("a.txt", "2");

        let mut repo = fx.open();
        stash_save(&mut repo, "作業中の覚え書き").unwrap();

        let list = stash_list(&mut repo).unwrap();
        assert_eq!(list.len(), 1);
        assert!(
            list[0].message.contains("作業中の覚え書き"),
            "メッセージに名前が反映されること: {}",
            list[0].message
        );
    }

    // 空メッセージのときは git の自動メッセージ（WIP on ...）にフォールバックすること（#110）。
    #[test]
    fn stash_save_empty_message_uses_auto_name() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        fx.write_file("a.txt", "2");

        let mut repo = fx.open();
        stash_save(&mut repo, "").unwrap();

        let list = stash_list(&mut repo).unwrap();
        assert_eq!(list.len(), 1);
        // libgit2 の自動メッセージは "WIP on <branch>: ..." の形になる。
        assert!(
            list[0].message.contains("WIP on") || list[0].message.contains("On "),
            "自動命名のメッセージになること: {}",
            list[0].message
        );
    }

    // stash_list が各退避の変更ファイル数を正しく数えること（#110）。
    #[test]
    fn stash_list_reports_file_count() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        // 追跡ファイルの変更 + 未追跡ファイルの追加 → 2 ファイル。
        fx.write_file("a.txt", "2");
        fx.write_file("new.txt", "fresh");

        let mut repo = fx.open();
        stash_save(&mut repo, "wip").unwrap();

        let list = stash_list(&mut repo).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].file_count, 2);
    }

    // stash_diff が変更ファイル一覧（パスと変更種別）を返し、退避を適用しないこと（#110）。
    #[test]
    fn stash_diff_returns_changed_files_without_applying() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "original\n");
        fx.write_file("gone.txt", "delete me\n");
        fx.stage_all();
        fx.commit("c1");

        // a.txt を変更、gone.txt を削除、new.txt を新規追加。
        fx.write_file("a.txt", "changed\n");
        std::fs::remove_file(fx.path().join("gone.txt")).unwrap();
        fx.write_file("new.txt", "fresh\n");

        {
            let mut repo = fx.open();
            stash_save(&mut repo, "wip").unwrap();
        }

        // 退避後は作業ツリーがクリーンであること（diff は適用しない前提）。
        {
            let repo = fx.open();
            assert!(status(&repo).unwrap().is_clean);
        }

        let mut repo = fx.open();
        let mut changes = stash_diff(&mut repo, 0).unwrap();
        changes.sort_by(|x, y| x.path.cmp(&y.path));

        assert_eq!(changes.len(), 3);
        let find = |p: &str| changes.iter().find(|c| c.path == p).map(|c| c.kind);
        assert_eq!(find("a.txt"), Some(crate::model::ChangeKind::Modified));
        assert_eq!(find("gone.txt"), Some(crate::model::ChangeKind::Deleted));
        assert_eq!(find("new.txt"), Some(crate::model::ChangeKind::Added));

        // stash_diff を呼んでも退避は一覧に残り、作業ツリーは変わらない。
        assert_eq!(stash_list(&mut repo).unwrap().len(), 1);
        assert!(status(&repo).unwrap().is_clean);
    }

    // 存在しない index への stash_diff は入力エラーになること（#110）。
    #[test]
    fn stash_diff_unknown_index_is_rejected() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let mut repo = fx.open();
        assert!(matches!(
            stash_diff(&mut repo, 0).unwrap_err(),
            CoreError::InvalidInput(_)
        ));
    }

    // stash_drop が退避を一覧から取り除くこと（#156: stash_pop がコンフリクトで
    // 退避を残したあと、ユーザーが手動で削除する用途を想定）。
    #[test]
    fn stash_drop_removes_stash_from_list() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        fx.write_file("a.txt", "2");
        {
            let mut repo = fx.open();
            stash_save(&mut repo, "wip").unwrap();
        }

        let mut repo = fx.open();
        let list = stash_list(&mut repo).unwrap();
        assert_eq!(list.len(), 1);
        stash_drop(&mut repo, &list[0].id).unwrap();
        assert!(stash_list(&mut repo).unwrap().is_empty());
    }

    // 番号がずれても（あとから別の退避が作られても）、ID で指定した退避だけを消すこと。
    #[test]
    fn stash_drop_by_id_is_not_confused_by_shifted_index() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        fx.write_file("a.txt", "first");
        let first_id = {
            let mut repo = fx.open();
            stash_save(&mut repo, "first").unwrap();
            stash_list(&mut repo).unwrap()[0].id.clone()
        };
        // 新しい退避を作ると、first の番号は 0 → 1 にずれる。
        fx.write_file("a.txt", "second");
        {
            let mut repo = fx.open();
            stash_save(&mut repo, "second").unwrap();
        }

        let mut repo = fx.open();
        stash_drop(&mut repo, &first_id).unwrap();
        let rest = stash_list(&mut repo).unwrap();
        assert_eq!(rest.len(), 1);
        assert!(rest[0].message.contains("second"), "{rest:?}");
    }

    // 存在しない ID・不正な ID への stash_drop は入力エラーになること。
    #[test]
    fn stash_drop_unknown_id_is_rejected() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let mut repo = fx.open();
        assert!(matches!(
            stash_drop(&mut repo, "0123456789012345678901234567890123456789").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
        assert!(matches!(
            stash_drop(&mut repo, "not-an-id").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
    }

    // ダーティな状態でのブランチ切り替えが Blocked エラーになること（#96）。
    #[test]
    fn switch_branch_with_dirty_tree_is_blocked() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        // feature を c1 から作成（a.txt = "1"）。
        {
            let repo = fx.open();
            create_branch(&repo, "feature").unwrap();
        }

        // main を進めて feature と分岐させる（a.txt = "2"）。
        fx.write_file("a.txt", "2");
        fx.stage_all();
        fx.commit("c2");

        // 作業ツリーを汚す（未コミット変更）。
        fx.write_file("a.txt", "dirty");

        // feature に切り替えると a.txt を "1" にする必要があるが、
        // 未コミット変更があるため Blocked エラーになる。
        let repo = fx.open();
        let err = switch_branch(&repo, "feature").unwrap_err();
        assert!(matches!(err, CoreError::Blocked(_)));
        let msg = err.to_string();
        assert!(
            msg.contains("未コミット"),
            "日本語メッセージに「未コミット」を含むこと: {msg}"
        );
    }

    // 既存ブランチと同名で作成しようとすると日本語エラーになること（#96）。
    #[test]
    fn create_duplicate_branch_fails_with_japanese_message() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        create_branch(&repo, "feature").unwrap();

        // 同名ブランチを再作成すると InvalidInput エラーになる。
        let err = create_branch(&repo, "feature").unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput(_)));
        let msg = err.to_string();
        assert!(
            msg.contains("すでに存在します") || msg.contains("feature"),
            "日本語エラーメッセージにブランチ名を含むこと: {msg}"
        );
    }

    // 現在チェックアウト中のブランチ削除が日本語メッセージ付きで Blocked になること（#96）。
    #[test]
    fn delete_current_branch_is_blocked_with_japanese_message() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        create_branch(&repo, "feature").unwrap();
        switch_branch(&repo, "feature").unwrap();

        // 今チェックアウト中のブランチは削除できない。
        let err = delete_branch(&repo, "feature").unwrap_err();
        assert!(matches!(err, CoreError::Blocked(_)));
        let msg = err.to_string();
        assert!(
            msg.contains("チェックアウト") || msg.contains("削除できません"),
            "日本語メッセージがチェックアウト中を説明すること: {msg}"
        );
    }

    // --- save_protected_branches / load_protected_branches のテスト ---

    #[test]
    fn save_and_load_protected_branches_roundtrip() {
        let fx = TestRepo::new();
        let repo = fx.open();

        save_protected_branches(&repo, &["develop".to_string(), "release".to_string()]).unwrap();

        let loaded = crate::repo::load_protected_branches(&repo).unwrap();
        assert_eq!(loaded, vec!["develop".to_string(), "release".to_string()]);
    }

    #[test]
    fn load_protected_branches_defaults_when_unset() {
        let fx = TestRepo::new();
        let repo = fx.open();
        let loaded = crate::repo::load_protected_branches(&repo).unwrap();
        assert_eq!(loaded, vec!["main".to_string(), "master".to_string()]);
    }

    #[test]
    fn save_protected_branches_normalizes_and_dedupes() {
        let fx = TestRepo::new();
        let repo = fx.open();

        save_protected_branches(
            &repo,
            &[
                " develop ".to_string(),
                "".to_string(),
                "develop".to_string(),
                "  ".to_string(),
            ],
        )
        .unwrap();

        let loaded = crate::repo::load_protected_branches(&repo).unwrap();
        assert_eq!(loaded, vec!["develop".to_string()]);
    }

    #[test]
    fn save_empty_protected_branches_resets_to_default() {
        let fx = TestRepo::new();
        let repo = fx.open();

        save_protected_branches(&repo, &["develop".to_string()]).unwrap();
        assert_eq!(
            crate::repo::load_protected_branches(&repo).unwrap(),
            vec!["develop".to_string()]
        );

        // 空配列で保存すると既定値（main/master）に戻る。
        save_protected_branches(&repo, &[]).unwrap();
        assert_eq!(
            crate::repo::load_protected_branches(&repo).unwrap(),
            vec!["main".to_string(), "master".to_string()]
        );
    }

    #[test]
    fn save_protected_branches_rejects_invalid_name() {
        let fx = TestRepo::new();
        let repo = fx.open();

        let err = save_protected_branches(&repo, &["with spaces".to_string()]).unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput(_)));

        // 不正な名前があると何も書き込まれない（既定値のまま）。
        assert_eq!(
            crate::repo::load_protected_branches(&repo).unwrap(),
            vec!["main".to_string(), "master".to_string()]
        );
    }

    #[test]
    fn protected_branches_config_is_independent_per_repo() {
        let fx_a = TestRepo::new();
        let fx_b = TestRepo::new();

        save_protected_branches(&fx_a.open(), &["develop".to_string()]).unwrap();

        // fx_a にだけ設定したので、fx_b は既定値のまま。
        assert_eq!(
            crate::repo::load_protected_branches(&fx_a.open()).unwrap(),
            vec!["develop".to_string()]
        );
        assert_eq!(
            crate::repo::load_protected_branches(&fx_b.open()).unwrap(),
            vec!["main".to_string(), "master".to_string()]
        );
    }

    #[test]
    fn custom_protected_branch_makes_delete_and_force_push_destructive() {
        use crate::safety::{assess, OperationKind, RiskLevel, SafetyContext};

        let fx = TestRepo::new();
        let repo = fx.open();
        save_protected_branches(&repo, &["develop".to_string()]).unwrap();
        let protected = crate::repo::load_protected_branches(&repo).unwrap();

        let ctx = SafetyContext {
            target_branch: Some("develop".to_string()),
            protected_branches: protected,
            ..Default::default()
        };

        assert_eq!(
            assess(OperationKind::DeleteBranch, &ctx).level,
            RiskLevel::Destructive,
            "カスタム保護ブランチの削除は Destructive のはず"
        );
        let fp = assess(OperationKind::ForcePush, &ctx);
        assert_eq!(fp.level, RiskLevel::Destructive);
        assert!(
            fp.reasons.iter().any(|r| r.contains("保護ブランチ")),
            "カスタム保護ブランチへの force push は保護ブランチである旨の理由を含むはず"
        );
    }

    /// 指定パスの未ステージ差分から hunk ヘッダー文字列を集める（テスト用ヘルパー）。
    fn collect_hunk_headers(repo: &Repository, path: &str) -> Vec<String> {
        let mut opts = git2::DiffOptions::new();
        opts.pathspec(path).context_lines(3);
        let diff = repo.diff_index_to_workdir(None, Some(&mut opts)).unwrap();
        let headers = RefCell::new(Vec::new());
        diff.foreach(
            &mut |_d, _p| true,
            None,
            Some(&mut |_d, hunk| {
                headers.borrow_mut().push(
                    String::from_utf8_lossy(hunk.header())
                        .trim_end()
                        .to_string(),
                );
                true
            }),
            None,
        )
        .unwrap();
        headers.into_inner()
    }

    #[test]
    fn stage_hunk_stages_only_matching_hunk_then_undo_restores() {
        let fx = TestRepo::new();
        // 10 行のファイルを用意してコミットする。離れた 2 箇所を変えて 2 つの hunk を作る。
        fx.write_file("f.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
        fx.stage_all();
        fx.commit("c1");

        // 先頭付近（1行目）と末尾付近（10行目）をそれぞれ変える → 2 つの hunk になる。
        fx.write_file("f.txt", "1-changed\n2\n3\n4\n5\n6\n7\n8\n9\n10-changed\n");

        let repo = fx.open();
        let headers = collect_hunk_headers(&repo, "f.txt");
        assert_eq!(
            headers.len(),
            2,
            "離れた 2 箇所の変更で 2 hunk になること: {headers:?}"
        );

        // 1 つ目の hunk だけをステージする。
        stage_hunk(&repo, "f.txt", &headers[0]).unwrap();

        // ステージ済みに f.txt が現れ、未ステージにも f.txt が残る（もう片方の hunk）。
        let st = status(&repo).unwrap();
        assert!(
            st.staged.iter().any(|c| c.path == "f.txt"),
            "片方の hunk がステージされること: {st:?}"
        );
        assert!(
            st.unstaged.iter().any(|c| c.path == "f.txt"),
            "もう片方の hunk は未ステージのまま残ること: {st:?}"
        );

        // ステージ済み差分には 1 hunk だけ入っている（1 つ目の hunk）。
        let mut sopts = git2::DiffOptions::new();
        sopts.pathspec("f.txt").context_lines(3);
        let head_tree = repo.head().unwrap().peel_to_tree().unwrap();
        let staged_diff = repo
            .diff_tree_to_index(Some(&head_tree), None, Some(&mut sopts))
            .unwrap();
        let mut staged_hunks = 0usize;
        staged_diff
            .foreach(
                &mut |_d, _p| true,
                None,
                Some(&mut |_d, _h| {
                    staged_hunks += 1;
                    true
                }),
                None,
            )
            .unwrap();
        assert_eq!(
            staged_hunks, 1,
            "ステージされた hunk はちょうど 1 つであること"
        );

        // Undo でステージ前に戻る（f.txt は未ステージのみになる）。
        undo_last(&repo).unwrap();
        let repo = fx.open();
        let st = status(&repo).unwrap();
        assert!(
            st.staged.is_empty(),
            "undo でステージが空に戻ること: {st:?}"
        );
        assert!(
            st.unstaged.iter().any(|c| c.path == "f.txt"),
            "変更内容は未ステージとして保持されること: {st:?}"
        );
    }

    #[test]
    fn stage_hunk_with_unknown_header_is_rejected() {
        let fx = TestRepo::new();
        fx.write_file("f.txt", "a\n");
        fx.stage_all();
        fx.commit("c1");
        fx.write_file("f.txt", "b\n");

        let repo = fx.open();
        let err = stage_hunk(&repo, "f.txt", "@@ -999,0 +999,0 @@").unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput(_)));
    }

    #[test]
    fn stage_hunk_rejects_empty_arguments() {
        let fx = TestRepo::new();
        let repo = fx.open();
        assert!(matches!(
            stage_hunk(&repo, "  ", "@@ -1 +1 @@").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
        assert!(matches!(
            stage_hunk(&repo, "f.txt", "   ").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
    }

    // ステージ済み差分（HEAD ↔ index）の hunk ヘッダー一覧を集める（unstage_hunk のテスト用）。
    fn collect_staged_hunk_headers(repo: &Repository, path: &str) -> Vec<String> {
        let mut opts = git2::DiffOptions::new();
        opts.pathspec(path).context_lines(3);
        let head_tree = repo.head().ok().map(|h| h.peel_to_tree().unwrap());
        let diff = repo
            .diff_tree_to_index(head_tree.as_ref(), None, Some(&mut opts))
            .unwrap();
        let headers = RefCell::new(Vec::new());
        diff.foreach(
            &mut |_d, _p| true,
            None,
            Some(&mut |_d, hunk| {
                headers.borrow_mut().push(
                    String::from_utf8_lossy(hunk.header())
                        .trim_end()
                        .to_string(),
                );
                true
            }),
            None,
        )
        .unwrap();
        headers.into_inner()
    }

    #[test]
    fn unstage_hunk_unstages_only_matching_hunk_then_undo_restages() {
        let fx = TestRepo::new();
        // 10 行のファイルを用意してコミットする。離れた 2 箇所を変えて 2 つの hunk を作る。
        fx.write_file("f.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
        fx.stage_all();
        fx.commit("c1");

        fx.write_file("f.txt", "1-changed\n2\n3\n4\n5\n6\n7\n8\n9\n10-changed\n");
        fx.stage_all();

        let repo = fx.open();
        let headers = collect_staged_hunk_headers(&repo, "f.txt");
        assert_eq!(
            headers.len(),
            2,
            "離れた 2 箇所の変更で 2 hunk になること: {headers:?}"
        );

        // 1 つ目の hunk だけをアンステージする。
        unstage_hunk(&repo, "f.txt", &headers[0]).unwrap();

        // ステージ済み差分にはもう片方の hunk だけが残る。
        let remaining = collect_staged_hunk_headers(&repo, "f.txt");
        assert_eq!(
            remaining,
            vec![headers[1].clone()],
            "アンステージした hunk 以外はステージされたまま残ること"
        );

        // f.txt はステージ済み・未ステージの両方に現れる（もう片方の hunk が未ステージ化された）。
        let st = status(&repo).unwrap();
        assert!(
            st.staged.iter().any(|c| c.path == "f.txt"),
            "もう片方の hunk はステージされたまま: {st:?}"
        );
        assert!(
            st.unstaged.iter().any(|c| c.path == "f.txt"),
            "アンステージした hunk が未ステージとして戻ること: {st:?}"
        );

        // 作業ツリーは一切変更されない（両方の変更が残っている）。
        let contents = std::fs::read_to_string(fx.path().join("f.txt")).unwrap();
        assert_eq!(contents, "1-changed\n2\n3\n4\n5\n6\n7\n8\n9\n10-changed\n");

        // Undo でアンステージ前に戻る（両方の hunk が再びステージされる）。
        undo_last(&repo).unwrap();
        let repo = fx.open();
        let restored = collect_staged_hunk_headers(&repo, "f.txt");
        assert_eq!(
            restored.len(),
            2,
            "undo で両方の hunk がステージされた状態に戻ること: {restored:?}"
        );
        let st = status(&repo).unwrap();
        assert!(
            st.unstaged.iter().all(|c| c.path != "f.txt"),
            "undo 後は未ステージ側に f.txt が残らないこと: {st:?}"
        );
    }

    #[test]
    fn unstage_hunk_on_new_file_removes_it_from_index_then_undo_restores() {
        // HEAD に存在しない新規ファイルを丸ごとステージし、その唯一の hunk を
        // アンステージすると、インデックスからファイルごと消えること。
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1\n");
        fx.stage_all();
        fx.commit("base");

        fx.write_file("new.txt", "hello\nworld\n");
        fx.stage_all();

        let repo = fx.open();
        let headers = collect_staged_hunk_headers(&repo, "new.txt");
        assert_eq!(headers.len(), 1);

        unstage_hunk(&repo, "new.txt", &headers[0]).unwrap();

        let st = status(&repo).unwrap();
        assert!(
            st.staged.iter().all(|c| c.path != "new.txt"),
            "新規ファイルはインデックスから完全に消えること: {st:?}"
        );
        assert!(
            st.untracked.iter().any(|p| p == "new.txt"),
            "作業ツリーには残り、未追跡に戻ること: {st:?}"
        );
        // 作業ツリーのファイル内容自体は変わらない。
        assert_eq!(
            std::fs::read_to_string(fx.path().join("new.txt")).unwrap(),
            "hello\nworld\n"
        );

        // Undo で再びステージされた状態に戻る。
        undo_last(&repo).unwrap();
        let repo = fx.open();
        let st = status(&repo).unwrap();
        assert!(
            st.staged.iter().any(|c| c.path == "new.txt"),
            "undo で新規ファイルが再ステージされること: {st:?}"
        );
    }

    #[test]
    fn unstage_hunk_with_unknown_header_is_rejected() {
        let fx = TestRepo::new();
        fx.write_file("f.txt", "a\n");
        fx.stage_all();
        fx.commit("c1");
        fx.write_file("f.txt", "b\n");
        fx.stage_all();

        let repo = fx.open();
        let err = unstage_hunk(&repo, "f.txt", "@@ -999,0 +999,0 @@").unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput(_)));
    }

    #[test]
    fn unstage_hunk_rejects_empty_arguments() {
        let fx = TestRepo::new();
        let repo = fx.open();
        assert!(matches!(
            unstage_hunk(&repo, "  ", "@@ -1 +1 @@").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
        assert!(matches!(
            unstage_hunk(&repo, "f.txt", "   ").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
    }

    #[test]
    fn create_lightweight_tag_appears_in_list() {
        use crate::repo::list_tags;

        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        create_tag(&repo, "v1.0.0", None, None).unwrap();

        let tags = list_tags(&repo).unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "v1.0.0");
        assert!(tags[0].message.is_none());
        // タグ作成も undo を記録する（create_branch / DeleteBranch と同じパターン）。
        assert!(crate::undo::can_undo(&repo).unwrap());
    }

    // タグ作成（軽量）は DeleteTag の undo を記録し、Undo でそのタグが削除されること。
    #[test]
    fn create_lightweight_tag_then_undo_removes_it() {
        use crate::repo::list_tags;

        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        create_tag(&repo, "v1.0.0", None, None).unwrap();
        assert_eq!(list_tags(&repo).unwrap().len(), 1);

        let desc = undo_last(&repo).unwrap();
        assert!(desc.contains("v1.0.0"));
        assert!(list_tags(&repo).unwrap().is_empty());
        assert!(!crate::undo::can_undo(&repo).unwrap());
    }

    // タグ作成（注釈付き）も同様に Undo でタグが削除されること。
    #[test]
    fn create_annotated_tag_then_undo_removes_it() {
        use crate::repo::list_tags;

        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        create_tag(&repo, "v2.0.0", None, Some("メジャーリリース")).unwrap();
        assert_eq!(list_tags(&repo).unwrap().len(), 1);

        undo_last(&repo).unwrap();
        assert!(list_tags(&repo).unwrap().is_empty());
    }

    // DeleteTag の apply は冪等: 2回適用してもエラーにならず、タグが消えたままであること。
    #[test]
    fn undo_delete_tag_apply_is_idempotent() {
        use crate::repo::list_tags;
        use crate::undo::{self, UndoAction, UndoEntry};

        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        create_tag(&repo, "v1.0.0", None, None).unwrap();

        // 直接 DeleteTag アクションを2回積んで適用しても壊れない（1回目は削除、2回目はno-op）。
        undo::push(
            &repo,
            UndoEntry {
                op: OperationKind::CreateTag,
                description: "タグ「v1.0.0」の作成を取り消す".into(),
                action: UndoAction::DeleteTag {
                    name: "v1.0.0".into(),
                },
            },
        )
        .unwrap();
        undo_last(&repo).unwrap();
        assert!(list_tags(&repo).unwrap().is_empty());

        // 既に削除済みのタグに対してもう一度同じアクションを適用してもエラーにならない。
        undo::push(
            &repo,
            UndoEntry {
                op: OperationKind::CreateTag,
                description: "タグ「v1.0.0」の作成を取り消す".into(),
                action: UndoAction::DeleteTag {
                    name: "v1.0.0".into(),
                },
            },
        )
        .unwrap();
        undo_last(&repo).unwrap();
        assert!(list_tags(&repo).unwrap().is_empty());
    }

    #[test]
    fn create_annotated_tag_keeps_message() {
        use crate::repo::list_tags;

        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        create_tag(&repo, "v2.0.0", None, Some("メジャーリリース")).unwrap();

        let tags = list_tags(&repo).unwrap();
        assert_eq!(tags[0].message.as_deref(), Some("メジャーリリース"));
    }

    #[test]
    fn create_tag_rejects_empty_name_and_duplicate() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        // 空名は入力エラー。
        assert!(matches!(
            create_tag(&repo, "  ", None, None).unwrap_err(),
            CoreError::InvalidInput(_)
        ));

        // 同名タグの再作成は入力エラー。
        create_tag(&repo, "dup", None, None).unwrap();
        assert!(matches!(
            create_tag(&repo, "dup", None, None).unwrap_err(),
            CoreError::InvalidInput(_)
        ));
    }

    #[test]
    fn delete_tag_removes_from_list() {
        use crate::repo::list_tags;

        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        create_tag(&repo, "v1.0.0", None, None).unwrap();
        assert_eq!(list_tags(&repo).unwrap().len(), 1);

        delete_tag(&repo, "v1.0.0").unwrap();
        assert!(list_tags(&repo).unwrap().is_empty());

        // 存在しないタグの削除は入力エラー。
        assert!(matches!(
            delete_tag(&repo, "no-such").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
    }

    #[test]
    fn delete_lightweight_tag_then_undo_restores_it() {
        use crate::repo::list_tags;

        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        create_tag(&repo, "v1.0.0", None, None).unwrap();
        delete_tag(&repo, "v1.0.0").unwrap();
        assert!(list_tags(&repo).unwrap().is_empty());

        // Undo で軽量タグが復元される（メッセージ無しのまま）。
        undo_last(&repo).unwrap();
        let tags = list_tags(&repo).unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "v1.0.0");
        assert!(tags[0].message.is_none());
    }

    #[test]
    fn delete_annotated_tag_then_undo_restores_message() {
        use crate::repo::list_tags;

        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        create_tag(&repo, "v2.0.0", None, Some("リリース 2.0")).unwrap();
        delete_tag(&repo, "v2.0.0").unwrap();
        assert!(list_tags(&repo).unwrap().is_empty());

        // Undo で注釈付きタグが復元され、メッセージも戻る。
        undo_last(&repo).unwrap();
        let tags = list_tags(&repo).unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].message.as_deref(), Some("リリース 2.0"));
    }

    #[test]
    fn unstage_moves_file_back_to_unstaged() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        fx.write_file("a.txt", "2");
        stage_all(&repo).unwrap();
        assert_eq!(status(&repo).unwrap().staged.len(), 1);

        unstage(&repo, "a.txt").unwrap();
        let st = status(&repo).unwrap();
        assert!(st.staged.is_empty());
        assert_eq!(st.unstaged.len(), 1);
    }

    /// `a.txt` がコンフリクト中になった一時リポジトリを作る（main を other にマージ）。
    fn repo_with_conflict() -> TestRepo {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "base\n");
        fx.stage_all();
        let base_oid = fx.commit("base");

        let repo = fx.open();
        let base_commit = repo.find_commit(base_oid).unwrap();
        repo.branch("other", &base_commit, false).unwrap();

        // main 側の変更。
        fx.write_file("a.txt", "main side\n");
        fx.stage_all();
        let main_oid = fx.commit("main change");

        // other へ切り替えて別の変更。
        let repo = fx.open();
        let obj = repo.revparse_single("refs/heads/other").unwrap();
        let mut co = git2::build::CheckoutBuilder::new();
        co.force();
        repo.checkout_tree(&obj, Some(&mut co)).unwrap();
        repo.set_head("refs/heads/other").unwrap();

        fx.write_file("a.txt", "other side\n");
        fx.stage_all();
        fx.commit("other change");

        // main を other にマージしてコンフリクトさせる。
        let repo = fx.open();
        let main_commit = repo.find_commit(main_oid).unwrap();
        let annotated = repo.find_annotated_commit(main_commit.id()).unwrap();
        repo.merge(&[&annotated], None, None).unwrap();

        fx
    }

    #[test]
    fn mark_resolved_clears_conflict_and_stages() {
        use crate::repo::get_conflicts;

        let fx = repo_with_conflict();
        let repo = fx.open();
        // 最初はコンフリクト中。
        assert_eq!(get_conflicts(&repo).unwrap().len(), 1);

        // 競合の目印を取り除いて解消した想定の内容を書き込む。
        fx.write_file("a.txt", "resolved\n");
        let repo = fx.open();
        mark_resolved(&repo, "a.txt").unwrap();

        let repo = fx.open();
        // コンフリクトが消え、解消済みのファイルがステージされている。
        assert!(get_conflicts(&repo).unwrap().is_empty());
        let st = status(&repo).unwrap();
        assert!(st.conflicted.is_empty());
        assert!(st.staged.iter().any(|f| f.path == "a.txt"));
    }

    // 別ブランチのコミットを cherry-pick すると、その変更が現在ブランチに入り、
    // undo で取り消せること（#82）。
    #[test]
    fn cherry_pick_copies_commit_then_undo_restores() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "base\n");
        fx.stage_all();
        fx.commit("c1");

        // feature ブランチを作り、そこに b.txt を追加するコミットを積む。
        {
            let repo = fx.open();
            create_branch(&repo, "feature").unwrap();
            switch_branch(&repo, "feature").unwrap();
        }
        fx.write_file("b.txt", "feature work\n");
        fx.stage_all();
        let feature_oid = fx.commit("feature: b.txt を追加");

        // main に戻り、main 側を 1 つ進めて feature と分岐させる（コピー先の親を変える）。
        {
            let repo = fx.open();
            switch_branch(&repo, "main").unwrap();
        }
        assert!(!fx.path().join("b.txt").exists());
        fx.write_file("c.txt", "main work\n");
        fx.stage_all();
        fx.commit("main: c.txt を追加");
        assert_eq!(log(&fx.open(), 10).unwrap().len(), 2);

        // feature のコミットを main へ cherry-pick する。
        let repo = fx.open();
        let info = cherry_pick(&repo, &feature_oid.to_string()).unwrap();
        assert_eq!(info.summary, "feature: b.txt を追加");

        // main に b.txt がコピーされ、コミット数が 1 増えている（c1 + c.txt + コピー）。
        let repo = fx.open();
        assert!(fx.path().join("b.txt").exists());
        assert_eq!(log(&repo, 10).unwrap().len(), 3);
        // 別のコミットになっている（親が違うので元のコミットとは ID が異なる）。
        assert_ne!(info.id, feature_oid.to_string());

        // Undo でコピーを取り消すと、main は元の 2 コミットに戻る。
        undo_last(&repo).unwrap();
        let repo = fx.open();
        assert_eq!(log(&repo, 10).unwrap().len(), 2);
    }

    // 不正なコミット指定は InvalidInput になること（#82）。
    #[test]
    fn cherry_pick_invalid_oid_is_input_error() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        assert!(matches!(
            cherry_pick(&repo, "not-a-valid-oid").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
    }

    // 同じ箇所を変更したコミットの cherry-pick はコンフリクトで Blocked になり、
    // 作業ツリーの状態が保全されること（#82）。
    #[test]
    fn cherry_pick_conflict_is_blocked_and_preserves_state() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "base\n");
        fx.stage_all();
        fx.commit("c1");

        // feature で a.txt を別内容に変更するコミットを作る。
        {
            let repo = fx.open();
            create_branch(&repo, "feature").unwrap();
            switch_branch(&repo, "feature").unwrap();
        }
        fx.write_file("a.txt", "feature change\n");
        fx.stage_all();
        let feature_oid = fx.commit("feature: a.txt を変更");

        // main に戻り、a.txt を別の内容に変更して分岐させる。
        {
            let repo = fx.open();
            switch_branch(&repo, "main").unwrap();
        }
        fx.write_file("a.txt", "main change\n");
        fx.stage_all();
        fx.commit("main: a.txt を変更");
        let main_head_before = fx.head_oid();

        // 同じ行を触るのでコンフリクトになり、Blocked エラーになる。
        let repo = fx.open();
        let err = cherry_pick(&repo, &feature_oid.to_string()).unwrap_err();
        assert!(matches!(err, CoreError::Blocked(_)));

        // 状態保全: HEAD も作業ツリーも変わっていない。
        let repo = fx.open();
        assert_eq!(fx.head_oid(), main_head_before);
        assert_eq!(
            std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
            "main change\n"
        );
        assert!(status(&repo).unwrap().is_clean);
    }

    // revert: 打ち消しコミットが積まれ、内容が戻り、undo で取り消せること（#195）。
    #[test]
    fn revert_commit_adds_inverse_commit_then_undo_restores() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "base\n");
        fx.stage_all();
        fx.commit("c1");
        fx.write_file("b.txt", "added\n");
        fx.stage_all();
        let target = fx.commit("b.txt を追加");
        fx.write_file("c.txt", "later\n");
        fx.stage_all();
        fx.commit("c.txt を追加");

        let repo = fx.open();
        let info = revert_commit(&repo, &target.to_string()).unwrap();
        assert_eq!(info.summary, "Revert \"b.txt を追加\"");
        // 履歴は書き換わらず 1 つ増える。b.txt だけ消え、c.txt は残る。
        assert_eq!(log(&repo, 10).unwrap().len(), 4);
        assert!(!fx.path().join("b.txt").exists());
        assert!(fx.path().join("c.txt").exists());
        assert!(status(&repo).unwrap().is_clean);

        undo_last(&repo).unwrap();
        let repo = fx.open();
        assert_eq!(log(&repo, 10).unwrap().len(), 3);
        // soft reset なので打ち消しの変更はステージ済みで残る（cherry-pick の undo と同じ挙動）。
        assert!(status(&repo)
            .unwrap()
            .staged
            .iter()
            .any(|f| f.path == "b.txt"));
    }

    #[test]
    fn revert_commit_invalid_oid_is_input_error() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "x\n");
        fx.stage_all();
        fx.commit("c1");
        let repo = fx.open();
        assert!(matches!(
            revert_commit(&repo, "not-a-valid-oid").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
    }

    // 後続コミットが同じ箇所を変えていると競合し、Blocked で状態が保全されること（#195）。
    #[test]
    fn revert_commit_conflict_is_blocked_and_preserves_state() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "base\n");
        fx.stage_all();
        fx.commit("c1");
        fx.write_file("a.txt", "first change\n");
        fx.stage_all();
        let target = fx.commit("a.txt を変更");
        fx.write_file("a.txt", "second change\n");
        fx.stage_all();
        fx.commit("a.txt をさらに変更");
        let head_before = fx.head_oid();

        let repo = fx.open();
        let err = revert_commit(&repo, &target.to_string()).unwrap_err();
        assert!(matches!(err, CoreError::Blocked(_)));

        let repo = fx.open();
        assert_eq!(fx.head_oid(), head_before);
        assert_eq!(
            std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
            "second change\n"
        );
        assert!(status(&repo).unwrap().is_clean);
        assert!(status(&repo).unwrap().conflicted.is_empty());
    }

    // マージコミットは v1 では対象外で Blocked（#195）。
    #[test]
    fn revert_commit_merge_commit_is_blocked() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "base\n");
        fx.stage_all();
        fx.commit("c1");
        {
            let repo = fx.open();
            create_branch(&repo, "feature").unwrap();
            switch_branch(&repo, "feature").unwrap();
        }
        fx.write_file("f.txt", "1\n");
        fx.stage_all();
        fx.commit("feature");
        {
            let repo = fx.open();
            switch_branch(&repo, "main").unwrap();
        }
        fx.write_file("m.txt", "1\n");
        fx.stage_all();
        fx.commit("main");
        let repo = fx.open();
        let merge_oid = match merge_branch(&repo, "feature").unwrap() {
            MergeOutcome::Merged { commit } => commit.id,
            other => panic!("Merged を期待したが {other:?} だった"),
        };
        let head_before = fx.head_oid();
        let err = revert_commit(&repo, &merge_oid).unwrap_err();
        assert!(matches!(err, CoreError::Blocked(_)));
        assert_eq!(fx.head_oid(), head_before);
    }

    // ステージ済みの変更があると Blocked で、ステージは保たれること（#195）。
    #[test]
    fn revert_commit_with_staged_changes_is_blocked() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "base\n");
        fx.stage_all();
        fx.commit("c1");
        fx.write_file("b.txt", "x\n");
        fx.stage_all();
        let target = fx.commit("b.txt を追加");
        fx.write_file("c.txt", "staged\n");
        fx.stage_all();
        let repo = fx.open();
        let err = revert_commit(&repo, &target.to_string()).unwrap_err();
        assert!(matches!(err, CoreError::Blocked(_)));
        assert!(fx.path().join("b.txt").exists());
        assert!(status(&repo)
            .unwrap()
            .staged
            .iter()
            .any(|f| f.path == "c.txt"));
    }

    #[test]
    fn add_to_gitignore_creates_file_when_missing() {
        use crate::repo::read_gitignore;

        let fx = TestRepo::new();
        let repo = fx.open();
        // .gitignore が無い状態から追記すると新規作成され、末尾は改行で終わる。
        add_to_gitignore(&repo, ".env").unwrap();
        assert_eq!(read_gitignore(&repo).unwrap().as_deref(), Some(".env\n"));
    }

    #[test]
    fn add_to_gitignore_appends_with_separating_newline() {
        use crate::repo::read_gitignore;

        let fx = TestRepo::new();
        // 末尾に改行が無い既存ファイルでも、行が連結されないよう改行を補う。
        fx.write_file(".gitignore", "target/");
        let repo = fx.open();
        add_to_gitignore(&repo, "*.log").unwrap();
        assert_eq!(
            read_gitignore(&repo).unwrap().as_deref(),
            Some("target/\n*.log\n")
        );
    }

    #[test]
    fn add_to_gitignore_is_idempotent_for_existing_pattern() {
        use crate::repo::read_gitignore;

        let fx = TestRepo::new();
        fx.write_file(".gitignore", "node_modules/\n.env\n");
        let repo = fx.open();
        // すでにある行を追記しても重複しない。
        add_to_gitignore(&repo, ".env").unwrap();
        assert_eq!(
            read_gitignore(&repo).unwrap().as_deref(),
            Some("node_modules/\n.env\n")
        );
    }

    #[test]
    fn add_to_gitignore_rejects_empty_and_multiline() {
        let fx = TestRepo::new();
        let repo = fx.open();
        // 空・空白のみ・改行入りはエラー。
        assert!(matches!(
            add_to_gitignore(&repo, "   ").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
        assert!(matches!(
            add_to_gitignore(&repo, "a\nb").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
        // どちらの失敗でも .gitignore は作られない。
        assert!(crate::repo::read_gitignore(&repo).unwrap().is_none());
    }

    #[test]
    fn add_to_gitignore_rejects_invalid_glob_syntax() {
        let fx = TestRepo::new();
        let repo = fx.open();
        // 閉じていない "[" は不正な glob として拒否される。
        assert!(matches!(
            add_to_gitignore(&repo, "*.[oa").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
        assert!(crate::repo::read_gitignore(&repo).unwrap().is_none());
    }

    #[test]
    fn validate_gitignore_pattern_accepts_common_patterns() {
        // 通常のパターン。
        assert!(validate_gitignore_pattern("*.log").valid);
        assert!(validate_gitignore_pattern("build/").valid);
        assert!(validate_gitignore_pattern("/build/output.log").valid);
        // 否定パターン（除外）。
        assert!(validate_gitignore_pattern("!important.log").valid);
        // 文字クラス（きちんと閉じている）。
        assert!(validate_gitignore_pattern("*.[oa]").valid);
        // "**" の正しい使い方。
        assert!(validate_gitignore_pattern("**/foo").valid);
        assert!(validate_gitignore_pattern("foo/**").valid);
        assert!(validate_gitignore_pattern("a/**/b").valid);
        assert!(validate_gitignore_pattern("**").valid);
        // エスケープされた末尾の "\\"（偶数個）は問題ない。
        assert!(validate_gitignore_pattern("foo\\\\").valid);
    }

    #[test]
    fn validate_gitignore_pattern_rejects_empty_and_comment() {
        let empty = validate_gitignore_pattern("");
        assert!(!empty.valid);
        assert!(empty.error.is_some());

        let spaces = validate_gitignore_pattern("   ");
        assert!(!spaces.valid);

        let negated_empty = validate_gitignore_pattern("!");
        assert!(!negated_empty.valid);

        let comment = validate_gitignore_pattern("# コメント");
        assert!(!comment.valid);

        let multiline = validate_gitignore_pattern("a\nb");
        assert!(!multiline.valid);
    }

    #[test]
    fn validate_gitignore_pattern_rejects_unclosed_bracket() {
        let check = validate_gitignore_pattern("*.[oa");
        assert!(!check.valid);
        assert!(check.error.unwrap().contains('['));
    }

    #[test]
    fn validate_gitignore_pattern_rejects_dangling_trailing_backslash() {
        // 末尾が奇数個の "\\" はエスケープ対象の文字が続いていないため不正。
        let check = validate_gitignore_pattern("foo\\");
        assert!(!check.valid);
    }

    #[test]
    fn validate_gitignore_pattern_rejects_misplaced_double_asterisk() {
        assert!(!validate_gitignore_pattern("foo**").valid);
        assert!(!validate_gitignore_pattern("**foo").valid);
        assert!(!validate_gitignore_pattern("foo**bar").valid);
        assert!(!validate_gitignore_pattern("foo***").valid);
    }

    #[test]
    fn check_gitignore_pattern_detects_duplicate_ignoring_comments_and_blank_lines() {
        let fx = TestRepo::new();
        fx.write_file(".gitignore", "# コメント\n\nnode_modules/\n  .env  \n");
        let repo = fx.open();

        // 完全一致（前後空白の正規化込み）は重複扱い。
        let dup = check_gitignore_pattern(&repo, ".env").unwrap();
        assert!(dup.valid);
        assert!(dup.duplicate);

        // 新規パターンは重複ではない。
        let fresh = check_gitignore_pattern(&repo, "*.log").unwrap();
        assert!(fresh.valid);
        assert!(!fresh.duplicate);

        // コメント行の文字列そのものは重複と誤認しない。
        let comment_like = check_gitignore_pattern(&repo, "コメント").unwrap();
        assert!(comment_like.valid);
        assert!(!comment_like.duplicate);

        // 構文が不正なら重複判定は行わず valid=false のみ返す。
        let invalid = check_gitignore_pattern(&repo, "*.[oa").unwrap();
        assert!(!invalid.valid);
        assert!(!invalid.duplicate);
    }

    #[test]
    fn check_gitignore_pattern_no_duplicate_when_file_missing() {
        let fx = TestRepo::new();
        let repo = fx.open();
        let check = check_gitignore_pattern(&repo, "*.log").unwrap();
        assert!(check.valid);
        assert!(!check.duplicate);
    }

    #[test]
    fn suggest_gitignore_patterns_nested_file_with_extension() {
        let suggestions = suggest_gitignore_patterns("build/output.log");
        assert_eq!(suggestions.len(), 3);
        assert_eq!(suggestions[0].pattern, "/build/output.log");
        assert_eq!(suggestions[1].pattern, "*.log");
        assert_eq!(suggestions[2].pattern, "build/");
    }

    #[test]
    fn suggest_gitignore_patterns_root_file_without_extension_skips_extras() {
        // ルート直下・拡張子なしのファイルは「このファイルのみ」しか出せない。
        let suggestions = suggest_gitignore_patterns("Makefile");
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].pattern, "/Makefile");
    }

    #[test]
    fn suggest_gitignore_patterns_dotfile_at_root_skips_extension_suggestion() {
        // ".env" のようなドットファイルは Rust の Path::extension() 上「拡張子なし」
        // 扱いになるため、拡張子まとめ候補は出さない。ルート直下でもある。
        let suggestions = suggest_gitignore_patterns(".env");
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].pattern, "/.env");
    }

    #[test]
    fn suggest_gitignore_patterns_escapes_glob_special_characters() {
        // ファイル名の `[` `]` や行頭の `#` が glob / コメントとして解釈されないようにする。
        let suggestions = suggest_gitignore_patterns("logs[1]/#memo*.txt");
        assert_eq!(suggestions[0].pattern, "/logs\\[1\\]/\\#memo\\*.txt");
        assert_eq!(suggestions[1].pattern, "*.txt");
        assert_eq!(suggestions[2].pattern, "logs\\[1\\]/");
        // エスケープ済みの候補はそのままバリデーションを通る。
        for s in &suggestions {
            assert!(
                validate_gitignore_pattern(&s.pattern).valid,
                "{}",
                s.pattern
            );
        }
    }

    #[test]
    fn suggest_gitignore_patterns_empty_path_returns_empty() {
        assert!(suggest_gitignore_patterns("").is_empty());
        assert!(suggest_gitignore_patterns("   ").is_empty());
    }

    // --- リモート操作テスト ---

    #[test]
    fn remote_add_increases_list() {
        let fx = TestRepo::new();
        let repo = fx.open();

        let before = crate::repo::list_remotes(&repo).unwrap();
        assert!(before.is_empty(), "初期状態ではリモートが無いはず");

        add_remote(&repo, "origin", "https://example.com/repo.git").unwrap();

        let after = crate::repo::list_remotes(&repo).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].name, "origin");
        assert_eq!(after[0].fetch_url, "https://example.com/repo.git");
        assert!(after[0].push_url.is_none());
    }

    #[test]
    fn remote_set_url_changes_fetch_url() {
        let fx = TestRepo::new();
        let repo = fx.open();

        add_remote(&repo, "origin", "https://old.example.com/repo.git").unwrap();
        set_remote_url(&repo, "origin", "https://new.example.com/repo.git").unwrap();

        let remotes = crate::repo::list_remotes(&repo).unwrap();
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].fetch_url, "https://new.example.com/repo.git");
    }

    #[test]
    fn remote_remove_decreases_list() {
        let fx = TestRepo::new();
        let repo = fx.open();

        add_remote(&repo, "origin", "https://example.com/repo.git").unwrap();
        add_remote(&repo, "upstream", "https://upstream.example.com/repo.git").unwrap();

        let before = crate::repo::list_remotes(&repo).unwrap();
        assert_eq!(before.len(), 2);

        remove_remote(&repo, "origin").unwrap();

        let after = crate::repo::list_remotes(&repo).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].name, "upstream");
    }

    #[test]
    fn remote_add_rejects_empty_name_and_url() {
        let fx = TestRepo::new();
        let repo = fx.open();

        assert!(matches!(
            add_remote(&repo, "  ", "https://example.com").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
        assert!(matches!(
            add_remote(&repo, "origin", "  ").unwrap_err(),
            CoreError::InvalidInput(_)
        ));
    }

    // --- restore_file_from_commit のテスト ---

    // 2 回コミットして内容を変えた後、古いコミット ID で復元すると古い内容に戻りステージされる。
    #[test]
    fn restore_file_from_commit_restores_old_content_and_stages() {
        let fx = TestRepo::new();
        fx.write_file("hello.txt", "初版の内容\n");
        fx.stage_all();
        fx.commit("c1: 初版");
        let old_oid = fx.head_oid().to_string();

        fx.write_file("hello.txt", "改訂版の内容\n");
        fx.stage_all();
        fx.commit("c2: 改訂");

        // 現在の作業ツリーは改訂版。
        assert_eq!(
            std::fs::read_to_string(fx.path().join("hello.txt")).unwrap(),
            "改訂版の内容\n"
        );

        // 古いコミット（c1）時点に復元する。
        let repo = fx.open();
        restore_file_from_commit(&repo, &old_oid, "hello.txt").unwrap();

        // 作業ツリーが初版に戻る。
        assert_eq!(
            std::fs::read_to_string(fx.path().join("hello.txt")).unwrap(),
            "初版の内容\n"
        );

        // ステージされている（status.staged に含まれる）。
        let st = status(&repo).unwrap();
        assert!(
            st.staged.iter().any(|f| f.path == "hello.txt"),
            "hello.txt がステージされているはず: {st:?}"
        );

        // undo エントリ（UnstagePath）が記録されている。
        let entry = crate::undo::peek(&repo).unwrap().unwrap();
        assert!(matches!(
            entry.action,
            crate::undo::UndoAction::UnstagePath { ref path } if path == "hello.txt"
        ));
    }

    // 指定コミットに存在しないパスを指定すると InvalidInput エラーを返す。
    #[test]
    fn restore_file_from_commit_errors_on_missing_path() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "内容");
        fx.stage_all();
        fx.commit("c1");
        let oid = fx.head_oid().to_string();

        let repo = fx.open();
        let err = restore_file_from_commit(&repo, &oid, "nonexistent.txt").unwrap_err();
        assert!(
            matches!(err, CoreError::InvalidInput(_)),
            "存在しないパスは InvalidInput エラー: {err:?}"
        );
    }

    // --- サブモジュールへの書き込み操作の拒否（#203） ---

    /// サブモジュールを1つ含む親リポジトリを作り、そのサブモジュールパス
    /// （"libs/foo"）を返す。中身は upstream のコミット1件を指したまま。
    fn repo_with_submodule() -> (TestRepo, &'static str) {
        let upstream = TestRepo::new();
        upstream.write_file("readme.txt", "hello");
        upstream.stage_all();
        upstream.commit("upstream c1");

        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        fx.add_submodule("libs/foo", &upstream);
        fx.commit("サブモジュールを追加");

        (fx, "libs/foo")
    }

    #[test]
    fn stage_path_rejects_submodule() {
        let (fx, sub_path) = repo_with_submodule();
        let repo = fx.open();

        let err = stage_path(&repo, sub_path).unwrap_err();
        assert!(
            matches!(err, CoreError::Blocked(_)),
            "サブモジュールへの stage は Blocked のはず: {err:?}"
        );
        // 拒否メッセージには何が起きたか・なぜダメかが日本語で含まれる。
        assert!(err.to_string().contains("サブモジュール"));

        // 状態は変わっていない（壊れた中間状態を作らない）。
        let st = status(&repo).unwrap();
        assert!(st.is_clean, "拒否後も状態はクリーンなまま: {st:?}");
    }

    #[test]
    fn discard_path_rejects_submodule() {
        let (fx, sub_path) = repo_with_submodule();
        let repo = fx.open();

        let err = discard_path(&repo, sub_path).unwrap_err();
        assert!(
            matches!(err, CoreError::Blocked(_)),
            "サブモジュールへの discard は Blocked のはず: {err:?}"
        );

        // サブモジュールのディレクトリ自体は消えていない。
        assert!(fx.path().join(sub_path).join("readme.txt").exists());
    }

    #[test]
    fn stage_hunk_rejects_submodule() {
        let (fx, sub_path) = repo_with_submodule();
        let repo = fx.open();

        let err = stage_hunk(&repo, sub_path, "@@ -1,1 +1,1 @@").unwrap_err();
        assert!(
            matches!(err, CoreError::Blocked(_)),
            "サブモジュールへの hunk ステージは Blocked のはず: {err:?}"
        );
    }

    #[test]
    fn unstage_hunk_rejects_submodule() {
        let (fx, sub_path) = repo_with_submodule();
        let repo = fx.open();

        let err = unstage_hunk(&repo, sub_path, "@@ -1,1 +1,1 @@").unwrap_err();
        assert!(
            matches!(err, CoreError::Blocked(_)),
            "サブモジュールへの hunk アンステージは Blocked のはず: {err:?}"
        );
    }

    #[test]
    fn stage_path_still_works_for_normal_files_alongside_submodule() {
        // サブモジュールが存在していても、通常ファイルの stage は妨げられない。
        let (fx, _sub_path) = repo_with_submodule();
        fx.write_file("b.txt", "normal file");

        let repo = fx.open();
        stage_path(&repo, "b.txt").unwrap();

        let st = status(&repo).unwrap();
        assert!(st
            .staged
            .iter()
            .any(|f| f.path == "b.txt" && !f.is_submodule));
    }

    // --- 出力形式のスナップショットテスト（#175） ---------------------------------
    //
    // ops.rs の返り値の「形式」（serde でフロントに渡る JSON 形状、stash の自動命名
    // 規則、squash の合成メッセージ形式など）が静かに変わっても気づけるよう、insta の
    // スナップショットで固定する。コミット id / short_id / タイムスタンプのように
    // 実行のたびに変わる値は redaction で伏せるが、伏せる前に「長さ・16進である」など
    // 形式そのものを assert で検証してから伏せる（伏せすぎて形式を検証しなくなるのを
    // 避けるため）。
    mod snapshot_format {
        use super::*;

        /// 完全なコミットID（40桁の16進文字列）であることを検証する。
        fn assert_full_oid_format(id: &str) {
            assert_eq!(id.len(), 40, "完全なコミットIDは40桁のはず: {id}");
            assert!(
                id.chars().all(|c| c.is_ascii_hexdigit()),
                "完全なコミットIDは16進のはず: {id}"
            );
        }

        /// 短縮コミットID（7桁の16進文字列）であることを検証する。
        fn assert_short_oid_format(id: &str) {
            assert_eq!(id.len(), 7, "短縮コミットIDは7桁のはず: {id}");
            assert!(
                id.chars().all(|c| c.is_ascii_hexdigit()),
                "短縮コミットIDは16進のはず: {id}"
            );
        }

        #[test]
        fn commit_info_initial_commit_shape() {
            let fx = TestRepo::new();
            fx.write_file("a.txt", "hello");
            let repo = fx.open();
            stage_all(&repo).unwrap();
            let info = commit(&repo, "最初のコミット").unwrap();

            // 実行ごとに変わる値は、まず形式そのものを検証してから伏せる。
            assert_full_oid_format(&info.id);
            assert_short_oid_format(&info.short_id);
            assert!(info.id.starts_with(&info.short_id));
            assert!(info.parent_ids.is_empty(), "最初のコミットは親を持たない");

            insta::assert_yaml_snapshot!(info, {
                ".id" => "[id]",
                ".short_id" => "[short_id]",
                ".time" => "[time]",
            });
        }

        #[test]
        fn commit_info_with_parent_shape() {
            let fx = TestRepo::new();
            fx.write_file("a.txt", "1");
            fx.stage_all();
            fx.commit("c1");

            let repo = fx.open();
            fx.write_file("a.txt", "2");
            stage_all(&repo).unwrap();
            let info = commit(&repo, "c2").unwrap();

            assert_full_oid_format(&info.id);
            assert_short_oid_format(&info.short_id);
            assert_eq!(info.parent_ids.len(), 1);
            assert_full_oid_format(&info.parent_ids[0]);

            insta::assert_yaml_snapshot!(info, {
                ".id" => "[id]",
                ".short_id" => "[short_id]",
                ".time" => "[time]",
                ".parent_ids[]" => "[parent_id]",
            });
        }

        // 空メッセージでの退避は、libgit2 の自動命名（"WIP on <branch>: <短縮id> <summary>"
        // または "On <branch>: ..."）に落ち着く。この形式が変わると一覧表示の見え方が
        // 変わるため固定する（#110 関連）。
        #[test]
        fn stash_save_empty_message_auto_name_shape() {
            let fx = TestRepo::new();
            fx.write_file("a.txt", "1");
            fx.stage_all();
            fx.commit("c1");
            fx.write_file("a.txt", "2");

            let mut repo = fx.open();
            stash_save(&mut repo, "").unwrap();
            let list = stash_list(&mut repo).unwrap();
            assert_eq!(list.len(), 1);
            let info = list.into_iter().next().unwrap();

            assert_full_oid_format(&info.id);

            // 自動命名メッセージの形式を検証する: "(WIP on|On) main: <短縮id> c1"。
            let prefix = if info.message.starts_with("WIP on main: ") {
                "WIP on main: "
            } else if info.message.starts_with("On main: ") {
                "On main: "
            } else {
                panic!("想定外の自動命名メッセージ: {}", info.message);
            };
            let rest = &info.message[prefix.len()..];
            let mut parts = rest.splitn(2, ' ');
            let short = parts.next().unwrap_or("");
            let summary = parts.next().unwrap_or("");
            assert_short_oid_format(short);
            assert_eq!(summary, "c1");

            // 上ですでに可変部分（短縮id）の形式は検証済みなので、スナップショットでは
            // 固定のプレースホルダーに伏せる。接頭辞（"WIP on"/"On"）はそのまま残す。
            insta::assert_yaml_snapshot!(info, {
                ".id" => "[id]",
                ".message" => insta::dynamic_redaction(move |value, _path| {
                    let s = value.as_str().expect("message は文字列のはず");
                    let prefix = if s.starts_with("WIP on main: ") {
                        "WIP on main: "
                    } else {
                        "On main: "
                    };
                    format!("{prefix}[short_id] c1")
                }),
            });
        }

        // squash が「まとめ後のメッセージ」を一切加工せずそのまま採用すること（#175）。
        // フロントエンド（RebaseWizard）は選んだコミットのメッセージを古い順に "\n\n" で
        // 連結して渡す。この複数段落のメッセージが squash 後もそのまま保たれ、
        // summary（先頭段落）が正しく切り出されることを固定する。
        #[test]
        fn squash_commits_preserves_composed_message_format() {
            let fx = TestRepo::new();
            fx.write_file("a.txt", "1\n");
            fx.stage_all();
            fx.commit("c1");
            fx.write_file("a.txt", "2\n");
            fx.stage_all();
            fx.commit("c2");
            fx.write_file("a.txt", "3\n");
            fx.stage_all();
            fx.commit("c3");

            let repo = fx.open();
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            let c3 = head.id();
            let c2 = head.parent(0).unwrap().id();

            // フロントエンドと同じ形式: 古い順のメッセージを "\n\n" で連結する。
            let composed = "c2\n\nc3";
            squash_commits(&repo, &[&c3.to_string(), &c2.to_string()], composed).unwrap();

            let repo = fx.open();
            let squashed = repo.head().unwrap().peel_to_commit().unwrap();
            let full_message = squashed.message().unwrap().to_string();
            let summary = squashed.summary().unwrap().unwrap_or("").to_string();

            #[derive(serde::Serialize)]
            struct SquashedMessageShape {
                summary: String,
                full_message: String,
            }

            // ここに現れる値はすべて決定的（コミットハッシュや時刻を含まない）ので
            // redaction は不要。
            insta::assert_yaml_snapshot!(SquashedMessageShape {
                summary,
                full_message
            });
        }

        // cherry-pick 成功時の CommitInfo の形式（#175）。
        #[test]
        fn cherry_pick_success_commit_info_shape() {
            let fx = TestRepo::new();
            fx.write_file("a.txt", "base\n");
            fx.stage_all();
            fx.commit("c1");

            {
                let repo = fx.open();
                create_branch(&repo, "feature").unwrap();
                switch_branch(&repo, "feature").unwrap();
            }
            fx.write_file("b.txt", "feature work\n");
            fx.stage_all();
            let feature_oid = fx.commit("feature: b.txt を追加");

            {
                let repo = fx.open();
                switch_branch(&repo, "main").unwrap();
            }
            fx.write_file("c.txt", "main work\n");
            fx.stage_all();
            fx.commit("main: c.txt を追加");

            let repo = fx.open();
            let info = cherry_pick(&repo, &feature_oid.to_string()).unwrap();

            assert_full_oid_format(&info.id);
            assert_short_oid_format(&info.short_id);
            assert_eq!(info.parent_ids.len(), 1);
            assert_full_oid_format(&info.parent_ids[0]);

            insta::assert_yaml_snapshot!(info, {
                ".id" => "[id]",
                ".short_id" => "[short_id]",
                ".time" => "[time]",
                ".parent_ids[]" => "[parent_id]",
            });
        }

        // cherry-pick がコンフリクトで Blocked になったときのメッセージ文言（#175）。
        // この文言は固定の日本語文字列で動的な値を含まないため、redaction は不要。
        #[test]
        fn cherry_pick_conflict_blocked_message_shape() {
            let fx = TestRepo::new();
            fx.write_file("a.txt", "base\n");
            fx.stage_all();
            fx.commit("c1");

            {
                let repo = fx.open();
                create_branch(&repo, "feature").unwrap();
                switch_branch(&repo, "feature").unwrap();
            }
            fx.write_file("a.txt", "feature change\n");
            fx.stage_all();
            let feature_oid = fx.commit("feature: a.txt を変更");

            {
                let repo = fx.open();
                switch_branch(&repo, "main").unwrap();
            }
            fx.write_file("a.txt", "main change\n");
            fx.stage_all();
            fx.commit("main: a.txt を変更");

            let repo = fx.open();
            let err = cherry_pick(&repo, &feature_oid.to_string()).unwrap_err();
            let message = match err {
                CoreError::Blocked(m) => m,
                other => panic!("Blocked エラーのはず: {other:?}"),
            };

            insta::assert_snapshot!(message);
        }
    }
}
