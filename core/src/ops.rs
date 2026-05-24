use std::cell::RefCell;
use std::path::Path;

use git2::build::CheckoutBuilder;
use git2::{
    BranchType, Commit, Cred, CredentialType, IndexAddOption, PushOptions, RemoteCallbacks,
    Repository, ResetType,
};

use crate::error::{CoreError, Result};
use crate::model::CommitInfo;
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
    let author = commit.author();
    Ok(CommitInfo {
        id: oid.to_string(),
        short_id: oid.to_string().chars().take(7).collect(),
        summary: commit.summary().unwrap_or("").to_string(),
        author_name: author.name().unwrap_or("").to_string(),
        author_email: author.email().unwrap_or("").to_string(),
        time: commit.time().seconds(),
    })
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

/// ローカルのコミットをリモートへ送信（push）する。
///
/// `remote` はリモート名（例: `origin`）、`refspec` は送信するブランチの指定
/// （例: `refs/heads/main:refs/heads/main`）。`force` が真のときは強制 push（リモートの
/// 履歴を上書き）を行う。push はローカルだけでは取り消せないため undo は記録しない。
pub fn push(repo: &Repository, remote: &str, refspec: &str, force: bool) -> Result<()> {
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

    {
        let mut callbacks = RemoteCallbacks::new();
        // 認証は SSH エージェント → 資格情報ヘルパ（トークン等）→ 既定 の順で試す。
        // ローカルパスのリモートでは認証は不要で、この callback は呼ばれない。
        callbacks.credentials(|url, username_from_url, allowed| {
            if allowed.contains(CredentialType::SSH_KEY) {
                return Cred::ssh_key_from_agent(username_from_url.unwrap_or("git"));
            }
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
        });
        // 各参照の更新結果。status が Some なら、その参照はリモートに拒否されている。
        callbacks.push_update_reference(|refname, status| {
            if let Some(msg) = status {
                *rejection.borrow_mut() = Some(format!("{refname}: {msg}"));
            }
            Ok(())
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
        _ => CoreError::Git(format!("リモートへの送信に失敗しました: {}", e.message())),
    }
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
