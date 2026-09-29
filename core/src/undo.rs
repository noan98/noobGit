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
    /// 指定パスのインデックスエントリを、記録した blob（と実行モード）に置き換える。
    /// hunk 単位のアンステージ（`unstage_hunk`）の取り消し（再ステージ）に使う。
    /// `blob` が `None` なら操作前にそのパスがインデックスに無かったことを表し、
    /// 取り消しは index からそのパスを取り除く。同じ内容を何度適用しても結果は
    /// 変わらない（冪等）。
    RestoreIndexEntry {
        path: String,
        blob: Option<String>,
        mode: u32,
    },
    /// 削除したタグを再作成する。`message` が Some なら注釈付き、None なら軽量タグ。
    /// 既に同名タグがあれば何もしない（冪等）。
    RecreateTag {
        name: String,
        target: String,
        message: Option<String>,
    },
    /// Bisect（バグ混入コミットの二分探索）セッションの開始を取り消し、開始前の
    /// ブランチ（`original_branch` が Some）または具体的なコミット（`original_commit`。
    /// `original_branch` が None、または当該ブランチが既に削除されている場合の
    /// フォールバック）へ戻す。Bisect セッション全体を「開始前の状態」へ一括で巻き戻す
    /// という設計（判定=`bisect_mark`ごとの個別 undo は記録しない）。
    /// `crate::bisect::bisect_reset` と同じ復元ロジックを使うため冪等
    /// （セッションが既に手動で終了していても、同じ場所へチェックアウトし直すだけ）。
    RestoreBisectHead {
        original_branch: Option<String>,
        original_commit: String,
    },
    /// 作成したタグを削除して取り消す。既に削除済みなら何もしない（冪等）。
    DeleteTag { name: String },
    /// detached HEAD の救出（ブランチ作成＋そこへ乗り換え）を取り消し、元の detached HEAD
    /// （`commit`）に戻してブランチ `branch` を削除する。ブランチが救出後に進んでいる
    /// （先端が `commit` でない）場合は、コミットを失わせないよう何もせず中断する。
    /// 既に取り消し済み（ブランチが無い）なら何もしない（冪等）。
    RestoreDetachedHead { commit: String, branch: String },
}

/// 取り消し履歴の1エントリ。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoEntry {
    pub op: OperationKind,
    /// 「何を取り消すのか」を表す日本語の説明。
    pub description: String,
    pub action: UndoAction,
    /// 記録した時点（操作が成功した直後）の HEAD コミット id。記録後に外部ツール等で
    /// 履歴が進んだかどうかを [`validate_entry`] が判定するために使う。
    /// 旧形式のジャーナルには無いので `Option` + `#[serde(default)]`（後方互換）。
    /// 記録側で `None` のまま [`push`] すると、その時点の HEAD が自動で入る。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_at_record: Option<String>,
}

/// 取り消しエントリを「今のリポジトリ状態で適用してよいか」を検証した結果（Issue #201）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UndoApplicability {
    /// そのまま安全に適用できる。
    Applicable,
    /// すでに取り消したのと同じ状態になっている（適用しても何も変わらない。冪等）。
    AlreadyUndone,
    /// 記録した oid や参照が見つからず、適用できない（GC 済み・参照が削除済みなど）。
    Unresolvable { reason: String },
    /// 適用はできるが、記録後に履歴が進んでおり、新しい作業も一緒に巻き戻る恐れがある。
    Risky { reason: String },
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
pub fn push(repo: &Repository, mut entry: UndoEntry) -> Result<()> {
    if entry.head_at_record.is_none() {
        entry.head_at_record = current_head(repo).map(|o| o.to_string());
    }
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

/// 各エントリを現在のリポジトリ状態で検証した結果を返す（履歴と同じ古い順）。
/// 実際に適用されるのは末尾（最新）のエントリだけなので、それ以前のエントリの
/// 結果は「今それを適用したら」という参考情報になる。
pub fn validate_journal(repo: &Repository) -> Result<Vec<UndoApplicability>> {
    Ok(load(repo)?
        .iter()
        .map(|e| validate_entry(repo, e))
        .collect())
}

/// 適用不能（`Unresolvable`）なエントリを履歴から取り除き、取り除いた件数を返す。
pub fn prune_unresolvable(repo: &Repository) -> Result<usize> {
    let entries = load(repo)?;
    let before = entries.len();
    let kept: Vec<UndoEntry> = entries
        .into_iter()
        .filter(|e| {
            !matches!(
                validate_entry(repo, e),
                UndoApplicability::Unresolvable { .. }
            )
        })
        .collect();
    let removed = before - kept.len();
    if removed > 0 {
        save(repo, &kept)?;
    }
    Ok(removed)
}

fn current_head(repo: &Repository) -> Option<git2::Oid> {
    repo.head().ok()?.peel_to_commit().ok().map(|c| c.id())
}

fn commit_exists(repo: &Repository, oid_str: &str) -> bool {
    git2::Oid::from_str(oid_str)
        .ok()
        .is_some_and(|oid| repo.find_object(oid, None).is_ok())
}

/// `from` から辿れて `keep` から辿れないコミット（＝ `keep` へ戻すと届かなくなるもの）の
/// 件名を新しい順に返す。
fn commits_lost(repo: &Repository, from: git2::Oid, keep: Option<git2::Oid>) -> Vec<String> {
    let Ok(mut walk) = repo.revwalk() else {
        return Vec::new();
    };
    if walk.push(from).is_err() {
        return Vec::new();
    }
    if let Some(k) = keep {
        let _ = walk.hide(k);
    }
    walk.filter_map(|r| r.ok())
        .filter_map(|oid| repo.find_commit(oid).ok())
        .map(|c| c.summary().ok().flatten().unwrap_or("").to_string())
        .collect()
}

fn risky_lost_commits(lost: &[String], what: &str) -> UndoApplicability {
    let shown: Vec<String> = lost.iter().take(5).map(|s| format!("「{s}」")).collect();
    let more = if lost.len() > 5 {
        format!(" ほか{}件", lost.len() - 5)
    } else {
        String::new()
    };
    UndoApplicability::Risky {
        reason: format!(
            "この操作の記録後に、外部のツールなどで履歴が進んでいます。取り消すと、{what}{}件のコミットも一緒に巻き戻ります: {}{more}",
            lost.len(),
            shown.join(" ")
        ),
    }
}

/// エントリを「今のリポジトリ状態で適用してよいか」検証する（読み取り専用）。
///
/// 判定できない場合（記録に HEAD が無い旧形式など）は `Applicable` として扱い、
/// 従来どおりの動作を保つ。
pub fn validate_entry(repo: &Repository, entry: &UndoEntry) -> UndoApplicability {
    use UndoApplicability::*;
    let missing = |what: &str| {
        Unresolvable {
        reason: format!("{what}が見つからないため（削除・整理済みの可能性があります）、この操作は取り消せません。"),
    }
    };
    match &entry.action {
        UndoAction::SoftResetTo { previous } | UndoAction::HardResetTo { previous } => {
            let Ok(prev) = git2::Oid::from_str(previous) else {
                return missing("戻し先のコミット");
            };
            if repo.find_object(prev, None).is_err() {
                return missing("戻し先のコミット");
            }
            let head = current_head(repo);
            if head == Some(prev) {
                return AlreadyUndone;
            }
            let moved = match (&entry.head_at_record, head) {
                (Some(rec), Some(h)) => rec != &h.to_string(),
                (Some(_), None) => true,
                (None, _) => false,
            };
            if moved {
                if let Some(h) = head {
                    let lost = commits_lost(repo, h, Some(prev));
                    if !lost.is_empty() {
                        return risky_lost_commits(&lost, "あとから積まれた");
                    }
                }
            }
            Applicable
        }
        UndoAction::RecreateBranch { name, target } => {
            if repo.find_branch(name, git2::BranchType::Local).is_ok() {
                AlreadyUndone
            } else if !commit_exists(repo, target) {
                missing("ブランチの復元先のコミット")
            } else {
                Applicable
            }
        }
        UndoAction::DeleteBranch { name } => {
            let Ok(branch) = repo.find_branch(name, git2::BranchType::Local) else {
                return AlreadyUndone;
            };
            let tip = branch.get().peel_to_commit().ok().map(|c| c.id());
            if let (Some(rec), Some(tip)) = (&entry.head_at_record, tip) {
                if rec != &tip.to_string() {
                    let base = git2::Oid::from_str(rec).ok();
                    let lost = commits_lost(repo, tip, base);
                    if !lost.is_empty() {
                        return risky_lost_commits(
                            &lost,
                            &format!("ブランチ「{name}」に作成後に積まれた"),
                        );
                    }
                }
            }
            Applicable
        }
        UndoAction::UncommitInitial { branch } => {
            let refname = format!("refs/heads/{branch}");
            let Ok(r) = repo.find_reference(&refname) else {
                return AlreadyUndone;
            };
            let tip = r.peel_to_commit().ok().map(|c| c.id());
            if let (Some(rec), Some(tip)) = (&entry.head_at_record, tip) {
                if rec != &tip.to_string() {
                    let lost = commits_lost(repo, tip, None);
                    return risky_lost_commits(&lost, "ブランチ上のあとから積まれた");
                }
            }
            Applicable
        }
        UndoAction::PopStash { id } => {
            let Ok(mut r) = Repository::open(repo.path()) else {
                return Applicable;
            };
            let Ok(target) = git2::Oid::from_str(id) else {
                return missing("退避（stash）");
            };
            let mut found = false;
            let _ = r.stash_foreach(|_, _, oid| {
                if *oid == target {
                    found = true;
                    false
                } else {
                    true
                }
            });
            if found {
                Applicable
            } else {
                AlreadyUndone
            }
        }
        UndoAction::UnstagePath { .. } => Applicable,
        UndoAction::RestoreIndexEntry { path, blob, .. } => {
            let Ok(index) = repo.index() else {
                return Applicable;
            };
            let current = index.get_path(std::path::Path::new(path), 0);
            match blob {
                Some(b) => {
                    let Ok(oid) = git2::Oid::from_str(b) else {
                        return missing("ステージ内容");
                    };
                    if repo.find_blob(oid).is_err() {
                        return missing("ステージ内容");
                    }
                    if current.is_some_and(|e| e.id == oid) {
                        AlreadyUndone
                    } else {
                        Applicable
                    }
                }
                None => {
                    if current.is_none() {
                        AlreadyUndone
                    } else {
                        Applicable
                    }
                }
            }
        }
        UndoAction::RecreateTag { name, target, .. } => {
            if repo.find_reference(&format!("refs/tags/{name}")).is_ok() {
                AlreadyUndone
            } else if !commit_exists(repo, target) {
                missing("タグの付け先")
            } else {
                Applicable
            }
        }
        UndoAction::DeleteTag { name } => {
            if repo.find_reference(&format!("refs/tags/{name}")).is_ok() {
                Applicable
            } else {
                AlreadyUndone
            }
        }
        UndoAction::RestoreBisectHead {
            original_commit, ..
        } => {
            if commit_exists(repo, original_commit) {
                Applicable
            } else {
                missing("Bisect 開始前のコミット")
            }
        }
    }
}

/// 直前の操作を取り消す。取り消した操作の説明を返す。
///
/// 履歴が進んでいて新しい作業も巻き戻る恐れがある（`Risky`）場合は、何も変えずに
/// [`CoreError::Blocked`] で中断する。確認のうえで進めるなら [`undo_last_confirmed`]。
pub fn undo_last(repo: &Repository) -> Result<String> {
    undo_last_confirmed(repo, false)
}

/// [`undo_last`] の確認済みフラグ付き版。`confirm_risky` が true のときだけ
/// `Risky` なエントリも適用する。適用不能（`Unresolvable`）なエントリは、
/// 履歴から取り除いたうえで平易なエラーを返す。
pub fn undo_last_confirmed(repo: &Repository, confirm_risky: bool) -> Result<String> {
    let mut entries = load(repo)?;
    let entry = entries
        .last()
        .cloned()
        .ok_or_else(|| CoreError::NothingToUndo("取り消せる操作がありません。".to_string()))?;

    match validate_entry(repo, &entry) {
        UndoApplicability::Risky { reason } if !confirm_risky => {
            // エントリは消費しない（確認後にやり直せるように）。
            return Err(CoreError::Blocked(format!(
                "{reason}（確認のうえ実行してください）"
            )));
        }
        UndoApplicability::Unresolvable { reason } => {
            // 二度と適用できないので履歴から整理し、次の Undo が詰まらないようにする。
            entries.pop();
            save(repo, &entries)?;
            return Err(CoreError::NothingToUndo(format!(
                "「{}」: {reason} この履歴は整理しました。",
                entry.description
            )));
        }
        _ => {}
    }

    // apply の成否にかかわらずエントリを消費する。
    // apply が失敗しても再実行すると同じ結果になるため、消費して次の Undo が動けるようにする
    // （例: stash pop のコンフリクト時に同じエントリで失敗し続ける「ブロック状態」を防ぐ）。
    entries.pop();
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
                // 退避前のステージ状態（どのファイルをステージしていたか）も含めて
                // 元通りにするため、インデックスも復元する（reinstantiate_index）。
                let mut opts = git2::StashApplyOptions::new();
                opts.reinstantiate_index();
                r.stash_pop(index, Some(&mut opts))?;
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
        UndoAction::RestoreIndexEntry { path, blob, mode } => {
            let mut index = repo.index()?;
            let p = std::path::Path::new(path);
            match blob {
                Some(blob_str) => {
                    let oid = git2::Oid::from_str(blob_str)?;
                    // ctime/mtime 等は 0 のままでよい（libgit2 は次回のステータス走査時に
                    // 実ファイルと比較して自動的に再計算する）。mode と id と path だけが
                    // 復元に必要な情報。
                    let entry = git2::IndexEntry {
                        ctime: git2::IndexTime::new(0, 0),
                        mtime: git2::IndexTime::new(0, 0),
                        dev: 0,
                        ino: 0,
                        mode: *mode,
                        uid: 0,
                        gid: 0,
                        file_size: 0,
                        id: oid,
                        flags: 0,
                        flags_extended: 0,
                        path: path.as_bytes().to_vec(),
                    };
                    index.add(&entry)?;
                }
                None => {
                    // 操作前はこのパスがインデックスに無かった。すでに無ければ何もしない（冪等）。
                    if index.get_path(p, 0).is_some() {
                        index.remove_path(p)?;
                    }
                }
            }
            index.write()?;
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
        UndoAction::RestoreBisectHead {
            original_branch,
            original_commit,
        } => {
            // 復元は強制チェックアウトなので、未コミットの変更があると消してしまう。
            // bisect 終了後に作業を続けてから「取り消し」を押すこともあるため、
            // その場合は何も変えずに中断する（bisect_reset と同じ安全ルール）。
            if crate::repo::is_dirty(repo)? {
                return Err(CoreError::Blocked(
                    "未コミットの変更があるため、Bisect の開始を取り消せません。先にコミットするか退避(stash)してください。"
                        .to_string(),
                ));
            }
            // bisect_reset と同じ復元ロジックを共有する（冪等: 同じ場所へ checkout し
            // 直すだけなので、セッションが既に手動で終了していても壊れない）。
            crate::bisect::restore_original_head(
                repo,
                original_branch.as_deref(),
                original_commit,
            )?;
            // セッションファイルもあわせて破棄する（無ければ何もしない＝冪等）。
            // 破棄に失敗しても、根底の HEAD 復元は既に成功しているので undo 自体は
            // 成功として扱う（ベストエフォート方針）。
            let _ = crate::bisect::clear_session(repo);
        }
        UndoAction::RestoreDetachedHead { commit, branch } => {
            // 既に取り消し済み（ブランチが無い）なら何もしない（冪等）。
            if let Ok(mut b) = repo.find_branch(branch, git2::BranchType::Local) {
                let oid = git2::Oid::from_str(commit)?;
                // 救出後にそのブランチへコミットを積んでいたら、消すとコミットを見失うので中断。
                if b.get().target() != Some(oid) {
                    return Err(CoreError::Blocked(format!(
                        "ブランチ「{branch}」にはその後のコミットがあるため、取り消せません。"
                    )));
                }
                // 先に HEAD を detached に戻してからブランチを消す（HEAD 中のブランチは消せない）。
                // 先端が同じコミットなので、作業ツリー・インデックスは触らない。
                if b.is_head() {
                    repo.set_head_detached(oid)?;
                }
                b.delete()?;
            }
        }
        UndoAction::DeleteTag { name } => {
            // 既に削除済みなら何もしない（冪等）。
            if repo.find_reference(&format!("refs/tags/{name}")).is_ok() {
                repo.tag_delete(name)?;
            }
        }
    }
    Ok(())
}

// プロパティベーステスト（proptest）。private な `apply` を検証するため子モジュールにする。
#[cfg(test)]
mod proptests;

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
                head_at_record: None,
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
                head_at_record: None,
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
                    head_at_record: None,
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
                head_at_record: None,
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

    // SoftResetTo: 固定oidへのソフトリセットは2回適用してもエラーにならず、
    // HEADが同じ位置に留まり、インデックス・作業ツリーは変わらない（冪等）。
    #[test]
    fn apply_is_idempotent_for_soft_reset_to() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        let first = fx.head_oid().to_string();

        fx.write_file("a.txt", "2");
        fx.stage_all();
        fx.commit("c2");

        let repo = fx.open();
        let action = UndoAction::SoftResetTo {
            previous: first.clone(),
        };
        apply(&repo, &action).unwrap();
        assert_eq!(repo.head().unwrap().target().unwrap().to_string(), first);

        // 2回目も成功し、HEADは同じ位置のまま。
        apply(&repo, &action).unwrap();
        assert_eq!(repo.head().unwrap().target().unwrap().to_string(), first);
    }

    // HardResetTo: 固定oidへのハードリセットも2回適用してエラーにならず、
    // 作業ツリーの内容も同じ結果に落ち着く（冪等）。
    #[test]
    fn apply_is_idempotent_for_hard_reset_to() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        let first = fx.head_oid().to_string();

        fx.write_file("a.txt", "2");
        fx.stage_all();
        fx.commit("c2");

        let repo = fx.open();
        let action = UndoAction::HardResetTo {
            previous: first.clone(),
        };
        apply(&repo, &action).unwrap();
        assert_eq!(repo.head().unwrap().target().unwrap().to_string(), first);
        assert_eq!(
            std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
            "1"
        );

        apply(&repo, &action).unwrap();
        assert_eq!(repo.head().unwrap().target().unwrap().to_string(), first);
        assert_eq!(
            std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
            "1"
        );
    }

    // UncommitInitial: 最初のコミットを取り消すブランチ参照の削除は、
    // 2回適用しても（2回目は参照が既に無いので no-op）エラーにならない（冪等）。
    #[test]
    fn apply_is_idempotent_for_uncommit_initial() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        // 前提: 適用前は main が存在する（存在しないまま「無いこと」を確かめる空振りを防ぐ）。
        assert!(repo.find_reference("refs/heads/main").is_ok());
        let action = UndoAction::UncommitInitial {
            branch: "main".into(),
        };
        apply(&repo, &action).unwrap();
        assert!(repo.find_reference("refs/heads/main").is_err());

        // 2回目: 参照は既に無いので no-op。エラーにならない。
        apply(&repo, &action).unwrap();
        assert!(repo.find_reference("refs/heads/main").is_err());
    }

    // UnstagePath: ステージ済みの変更をHEADの内容に戻す（アンステージ）操作を
    // 2回適用しても、エラーにならず結果（未ステージ）が変わらない（冪等）。
    #[test]
    fn apply_is_idempotent_for_unstage_path() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        // HEADと異なる内容をステージする。
        fx.write_file("a.txt", "2");
        fx.stage_all();

        let repo = fx.open();
        // 前提: 適用前はステージ済みの変更がある。
        assert_eq!(crate::repo::status(&repo).unwrap().staged.len(), 1);
        let action = UndoAction::UnstagePath {
            path: "a.txt".into(),
        };
        apply(&repo, &action).unwrap();
        assert!(crate::repo::status(&repo).unwrap().staged.is_empty());

        // 2回目も成功し、引き続き未ステージのまま。
        apply(&repo, &action).unwrap();
        assert!(crate::repo::status(&repo).unwrap().staged.is_empty());
    }

    // RecreateTag: 削除したタグの再作成は、2回適用しても（2回目は同名タグが
    // 既に存在するので no-op）エラーにならない（冪等）。軽量・注釈付きの両方を確認する。
    #[test]
    fn apply_is_idempotent_for_recreate_tag() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        let target = fx.head_oid().to_string();

        let repo = fx.open();

        // 軽量タグ。
        let lightweight = UndoAction::RecreateTag {
            name: "v1".into(),
            target: target.clone(),
            message: None,
        };
        apply(&repo, &lightweight).unwrap();
        assert!(repo.find_reference("refs/tags/v1").is_ok());
        apply(&repo, &lightweight).unwrap();
        assert!(repo.find_reference("refs/tags/v1").is_ok());

        // 注釈付きタグ。
        let annotated = UndoAction::RecreateTag {
            name: "v2".into(),
            target,
            message: Some("リリース v2".into()),
        };
        apply(&repo, &annotated).unwrap();
        assert!(repo.find_reference("refs/tags/v2").is_ok());
        apply(&repo, &annotated).unwrap();
        assert!(repo.find_reference("refs/tags/v2").is_ok());
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

    // RestoreIndexEntry: 同じ内容を2回適用してもインデックスの状態は変わらない（冪等）。
    // blob が Some（既存パスの復元）と None（操作前は未追跡だったパスの復元）の両方を確認する。
    #[test]
    fn restore_index_entry_apply_is_idempotent_for_both_some_and_none_blob() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "hello");
        fx.stage_all();
        fx.commit("c1");
        let repo = fx.open();
        let blob_id = repo
            .index()
            .unwrap()
            .get_path(std::path::Path::new("a.txt"), 0)
            .unwrap()
            .id
            .to_string();

        // Some(blob): インデックスから外してから、記録した blob へ戻す。2回適用しても同じ。
        {
            let mut index = repo.index().unwrap();
            index.remove_path(std::path::Path::new("a.txt")).unwrap();
            index.write().unwrap();
        }
        let restore = UndoAction::RestoreIndexEntry {
            path: "a.txt".into(),
            blob: Some(blob_id.clone()),
            mode: 0o100644,
        };
        apply(&repo, &restore).unwrap();
        apply(&repo, &restore).unwrap();
        let index = repo.index().unwrap();
        let entry = index
            .get_path(std::path::Path::new("a.txt"), 0)
            .expect("a.txt がインデックスに復元されていること");
        assert_eq!(entry.id.to_string(), blob_id);

        // None: 操作前はパスがインデックスに無かったケース。2回適用してもエラーにならず、
        // インデックスに存在しないまま（冪等）。
        let remove = UndoAction::RestoreIndexEntry {
            path: "a.txt".into(),
            blob: None,
            mode: 0,
        };
        apply(&repo, &remove).unwrap();
        apply(&repo, &remove).unwrap();
        let index = repo.index().unwrap();
        assert!(index.get_path(std::path::Path::new("a.txt"), 0).is_none());
    }

    // ---- Issue #201: stale（失効）検出 ----

    fn entry(op: OperationKind, action: UndoAction) -> UndoEntry {
        UndoEntry {
            head_at_record: None,
            op,
            description: "テスト用".into(),
            action,
        }
    }

    // head_at_record の無い旧形式エントリが読め、Risky 判定はされず従来どおり適用できる。
    #[test]
    fn legacy_entry_without_head_at_record_is_readable_and_applicable() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        let c1 = fx.commit("c1");
        fx.write_file("a.txt", "2");
        fx.stage_all();
        fx.commit("c2");
        let repo = fx.open();
        let legacy = serde_json::json!({
            "version": 1,
            "entries": [{
                "op": "commit",
                "description": "旧形式",
                "action": { "action": "soft_reset_to", "previous": c1.to_string() }
            }]
        });
        std::fs::write(
            repo.path().join("noobgit_undo.json"),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();
        let entries = list(&repo).unwrap();
        assert_eq!(entries[0].head_at_record, None);
        assert_eq!(
            validate_entry(&repo, &entries[0]),
            UndoApplicability::Applicable
        );
        undo_last(&repo).unwrap();
        assert_eq!(fx.open().head().unwrap().target().unwrap(), c1);
    }

    // push は head_at_record を自動で埋め、保存後も読み戻せる。
    #[test]
    fn push_fills_head_at_record() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        let c1 = fx.commit("c1");
        let repo = fx.open();
        push(
            &repo,
            entry(
                OperationKind::CreateBranch,
                UndoAction::DeleteBranch { name: "x".into() },
            ),
        )
        .unwrap();
        let e = peek(&repo).unwrap().unwrap();
        assert_eq!(e.head_at_record, Some(c1.to_string()));
    }

    #[test]
    fn soft_reset_applicable_when_head_unchanged() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        let c1 = fx.commit("c1");
        fx.write_file("a.txt", "2");
        fx.stage_all();
        fx.commit("c2");
        let repo = fx.open();
        let e = entry(
            OperationKind::Commit,
            UndoAction::SoftResetTo {
                previous: c1.to_string(),
            },
        );
        push(&repo, e).unwrap();
        let e = peek(&repo).unwrap().unwrap();
        assert_eq!(validate_entry(&repo, &e), UndoApplicability::Applicable);
    }

    // 外部で履歴が進んだ後の undo は、確認なしでは新しいコミットを巻き戻さない。
    #[test]
    fn soft_reset_is_risky_after_external_commits_and_needs_confirmation() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        let c1 = fx.commit("c1");
        fx.write_file("a.txt", "2");
        fx.stage_all();
        let c2 = fx.commit("c2");
        let repo = fx.open();
        push(
            &repo,
            entry(
                OperationKind::Commit,
                UndoAction::SoftResetTo {
                    previous: c1.to_string(),
                },
            ),
        )
        .unwrap();
        // 外部ツールで 3 コミット積まれた想定。
        for i in 3..=5 {
            fx.write_file("a.txt", &i.to_string());
            fx.stage_all();
            fx.commit(&format!("external{i}"));
        }
        let head_before = fx.head_oid();
        let repo = fx.open();
        let e = peek(&repo).unwrap().unwrap();
        match validate_entry(&repo, &e) {
            UndoApplicability::Risky { reason } => {
                assert!(reason.contains("4件"), "{reason}");
                assert!(reason.contains("external5"));
            }
            other => panic!("Risky のはず: {other:?}"),
        }
        // 確認なしはブロックされ、何も変わらずエントリも残る。
        assert!(matches!(
            undo_last(&repo).unwrap_err(),
            CoreError::Blocked(_)
        ));
        assert_eq!(fx.head_oid(), head_before);
        assert!(can_undo(&repo).unwrap());
        // 確認済みなら適用される。
        undo_last_confirmed(&repo, true).unwrap();
        assert_eq!(fx.open().head().unwrap().target().unwrap(), c1);
        let _ = c2;
    }

    #[test]
    fn soft_reset_already_undone_when_head_is_previous() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        let c1 = fx.commit("c1");
        let repo = fx.open();
        let e = entry(
            OperationKind::Commit,
            UndoAction::SoftResetTo {
                previous: c1.to_string(),
            },
        );
        assert_eq!(validate_entry(&repo, &e), UndoApplicability::AlreadyUndone);
    }

    // oid が存在しない・壊れている場合は Unresolvable。undo_last は履歴から整理する。
    #[test]
    fn unresolvable_oid_is_pruned_with_plain_message() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        let repo = fx.open();
        for prev in ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "not-hex"] {
            let e = entry(
                OperationKind::ResetHard,
                UndoAction::HardResetTo {
                    previous: prev.into(),
                },
            );
            assert!(matches!(
                validate_entry(&repo, &e),
                UndoApplicability::Unresolvable { .. }
            ));
        }
        push(
            &repo,
            entry(
                OperationKind::ResetHard,
                UndoAction::HardResetTo {
                    previous: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                },
            ),
        )
        .unwrap();
        let err = undo_last(&repo).unwrap_err().to_string();
        assert!(err.contains("整理しました"), "{err}");
        assert!(!can_undo(&repo).unwrap());
    }

    #[test]
    fn prune_unresolvable_removes_only_stale_entries() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        let repo = fx.open();
        push(
            &repo,
            entry(
                OperationKind::CreateTag,
                UndoAction::DeleteTag { name: "v1".into() },
            ),
        )
        .unwrap();
        push(
            &repo,
            entry(
                OperationKind::DeleteTag,
                UndoAction::RecreateTag {
                    name: "gone".into(),
                    target: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
                    message: None,
                },
            ),
        )
        .unwrap();
        let v = validate_journal(&repo).unwrap();
        assert_eq!(v[0], UndoApplicability::AlreadyUndone);
        assert!(matches!(v[1], UndoApplicability::Unresolvable { .. }));
        assert_eq!(prune_unresolvable(&repo).unwrap(), 1);
        assert_eq!(list(&repo).unwrap().len(), 1);
        assert_eq!(prune_unresolvable(&repo).unwrap(), 0);
    }

    #[test]
    fn branch_and_tag_applicability() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        let c1 = fx.commit("c1");
        let repo = fx.open();
        let c1_commit = repo.find_commit(c1).unwrap();
        repo.branch("feat", &c1_commit, false).unwrap();

        // RecreateBranch: 既存 -> AlreadyUndone / 無くて oid あり -> Applicable
        let rb = |n: &str| {
            entry(
                OperationKind::DeleteBranch,
                UndoAction::RecreateBranch {
                    name: n.into(),
                    target: c1.to_string(),
                },
            )
        };
        assert_eq!(
            validate_entry(&repo, &rb("feat")),
            UndoApplicability::AlreadyUndone
        );
        assert_eq!(
            validate_entry(&repo, &rb("other")),
            UndoApplicability::Applicable
        );

        // DeleteBranch: 無ければ AlreadyUndone。作成後に進んでいれば Risky。
        let db = |n: &str| UndoEntry {
            head_at_record: Some(c1.to_string()),
            ..entry(
                OperationKind::CreateBranch,
                UndoAction::DeleteBranch { name: n.into() },
            )
        };
        assert_eq!(
            validate_entry(&repo, &db("nope")),
            UndoApplicability::AlreadyUndone
        );
        assert_eq!(
            validate_entry(&repo, &db("feat")),
            UndoApplicability::Applicable
        );
        fx.set_branch("feat", {
            fx.write_file("a.txt", "2");
            fx.stage_all();
            fx.commit("c2")
        });
        let repo = fx.open();
        assert!(matches!(
            validate_entry(&repo, &db("feat")),
            UndoApplicability::Risky { .. }
        ));

        // タグ
        let c1_obj = repo.find_object(c1, None).unwrap();
        repo.tag_lightweight("t1", &c1_obj, false).unwrap();
        let rt = |n: &str| {
            entry(
                OperationKind::DeleteTag,
                UndoAction::RecreateTag {
                    name: n.into(),
                    target: c1.to_string(),
                    message: None,
                },
            )
        };
        assert_eq!(
            validate_entry(&repo, &rt("t1")),
            UndoApplicability::AlreadyUndone
        );
        assert_eq!(
            validate_entry(&repo, &rt("t2")),
            UndoApplicability::Applicable
        );
        let dt = |n: &str| {
            entry(
                OperationKind::CreateTag,
                UndoAction::DeleteTag { name: n.into() },
            )
        };
        assert_eq!(
            validate_entry(&repo, &dt("t1")),
            UndoApplicability::Applicable
        );
        assert_eq!(
            validate_entry(&repo, &dt("t2")),
            UndoApplicability::AlreadyUndone
        );
    }

    #[test]
    fn uncommit_initial_is_risky_when_branch_advanced() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        let c1 = fx.commit("c1");
        let repo = fx.open();
        let branch = repo.head().unwrap().shorthand().unwrap().to_string();
        let e = UndoEntry {
            head_at_record: Some(c1.to_string()),
            ..entry(
                OperationKind::Commit,
                UndoAction::UncommitInitial {
                    branch: branch.clone(),
                },
            )
        };
        assert_eq!(validate_entry(&repo, &e), UndoApplicability::Applicable);
        fx.write_file("a.txt", "2");
        fx.stage_all();
        fx.commit("c2");
        let repo = fx.open();
        assert!(matches!(
            validate_entry(&repo, &e),
            UndoApplicability::Risky { .. }
        ));
        let gone = entry(
            OperationKind::Commit,
            UndoAction::UncommitInitial {
                branch: "nope".into(),
            },
        );
        assert_eq!(
            validate_entry(&repo, &gone),
            UndoApplicability::AlreadyUndone
        );
    }

    #[test]
    fn index_entry_and_bisect_and_stash_applicability() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        let c1 = fx.commit("c1");
        let repo = fx.open();
        let ghost = "cccccccccccccccccccccccccccccccccccccccc";
        let ri = |blob: Option<&str>| {
            entry(
                OperationKind::Unstage,
                UndoAction::RestoreIndexEntry {
                    path: "a.txt".into(),
                    blob: blob.map(|s| s.to_string()),
                    mode: 0o100644,
                },
            )
        };
        assert!(matches!(
            validate_entry(&repo, &ri(Some(ghost))),
            UndoApplicability::Unresolvable { .. }
        ));
        let cur = repo
            .index()
            .unwrap()
            .get_path(std::path::Path::new("a.txt"), 0)
            .unwrap()
            .id
            .to_string();
        assert_eq!(
            validate_entry(&repo, &ri(Some(&cur))),
            UndoApplicability::AlreadyUndone
        );
        assert_eq!(
            validate_entry(&repo, &ri(None)),
            UndoApplicability::Applicable
        );

        let bi = |c: &str| {
            entry(
                OperationKind::BisectStart,
                UndoAction::RestoreBisectHead {
                    original_branch: None,
                    original_commit: c.into(),
                },
            )
        };
        assert_eq!(
            validate_entry(&repo, &bi(&c1.to_string())),
            UndoApplicability::Applicable
        );
        assert!(matches!(
            validate_entry(&repo, &bi(ghost)),
            UndoApplicability::Unresolvable { .. }
        ));

        let ps = entry(
            OperationKind::StashSave,
            UndoAction::PopStash { id: ghost.into() },
        );
        assert_eq!(validate_entry(&repo, &ps), UndoApplicability::AlreadyUndone);
    }
}
