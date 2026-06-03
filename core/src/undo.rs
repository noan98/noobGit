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
    /// 退避（stash）を取り消す。記録時の退避コミットを `id` で探して pop（取り出し）する。
    /// 該当 id が見つからない（すでに取り出し済み）なら何もしない（冪等）。
    PopStash { id: String },
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
    Ok(load(repo)?.last().cloned())
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

    // apply の成否にかかわらずエントリを消費する。
    // apply が失敗しても再実行すると同じ結果になるため、消費して次の Undo が動けるようにする
    // （例: stash pop のコンフリクト時に同じエントリで失敗し続ける「ブロック状態」を防ぐ）。
    let result = apply(repo, &entry.action);
    save(repo, &entries)?;
    result?;
    Ok(entry.description)
}

// apply は冪等に保つ。undo_last は apply 後に save するため、apply 成功・save 失敗の後で
// 同じUndoを再実行しても「branch already exists」「reference not found」等で壊れないようにする。
// （ベストエフォート方針に沿い、進行中マーカー等の重い二段階更新は採らない。）
fn apply(repo: &Repository, action: &UndoAction) -> Result<()> {
    match action {
        // 固定oidへのリセットは何度実行しても同じ結果になる（冪等）。
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
            // 既に復元済みなら何もしない。
            if repo.find_branch(name, git2::BranchType::Local).is_err() {
                let oid = git2::Oid::from_str(target)?;
                let commit = repo.find_commit(oid)?;
                repo.branch(name, &commit, false)?;
            }
        }
        UndoAction::DeleteBranch { name } => {
            // 既に削除済みなら何もしない。
            if let Ok(mut branch) = repo.find_branch(name, git2::BranchType::Local) {
                branch.delete()?;
            }
        }
        UndoAction::UncommitInitial { branch } => {
            let refname = format!("refs/heads/{branch}");
            if let Ok(mut r) = repo.find_reference(&refname) {
                r.delete()?;
            }
        }
        UndoAction::PopStash { id } => {
            // stash 操作は &mut Repository を要するので、同じパスで開き直す。
            let mut r = Repository::open(repo.path())?;
            let target = git2::Oid::from_str(id)?;
            // 記録時の退避コミットと一致する退避の index を探す。
            let mut found: Option<usize> = None;
            r.stash_foreach(|index, _message, oid| {
                if *oid == target {
                    found = Some(index);
                    false
                } else {
                    true
                }
            })?;
            // 見つかったときだけ pop する。無ければ取り出し済みとみなし何もしない（冪等）。
            if let Some(index) = found {
                r.stash_pop(index, None)?;
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

    // 退避(stash)の取り消し(PopStash)で変更が作業ツリーに戻り、再適用しても壊れない（冪等）。
    #[test]
    fn pop_stash_undo_restores_changes_and_is_idempotent() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        // 変更を作って退避する（stash_save が PopStash の undo を積む）。
        fx.write_file("a.txt", "2");
        let stash_id = {
            let mut repo = fx.open();
            crate::ops::stash_save(&mut repo, "wip").unwrap();
            match peek(&repo).unwrap().unwrap().action {
                UndoAction::PopStash { id } => id,
                other => panic!("PopStash を期待したが {other:?} だった"),
            }
        };
        // 退避後は作業ツリーがクリーン。
        assert!(crate::repo::status(&fx.open()).unwrap().is_clean);

        // 1回目の適用: 退避を取り出して変更が戻る。
        let action = UndoAction::PopStash { id: stash_id };
        let repo = fx.open();
        apply(&repo, &action).unwrap();
        assert_eq!(
            std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
            "2"
        );

        // 2回目の適用: 該当の退避はもう無いので no-op（エラーにならない）。
        apply(&fx.open(), &action).unwrap();
    }

    // stash pop がコンフリクトで失敗しても、エントリは消費されて次の Undo が動くこと。
    // 修正前: apply 失敗 → save が呼ばれず → エントリが残る → 次の undo_last も同じエラー（永久ブロック）。
    // 修正後: apply の成否にかかわらず save して消費する → 次の undo_last は NothingToUndo になる。
    #[test]
    fn pop_stash_conflict_does_not_permanently_block_undo() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "base");
        fx.stage_all();
        fx.commit("c1");

        // 変更を退避する（PopStash の undo エントリを積む）。
        fx.write_file("a.txt", "stashed");
        {
            let mut repo = fx.open();
            crate::ops::stash_save(&mut repo, "wip").unwrap();
        }
        // 退避後の作業ツリーは a.txt = "base"（コミット状態）。

        // コンフリクトを起こす変更を作業ツリーに加える（stash のベース "base" とも "stashed" とも違う）。
        fx.write_file("a.txt", "conflict");

        // undo_last: stash_pop を試みるがコンフリクトでエラーになる。
        let repo = fx.open();
        let err = undo_last(&repo).unwrap_err();
        assert!(
            matches!(err, CoreError::Blocked(_) | CoreError::Git(_)),
            "stash コンフリクト時に何らかのエラーが返ること: {err:?}"
        );

        // エントリは消費済みなので、次の undo_last は NothingToUndo になる（ブロックされない）。
        let err2 = undo_last(&repo).unwrap_err();
        assert!(
            matches!(err2, CoreError::NothingToUndo(_)),
            "エントリ消費後は NothingToUndo になること: {err2:?}"
        );
    }

    // apply 後に save が失敗して同じUndoが再実行される事態に備え、apply は冪等であること。
    #[test]
    fn apply_is_idempotent_for_branch_actions() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        let target = fx.head_oid().to_string();

        let repo = fx.open();

        // RecreateBranch: 2回適用してもエラーにならず、ブランチが存在する。
        let recreate = UndoAction::RecreateBranch {
            name: "feature".into(),
            target: target.clone(),
        };
        apply(&repo, &recreate).unwrap();
        apply(&repo, &recreate).unwrap();
        assert!(repo.find_branch("feature", git2::BranchType::Local).is_ok());

        // DeleteBranch: 2回適用してもエラーにならず、ブランチが消えている。
        let delete = UndoAction::DeleteBranch {
            name: "feature".into(),
        };
        apply(&repo, &delete).unwrap();
        apply(&repo, &delete).unwrap();
        assert!(repo
            .find_branch("feature", git2::BranchType::Local)
            .is_err());
    }
}
