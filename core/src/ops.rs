use std::path::Path;

use git2::build::CheckoutBuilder;
use git2::{BranchType, Commit, IndexAddOption, Repository, ResetType};

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
    undo::push(
        repo,
        UndoEntry {
            op: OperationKind::Commit,
            description: format!("コミット「{}」を取り消す", first_line(message)),
            action,
        },
    )?;

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
    undo::push(
        repo,
        UndoEntry {
            op: OperationKind::CreateBranch,
            description: format!("ブランチ「{name}」の作成を取り消す"),
            action: UndoAction::DeleteBranch {
                name: name.to_string(),
            },
        },
    )?;
    Ok(())
}

/// 既存ブランチへ切り替える。未コミット変更と衝突する場合は安全のため失敗する。
pub fn switch_branch(repo: &Repository, name: &str) -> Result<()> {
    let name = name.trim();
    repo.find_branch(name, BranchType::Local).map_err(|_| {
        CoreError::InvalidInput(format!("ブランチ「{name}」が見つかりません。"))
    })?;

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
    undo::push(
        repo,
        UndoEntry {
            op: OperationKind::DeleteBranch,
            description: format!("ブランチ「{name}」の削除を取り消す"),
            action: UndoAction::RecreateBranch {
                name: name.to_string(),
                target: target.to_string(),
            },
        },
    )?;
    Ok(())
}

/// 指定地点までハードリセットする。破壊的操作。直後にコミット位置を Undo で戻せる。
pub fn reset_hard(repo: &Repository, revspec: &str) -> Result<()> {
    let prev = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .ok_or_else(|| CoreError::Blocked("まだコミットが無いためリセットできません。".to_string()))?;

    let obj = repo.revparse_single(revspec)?;
    let commit = obj
        .peel_to_commit()
        .map_err(|_| CoreError::InvalidInput(format!("コミットを特定できません: {revspec}")))?;

    repo.reset(commit.as_object(), ResetType::Hard, None)?;
    undo::push(
        repo,
        UndoEntry {
            op: OperationKind::ResetHard,
            description: "ハードリセットを取り消す（リセット前の位置に戻す）".to_string(),
            action: UndoAction::HardResetTo {
                previous: prev.to_string(),
            },
        },
    )?;
    Ok(())
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("").trim()
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
        assert_eq!(crate::repo::current_branch(&repo).as_deref(), Some("feature"));

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
