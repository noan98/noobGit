use git2::{BranchType, DiffOptions, Repository, Status, StatusOptions};

use crate::error::{CoreError, Result};
use crate::model::{
    BranchInfo, ChangeKind, CommitInfo, DiffLine, DiffLineKind, FileChange, FileDiff, RepoStatus,
};
use crate::safety::is_protected;

/// 差分として保持する最大行数。これを超えた分は打ち切り、`truncated` を立てる。
/// 巨大な差分で UI と通信が重くなるのを防ぐための保護。
const MAX_DIFF_LINES: usize = 2000;

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
            .and_then(|r| r.symbolic_target().map(strip_branch_prefix)),
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
            staged.push(FileChange {
                path: path.clone(),
                kind,
            });
        }
        if let Some(kind) = unstaged_kind(s) {
            unstaged.push(FileChange {
                path: path.clone(),
                kind,
            });
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
    log_paged(repo, 0, max)
}

/// `skip` 件読み飛ばした位置から `max` 件のコミット履歴を新しい順に返す。
///
/// 履歴パネルの「もっと見る」のような追記読み込み（ページング）に使う。
/// `skip` がコミット総数を超える場合は空のベクタを返す。
pub fn log_paged(repo: &Repository, skip: usize, max: usize) -> Result<Vec<CommitInfo>> {
    if repo.head().is_err() {
        // コミットが1件も無いリポジトリ。
        return Ok(Vec::new());
    }

    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;
    revwalk.set_sorting(git2::Sort::TIME)?;

    let mut out = Vec::new();
    for oid in revwalk.skip(skip).take(max) {
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

/// 未ステージの変更（インデックス↔作業ツリー）のうち、指定パスの差分を返す。
///
/// 未追跡（新規）ファイルも対象に含め、その中身を「追加行」として表示する。
pub fn diff_unstaged(repo: &Repository, path: &str) -> Result<FileDiff> {
    let mut opts = DiffOptions::new();
    opts.pathspec(path)
        .context_lines(3)
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .show_untracked_content(true);
    let index = repo.index()?;
    let diff = repo.diff_index_to_workdir(Some(&index), Some(&mut opts))?;
    build_file_diff(path, &diff)
}

/// ステージ済みの変更（HEAD↔インデックス）のうち、指定パスの差分を返す。
///
/// まだコミットが1件も無い場合は、インデックスの内容すべてを「追加」として表示する。
pub fn diff_staged(repo: &Repository, path: &str) -> Result<FileDiff> {
    let mut opts = DiffOptions::new();
    opts.pathspec(path).context_lines(3);
    let index = repo.index()?;
    let head_tree = match repo.head() {
        Ok(h) => Some(h.peel_to_tree()?),
        Err(_) => None,
    };
    let diff = repo.diff_tree_to_index(head_tree.as_ref(), Some(&index), Some(&mut opts))?;
    build_file_diff(path, &diff)
}

#[derive(Default)]
struct DiffBuild {
    is_binary: bool,
    truncated: bool,
    lines: Vec<DiffLine>,
}

/// `git2::Diff` を走査し、serde 可能な [`FileDiff`] に組み立てる。
///
/// バイナリファイルは行を出さず `is_binary` を立てる。行数が [`MAX_DIFF_LINES`] を
/// 超えたら以降を捨てて `truncated` を立てる（打ち切っても走査自体は最後まで回す）。
fn build_file_diff(path: &str, diff: &git2::Diff) -> Result<FileDiff> {
    use std::cell::RefCell;

    let state = RefCell::new(DiffBuild::default());

    let mut file_cb = |_delta: git2::DiffDelta, _progress: f32| -> bool { true };

    let mut binary_cb = |_delta: git2::DiffDelta, _binary: git2::DiffBinary| -> bool {
        state.borrow_mut().is_binary = true;
        true
    };

    let mut hunk_cb = |_delta: git2::DiffDelta, hunk: git2::DiffHunk| -> bool {
        let mut s = state.borrow_mut();
        if s.lines.len() >= MAX_DIFF_LINES {
            s.truncated = true;
            return true;
        }
        let header = String::from_utf8_lossy(hunk.header());
        s.lines.push(DiffLine {
            kind: DiffLineKind::Hunk,
            old_lineno: None,
            new_lineno: None,
            content: header.trim_end().to_string(),
        });
        true
    };

    let mut line_cb = |_delta: git2::DiffDelta,
                       _hunk: Option<git2::DiffHunk>,
                       line: git2::DiffLine|
     -> bool {
        let mut s = state.borrow_mut();
        if s.lines.len() >= MAX_DIFF_LINES {
            s.truncated = true;
            return true;
        }
        let kind = match line.origin_value() {
            git2::DiffLineType::Addition | git2::DiffLineType::AddEOFNL => DiffLineKind::Addition,
            git2::DiffLineType::Deletion | git2::DiffLineType::DeleteEOFNL => {
                DiffLineKind::Deletion
            }
            _ => DiffLineKind::Context,
        };
        let raw = String::from_utf8_lossy(line.content());
        let content = raw
            .trim_end_matches('\n')
            .trim_end_matches('\r')
            .to_string();
        s.lines.push(DiffLine {
            kind,
            old_lineno: line.old_lineno(),
            new_lineno: line.new_lineno(),
            content,
        });
        true
    };

    diff.foreach(
        &mut file_cb,
        Some(&mut binary_cb),
        Some(&mut hunk_cb),
        Some(&mut line_cb),
    )?;

    let mut build = state.into_inner();
    // バイナリ判定の取りこぼし対策: 走査後はデルタにフラグが立つので念のため確認する。
    if !build.is_binary
        && diff
            .deltas()
            .any(|d| d.flags().contains(git2::DiffFlags::BINARY))
    {
        build.is_binary = true;
    }

    Ok(FileDiff {
        path: path.to_string(),
        is_binary: build.is_binary,
        truncated: build.truncated,
        lines: if build.is_binary {
            Vec::new()
        } else {
            build.lines
        },
    })
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
    fn log_paged_skips_and_takes() {
        let fx = TestRepo::new();
        for i in 0..5 {
            fx.write_file("a.txt", &format!("v{i}"));
            fx.stage_all();
            fx.commit(&format!("c{i}"));
        }

        let repo = fx.open();
        // 全件をひとまとめに取得した並びを基準にする。テストではコミット時刻が同秒に
        // なりうるため、特定の並び順を仮定せず「ページは全件の連続したスライスである」
        // という性質を検証する。
        let all = log_paged(&repo, 0, 100).unwrap();
        let all_ids: Vec<&str> = all.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(all.len(), 5);

        let page_ids = |skip, max| -> Vec<String> {
            log_paged(&repo, skip, max)
                .unwrap()
                .into_iter()
                .map(|c| c.id)
                .collect()
        };

        // 先頭ページ。
        assert_eq!(page_ids(0, 2), all_ids[0..2]);
        // 次のページは前ページの続きから始まる（重複も欠落もない）。
        assert_eq!(page_ids(2, 2), all_ids[2..4]);
        // 残りは1件だけ（max より少なくても正しく返る）。
        assert_eq!(page_ids(4, 10), all_ids[4..5]);

        // skip がコミット総数以上なら空。
        assert!(log_paged(&repo, 5, 10).unwrap().is_empty());
        assert!(log_paged(&repo, 99, 10).unwrap().is_empty());
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

    #[test]
    fn diff_unstaged_shows_added_and_removed_lines() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "line1\nline2\nline3\n");
        fx.stage_all();
        fx.commit("c1");

        fx.write_file("a.txt", "line1\nCHANGED\nline3\n");
        let repo = fx.open();
        let diff = diff_unstaged(&repo, "a.txt").unwrap();

        assert!(!diff.is_binary);
        assert!(!diff.truncated);
        assert!(diff
            .lines
            .iter()
            .any(|l| l.kind == DiffLineKind::Deletion && l.content == "line2"));
        assert!(diff
            .lines
            .iter()
            .any(|l| l.kind == DiffLineKind::Addition && l.content == "CHANGED"));
        assert!(diff
            .lines
            .iter()
            .any(|l| l.kind == DiffLineKind::Context && l.content == "line1"));
        // 行番号が付与される（追加行には新側、削除行には旧側）。
        let added = diff
            .lines
            .iter()
            .find(|l| l.kind == DiffLineKind::Addition)
            .unwrap();
        assert!(added.new_lineno.is_some());
    }

    #[test]
    fn diff_staged_shows_new_file_as_additions() {
        let fx = TestRepo::new();
        fx.write_file("new.txt", "hello\nworld\n");
        fx.stage_all();

        let repo = fx.open();
        let diff = diff_staged(&repo, "new.txt").unwrap();

        assert!(!diff.is_binary);
        let adds: Vec<_> = diff
            .lines
            .iter()
            .filter(|l| l.kind == DiffLineKind::Addition)
            .collect();
        assert_eq!(adds.len(), 2);
        assert_eq!(adds[0].content, "hello");
        assert_eq!(adds[1].content, "world");
        // ハンク見出しが含まれる。
        assert!(diff.lines.iter().any(|l| l.kind == DiffLineKind::Hunk));
    }

    #[test]
    fn diff_unstaged_untracked_file_shows_content() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "x");
        fx.stage_all();
        fx.commit("c1");

        // まだ追跡されていない新規ファイル。
        fx.write_file("untracked.txt", "fresh\nlines\n");
        let repo = fx.open();
        let diff = diff_unstaged(&repo, "untracked.txt").unwrap();

        assert!(!diff.is_binary);
        assert!(diff
            .lines
            .iter()
            .any(|l| l.kind == DiffLineKind::Addition && l.content == "fresh"));
    }

    #[test]
    fn diff_deleted_file_shows_deletions() {
        let fx = TestRepo::new();
        fx.write_file("gone.txt", "a\nb\n");
        fx.stage_all();
        fx.commit("c1");

        std::fs::remove_file(fx.path().join("gone.txt")).unwrap();
        let repo = fx.open();
        let diff = diff_unstaged(&repo, "gone.txt").unwrap();

        assert!(!diff.is_binary);
        assert!(diff.lines.iter().any(|l| l.kind == DiffLineKind::Deletion));
    }

    #[test]
    fn diff_binary_file_is_marked_without_lines() {
        let fx = TestRepo::new();
        // NUL を含む内容は libgit2 がバイナリと判定する。
        fx.write_file("data.bin", "\0\0\0binary\0content\0");
        fx.stage_all();

        let repo = fx.open();
        let diff = diff_staged(&repo, "data.bin").unwrap();

        assert!(diff.is_binary);
        assert!(diff.lines.is_empty());
    }

    #[test]
    fn diff_clean_file_has_no_lines() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "stable\n");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        let diff = diff_unstaged(&repo, "a.txt").unwrap();

        assert!(!diff.is_binary);
        assert!(!diff.truncated);
        assert!(diff.lines.is_empty());
    }

    #[test]
    fn diff_large_file_is_truncated() {
        let fx = TestRepo::new();
        let big: String = (0..MAX_DIFF_LINES + 500)
            .map(|i| format!("line{i}\n"))
            .collect();
        fx.write_file("big.txt", &big);
        fx.stage_all();

        let repo = fx.open();
        let diff = diff_staged(&repo, "big.txt").unwrap();

        assert!(diff.truncated);
        assert!(diff.lines.len() <= MAX_DIFF_LINES);
    }
}
