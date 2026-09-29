//! 操作の「影響プレビュー」（Issue #196）。
//!
//! 破壊的・注意が必要な操作を実行する前に、確認ダイアログへ「この操作で具体的に
//! 何が失われるか」をそのリポジトリのデータで見せるための計算。すべて読み取り専用で、
//! リポジトリ・作業ツリー・インデックスを一切変更しない。
//!
//! 計算に失敗しても操作をブロックしないのが原則: この module の関数は失敗時に
//! `Err` を返すだけで、呼び出し側（フロントエンド）は「プレビューなし」として
//! 確認ダイアログを通常どおり表示する。新しい操作のプレビューは
//! [`ImpactPreview`] にバリアントを足し、[`preview`] に分岐を足すだけで増やせる。

use std::collections::HashSet;

use git2::{BranchType, Repository};

use crate::error::{CoreError, Result};
use crate::model::{CommitInfo, DiscardedDiff, FileDiff, ImpactPreview, ImpactRequest, RebaseStep};
use crate::{ops, repo};

/// プレビューに載せるコミット数の上限。超えた分は `truncated` で示す。
const MAX_PREVIEW_COMMITS: usize = 50;
/// discard プレビューで差分を載せるファイル数の上限。超えた分は `omitted_files` で示す。
const MAX_PREVIEW_DIFF_FILES: usize = 10;
/// discard プレビューで 1 つの差分に載せる行数の上限（画面と通信を重くしないため）。
const MAX_PREVIEW_DIFF_LINES: usize = 200;

/// 依頼に応じた影響プレビューを計算する。
///
/// 退避の差分を読むために `&mut Repository` を取る（`stash_foreach` の都合）が、
/// リポジトリの中身は変更しない。
pub fn preview(repo: &mut Repository, request: &ImpactRequest) -> Result<ImpactPreview> {
    match request {
        ImpactRequest::ResetHard => reset_hard_preview(repo),
        ImpactRequest::Discard { paths } => discard_preview(repo, paths),
        ImpactRequest::DeleteBranch { name } => delete_branch_preview(repo, name),
        ImpactRequest::ForcePush { remote, branch } => force_push_preview(repo, remote, branch),
        ImpactRequest::StashApply { index } | ImpactRequest::StashPop { index } => {
            stash_restore_preview(repo, *index)
        }
        ImpactRequest::Rebase { commit_ids } => rewrite_preview(repo, commit_ids),
        ImpactRequest::RebasePlan { plan } => rebase_plan_preview(repo, plan),
    }
}

/// reset_hard で失われる未コミットの変更ファイル一覧（ステージ済み + 未ステージ）。
pub fn reset_hard_preview(repo: &Repository) -> Result<ImpactPreview> {
    let s = repo::status(repo)?;
    let mut files = s.staged;
    files.extend(s.unstaged);
    Ok(ImpactPreview::LostChanges { files })
}

/// discard で失われる差分そのもの（指定パスごとのステージ済み・未ステージ差分）。
///
/// 既存の `diff_staged` / `diff_unstaged` をパス指定で再利用する。差分が空の側は `None`。
/// ファイル数・行数には上限があり、超えた分は省略する。
pub fn discard_preview(repo: &Repository, paths: &[String]) -> Result<ImpactPreview> {
    let mut diffs = Vec::new();
    let mut omitted_files = 0;
    for path in paths {
        let staged = non_empty(repo::diff_staged(repo, path)?);
        let unstaged = non_empty(repo::diff_unstaged(repo, path)?);
        if staged.is_none() && unstaged.is_none() {
            continue;
        }
        if diffs.len() >= MAX_PREVIEW_DIFF_FILES {
            omitted_files += 1;
            continue;
        }
        diffs.push(DiscardedDiff {
            path: path.clone(),
            staged: staged.map(clip_diff),
            unstaged: unstaged.map(clip_diff),
        });
    }
    Ok(ImpactPreview::DiscardedDiffs {
        diffs,
        omitted_files,
    })
}

/// 表示できる中身（行またはバイナリ）が無い差分は `None` にする。
fn non_empty(diff: FileDiff) -> Option<FileDiff> {
    if diff.lines.is_empty() && !diff.is_binary {
        None
    } else {
        Some(diff)
    }
}

/// 差分の行数をプレビュー用の上限に切り詰める。
fn clip_diff(mut diff: FileDiff) -> FileDiff {
    if diff.lines.len() > MAX_PREVIEW_DIFF_LINES {
        diff.lines.truncate(MAX_PREVIEW_DIFF_LINES);
        diff.truncated = true;
    }
    diff
}

/// delete_branch で辿れなくなるコミット（そのブランチにしかないコミット）。
///
/// ブランチの先端から辿れるコミットのうち、他のローカルブランチ・リモート追跡ブランチ・
/// タグ・HEAD のいずれからも辿れないものを新しい順に返す。空なら消しても履歴は失われない。
pub fn delete_branch_preview(repo: &Repository, name: &str) -> Result<ImpactPreview> {
    let name = name.trim();
    let branch = repo
        .find_branch(name, BranchType::Local)
        .map_err(|_| CoreError::InvalidInput(format!("ブランチ「{name}」が見つかりません。")))?;
    let own_ref = branch.get().name().unwrap_or_default().to_string();
    let tip = branch
        .get()
        .target()
        .ok_or_else(|| CoreError::Git("ブランチの参照先を取得できませんでした。".to_string()))?;

    let mut walk = repo.revwalk()?;
    walk.set_sorting(git2::Sort::TIME)?;
    walk.push(tip)?;
    for r in repo.references()? {
        let Ok(r) = r else { continue };
        let Ok(refname) = r.name() else { continue };
        if refname == own_ref {
            continue;
        }
        let relevant = refname.starts_with("refs/heads/")
            || refname.starts_with("refs/remotes/")
            || refname.starts_with("refs/tags/");
        if !relevant {
            continue;
        }
        if let Ok(commit) = r.peel_to_commit() {
            let _ = walk.hide(commit.id());
        }
    }
    // detached HEAD 上のコミットも「他から辿れる」ものとして守る。
    if let Ok(head) = repo.head() {
        if let Ok(commit) = head.peel_to_commit() {
            let _ = walk.hide(commit.id());
        }
    }

    let (commits, truncated) = collect_commits(repo, walk)?;
    Ok(ImpactPreview::UniqueCommits {
        branch: name.to_string(),
        commits,
        truncated,
    })
}

/// force push でリモート上から消えるコミット。
///
/// リモート追跡ブランチ（`refs/remotes/<remote>/<branch>`、最後の fetch 時点）から辿れて、
/// ローカルのブランチ先端から辿れないコミットを新しい順に返す。追跡ブランチが無い
/// （まだ push していない等）ときはエラー＝プレビューなしになる。
pub fn force_push_preview(repo: &Repository, remote: &str, branch: &str) -> Result<ImpactPreview> {
    let remote = remote.trim();
    let branch = branch.trim();
    let remote_ref = format!("{remote}/{branch}");
    let remote_oid = repo
        .find_reference(&format!("refs/remotes/{remote_ref}"))
        .ok()
        .and_then(|r| r.peel_to_commit().ok())
        .map(|c| c.id())
        .ok_or_else(|| {
            CoreError::InvalidInput(format!(
                "リモート追跡ブランチ「{remote_ref}」が見つかりません。"
            ))
        })?;
    let local_oid = repo
        .find_branch(branch, BranchType::Local)
        .ok()
        .and_then(|b| b.get().target())
        .ok_or_else(|| {
            CoreError::InvalidInput(format!("ブランチ「{branch}」が見つかりません。"))
        })?;

    let mut walk = repo.revwalk()?;
    walk.set_sorting(git2::Sort::TIME)?;
    walk.push(remote_oid)?;
    walk.hide(local_oid)?;

    let (commits, truncated) = collect_commits(repo, walk)?;
    Ok(ImpactPreview::OverwrittenCommits {
        remote_ref,
        commits,
        truncated,
    })
}

/// stash_apply / stash_pop で衝突しうるファイル（退避の変更 ∩ 今の作業ツリーの変更）。
///
/// 既存の `stash_diff`（ツリー比較のみ）と `status` の交差を取るだけで、退避は適用しない。
pub fn stash_restore_preview(repo: &mut Repository, index: usize) -> Result<ImpactPreview> {
    let stash_files = ops::stash_diff(repo, index)?;
    let s = repo::status(repo)?;
    let mut dirty: HashSet<String> = HashSet::new();
    dirty.extend(s.staged.iter().map(|f| f.path.clone()));
    dirty.extend(s.unstaged.iter().map(|f| f.path.clone()));
    dirty.extend(s.untracked.iter().cloned());
    dirty.extend(s.conflicted.iter().cloned());

    let stash_file_count = stash_files.len();
    let overlapping = stash_files
        .into_iter()
        .filter(|f| dirty.contains(&f.path))
        .collect();
    Ok(ImpactPreview::StashOverlap {
        stash_file_count,
        overlapping,
    })
}

/// squash / reword で書き換わるコミット一覧と、公開済みかどうか。
///
/// `commit_ids` は書き換え対象（squash なら選択した連続範囲）。空なら HEAD の 1 件
/// （reword）。公開済みかは既存の `head_is_published` で判定する。
pub fn rewrite_preview(repo: &Repository, commit_ids: &[String]) -> Result<ImpactPreview> {
    let mut commits = Vec::new();
    if commit_ids.is_empty() {
        let head = repo.head()?.peel_to_commit()?;
        commits.push(repo::commit_info_from(head.id(), &head));
    } else {
        for id in commit_ids {
            let oid = git2::Oid::from_str(id.trim())
                .map_err(|_| CoreError::InvalidInput(format!("コミットを特定できません: {id}")))?;
            let commit = repo.find_commit(oid)?;
            commits.push(repo::commit_info_from(oid, &commit));
        }
    }
    let published = repo::head_is_published(repo)?;
    Ok(ImpactPreview::RewrittenCommits { commits, published })
}

/// リベースプラン（並べ替え・削除・reword・squash）の実行前後の履歴。
///
/// `before` は変更前（新しい順）、`after` は実行後の予想（新しい順）、`dropped` は履歴から
/// 消えるコミット。プランが不正なら検証エラーをそのまま返す（＝プレビューなし）。
pub fn rebase_plan_preview(repo: &Repository, plan: &[RebaseStep]) -> Result<ImpactPreview> {
    let v = ops::validate_rebase_plan(repo, plan)?;
    let info = |c: &git2::Commit| repo::commit_info_from(c.id(), c);

    let before: Vec<CommitInfo> = v.range.iter().map(info).collect();
    let mut dropped = Vec::new();
    let mut after_oldest_first: Vec<CommitInfo> = Vec::new();
    for (step, commit) in plan.iter().zip(v.steps.iter()) {
        match step {
            RebaseStep::Drop { .. } => dropped.push(info(commit)),
            // squash は直前のエントリに取り込まれるので、履歴の行は増えない。
            RebaseStep::Squash { .. } => {}
            RebaseStep::Reword { message, .. } => {
                let mut i = info(commit);
                i.summary = message.lines().next().unwrap_or("").trim().to_string();
                after_oldest_first.push(i);
            }
            RebaseStep::Pick { .. } => after_oldest_first.push(info(commit)),
        }
    }
    after_oldest_first.reverse();
    dropped.reverse();
    let published = repo::head_is_published(repo)?;
    Ok(ImpactPreview::RebasePlan {
        before,
        after: after_oldest_first,
        dropped,
        published,
    })
}

/// revwalk から最大 [`MAX_PREVIEW_COMMITS`] 件を取り出す。超過は `truncated`。
fn collect_commits(repo: &Repository, walk: git2::Revwalk) -> Result<(Vec<CommitInfo>, bool)> {
    let mut commits = Vec::new();
    let mut truncated = false;
    for oid in walk {
        let oid = oid?;
        if commits.len() >= MAX_PREVIEW_COMMITS {
            truncated = true;
            break;
        }
        let commit = repo.find_commit(oid)?;
        commits.push(repo::commit_info_from(oid, &commit));
    }
    Ok((commits, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ChangeKind;
    use crate::test_support::*;

    fn base() -> TestRepo {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1\n");
        fx.stage_all();
        fx.commit("最初");
        fx
    }

    #[test]
    fn reset_hard_lists_staged_and_unstaged() {
        let fx = base();
        fx.write_file("a.txt", "2\n");
        fx.write_file("b.txt", "new\n");
        fx.stage_all();
        fx.write_file("a.txt", "3\n");
        let repo = fx.open();
        let ImpactPreview::LostChanges { files } = reset_hard_preview(&repo).unwrap() else {
            panic!("LostChanges のはず");
        };
        let paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"a.txt") && paths.contains(&"b.txt"));
    }

    #[test]
    fn reset_hard_clean_is_empty() {
        let fx = base();
        let repo = fx.open();
        assert_eq!(
            reset_hard_preview(&repo).unwrap(),
            ImpactPreview::LostChanges { files: vec![] }
        );
    }

    #[test]
    fn discard_shows_unstaged_and_staged_diffs() {
        let fx = base();
        fx.write_file("a.txt", "2\n");
        fx.stage_all();
        fx.write_file("a.txt", "3\n");
        fx.write_file("clean.txt", "x\n");
        let repo = fx.open();
        let p = discard_preview(&repo, &["a.txt".to_string()]).unwrap();
        let ImpactPreview::DiscardedDiffs {
            diffs,
            omitted_files,
        } = p
        else {
            panic!("DiscardedDiffs のはず");
        };
        assert_eq!(omitted_files, 0);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "a.txt");
        let staged = diffs[0].staged.as_ref().expect("ステージ済み差分");
        assert!(staged.lines.iter().any(|l| l.content.contains('2')));
        let unstaged = diffs[0].unstaged.as_ref().expect("未ステージ差分");
        assert!(unstaged.lines.iter().any(|l| l.content.contains('3')));
    }

    #[test]
    fn discard_skips_paths_without_changes_and_caps_files() {
        let fx = base();
        let mut paths = vec!["a.txt".to_string()]; // 変更なし
        for i in 0..12 {
            let name = format!("n{i}.txt");
            fx.write_file(&name, "x\n");
            paths.push(name);
        }
        let repo = fx.open();
        let ImpactPreview::DiscardedDiffs {
            diffs,
            omitted_files,
        } = discard_preview(&repo, &paths).unwrap()
        else {
            panic!();
        };
        assert_eq!(diffs.len(), MAX_PREVIEW_DIFF_FILES);
        assert_eq!(omitted_files, 2);
        assert!(diffs.iter().all(
            |d| d.unstaged.as_ref().unwrap().kind == ChangeKind::Untracked
                || d.unstaged.as_ref().unwrap().kind == ChangeKind::Added
        ));
    }

    #[test]
    fn discard_clips_long_diffs() {
        let fx = base();
        let big: String = (0..500).map(|i| format!("line{i}\n")).collect();
        fx.write_file("big.txt", &big);
        let repo = fx.open();
        let ImpactPreview::DiscardedDiffs { diffs, .. } =
            discard_preview(&repo, &["big.txt".to_string()]).unwrap()
        else {
            panic!();
        };
        let d = diffs[0].unstaged.as_ref().unwrap();
        assert_eq!(d.lines.len(), MAX_PREVIEW_DIFF_LINES);
        assert!(d.truncated);
    }

    #[test]
    fn delete_branch_lists_only_unique_commits() {
        let fx = base();
        let repo = fx.open();
        repo.branch(
            "feature",
            &repo.head().unwrap().peel_to_commit().unwrap(),
            false,
        )
        .unwrap();
        drop(repo);
        // feature にだけコミットを 2 つ積む。
        ops::switch_branch(&fx.open(), "feature").unwrap();
        fx.write_file("f.txt", "1\n");
        fx.stage_all();
        fx.commit("feature 1");
        fx.write_file("f.txt", "2\n");
        fx.stage_all();
        fx.commit("feature 2");
        ops::switch_branch(&fx.open(), "main").unwrap();

        let repo = fx.open();
        let ImpactPreview::UniqueCommits {
            branch,
            commits,
            truncated,
        } = delete_branch_preview(&repo, "feature").unwrap()
        else {
            panic!();
        };
        assert_eq!(branch, "feature");
        assert!(!truncated);
        let summaries: Vec<_> = commits.iter().map(|c| c.summary.as_str()).collect();
        assert_eq!(summaries, vec!["feature 2", "feature 1"]);
    }

    #[test]
    fn delete_branch_merged_has_no_unique_commits() {
        let fx = base();
        let repo = fx.open();
        repo.branch(
            "merged",
            &repo.head().unwrap().peel_to_commit().unwrap(),
            false,
        )
        .unwrap();
        let ImpactPreview::UniqueCommits { commits, .. } =
            delete_branch_preview(&repo, "merged").unwrap()
        else {
            panic!();
        };
        assert!(commits.is_empty());
    }

    #[test]
    fn delete_branch_unknown_is_error() {
        let fx = base();
        assert!(delete_branch_preview(&fx.open(), "nope").is_err());
    }

    /// bare リモートへ push 済みの状態で、ローカルを巻き戻して分岐させる。
    fn diverged_with_remote() -> (TestRepo, TestRepo) {
        let remote = TestRepo::new_bare();
        let fx = base();
        fx.add_remote("origin", remote.path().to_str().unwrap());
        let repo = fx.open();
        ops::push(&repo, "origin", "refs/heads/main:refs/heads/main", false).unwrap();
        // 追跡ブランチ参照は push だけでは作られないことがあるので fetch で揃える。
        ops::fetch(&repo, "origin").unwrap();
        (fx, remote)
    }

    #[test]
    fn force_push_lists_remote_only_commits() {
        let (fx, _remote) = diverged_with_remote();
        // リモートに載っているコミットを 2 つ積んで push する。
        fx.write_file("a.txt", "2\n");
        fx.stage_all();
        fx.commit("remote 1");
        fx.write_file("a.txt", "3\n");
        fx.stage_all();
        fx.commit("remote 2");
        let repo = fx.open();
        ops::push(&repo, "origin", "refs/heads/main:refs/heads/main", false).unwrap();
        ops::fetch(&repo, "origin").unwrap();
        // ローカルを最初のコミットへ巻き戻して別のコミットを作る（分岐）。
        let first = repo.revparse_single("HEAD~2").unwrap().id().to_string();
        ops::reset_hard(&repo, &first).unwrap();
        fx.write_file("z.txt", "z\n");
        fx.stage_all();
        fx.commit("local only");

        let repo = fx.open();
        let ImpactPreview::OverwrittenCommits {
            remote_ref,
            commits,
            truncated,
        } = force_push_preview(&repo, "origin", "main").unwrap()
        else {
            panic!();
        };
        assert_eq!(remote_ref, "origin/main");
        assert!(!truncated);
        let summaries: Vec<_> = commits.iter().map(|c| c.summary.as_str()).collect();
        assert_eq!(summaries, vec!["remote 2", "remote 1"]);
    }

    #[test]
    fn force_push_without_tracking_ref_is_error() {
        let fx = base();
        assert!(force_push_preview(&fx.open(), "origin", "main").is_err());
    }

    #[test]
    fn stash_preview_reports_only_overlapping_files() {
        let fx = base();
        fx.write_file("b.txt", "1\n");
        fx.stage_all();
        fx.commit("b");
        // a.txt と b.txt を変更して退避する。
        fx.write_file("a.txt", "stashed\n");
        fx.write_file("b.txt", "stashed\n");
        let mut repo = fx.open();
        ops::stash_save(&mut repo, "wip").unwrap();
        // 退避後、a.txt だけを再び変更する（b.txt は触らない）。
        fx.write_file("a.txt", "again\n");

        let p = stash_restore_preview(&mut repo, 0).unwrap();
        let ImpactPreview::StashOverlap {
            stash_file_count,
            overlapping,
        } = p
        else {
            panic!();
        };
        assert_eq!(stash_file_count, 2);
        let paths: Vec<_> = overlapping.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["a.txt"]);
    }

    #[test]
    fn stash_preview_clean_tree_has_no_overlap_and_bad_index_errors() {
        let fx = base();
        fx.write_file("a.txt", "s\n");
        let mut repo = fx.open();
        ops::stash_save(&mut repo, "wip").unwrap();
        let ImpactPreview::StashOverlap { overlapping, .. } =
            stash_restore_preview(&mut repo, 0).unwrap()
        else {
            panic!();
        };
        assert!(overlapping.is_empty());
        assert!(stash_restore_preview(&mut repo, 5).is_err());
    }

    #[test]
    fn rewrite_preview_lists_given_commits_and_head_by_default() {
        let fx = base();
        fx.write_file("a.txt", "2\n");
        fx.stage_all();
        let c2 = fx.commit("二番目");
        let repo = fx.open();

        let ImpactPreview::RewrittenCommits { commits, published } =
            rewrite_preview(&repo, &[]).unwrap()
        else {
            panic!();
        };
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].id, c2.to_string());
        assert!(!published, "上流が無ければローカル扱い");

        let ids = vec![c2.to_string(), fx.head_oid().to_string()];
        let ImpactPreview::RewrittenCommits { commits, .. } = rewrite_preview(&repo, &ids).unwrap()
        else {
            panic!();
        };
        assert_eq!(commits.len(), 2);
        assert!(rewrite_preview(&repo, &["zzz".to_string()]).is_err());
    }

    #[test]
    fn rewrite_preview_flags_published_head() {
        let (fx, _remote) = diverged_with_remote();
        let repo = fx.open();
        repo.find_branch("main", BranchType::Local)
            .unwrap()
            .set_upstream(Some("origin/main"))
            .unwrap();
        let ImpactPreview::RewrittenCommits { published, .. } =
            rewrite_preview(&repo, &[]).unwrap()
        else {
            panic!();
        };
        assert!(published);
    }

    #[test]
    fn rebase_plan_preview_shows_before_after_and_dropped() {
        use crate::model::RebaseStep;
        let fx = TestRepo::new();
        let mut ids = Vec::new();
        for (i, f) in ["a.txt", "b.txt", "c.txt"].iter().enumerate() {
            fx.write_file(f, "x\n");
            fx.stage_all();
            ids.push(fx.commit(&format!("c{}", i + 1)));
        }
        let repo = fx.open();
        let plan = vec![
            RebaseStep::Drop {
                oid: ids[1].to_string(),
            },
            RebaseStep::Reword {
                oid: ids[2].to_string(),
                message: "新しい件名\n\n本文".to_string(),
            },
        ];
        let ImpactPreview::RebasePlan {
            before,
            after,
            dropped,
            published,
        } = rebase_plan_preview(&repo, &plan).unwrap()
        else {
            panic!("RebasePlan のはず");
        };
        let s = |v: &[CommitInfo]| v.iter().map(|c| c.summary.clone()).collect::<Vec<_>>();
        assert_eq!(s(&before), ["c3", "c2"]);
        assert_eq!(s(&after), ["新しい件名"]);
        assert_eq!(s(&dropped), ["c2"]);
        assert!(!published);
        // 不正なプランはエラー（プレビューなし）。
        assert!(rebase_plan_preview(&repo, &[]).is_err());
        // preview() からも tagged で呼べる。
        let mut repo = fx.open();
        let p = preview(&mut repo, &ImpactRequest::RebasePlan { plan }).unwrap();
        assert_eq!(serde_json::to_value(&p).unwrap()["kind"], "rebase_plan");
    }

    #[test]
    fn rebase_plan_preview_flags_published_head() {
        use crate::model::RebaseStep;
        let (fx, _remote) = diverged_with_remote();
        fx.write_file("a.txt", "2\n");
        fx.stage_all();
        fx.commit("公開済み");
        let repo = fx.open();
        ops::push(&repo, "origin", "refs/heads/main:refs/heads/main", false).unwrap();
        ops::fetch(&repo, "origin").unwrap();
        repo.find_branch("main", BranchType::Local)
            .unwrap()
            .set_upstream(Some("origin/main"))
            .unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let ImpactPreview::RebasePlan { published, .. } = rebase_plan_preview(
            &repo,
            &[RebaseStep::Reword {
                oid: head.id().to_string(),
                message: "x".to_string(),
            }],
        )
        .unwrap() else {
            panic!();
        };
        assert!(published);
    }

    #[test]
    fn preview_dispatches_by_request_and_serializes_tagged() {
        let fx = base();
        let mut repo = fx.open();
        let p = preview(&mut repo, &ImpactRequest::ResetHard).unwrap();
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["kind"], "lost_changes");
        let req: ImpactRequest =
            serde_json::from_str(r#"{"op":"force_push","remote":"origin","branch":"main"}"#)
                .unwrap();
        assert_eq!(
            req,
            ImpactRequest::ForcePush {
                remote: "origin".into(),
                branch: "main".into()
            }
        );
    }

    #[test]
    fn preview_is_read_only() {
        let fx = base();
        fx.write_file("a.txt", "2\n");
        let mut repo = fx.open();
        let before = repo::status(&repo).unwrap();
        preview(
            &mut repo,
            &ImpactRequest::Discard {
                paths: vec!["a.txt".into()],
            },
        )
        .unwrap();
        preview(&mut repo, &ImpactRequest::ResetHard).unwrap();
        assert_eq!(repo::status(&repo).unwrap(), before);
        assert_eq!(
            std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
            "2\n"
        );
    }
}
