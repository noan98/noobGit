use git2::{BranchType, Repository, Status, StatusOptions};

use crate::error::{CoreError, Result};
use crate::model::{BranchInfo, ChangeKind, CommitInfo, FileChange, RepoStatus};
use crate::safety::is_protected;

/// 指定パス（またはその親）からGitリポジトリを開く。
///
/// `.git` を上位ディレクトリへ辿って探すため、リポジトリ内のどのフォルダを
/// 指定しても開ける（初学者が迷いにくい）。
pub fn open(path: &str) -> Result<Repository> {
    Repository::discover(path).map_err(|e| CoreError::OpenRepo(e.message().to_string()))
}

/// 現在のブランチ名を取得する。未誕生ブランチ（コミット0件）でも名前を返す。
pub fn current_branch(repo: &Repository) -> Option<String> {
    match repo.head() {
        Ok(h) => h.shorthand().map(|s| s.to_string()),
        Err(_) => repo
            .find_reference("HEAD")
            .ok()
            .and_then(|r| r.symbolic_target().map(|t| strip_branch_prefix(t))),
    }
}

fn strip_branch_prefix(refname: &str) -> String {
    refname
        .strip_prefix("refs/heads/")
        .unwrap_or(refname)
        .to_string()
}

/// 作業ツリーに未コミットの変更（ステージ済み含む）があるか。
pub fn is_dirty(repo: &Repository) -> Result<bool> {
    let status = status(repo)?;
    Ok(!status.is_clean)
}

/// リポジトリの現在状態（git status 相当）を返す。
pub fn status(repo: &Repository) -> Result<RepoStatus> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false);

    let statuses = repo.statuses(Some(&mut opts))?;

    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();
    let mut conflicted = Vec::new();

    for entry in statuses.iter() {
        let s = entry.status();
        let path = entry.path().unwrap_or("").to_string();
        if path.is_empty() {
            continue;
        }

        if s.contains(Status::CONFLICTED) {
            conflicted.push(path.clone());
        }
        if s.contains(Status::WT_NEW) {
            untracked.push(path.clone());
        }

        if let Some(kind) = staged_kind(s) {
            staged.push(FileChange { path: path.clone(), kind });
        }
        if let Some(kind) = unstaged_kind(s) {
            unstaged.push(FileChange { path: path.clone(), kind });
        }
    }

    let is_clean =
        staged.is_empty() && unstaged.is_empty() && untracked.is_empty() && conflicted.is_empty();

    Ok(RepoStatus {
        branch: current_branch(repo),
        staged,
        unstaged,
        untracked,
        conflicted,
        is_clean,
    })
}

fn staged_kind(s: Status) -> Option<ChangeKind> {
    if s.contains(Status::INDEX_NEW) {
        Some(ChangeKind::Added)
    } else if s.contains(Status::INDEX_MODIFIED) {
        Some(ChangeKind::Modified)
    } else if s.contains(Status::INDEX_DELETED) {
        Some(ChangeKind::Deleted)
    } else if s.contains(Status::INDEX_RENAMED) {
        Some(ChangeKind::Renamed)
    } else if s.contains(Status::INDEX_TYPECHANGE) {
        Some(ChangeKind::TypeChange)
    } else {
        None
    }
}

fn unstaged_kind(s: Status) -> Option<ChangeKind> {
    if s.contains(Status::WT_MODIFIED) {
        Some(ChangeKind::Modified)
    } else if s.contains(Status::WT_DELETED) {
        Some(ChangeKind::Deleted)
    } else if s.contains(Status::WT_RENAMED) {
        Some(ChangeKind::Renamed)
    } else if s.contains(Status::WT_TYPECHANGE) {
        Some(ChangeKind::TypeChange)
    } else {
        None
    }
}

/// ローカル/リモートのブランチ一覧を返す。
pub fn branches(repo: &Repository, protected: &[String]) -> Result<Vec<BranchInfo>> {
    let mut out = Vec::new();

    for bt in [BranchType::Local, BranchType::Remote] {
        for item in repo.branches(Some(bt))? {
            let (branch, _) = item?;
            let name = match branch.name()? {
                Some(n) => n.to_string(),
                None => continue,
            };
            let is_remote = bt == BranchType::Remote;
            let upstream = if is_remote {
                None
            } else {
                branch
                    .upstream()
                    .ok()
                    .and_then(|u| u.name().ok().flatten().map(|s| s.to_string()))
            };
            out.push(BranchInfo {
                is_head: branch.is_head(),
                is_protected: !is_remote && is_protected(&name, protected),
                name,
                is_remote,
                upstream,
            });
        }
    }

    Ok(out)
}

/// 直近 `max` 件のコミット履歴を新しい順に返す。
pub fn log(repo: &Repository, max: usize) -> Result<Vec<CommitInfo>> {
    if repo.head().is_err() {
        // コミットが1件も無いリポジトリ。
        return Ok(Vec::new());
    }

    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;
    revwalk.set_sorting(git2::Sort::TIME)?;

    let mut out = Vec::new();
    for oid in revwalk.take(max) {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        let author = commit.author();
        out.push(CommitInfo {
            id: oid.to_string(),
            short_id: oid.to_string().chars().take(7).collect(),
            summary: commit.summary().unwrap_or("").to_string(),
            author_name: author.name().unwrap_or("").to_string(),
            author_email: author.email().unwrap_or("").to_string(),
            time: commit.time().seconds(),
        });
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[test]
    fn open_discovers_repo_from_subdir() {
        let fx = TestRepo::new();
        let sub = fx.path().join("a/b");
        std::fs::create_dir_all(&sub).unwrap();
        let repo = open(sub.to_str().unwrap()).unwrap();
        assert!(repo.workdir().is_some());
    }

    #[test]
    fn empty_repo_is_clean_with_branch_name() {
        let fx = TestRepo::new();
        let repo = fx.open();
        let st = status(&repo).unwrap();
        assert!(st.is_clean);
        // 初期ブランチ名が取得できる（未誕生でも）。
        assert!(st.branch.is_some());
        assert!(log(&repo, 10).unwrap().is_empty());
    }

    #[test]
    fn untracked_then_staged_then_committed() {
        let fx = TestRepo::new();
        fx.write_file("hello.txt", "hi");

        let repo = fx.open();
        let st = status(&repo).unwrap();
        assert_eq!(st.untracked, vec!["hello.txt".to_string()]);
        assert!(!st.is_clean);

        fx.stage_all();
        let repo = fx.open();
        let st = status(&repo).unwrap();
        assert_eq!(st.staged.len(), 1);
        assert_eq!(st.staged[0].kind, ChangeKind::Added);
        assert!(st.untracked.is_empty());

        fx.commit("最初のコミット");
        let repo = fx.open();
        let st = status(&repo).unwrap();
        assert!(st.is_clean);
        let log = log(&repo, 10).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].summary, "最初のコミット");
    }

    #[test]
    fn modified_tracked_file_is_unstaged() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        fx.write_file("a.txt", "2");
        let repo = fx.open();
        let st = status(&repo).unwrap();
        assert_eq!(st.unstaged.len(), 1);
        assert_eq!(st.unstaged[0].kind, ChangeKind::Modified);
    }

    #[test]
    fn branches_lists_local_with_protection_flag() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        let branches = branches(&repo, &[]).unwrap();
        let head = branches.iter().find(|b| b.is_head).unwrap();
        // 既定ブランチ名(main)は保護対象。
        assert!(head.is_protected);
        assert!(!head.is_remote);
    }
}
