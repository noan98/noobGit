use std::cell::{Cell, RefCell};
use std::path::{Component, Path};

use git2::build::CheckoutBuilder;
use git2::{
    BranchType, Commit, Cred, CredentialType, FetchOptions, IndexAddOption, PushOptions,
    RemoteCallbacks, Repository, ResetType, StashFlags,
};

use crate::error::{CoreError, Result};
use crate::model::{ChangeKind, CommitInfo, FetchOutcome, FileChange, PullOutcome, StashInfo};
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

/// コンフリクトを解消したファイルを「解消済み」としてマークする。
///
/// 解消した内容（作業ツリーの当該ファイル）をインデックスに載せると、libgit2 は
/// そのパスの conflict エントリ（stage 1/2/3）を取り除いて通常のステージ済み
/// （stage 0）に置き換える。これがコンフリクト解消マークの実体。ファイルが消えて
/// いる（削除で解消した）場合はインデックスから取り除く。マーク後はそのまま
/// コミットへ進める。undo は通常のステージと同じ扱いなので記録しない。
pub fn mark_resolved(repo: &Repository, path: &str) -> Result<()> {
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
                e.message()
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
            h.name().map(|s| s.to_string())
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
pub fn discard_path(repo: &Repository, path: &str) -> Result<()> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| CoreError::Git("作業ツリーがありません。".to_string()))?;

    // 作業ツリー外を指す相対パスは扱わない（安全のため）。
    let rel = Path::new(path);
    if rel.is_absolute() || rel.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(CoreError::InvalidInput(format!("不正なパスです: {path}")));
    }

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
            std::fs::remove_file(&full)
                .map_err(|e| CoreError::Git(format!("ファイルを削除できませんでした: {e}")))?;
        }
    }
    Ok(())
}

/// 現在の変更を一時的にしまう（stash 退避）。未追跡ファイルも含めて退避し、作業ツリーを
/// きれいな状態に戻す。`message` が空なら libgit2 が既定のメッセージを付ける。
///
/// 退避は変更を消さない安全操作。直後に取り出せるよう、PopStash の undo を記録する。
pub fn stash_save(repo: &mut Repository, message: &str) -> Result<()> {
    let sig = repo.signature().map_err(|_| {
        CoreError::InvalidInput(
            "退避（stash）には名前とメールの設定が必要です（git config user.name / user.email）。"
                .to_string(),
        )
    })?;

    let msg = message.trim();
    let msg = if msg.is_empty() { None } else { Some(msg) };
    let flags = StashFlags::INCLUDE_UNTRACKED;

    let stash_oid = repo.stash_save2(&sig, msg, Some(flags)).map_err(|e| {
        if e.code() == git2::ErrorCode::NotFound {
            CoreError::Blocked("退避できる変更がありません。".to_string())
        } else {
            CoreError::Git(format!("退避（stash）に失敗しました: {}", e.message()))
        }
    })?;

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

/// 退避を作業ツリーに取り出す（一覧には残す）。コンフリクトが起きることがある。
pub fn stash_apply(repo: &mut Repository, index: usize) -> Result<()> {
    repo.stash_apply(index, None).map_err(map_stash_restore_err)
}

/// 退避を作業ツリーに取り出し、一覧から取り除く（pop）。コンフリクトが起きることがある。
pub fn stash_pop(repo: &mut Repository, index: usize) -> Result<()> {
    repo.stash_pop(index, None).map_err(map_stash_restore_err)
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
        _ => CoreError::Git(format!("退避の取り出しに失敗しました: {}", e.message())),
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

/// コミットに目印（タグ）を付ける。
///
/// `target` が `None` なら HEAD のコミットに付ける。`Some` なら revparse で解決した対象に
/// 付ける（コミットの短縮 oid やブランチ名など）。`message` が空でなければ注釈付きタグ
/// （作成者・メッセージを持つ）、空または `None` なら軽量タグ（参照だけ）を作る。
/// 同名タグが既にあれば日本語エラーで案内する。タグ作成は undo を記録しない（安全操作）。
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
            tag.message().map(|m| m.trim_end().to_string()),
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

/// 指定したコミットの変更を、いまのブランチの先頭にコピーする（cherry-pick）。
///
/// `oid` はコピー元コミットのハッシュ。元のコミットはそのまま残り、現在ブランチに
/// 同じ変更を持つ新しいコミットを 1 つ積む。author は元コミットを引き継ぎ、committer は
/// 現在の identity に更新する（git の cherry-pick と同じ）。メッセージも元コミットを引き継ぐ。
///
/// コンフリクト（競合）が起きた場合は、作業ツリー・インデックスを元の HEAD 状態へ
/// 強制的に戻してから [`CoreError::Blocked`] を返す。状態は必ず保全する。
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

    // HEAD を土台に、コピー元コミットの変更を当てたインデックスをメモリ上に作る
    // （作業ツリー・実インデックスにはまだ触れない）。
    let mut merged = repo
        .cherrypick_commit(&commit, &head_commit, 0, None)
        .map_err(|e| {
            CoreError::Git(format!(
                "コピー（cherry-pick）に失敗しました: {}",
                e.message()
            ))
        })?;

    // コンフリクトがあれば、何も変えずに中断する（作業ツリーは元から触れていない）。
    if merged.has_conflicts() {
        return Err(CoreError::Blocked(
            "コンフリクト（競合）のため取り込めませんでした。状態は元に戻しました。先にいまの変更を整理してから、もう一度お試しください。"
                .to_string(),
        ));
    }

    // コンフリクトなし: 合成したインデックスからツリーを作り、新しいコミットを積む。
    let tree_id = merged.write_tree_to(repo)?;
    let tree = repo.find_tree(tree_id)?;
    let message = commit.message().unwrap_or("");

    // author は元コミットのまま、committer を現在の identity にして新コミットを作る。
    let new_oid = repo.commit(
        Some("HEAD"),
        &commit.author(),
        &sig,
        message,
        &tree,
        &[&head_commit],
    )?;

    // commit は HEAD を進めるだけなので、作業ツリーとインデックスを新コミットの内容へ合わせる。
    let mut co = CheckoutBuilder::new();
    co.force();
    repo.checkout_tree(tree.as_object(), Some(&mut co))?;

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
        // 軽量タグの作成は undo を記録しない（安全操作）。
        assert!(!crate::undo::can_undo(&repo).unwrap());
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
}
