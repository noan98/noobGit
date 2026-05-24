use std::cell::Cell;
use std::path::Path;

use git2::build::CheckoutBuilder;
use git2::{
    BranchType, Commit, Cred, CredentialType, FetchOptions, IndexAddOption, RemoteCallbacks,
    Repository, ResetType,
};

use crate::error::{CoreError, Result};
use crate::model::{CommitInfo, FetchOutcome, PullOutcome};
use crate::repo::current_branch;
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

/// 指定パスをステージする。ファイルが消えていれば削除としてステージする。
pub fn stage_path(repo: &Repository, path: &str) -> Result<()> {
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

/// ステージされた変更をコミットする。直後に Undo で取り消せる。
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
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    let prev = repo.head().ok().and_then(|h| h.target());
    let branch = current_branch(repo).unwrap_or_else(|| "main".to_string());

    // コミットする変更があるか確認する。
    match prev {
        Some(p) => {
            let parent_tree = repo.find_commit(p)?.tree()?.id();
            if parent_tree == tree_id {
                return Err(CoreError::InvalidInput(
                    "コミットする変更がありません。先に変更をステージしてください。".to_string(),
                ));
            }
        }
        None => {
            if index.is_empty() {
                return Err(CoreError::InvalidInput(
                    "コミットする変更がありません。先に変更をステージしてください。".to_string(),
                ));
            }
        }
    }

    let parents: Vec<Commit> = match prev {
        Some(p) => vec![repo.find_commit(p)?],
        None => vec![],
    };
    let parent_refs: Vec<&Commit> = parents.iter().collect();

    let oid = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)?;

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
        summary: commit.summary().unwrap_or("").to_string(),
        author_name: author.name().unwrap_or("").to_string(),
        author_email: author.email().unwrap_or("").to_string(),
        time: commit.time().seconds(),
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

/// リモートから最新を取得し、リモート追跡ブランチ（例: `origin/main`）を更新する。
///
/// 作業ツリー・インデックス・現在ブランチには一切触れない安全操作。取り込む前に
/// 「何が来ているか」を確認するために使う。更新された追跡ブランチ数を返す。
pub fn fetch(repo: &Repository, remote_name: &str) -> Result<FetchOutcome> {
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

        let mut fo = FetchOptions::new();
        fo.remote_callbacks(cb);

        // リモートに設定された取得 refspec（例: +refs/heads/*:refs/remotes/origin/*）で取得する。
        let refspecs: Vec<String> = remote
            .fetch_refspecs()?
            .iter()
            .flatten()
            .map(|s| s.to_string())
            .collect();
        // refspec が空のリモートでは libgit2 が既定の refspec を補う。
        remote
            .fetch(&refspecs, Some(&mut fo), None)
            .map_err(|e| CoreError::Git(format!("取得（fetch）に失敗しました: {}", e.message())))?;
    }

    Ok(FetchOutcome {
        remote: remote_name.to_string(),
        updated_refs: updated.get(),
    })
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
    let branch = branch.trim();
    if branch.is_empty() {
        return Err(CoreError::InvalidInput(
            "取り込むブランチ名を指定してください。".to_string(),
        ));
    }

    // 1. まずリモートの最新を取得する（ネットワーク操作はここだけ）。
    fetch(repo, remote_name)?;
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
        .symbolic_target()
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

    #[test]
    fn fetch_unknown_remote_is_rejected() {
        let fx = TestRepo::new();
        let repo = fx.open();
        assert!(matches!(
            fetch(&repo, "origin").unwrap_err(),
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
}
