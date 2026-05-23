use std::fs;
use std::path::PathBuf;

use git2::{Repository, ResetType};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::safety::OperationKind;

/// 取り消し方法の種別。各書き込み操作が「どう戻すか」を記録する。
///
/// `previous` 等のコミットOidは、その操作直前のHEAD位置（reflogの1つ前に相当）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum UndoAction {
    /// ブランチ参照だけを戻す（作業ツリー・インデックスは保持）。コミットの取り消しに使う。
    SoftResetTo { previous: String },
    /// 指定地点まで強制的に戻す。ハードリセットの取り消しに使う。
    HardResetTo { previous: String },
    /// 削除したブランチを復元する。
    RecreateBranch { name: String, target: String },
    /// 作成したブランチを削除して取り消す。
    DeleteBranch { name: String },
    /// 最初のコミットを取り消し、未誕生ブランチに戻す。
    UncommitInitial { branch: String },
}

/// 取り消し履歴の1エントリ。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoEntry {
    pub op: OperationKind,
    /// 「何を取り消すのか」を表す日本語の説明。
    pub description: String,
    pub action: UndoAction,
}

fn journal_path(repo: &Repository) -> PathBuf {
    // repo.path() は .git ディレクトリを指す。リポジトリと一緒に運ばれ、無視もされる。
    repo.path().join("noobgit_undo.json")
}

fn load(repo: &Repository) -> Result<Vec<UndoEntry>> {
    let path = journal_path(repo);
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
            CoreError::Git(format!(
                "取り消し履歴を読み取れませんでした（ファイルが壊れている可能性があります）: {e}"
            ))
        }),
        // ファイルが無いのは「履歴なし」。それ以外の読み取りエラーは握りつぶさず返す。
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(CoreError::Git(format!(
            "取り消し履歴の読み取りに失敗しました: {e}"
        ))),
    }
}

fn save(repo: &Repository, entries: &[UndoEntry]) -> Result<()> {
    let path = journal_path(repo);
    let bytes = serde_json::to_vec_pretty(entries)
        .map_err(|e| CoreError::Git(format!("取り消し履歴の保存に失敗しました: {e}")))?;
    // 一時ファイルへ書いてから rename することで、書き込み途中の中断で
    // ジャーナルが壊れる（＝Undoが消える）のを防ぐ。
    let tmp = path.with_file_name("noobgit_undo.json.tmp");
    fs::write(&tmp, bytes)
        .map_err(|e| CoreError::Git(format!("取り消し履歴の保存に失敗しました: {e}")))?;
    fs::rename(&tmp, &path)
        .map_err(|e| CoreError::Git(format!("取り消し履歴の保存に失敗しました: {e}")))?;
    Ok(())
}

/// 取り消しエントリを履歴の末尾に追加する。
pub fn push(repo: &Repository, entry: UndoEntry) -> Result<()> {
    let mut entries = load(repo)?;
    entries.push(entry);
    save(repo, &entries)
}

/// 次に取り消される操作の説明を覗き見る（実行はしない）。
pub fn peek(repo: &Repository) -> Result<Option<UndoEntry>> {
    Ok(load(repo)?.pop())
}

/// 取り消せる操作があるか。
pub fn can_undo(repo: &Repository) -> Result<bool> {
    Ok(!load(repo)?.is_empty())
}

/// 直前の操作を取り消す。取り消した操作の説明を返す。
pub fn undo_last(repo: &Repository) -> Result<String> {
    let mut entries = load(repo)?;
    let entry = entries
        .pop()
        .ok_or_else(|| CoreError::NothingToUndo("取り消せる操作がありません。".to_string()))?;

    apply(repo, &entry.action)?;
    save(repo, &entries)?;
    Ok(entry.description)
}

fn apply(repo: &Repository, action: &UndoAction) -> Result<()> {
    match action {
        UndoAction::SoftResetTo { previous } => {
            let oid = git2::Oid::from_str(previous)?;
            let obj = repo.find_object(oid, None)?;
            repo.reset(&obj, ResetType::Soft, None)?;
        }
        UndoAction::HardResetTo { previous } => {
            let oid = git2::Oid::from_str(previous)?;
            let obj = repo.find_object(oid, None)?;
            repo.reset(&obj, ResetType::Hard, None)?;
        }
        UndoAction::RecreateBranch { name, target } => {
            let oid = git2::Oid::from_str(target)?;
            let commit = repo.find_commit(oid)?;
            repo.branch(name, &commit, false)?;
        }
        UndoAction::DeleteBranch { name } => {
            let mut branch = repo.find_branch(name, git2::BranchType::Local)?;
            branch.delete()?;
        }
        UndoAction::UncommitInitial { branch } => {
            let refname = format!("refs/heads/{branch}");
            if let Ok(mut r) = repo.find_reference(&refname) {
                r.delete()?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestRepo;

    #[test]
    fn nothing_to_undo_on_fresh_repo() {
        let fx = TestRepo::new();
        let repo = fx.open();
        assert!(!can_undo(&repo).unwrap());
        assert!(undo_last(&repo).is_err());
    }

    #[test]
    fn corrupt_journal_is_surfaced_not_swallowed() {
        let fx = TestRepo::new();
        let repo = fx.open();
        std::fs::write(repo.path().join("noobgit_undo.json"), b"{ broken json").unwrap();
        // 壊れた履歴を「履歴なし」と誤認せず、エラーとして返す。
        assert!(can_undo(&repo).is_err());
        assert!(peek(&repo).is_err());
        assert!(undo_last(&repo).is_err());
    }

    #[test]
    fn push_peek_and_undo_recreate_branch() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        let target = fx.head_oid();

        let repo = fx.open();
        repo.branch("temp", &repo.find_commit(target).unwrap(), false)
            .unwrap();
        // temp を削除してから、取り消しで復元する。
        repo.find_branch("temp", git2::BranchType::Local)
            .unwrap()
            .delete()
            .unwrap();
        assert!(repo.find_branch("temp", git2::BranchType::Local).is_err());

        push(
            &repo,
            UndoEntry {
                op: OperationKind::DeleteBranch,
                description: "ブランチ temp の削除を取り消す".into(),
                action: UndoAction::RecreateBranch {
                    name: "temp".into(),
                    target: target.to_string(),
                },
            },
        )
        .unwrap();

        assert!(peek(&repo).unwrap().is_some());
        let desc = undo_last(&repo).unwrap();
        assert!(desc.contains("temp"));
        assert!(repo.find_branch("temp", git2::BranchType::Local).is_ok());
        assert!(!can_undo(&repo).unwrap());
    }
}
