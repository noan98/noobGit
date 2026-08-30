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
    /// 指定パスのステージを解除する（変更内容は保持）。hunk 単位のステージの取り消しに使う。
    /// HEAD があれば HEAD からそのパスを index に戻し、無ければ index から取り除く（冪等）。
    UnstagePath { path: String },
    /// 削除したタグを再作成する。`message` が Some なら注釈付き、None なら軽量タグ。
    /// 既に同名タグがあれば何もしない（冪等）。
    RecreateTag {
        name: String,
        target: String,
        message: Option<String>,
    },
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

/// 書き込み時に使うジャーナルのスキーマバージョン。
///
/// v0: バージョンフィールドの無い裸の配列（旧形式。読み込みのみ対応）。
/// v1: `{ "version": 1, "entries": [...] }`。現行の書き込み形式。
const CURRENT_VERSION: u64 = 1;

/// 書き込み用のジャーナル全体表現。
#[derive(Serialize)]
struct JournalFile<'a> {
    version: u64,
    entries: &'a [UndoEntry],
}

/// ジャーナル本体のバイト列を**寛容に**パースし、[`UndoEntry`] の一覧を返す。
///
/// undo はベストエフォートという方針に沿い、次のいずれの場合もパニックや
/// 全体エラーにはせず、可能な限り多くの正常なエントリを生かす:
///
/// - 旧形式（バージョンフィールドの無い裸の配列。v0）はそのまま読める。
/// - 新形式（`{ "version": N, "entries": [...] }`）は、`N` が現在の
///   バージョンより大きい（将来のバージョンで書かれた）場合でも同じ形で読む。
/// - 個々のエントリが未知の `UndoAction` バリアントや型不一致で
///   デコードできない場合、そのエントリだけをスキップし、他は生かす
///   （未知フィールドは serde が元々無視するため、そのまま読める）。
/// - JSON 全体が構文的に壊れている（途中切断など）場合は、ファイル全体を
///   「履歴なし」として扱う（Undo が使えなくなるだけで、他の機能は壊さない）。
fn parse_journal(bytes: &[u8]) -> Vec<UndoEntry> {
    let value: serde_json::Value = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(_) => {
            // JSON として構文的に壊れている（途中切断等）。読める部分もないため、
            // 履歴なしとして扱う。パニックはしない。
            eprintln!("noobgit: 取り消し履歴のファイルが壊れているため、履歴なしとして扱います");
            return Vec::new();
        }
    };

    let raw_entries: Vec<serde_json::Value> = match value {
        // v0: バージョンフィールドの無い裸の配列。
        serde_json::Value::Array(arr) => arr,
        // v1 以降: { "version": N, "entries": [...] }。
        // N が現在のバージョンより大きくても（将来のバージョン）同じ形で読む。
        serde_json::Value::Object(mut map) => match map.remove("entries") {
            Some(serde_json::Value::Array(arr)) => arr,
            _ => Vec::new(),
        },
        // 想定外の形（数値・文字列など）は履歴なしとして扱う。
        _ => Vec::new(),
    };

    let mut entries = Vec::with_capacity(raw_entries.len());
    let mut skipped = 0usize;
    for raw in raw_entries {
        match serde_json::from_value::<UndoEntry>(raw) {
            Ok(entry) => entries.push(entry),
            // 未知の UndoAction バリアントや型不一致など、個々のデコード失敗は
            // そのエントリだけスキップし、他の正常なエントリは生かす。
            Err(_) => skipped += 1,
        }
    }
    if skipped > 0 {
        eprintln!(
            "noobgit: 取り消し履歴のうち{skipped}件のエントリを読み込めなかったためスキップしました"
        );
    }
    entries
}

fn load(repo: &Repository) -> Result<Vec<UndoEntry>> {
    let path = journal_path(repo);
    match fs::read(&path) {
        Ok(bytes) => Ok(parse_journal(&bytes)),
        // ファイルが無いのは「履歴なし」。それ以外の読み取りエラー（権限不足等）は
        // 握りつぶさず返す — こちらはファイル内容ではなく I/O の失敗のため。
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(CoreError::Git(format!(
            "取り消し履歴の読み取りに失敗しました: {e}"
        ))),
    }
}

fn save(repo: &Repository, entries: &[UndoEntry]) -> Result<()> {
    let path = journal_path(repo);
    let file = JournalFile {
        version: CURRENT_VERSION,
        entries,
    };
    let bytes = serde_json::to_vec_pretty(&file)
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

/// 取り消し履歴の全エントリを返す（古い順。先頭が最初に記録された操作）。
pub fn list(repo: &Repository) -> Result<Vec<UndoEntry>> {
    load(repo)
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
        UndoAction::UnstagePath { path } => {
            // ops::unstage と同じロジックを undo 側で再現する（冪等）。
            // HEAD があればそのパスを HEAD の内容で index に戻し、無ければ index から取り除く。
            let p = std::path::Path::new(path);
            match repo.head() {
                Ok(head) => {
                    let commit = head.peel_to_commit()?;
                    repo.reset_default(Some(commit.as_object()), [p])?;
                }
                Err(_) => {
                    // まだコミットが無い（未誕生ブランチ）。index に載っていれば外す。
                    let mut index = repo.index()?;
                    if index.get_path(p, 0).is_some() {
                        index.remove_path(p)?;
                        index.write()?;
                    }
                }
            }
        }
        UndoAction::RecreateTag {
            name,
            target,
            message,
        } => {
            // 既に同名タグがあれば何もしない（冪等）。
            if repo.find_reference(&format!("refs/tags/{name}")).is_err() {
                let oid = git2::Oid::from_str(target)?;
                let obj = repo.find_object(oid, None)?;
                match message {
                    Some(msg) if !msg.trim().is_empty() => {
                        // 注釈付きタグの再作成には署名が要る。取れなければ軽量タグで復元する。
                        if let Ok(sig) = repo.signature() {
                            repo.tag(name, &obj, &sig, msg, false)?;
                        } else {
                            repo.tag_lightweight(name, &obj, false)?;
                        }
                    }
                    _ => {
                        repo.tag_lightweight(name, &obj, false)?;
                    }
                }
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

    // JSON として構文的に壊れている（＝途中で切断された等）ジャーナルは、
    // パニックもエラーも起こさず「履歴なし」として扱う（undo はベストエフォート）。
    // 中身は読めなくても、他の noobGit の機能は壊さないという明文化テスト。
    #[test]
    fn corrupt_truncated_journal_is_treated_as_empty_not_panicking() {
        let fx = TestRepo::new();
        let repo = fx.open();
        std::fs::write(repo.path().join("noobgit_undo.json"), b"{ broken json").unwrap();

        assert!(!can_undo(&repo).unwrap());
        assert_eq!(peek(&repo).unwrap(), None);
        assert!(list(&repo).unwrap().is_empty());
        // undo_last は「取り消せる操作がありません」であって、パース失敗のエラーではない。
        assert!(matches!(
            undo_last(&repo).unwrap_err(),
            CoreError::NothingToUndo(_)
        ));
    }

    // バージョンフィールドの無い旧形式（v0: 裸の配列）を透過的に読み込めること。
    #[test]
    fn legacy_bare_array_journal_v0_is_read_transparently() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        let target = fx.head_oid().to_string();

        let repo = fx.open();
        // v0 形式（バージョンフィールド無しの裸の配列）を手で書く。
        let legacy = serde_json::json!([
            {
                "op": "delete_branch",
                "description": "旧形式のエントリ",
                "action": {
                    "action": "recreate_branch",
                    "name": "legacy-branch",
                    "target": target,
                }
            }
        ]);
        std::fs::write(
            repo.path().join("noobgit_undo.json"),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();

        let entries = list(&repo).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].description, "旧形式のエントリ");

        // 取り消しも実際に動く。
        let desc = undo_last(&repo).unwrap();
        assert!(desc.contains("旧形式"));
        assert!(repo
            .find_branch("legacy-branch", git2::BranchType::Local)
            .is_ok());
    }

    // 未知の UndoAction バリアントや未知フィールドが混ざっていても、
    // デコードできる正常なエントリだけが生き残り、全体が失敗しないこと。
    #[test]
    fn unknown_variant_and_unknown_field_entries_are_skipped_not_fatal() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        let target = fx.head_oid().to_string();

        let repo = fx.open();
        let journal = serde_json::json!({
            "version": 1,
            "entries": [
                {
                    "op": "delete_branch",
                    "description": "正常なエントリ1",
                    "action": {
                        "action": "recreate_branch",
                        "name": "keep-me",
                        "target": target,
                    }
                },
                {
                    "op": "delete_branch",
                    "description": "未知バリアントのエントリ",
                    "action": {
                        "action": "future_unknown_action",
                        "some_field": "some_value",
                    }
                },
                {
                    "op": "delete_branch",
                    "description": "未知フィールド付きの正常なエントリ",
                    "action": {
                        "action": "delete_branch",
                        "name": "some-branch",
                        "future_field": "無視されるはず",
                    }
                }
            ]
        });
        std::fs::write(
            repo.path().join("noobgit_undo.json"),
            serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();

        let entries = list(&repo).unwrap();
        // 未知バリアントの1件だけがスキップされ、残り2件は生きている。
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].description, "正常なエントリ1");
        assert_eq!(entries[1].description, "未知フィールド付きの正常なエントリ");
    }

    // 現在よりバージョン番号が大きい（将来のバージョンで書かれた）ジャーナルも、
    // 同じ形で寛容に読み込めること（正常なエントリは生きる）。
    #[test]
    fn future_version_journal_is_still_readable() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        let target = fx.head_oid().to_string();

        let repo = fx.open();
        let journal = serde_json::json!({
            "version": 999,
            "entries": [
                {
                    "op": "delete_branch",
                    "description": "未来バージョンのエントリ",
                    "action": {
                        "action": "recreate_branch",
                        "name": "future-branch",
                        "target": target,
                    }
                }
            ]
        });
        std::fs::write(
            repo.path().join("noobgit_undo.json"),
            serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();

        let entries = list(&repo).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].description, "未来バージョンのエントリ");
    }

    // 書き込みは新形式（v1: { "version": 1, "entries": [...] }）で行われること。
    #[test]
    fn save_writes_versioned_journal_format() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        let target = fx.head_oid().to_string();

        let repo = fx.open();
        push(
            &repo,
            UndoEntry {
                op: OperationKind::DeleteBranch,
                description: "test".into(),
                action: UndoAction::RecreateBranch {
                    name: "tmp-branch".into(),
                    target,
                },
            },
        )
        .unwrap();

        let bytes = std::fs::read(repo.path().join("noobgit_undo.json")).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["version"], serde_json::json!(1));
        assert!(value["entries"].is_array());
        assert_eq!(value["entries"].as_array().unwrap().len(), 1);
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

    // list() は古い順（先頭が最初に記録）でエントリを返し、件数が一致すること。
    #[test]
    fn list_returns_all_entries_in_push_order() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        let target = fx.head_oid().to_string();

        let repo = fx.open();
        // 3件のエントリを順番に積む。
        for i in 1..=3 {
            push(
                &repo,
                UndoEntry {
                    op: OperationKind::DeleteBranch,
                    description: format!("操作{i}"),
                    action: UndoAction::RecreateBranch {
                        name: format!("branch-{i}"),
                        target: target.clone(),
                    },
                },
            )
            .unwrap();
        }

        let entries = list(&repo).unwrap();
        // 件数が一致する。
        assert_eq!(entries.len(), 3);
        // 古い順（push した順）で返ってくる。
        assert_eq!(entries[0].description, "操作1");
        assert_eq!(entries[1].description, "操作2");
        assert_eq!(entries[2].description, "操作3");
    }

    // save が tmp ファイルを経由して rename するため、成功後に .tmp ファイルが残らないこと。
    #[test]
    fn journal_save_leaves_no_tmp_file() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        let target = fx.head_oid();

        let repo = fx.open();
        push(
            &repo,
            UndoEntry {
                op: crate::safety::OperationKind::DeleteBranch,
                description: "test".into(),
                action: UndoAction::RecreateBranch {
                    name: "tmp-branch".into(),
                    target: target.to_string(),
                },
            },
        )
        .unwrap();

        // rename が成功しているので .tmp ファイルは存在しない。
        let tmp = repo.path().join("noobgit_undo.json.tmp");
        assert!(!tmp.exists(), ".tmp ファイルが残留している");

        // ジャーナル本体は書き込まれている。
        assert!(repo.path().join("noobgit_undo.json").exists());
    }

    // discard_path は不可逆なので undo を記録しない。
    #[test]
    fn discard_path_does_not_record_undo() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "original");
        fx.stage_all();
        fx.commit("c1");

        // 変更して discard する。
        fx.write_file("a.txt", "modified");
        let repo = fx.open();
        crate::ops::discard_path(&repo, "a.txt").unwrap();

        // undo エントリは積まれていない。
        assert!(!can_undo(&repo).unwrap());
        assert!(peek(&repo).unwrap().is_none());
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
