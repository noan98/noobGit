//! git bisect（二分探索でバグ混入コミットを見つける）の自前実装。
//!
//! libgit2 には bisect 用の API が無いため、`revwalk` を使って自前で候補コミットを
//! 絞り込む。git 本体との互換性（`.git/BISECT_HEAD` や `refs/bisect/*`）は目指さず、
//! noobGit 独自のセッションファイル（`.git/noobgit_bisect.json`）に必要最小限の状態
//! だけを持つ。書き込みは `undo.rs` と同じ tmp ファイル + rename のアトミック方式。
//!
//! # アルゴリズム
//!
//! 「壊れている（bad）」コミット1つと「動いていた（good）」コミットの集合を持ち、
//! 候補集合は「bad の祖先（bad 自身を含む）であり、good のどれかの祖先ではない」
//! コミット全体（`git rev-list bad --not good...` に相当）。マージコミットを含む
//! 履歴でも revwalk が全親を辿るため破綻しない。
//!
//! 判定のたびに:
//! - 「動いていた（good）」と答えたら、その commit を good 集合に追加する
//!   （それ以降、その祖先はすべて候補から除外される）。
//! - 「壊れている（bad）」と答えたら、その commit を新しい bad として置き換える
//!   （これは git 本体の `git bisect bad` と同じ挙動。新しい bad は旧 bad の祖先なので、
//!   候補集合はここでも単調に縮む）。
//!
//! これを候補集合が 1 件になるまで繰り返す。残った 1 件が「最初に壊れたコミット」。
//!
//! # Undo との関係
//!
//! `bisect_start` は、開始前の HEAD（ブランチ名 or 具体的なコミット）を
//! [`crate::undo::UndoAction::RestoreBisectHead`] として記録する。これは「Bisect
//! セッションそのものを開始前の状態に巻き戻す（元のブランチへ戻り、セッションを
//! 破棄する）」という意味の取り消しで、[`bisect_reset`] と同じ復元ロジックを使う。
//! 判定（`bisect_mark`）は HEAD を動かすが、個別の undo エントリは記録しない
//! （bisect はセッション全体を一つの作業単位として扱い、「Undo」は常にセッション
//! 開始前へ戻ることを意味する、という設計判断）。

use std::fs;
use std::path::PathBuf;

use git2::{BranchType, Oid, Repository};
use serde::{Deserialize, Serialize};

use crate::error::{describe_io_error, CoreError, Result};
use crate::model::{BisectStatus, CommitInfo};
use crate::repo;
use crate::safety::OperationKind;
use crate::undo::{self, UndoAction, UndoEntry};

/// bisect セッションの永続化データ（`.git/noobgit_bisect.json`）。
///
/// アプリ再起動やタブの再表示のあいだも進行状況を復元できるよう、undo.rs と同じ
/// tmp ファイル + rename のアトミック書き込みで保存する。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BisectSession {
    /// 開始前に HEAD が指していたブランチ名。detached HEAD で始めていた場合は None。
    original_branch: Option<String>,
    /// 開始前の HEAD のコミット。ブランチが後で削除されていた場合の復元先にも使う。
    original_commit: String,
    /// 現在の「壊れている」境界コミット。bad 判定のたびに、より近い（狭い）コミットへ置き換わる。
    bad: String,
    /// 「動いていた」と判定済みのコミット一覧（複数可）。
    good: Vec<String>,
    /// これまでに判定した回数（進捗表示用）。
    tested: usize,
}

fn session_path(repo: &Repository) -> PathBuf {
    repo.path().join("noobgit_bisect.json")
}

/// セッションファイルを読み込む。無ければ「セッションなし」として `None`。
/// 内容が壊れている場合も、undo.rs と同じベストエフォート方針で「セッションなし」
/// として扱う（他の機能を壊さないため）。
fn load_session(repo: &Repository) -> Result<Option<BisectSession>> {
    let path = session_path(repo);
    match fs::read(&path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CoreError::Git(format!(
            "Bisect セッションの読み取りに失敗しました: {}",
            describe_io_error(&e)
        ))),
    }
}

fn save_session(repo: &Repository, session: &BisectSession) -> Result<()> {
    let path = session_path(repo);
    let bytes = serde_json::to_vec_pretty(session)
        .map_err(|e| CoreError::Git(format!("Bisect セッションの保存に失敗しました: {e}")))?;
    let tmp = path.with_file_name("noobgit_bisect.json.tmp");
    fs::write(&tmp, bytes).map_err(|e| {
        CoreError::Git(format!(
            "Bisect セッションの保存に失敗しました: {}",
            describe_io_error(&e)
        ))
    })?;
    fs::rename(&tmp, &path).map_err(|e| {
        CoreError::Git(format!(
            "Bisect セッションの保存に失敗しました: {}",
            describe_io_error(&e)
        ))
    })?;
    Ok(())
}

/// セッションファイルを削除する（無ければ何もしない。冪等）。
/// `undo.rs` の `RestoreBisectHead` からも（ベストエフォートで）呼ばれるため `pub(crate)`。
pub(crate) fn clear_session(repo: &Repository) -> Result<()> {
    let path = session_path(repo);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(CoreError::Git(format!(
            "Bisect セッションの削除に失敗しました: {}",
            describe_io_error(&e)
        ))),
    }
}

/// `git2::Commit` を [`CommitInfo`] に変換する（`ops::commit_info` と同等。
/// モジュールをまたいだ private 関数の共有を避けるためここに複製する）。
fn commit_info(commit: &git2::Commit) -> CommitInfo {
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

/// revspec 文字列をコミットの oid に解決する。`label` はエラーメッセージに使う日本語の接頭辞。
fn resolve_commit(repo: &Repository, rev: &str, label: &str) -> Result<Oid> {
    let rev = rev.trim();
    if rev.is_empty() {
        return Err(CoreError::InvalidInput(format!(
            "{label}コミットを指定してください。"
        )));
    }
    let obj = repo.revparse_single(rev).map_err(|_| {
        CoreError::InvalidInput(format!("{label}コミット「{rev}」が見つかりませんでした。"))
    })?;
    let commit = obj
        .peel_to_commit()
        .map_err(|_| CoreError::InvalidInput(format!("「{rev}」はコミットではありません。")))?;
    Ok(commit.id())
}

/// bad の祖先（bad 自身を含む）であり、good のどれの祖先でもないコミットを集める。
/// `git rev-list bad --not good...` に相当する。マージコミットも全親を辿るため問題ない。
fn compute_candidates(repo: &Repository, bad: Oid, good: &[Oid]) -> Result<Vec<Oid>> {
    let mut walk = repo.revwalk()?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
    walk.push(bad)?;
    for g in good {
        // good が bad の祖先でない等で hide が無意味なケースもあるが、
        // 候補が単に減らないだけで安全（不正な組み合わせは bisect_start 側で弾く）。
        walk.hide(*g)?;
    }
    let mut out = Vec::new();
    for oid in walk {
        out.push(oid?);
    }
    Ok(out)
}

/// 残り絞り込み回数の概算（log2 の切り上げ）。
fn remaining_steps(candidate_count: usize) -> usize {
    if candidate_count <= 1 {
        0
    } else {
        (candidate_count as f64).log2().ceil() as usize
    }
}

/// 未コミット変更を上書きせず、指定コミットへ detached HEAD でチェックアウトする。
fn checkout_detached(repo: &Repository, oid: Oid) -> Result<()> {
    let commit = repo.find_commit(oid)?;
    let mut co = git2::build::CheckoutBuilder::new();
    repo.checkout_tree(commit.as_object(), Some(&mut co)).map_err(|_| {
        CoreError::Blocked(
            "未コミットの変更があるため、Bisect を進められません。先にコミットするか退避(stash)してください。"
                .to_string(),
        )
    })?;
    repo.set_head_detached(oid)?;
    Ok(())
}

/// bisect 開始前の HEAD（ブランチ名 or 具体的なコミット）へ復元する。
///
/// [`bisect_reset`] と、bisect_start の Undo
/// （[`crate::undo::UndoAction::RestoreBisectHead`]）の両方から呼ばれる共通ロジック。
/// ブランチが残っていればそのブランチへ、無くなっていた（削除済み）場合や detached
/// HEAD で開始していた場合は `original_commit` へ強制的にチェックアウトする。
///
/// bisect 中は作業ツリーを何度も切り替えているため、強制（force）チェックアウトで
/// 確実に開始前の内容へ戻す。呼び出し側（`bisect_reset` / undo apply）は事前に
/// 未コミット変更が無いことを確認しておくこと。
pub(crate) fn restore_original_head(
    repo: &Repository,
    original_branch: Option<&str>,
    original_commit: &str,
) -> Result<()> {
    let oid = Oid::from_str(original_commit)?;

    if let Some(name) = original_branch {
        if let Ok(branch) = repo.find_branch(name, BranchType::Local) {
            if let Some(target) = branch.get().target() {
                let commit = repo.find_commit(target)?;
                let mut co = git2::build::CheckoutBuilder::new();
                co.force().update_index(true);
                repo.checkout_tree(commit.as_object(), Some(&mut co))?;
                repo.set_head(&format!("refs/heads/{name}"))?;
                return Ok(());
            }
        }
        // ブランチが見つからない（削除済み等）場合は下の detached 復元にフォールバックする。
    }

    let commit = repo.find_commit(oid)?;
    let mut co = git2::build::CheckoutBuilder::new();
    co.force().update_index(true);
    repo.checkout_tree(commit.as_object(), Some(&mut co))?;
    repo.set_head_detached(oid)?;
    Ok(())
}

/// 開始前の HEAD を記録する。まだコミットが無い（未誕生ブランチ）場合はそもそも
/// bisect の対象コミットが無いため、呼び出し側の revparse で先に弾かれる想定だが、
/// 念のため HEAD が取れないケースも日本語エラーにする。
fn capture_original_head(repo: &Repository) -> Result<(Option<String>, String)> {
    let head = repo.head().map_err(|_| {
        CoreError::Blocked("まだコミットが無いため、Bisect を開始できません。".to_string())
    })?;
    let commit = head.peel_to_commit()?;
    let branch = if head.is_branch() {
        head.shorthand().ok().map(|s| s.to_string())
    } else {
        None
    };
    Ok((branch, commit.id().to_string()))
}

/// 現在のセッション状態から次の一手（候補の中間点への checkout、または確定）を計算し、
/// 作業ツリーへ反映したうえで [`BisectStatus`] を返す。呼び出し側は返った状態を保存すること。
fn advance(repo: &Repository, session: &BisectSession) -> Result<BisectStatus> {
    let bad_oid = Oid::from_str(&session.bad)?;
    let good_oids: Vec<Oid> = session
        .good
        .iter()
        .map(|s| Oid::from_str(s))
        .collect::<std::result::Result<_, _>>()?;

    let candidates = compute_candidates(repo, bad_oid, &good_oids)?;

    if candidates.len() <= 1 {
        // 完了: 残った1件（万一空なら bad 自身）が最初に壊れたコミット。
        let found_oid = candidates.first().copied().unwrap_or(bad_oid);
        checkout_detached(repo, found_oid)?;
        let commit = repo.find_commit(found_oid)?;
        return Ok(BisectStatus {
            current_commit: None,
            remaining_steps: 0,
            is_done: true,
            found_commit: Some(commit_info(&commit)),
            tested_count: session.tested,
        });
    }

    // bad 自身はすでに「壊れている」と判定済み（bisect_start の引数、または直近の
    // bisect_mark）なので、次に判定すべき候補からは除く。これを怠ると、bad_oid が
    // たまたま候補の中間点に選ばれたとき同じコミットを何度も選び直してしまい、
    // （何も状態が変わらないまま）絞り込みが進まなくなる恐れがある。
    let unknown: Vec<Oid> = candidates.into_iter().filter(|&o| o != bad_oid).collect();
    // 候補の中間あたりのコミットを次の判定対象に選ぶ（厳密な最適分割ではなく概算）。
    let mid = unknown[unknown.len() / 2];
    checkout_detached(repo, mid)?;
    let commit = repo.find_commit(mid)?;
    Ok(BisectStatus {
        current_commit: Some(commit_info(&commit)),
        remaining_steps: remaining_steps(unknown.len() + 1),
        is_done: false,
        found_commit: None,
        tested_count: session.tested,
    })
}

fn record_undo(repo: &Repository, entry: UndoEntry) {
    // Undo 記録はベストエフォート（ops.rs の record_undo と同じ方針）。
    let _ = undo::push(repo, entry);
}

/// Bisect（バグ混入コミットの二分探索）を開始する。
///
/// `bad` は「壊れている」ことが分かっているコミット、`good` は「動いていた」ことが
/// 分かっているコミット（revspec: ブランチ名・タグ・短縮 oid などなんでも可）。
/// `good` が `bad` の祖先であることを検証し、そうでなければ [`CoreError::InvalidInput`]。
///
/// 未コミットの変更がある、マージ中・コンフリクト解消中などの中間状態、既に別の
/// Bisect セッションが進行中、のいずれかに該当する場合は何も変えずに
/// [`CoreError::Blocked`] で中断する。
pub fn bisect_start(repo: &Repository, bad: &str, good: &str) -> Result<BisectStatus> {
    if load_session(repo)?.is_some() {
        return Err(CoreError::Blocked(
            "すでに Bisect セッションが進行中です。先に「Bisect を終了」してから始めてください。"
                .to_string(),
        ));
    }
    if repo::is_dirty(repo)? {
        return Err(CoreError::Blocked(
            "未コミットの変更があるため、Bisect を開始できません。先にコミットするか退避(stash)してください。"
                .to_string(),
        ));
    }
    if repo.state() != git2::RepositoryState::Clean {
        return Err(CoreError::Blocked(
            "コンフリクト解消中やマージ中は Bisect を開始できません。先にそちらを片付けてください。"
                .to_string(),
        ));
    }

    let bad_oid = resolve_commit(repo, bad, "壊れている（bad）")?;
    let good_oid = resolve_commit(repo, good, "動いていた（good）")?;

    if bad_oid == good_oid {
        return Err(CoreError::InvalidInput(
            "「壊れているコミット」と「動いていたコミット」が同じです。異なるコミットを指定してください。"
                .to_string(),
        ));
    }
    if !repo.graph_descendant_of(bad_oid, good_oid).unwrap_or(false) {
        return Err(CoreError::InvalidInput(
            "「動いていたコミット」は「壊れているコミット」の祖先ではありません。正しい順序で指定してください。"
                .to_string(),
        ));
    }

    let (original_branch, original_commit) = capture_original_head(repo)?;

    let session = BisectSession {
        original_branch: original_branch.clone(),
        original_commit: original_commit.clone(),
        bad: bad_oid.to_string(),
        good: vec![good_oid.to_string()],
        tested: 0,
    };

    let status = advance(repo, &session)?;
    save_session(repo, &session)?;

    record_undo(
        repo,
        UndoEntry {
            op: OperationKind::BisectStart,
            description: "Bisect の開始を取り消す（元のブランチに戻す）".to_string(),
            action: UndoAction::RestoreBisectHead {
                original_branch,
                original_commit,
            },
        },
    );

    Ok(status)
}

/// 現在 Bisect が調べているコミットについて「動いていた（good）」か「壊れている
/// （bad）」かを記録し、次の候補へ進める（または絞り込みが完了していれば確定させる）。
///
/// `commit` は判定対象のコミット（revspec）。いま Bisect がチェックアウトしている
/// コミットと一致しない場合は勘違いでの誤判定を防ぐため [`CoreError::InvalidInput`]。
/// 未コミットの変更がある場合は何も変えずに [`CoreError::Blocked`]。
pub fn bisect_mark(repo: &Repository, commit: &str, is_good: bool) -> Result<BisectStatus> {
    let mut session = load_session(repo)?.ok_or_else(|| {
        CoreError::InvalidInput("Bisect セッションが開始されていません。".to_string())
    })?;

    if repo::is_dirty(repo)? {
        return Err(CoreError::Blocked(
            "未コミットの変更があるため、判定を記録できません。先にコミットするか退避(stash)してください。"
                .to_string(),
        ));
    }

    let target = resolve_commit(repo, commit, "判定対象の")?;

    let head_oid = repo
        .head()
        .and_then(|h| h.peel_to_commit())
        .map(|c| c.id())
        .map_err(|_| CoreError::Blocked("HEAD を確認できませんでした。".to_string()))?;
    if target != head_oid {
        return Err(CoreError::InvalidInput(
            "指定したコミットは、いま Bisect が調べているコミットと一致しません。".to_string(),
        ));
    }

    if is_good {
        session.good.push(target.to_string());
    } else {
        // 新しく bad と判定されたコミットが、より近い（狭い）境界として bad を置き換える。
        session.bad = target.to_string();
    }
    session.tested += 1;

    let status = advance(repo, &session)?;
    save_session(repo, &session)?;
    Ok(status)
}

/// Bisect セッションを終了し、開始前のブランチ（または detached HEAD）へ戻す。
///
/// 未コミットの変更がある場合は何も変えずに [`CoreError::Blocked`]。セッションが
/// 開始されていない場合は [`CoreError::InvalidInput`]。
pub fn bisect_reset(repo: &Repository) -> Result<()> {
    let session = load_session(repo)?.ok_or_else(|| {
        CoreError::InvalidInput("Bisect セッションが開始されていません。".to_string())
    })?;

    if repo::is_dirty(repo)? {
        return Err(CoreError::Blocked(
            "未コミットの変更があるため、Bisect を終了できません。先にコミットするか退避(stash)してください。"
                .to_string(),
        ));
    }

    restore_original_head(
        repo,
        session.original_branch.as_deref(),
        &session.original_commit,
    )?;
    clear_session(repo)?;
    Ok(())
}

/// 現在の Bisect セッションの状態を返す（読み取り専用。作業ツリーには一切触れない）。
///
/// セッションが無ければ `None`。アプリ再起動やタブの再表示のときに、進行状況を
/// 復元するために使う。
pub fn bisect_status(repo: &Repository) -> Result<Option<BisectStatus>> {
    let session = match load_session(repo)? {
        Some(s) => s,
        None => return Ok(None),
    };

    let bad_oid = Oid::from_str(&session.bad)?;
    let good_oids: Vec<Oid> = session
        .good
        .iter()
        .map(|s| Oid::from_str(s))
        .collect::<std::result::Result<_, _>>()?;
    let candidates = compute_candidates(repo, bad_oid, &good_oids)?;

    if candidates.len() <= 1 {
        let found_oid = candidates.first().copied().unwrap_or(bad_oid);
        let commit = repo.find_commit(found_oid)?;
        return Ok(Some(BisectStatus {
            current_commit: None,
            remaining_steps: 0,
            is_done: true,
            found_commit: Some(commit_info(&commit)),
            tested_count: session.tested,
        }));
    }

    // HEAD は前回の advance で候補の中間点にチェックアウト済みのはずなので、それを
    // そのまま「調査中のコミット」として返す（作業ツリーには触れない）。
    let current = match repo.head().and_then(|h| h.peel_to_commit()) {
        Ok(c) => Some(commit_info(&c)),
        Err(_) => None,
    };

    Ok(Some(BisectStatus {
        current_commit: current,
        remaining_steps: remaining_steps(candidates.len()),
        is_done: false,
        found_commit: None,
        tested_count: session.tested,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestRepo;

    // 直線履歴で「良い」コミットと「悪い」コミットの間からバグ混入コミットを特定できること。
    #[test]
    fn linear_history_finds_bug_commit() {
        let fx = TestRepo::new();
        let mut oids = Vec::new();
        for i in 0..8 {
            fx.write_file("a.txt", &i.to_string());
            fx.stage_all();
            oids.push(fx.commit(&format!("c{i}")));
        }
        // c0..c3 は動いていた、c4 でバグが混入し、c4..c7 は壊れている、という想定。
        let bug_index = 4;

        let repo = fx.open();
        let bad = oids[7].to_string();
        let good = oids[0].to_string();
        let mut status = bisect_start(&repo, &bad, &good).unwrap();
        assert!(!status.is_done);

        // 現在チェックアウトされているコミットのインデックスを判定しつつ、bug_index を
        // 基準に good/bad を答えていく（実際のユーザー操作のシミュレーション）。
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 20, "収束しませんでした");
            let current = status
                .current_commit
                .clone()
                .expect("進行中は current があるはず");
            let idx = oids
                .iter()
                .position(|o| o.to_string() == current.id)
                .unwrap();
            let is_good = idx < bug_index;
            let repo = fx.open();
            status = bisect_mark(&repo, &current.id, is_good).unwrap();
            if status.is_done {
                break;
            }
        }

        let found = status
            .found_commit
            .expect("完了時は found_commit があるはず");
        assert_eq!(found.id, oids[bug_index].to_string());
    }

    // good/bad の指定ミス: bad が good の祖先ではない（順序が逆）場合は InvalidInput。
    #[test]
    fn good_not_ancestor_of_bad_is_invalid_input() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        let c1 = fx.commit("c1");
        fx.write_file("a.txt", "2");
        fx.stage_all();
        let c2 = fx.commit("c2");

        let repo = fx.open();
        // good と bad を逆に渡す。
        let err = bisect_start(&repo, &c1.to_string(), &c2.to_string()).unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput(_)));
    }

    // good と bad が同じコミットは InvalidInput。
    #[test]
    fn same_good_and_bad_is_invalid_input() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        let c1 = fx.commit("c1");

        let repo = fx.open();
        let err = bisect_start(&repo, &c1.to_string(), &c1.to_string()).unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput(_)));
    }

    // 未コミットの変更があると開始できず、何も変わらない（Blocked）。
    #[test]
    fn start_blocked_when_dirty() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        let c1 = fx.commit("c1");
        fx.write_file("a.txt", "2");
        fx.stage_all();
        let c2 = fx.commit("c2");
        // 未コミットの変更を作る。
        fx.write_file("a.txt", "dirty");

        let repo = fx.open();
        let err = bisect_start(&repo, &c2.to_string(), &c1.to_string()).unwrap_err();
        assert!(matches!(err, CoreError::Blocked(_)));
        // セッションは作られていない。
        assert!(bisect_status(&repo).unwrap().is_none());
    }

    // 未コミットの変更がある状態で mark すると Blocked になり、セッションは進まない。
    #[test]
    fn mark_blocked_when_dirty() {
        let fx = TestRepo::new();
        let mut oids = Vec::new();
        for i in 0..4 {
            fx.write_file("a.txt", &i.to_string());
            fx.stage_all();
            oids.push(fx.commit(&format!("c{i}")));
        }

        let repo = fx.open();
        let status = bisect_start(&repo, &oids[3].to_string(), &oids[0].to_string()).unwrap();
        let current_id = status.current_commit.unwrap().id;

        // 作業ツリーを汚す。
        fx.write_file("a.txt", "dirty-during-bisect");

        let repo = fx.open();
        let err = bisect_mark(&repo, &current_id, true).unwrap_err();
        assert!(matches!(err, CoreError::Blocked(_)));
    }

    // bisect_reset で元のブランチ（HEAD）に戻ること。
    #[test]
    fn reset_restores_original_branch() {
        let fx = TestRepo::new();
        let mut oids = Vec::new();
        for i in 0..5 {
            fx.write_file("a.txt", &i.to_string());
            fx.stage_all();
            oids.push(fx.commit(&format!("c{i}")));
        }

        let repo = fx.open();
        assert_eq!(repo::current_branch(&repo).as_deref(), Some("main"));
        bisect_start(&repo, &oids[4].to_string(), &oids[0].to_string()).unwrap();

        // 開始後は detached HEAD になっている。
        let repo = fx.open();
        assert!(!repo.head().unwrap().is_branch());

        bisect_reset(&repo).unwrap();

        let repo = fx.open();
        assert_eq!(repo::current_branch(&repo).as_deref(), Some("main"));
        assert_eq!(repo.head().unwrap().target(), Some(oids[4]));
        // セッションは破棄されている。
        assert!(bisect_status(&repo).unwrap().is_none());
    }

    // bisect_start の Undo は、元のブランチへ戻りセッションを破棄する。
    #[test]
    fn undo_restores_original_branch_and_discards_session() {
        let fx = TestRepo::new();
        let mut oids = Vec::new();
        for i in 0..5 {
            fx.write_file("a.txt", &i.to_string());
            fx.stage_all();
            oids.push(fx.commit(&format!("c{i}")));
        }

        let repo = fx.open();
        bisect_start(&repo, &oids[4].to_string(), &oids[0].to_string()).unwrap();

        let repo = fx.open();
        assert!(undo::can_undo(&repo).unwrap());
        let desc = undo::undo_last(&repo).unwrap();
        assert!(desc.contains("Bisect"));

        let repo = fx.open();
        assert_eq!(repo::current_branch(&repo).as_deref(), Some("main"));
        assert_eq!(repo.head().unwrap().target(), Some(oids[4]));
        assert!(bisect_status(&repo).unwrap().is_none());
    }

    // 未コミットの変更があるときの Undo は、変更を消さずに Blocked で中断する。
    #[test]
    fn undo_is_blocked_when_working_dir_dirty() {
        let fx = TestRepo::new();
        let mut oids = Vec::new();
        for i in 0..5 {
            fx.write_file("a.txt", &i.to_string());
            fx.stage_all();
            oids.push(fx.commit(&format!("c{i}")));
        }

        let repo = fx.open();
        bisect_start(&repo, &oids[4].to_string(), &oids[0].to_string()).unwrap();
        bisect_reset(&repo).unwrap();

        // 終了後に作業を続けて、未コミットの変更がある状態で取り消しを押す。
        fx.write_file("a.txt", "作業中の大事な変更");
        let repo = fx.open();
        assert!(matches!(
            undo::undo_last(&repo).unwrap_err(),
            CoreError::Blocked(_)
        ));
        assert_eq!(
            std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
            "作業中の大事な変更"
        );
    }

    // 進行状況の永続化: 別プロセス（リポジトリの開き直し）でも bisect_status から
    // 同じ内容が復元できる。
    #[test]
    fn status_persists_across_reopen() {
        let fx = TestRepo::new();
        let mut oids = Vec::new();
        for i in 0..8 {
            fx.write_file("a.txt", &i.to_string());
            fx.stage_all();
            oids.push(fx.commit(&format!("c{i}")));
        }

        let repo = fx.open();
        let started = bisect_start(&repo, &oids[7].to_string(), &oids[0].to_string()).unwrap();

        // リポジトリを開き直して復元する。
        let repo = fx.open();
        let restored = bisect_status(&repo).unwrap().expect("セッションがあるはず");
        assert_eq!(restored.current_commit, started.current_commit);
        assert_eq!(restored.remaining_steps, started.remaining_steps);
        assert!(!restored.is_done);
    }

    // マージコミットを含む履歴でも破綻しない。
    #[test]
    fn works_with_merge_commits() {
        let fx = TestRepo::new();
        fx.write_file("base.txt", "0");
        fx.stage_all();
        let base = fx.commit("base"); // good

        // main を進める。
        fx.write_file("main.txt", "1");
        fx.stage_all();
        let main1 = fx.commit("main1");

        // feature ブランチを base から分岐させる。
        {
            let repo = fx.open();
            repo.branch("feature", &repo.find_commit(base).unwrap(), false)
                .unwrap();
            repo.set_head("refs/heads/feature").unwrap();
        }
        fx.write_file("feature.txt", "1");
        fx.stage_all();
        let feature1 = fx.commit("feature1"); // これが「バグ混入」コミットという想定

        // main へ戻ってマージする（マージコミットを作る）。feature ブランチだけに
        // あった feature.txt は main の作業ツリーには不要なので、force に加えて
        // remove_untracked も指定し、後続の is_dirty チェックに引っかからないようにする。
        {
            let repo = fx.open();
            repo.set_head("refs/heads/main").unwrap();
            let mut co = git2::build::CheckoutBuilder::new();
            co.force().remove_untracked(true);
            repo.checkout_head(Some(&mut co)).unwrap();
        }

        let merge_oid = {
            let repo = fx.open();
            let sig = git2::Signature::now("Test User", "test@example.com").unwrap();
            let main_commit = repo.find_commit(main1).unwrap();
            let feature_commit = repo.find_commit(feature1).unwrap();
            let mut index = repo
                .merge_commits(&main_commit, &feature_commit, None)
                .unwrap();
            assert!(!index.has_conflicts());
            let tree_id = index.write_tree_to(&repo).unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            let oid = repo
                .commit(
                    Some("HEAD"),
                    &sig,
                    &sig,
                    "merge feature",
                    &tree,
                    &[&main_commit, &feature_commit],
                )
                .unwrap();
            // merge_commits は実際の作業ツリー・インデックスには触れないため、コミット後に
            // 明示的にチェックアウトして揃える（そうしないと HEAD のツリーと作業ツリーが
            // 食い違い、後続の bisect_start が「未コミットの変更あり」と判定してしまう）。
            let commit = repo.find_commit(oid).unwrap();
            let mut co = git2::build::CheckoutBuilder::new();
            co.force();
            repo.checkout_tree(commit.as_object(), Some(&mut co))
                .unwrap();
            oid
        };

        // bad = マージ後の main（壊れている）、good = base（動いていた）。
        let repo = fx.open();
        let mut status = bisect_start(&repo, &merge_oid.to_string(), &base.to_string()).unwrap();
        assert!(!status.is_done);

        // feature1 が「バグ混入」コミット。feature1 を取り込んだ merge_oid も当然
        // 壊れている（feature1 の子孫）ので bad、feature1 を含まない main1 だけが good。
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 20, "収束しませんでした");
            let current = status.current_commit.clone().expect("進行中のはず");
            let is_bad = current.id == feature1.to_string() || current.id == merge_oid.to_string();
            let repo = fx.open();
            status = bisect_mark(&repo, &current.id, !is_bad).unwrap();
            if status.is_done {
                break;
            }
        }

        let found = status.found_commit.expect("完了しているはず");
        assert_eq!(found.id, feature1.to_string());
    }
}
