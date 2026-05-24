use git2::{BranchType, Repository, Status, StatusOptions};

use crate::error::{CoreError, Result};
use crate::model::{
    BranchGraph, BranchInfo, BranchRelation, ChangeKind, CommitInfo, FileChange, LikelyBase,
    RepoStatus,
};
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

/// 現在ブランチと各ローカルブランチの関係（取り込み済み判定・ahead/behind・派生元推定）を返す。
///
/// すべて読み取り専用。判定の基準は現在ブランチの先端コミット。未誕生ブランチや
/// detached HEAD のように先端を特定できない場合は、関係を計算せず空で返す。
pub fn branch_graph(repo: &Repository) -> Result<BranchGraph> {
    let current = current_branch(repo);

    // 現在ブランチの先端 Oid。コミットが無い／detached 等で取れなければ関係は出せない。
    let head_oid = match repo.head().ok().and_then(|h| h.target()) {
        Some(o) => o,
        None => {
            return Ok(BranchGraph {
                current,
                likely_base: None,
                relations: Vec::new(),
            });
        }
    };

    let mut relations = Vec::new();
    // 派生元候補: (ブランチ名, 現在ブランチが先行している数, 現在ブランチが遅れている数)。
    // 「分岐点が現在ブランチの先端に最も近い」= 現在ブランチがそのブランチより先行している
    //  コミット数（ahead）が最小、という基準で推定する。
    let mut candidates: Vec<(String, usize, usize)> = Vec::new();

    for item in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = item?;
        let name = match branch.name()? {
            Some(n) => n.to_string(),
            None => continue,
        };
        let tip = match branch.get().target() {
            Some(o) => o,
            None => continue,
        };
        let is_current = current.as_deref() == Some(name.as_str());

        // (このブランチが現在ブランチより先行している数, 遅れている数)。
        let (ahead, behind) = repo.graph_ahead_behind(tip, head_oid)?;
        // 取り込み済み = このブランチの先端が現在ブランチの先祖（独自コミットが無い）。
        // 現在ブランチ自身は取り込み済み扱いにしない。
        let merged_into_current = !is_current && ahead == 0;

        relations.push(BranchRelation {
            name: name.clone(),
            is_current,
            merged_into_current,
            ahead,
            behind,
        });

        if !is_current {
            // 現在ブランチ視点に変換: 現在が先行している数 = behind、遅れている数 = ahead。
            candidates.push((name, behind, ahead));
        }
    }

    let likely_base = pick_likely_base(candidates);

    Ok(BranchGraph {
        current,
        likely_base,
        relations,
    })
}

/// 派生元（推定）を選ぶ。分岐点が現在ブランチの先端に最も近い（現在ブランチの先行数が
/// 最小の）ブランチを採用し、同点が複数あれば曖昧フラグを立てる。
fn pick_likely_base(mut candidates: Vec<(String, usize, usize)>) -> Option<LikelyBase> {
    if candidates.is_empty() {
        return None;
    }
    // 名前順に整えてから選ぶことで、同点時の採用候補を決定的にする。
    candidates.sort_by(|a, b| a.0.cmp(&b.0));

    let min_ahead = candidates.iter().map(|c| c.1).min()?;
    let tied = candidates.iter().filter(|c| c.1 == min_ahead).count();
    let (name, ahead, behind) = candidates.into_iter().find(|c| c.1 == min_ahead)?;

    Some(LikelyBase {
        name,
        ambiguous: tied > 1,
        ahead,
        behind,
    })
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
    fn branch_graph_detects_merged_and_unmerged() {
        use crate::ops::{create_branch, switch_branch};

        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1"); // main: c1

        let repo = fx.open();
        // merged は c1 のまま据え置く（後で main が進むので取り込み済みになる）。
        create_branch(&repo, "merged").unwrap();
        // feature は独自コミットを持たせて未取り込みにする。
        create_branch(&repo, "feature").unwrap();

        switch_branch(&repo, "feature").unwrap();
        fx.write_file("b.txt", "x");
        fx.stage_all();
        fx.commit("feature-c2"); // feature: c1 -> feature-c2

        let repo = fx.open();
        switch_branch(&repo, "main").unwrap();
        fx.write_file("a.txt", "2");
        fx.stage_all();
        fx.commit("main-c2"); // main: c1 -> main-c2

        let repo = fx.open();
        let g = branch_graph(&repo).unwrap();
        assert_eq!(g.current.as_deref(), Some("main"));

        let by = |n: &str| g.relations.iter().find(|r| r.name == n).unwrap();

        // merged は c1 のまま → main(c1->main-c2) に取り込み済み。
        assert!(by("merged").merged_into_current);
        assert_eq!(by("merged").ahead, 0);

        // feature は独自コミットがある → 未取り込み。ahead/behind が両方 1。
        assert!(!by("feature").merged_into_current);
        assert_eq!(by("feature").ahead, 1); // feature-c2
        assert_eq!(by("feature").behind, 1); // main-c2

        // 現在ブランチ自身は取り込み済み扱いにしない。
        assert!(by("main").is_current);
        assert!(!by("main").merged_into_current);
    }

    #[test]
    fn branch_graph_estimates_likely_base_with_ahead_behind() {
        use crate::ops::{create_branch, switch_branch};

        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        fx.write_file("a.txt", "2");
        fx.stage_all();
        fx.commit("c2"); // main: c1 -> c2

        let repo = fx.open();
        create_branch(&repo, "feature").unwrap();
        switch_branch(&repo, "feature").unwrap();
        fx.write_file("a.txt", "3");
        fx.stage_all();
        fx.commit("c3"); // feature: c1 -> c2 -> c3

        let repo = fx.open();
        let g = branch_graph(&repo).unwrap();
        assert_eq!(g.current.as_deref(), Some("feature"));

        let lb = g.likely_base.expect("派生元が推定できる");
        assert_eq!(lb.name, "main");
        assert!(!lb.ambiguous);
        // feature は main より c3 の分だけ先行し、遅れは無い。
        assert_eq!(lb.ahead, 1);
        assert_eq!(lb.behind, 0);
    }

    #[test]
    fn branch_graph_marks_ambiguous_base_on_tie() {
        use crate::ops::{create_branch, switch_branch};

        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1"); // main: c1

        let repo = fx.open();
        // a と b は c1 に据え置く（feature から見て同じ距離の候補になる）。
        create_branch(&repo, "a").unwrap();
        create_branch(&repo, "b").unwrap();
        create_branch(&repo, "feature").unwrap();

        switch_branch(&repo, "feature").unwrap();
        fx.write_file("a.txt", "2");
        fx.stage_all();
        fx.commit("c2"); // feature: c1 -> c2

        let repo = fx.open();
        let g = branch_graph(&repo).unwrap();
        let lb = g.likely_base.expect("候補はある");
        // main/a/b すべて c1 で同点 → 曖昧。採用名は決定的に名前順で先頭。
        assert!(lb.ambiguous);
        assert_eq!(lb.name, "a");
    }

    #[test]
    fn branch_graph_empty_repo_has_no_relations() {
        let fx = TestRepo::new();
        let repo = fx.open();
        let g = branch_graph(&repo).unwrap();
        assert!(g.relations.is_empty());
        assert!(g.likely_base.is_none());
    }

    #[test]
    fn branch_graph_single_branch_has_no_likely_base() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        let g = branch_graph(&repo).unwrap();
        // 自分以外のローカルブランチが無いので派生元は推定できない。
        assert!(g.likely_base.is_none());
        assert_eq!(g.relations.len(), 1);
        assert!(g.relations[0].is_current);
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
