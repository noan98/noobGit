use git2::{BranchType, DiffOptions, Repository, Status, StatusOptions};

use crate::error::{CoreError, Result};
use crate::model::{
    BranchGraph, BranchInfo, BranchRelation, ChangeKind, CommitInfo, DiffLine, DiffLineKind,
    FileChange, FileDiff, LikelyBase, RepoStatus,
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

/// 直前のコミット（HEAD）がすでにリモートへ送信（公開）済みとみなせるか。
///
/// 現在ブランチに上流（upstream）があり、ローカルが上流より先行していない
/// （＝HEAD が上流から辿れる）ときに `true` を返す。amend の危険度判定に使う。
/// 上流が無い・先端が取れない場合は判断できないので `false`（ローカル扱い）にする。
pub fn head_is_published(repo: &Repository) -> Result<bool> {
    let name = match current_branch(repo) {
        Some(n) => n,
        None => return Ok(false),
    };
    let local = match repo.find_branch(&name, BranchType::Local) {
        Ok(b) => b,
        Err(_) => return Ok(false),
    };
    let upstream = match local.upstream() {
        Ok(u) => u,
        Err(_) => return Ok(false),
    };
    let (local_oid, upstream_oid) = match (local.get().target(), upstream.get().target()) {
        (Some(l), Some(u)) => (l, u),
        _ => return Ok(false),
    };
    // ahead = ローカルにあって上流に無いコミット数。0 なら HEAD は上流に含まれる＝公開済み。
    let (ahead, _behind) = repo.graph_ahead_behind(local_oid, upstream_oid)?;
    Ok(ahead == 0)
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
        is_conflicted: false,
        lines: if build.is_binary {
            Vec::new()
        } else {
            build.lines
        },
    })
}

/// コンフリクト中ファイルの「いまの作業ツリーの中身」を返す。
///
/// コンフリクト中はインデックスに stage 0 が無く、通常の差分（インデックス↔作業
/// ツリー / HEAD↔インデックス）では何も出ない。そこで作業ツリーのファイル内容を
/// そのまま行として返し、`<<<<<<<` などの競合の目印を初学者が確認できるようにする。
/// 各行は文脈行として扱い、目印かどうかの色付けはフロント側で行う。
pub fn diff_conflict(repo: &Repository, path: &str) -> Result<FileDiff> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| CoreError::Git("作業ツリーがありません。".to_string()))?;
    // 作業ツリー外を指す相対パスは読み取らない（安全のため）。
    let rel = std::path::Path::new(path);
    if rel.is_absolute()
        || rel
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(CoreError::InvalidInput(format!("不正なパスです: {path}")));
    }
    // ファイルが消えているコンフリクト（削除競合など）もありうるので、読めなければ空にする。
    let bytes = std::fs::read(workdir.join(rel)).unwrap_or_default();

    if bytes.contains(&0) {
        return Ok(FileDiff {
            path: path.to_string(),
            is_binary: true,
            truncated: false,
            is_conflicted: true,
            lines: Vec::new(),
        });
    }

    let text = String::from_utf8_lossy(&bytes);
    let mut lines = Vec::new();
    let mut truncated = false;
    for (i, line) in text.lines().enumerate() {
        if lines.len() >= MAX_DIFF_LINES {
            truncated = true;
            break;
        }
        lines.push(DiffLine {
            kind: DiffLineKind::Context,
            old_lineno: None,
            new_lineno: Some((i + 1) as u32),
            content: line.to_string(),
        });
    }

    Ok(FileDiff {
        path: path.to_string(),
        is_binary: false,
        truncated,
        is_conflicted: true,
        lines,
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
    fn head_is_published_true_when_not_ahead_of_upstream() {
        use crate::ops::{commit, stage_all};

        // 上流（upstream）を用意してクローンする。クローン直後は HEAD == origin/main。
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "1");
        upstream.stage_all();
        upstream.commit("c1");

        let dir = tempfile::TempDir::new().unwrap();
        let dest = dir.path().join("clone");
        let cloned = git2::Repository::clone(upstream.path().to_str().unwrap(), &dest).unwrap();
        {
            let mut cfg = cloned.config().unwrap();
            cfg.set_str("user.name", "Clone User").unwrap();
            cfg.set_str("user.email", "clone@example.com").unwrap();
        }

        // 上流より先行していない → 公開済みとみなす。
        let repo = git2::Repository::open(&dest).unwrap();
        assert!(head_is_published(&repo).unwrap());

        // ローカルにコミットを積むと上流より先行する → 未公開扱い。
        std::fs::write(dest.join("a.txt"), "2").unwrap();
        stage_all(&repo).unwrap();
        commit(&repo, "local-c2").unwrap();
        let repo = git2::Repository::open(&dest).unwrap();
        assert!(!head_is_published(&repo).unwrap());
    }

    #[test]
    fn head_is_published_false_without_upstream() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        // 上流が無いローカルブランチは判断できないので false。
        assert!(!head_is_published(&fx.open()).unwrap());
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

    /// `a.txt` がコンフリクト中になった一時リポジトリを作る。
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
    fn conflicted_file_only_appears_in_conflicted_list() {
        let fx = repo_with_conflict();
        let repo = fx.open();
        let st = status(&repo).unwrap();
        assert_eq!(st.conflicted, vec!["a.txt".to_string()]);
        // 通常の差分ではコンフリクト中ファイルは何も出ない（stage 0 が無いため）。
        assert!(diff_unstaged(&repo, "a.txt").unwrap().lines.is_empty());
        assert!(diff_staged(&repo, "a.txt").unwrap().lines.is_empty());
    }

    #[test]
    fn diff_conflict_shows_working_tree_with_markers() {
        let fx = repo_with_conflict();
        let repo = fx.open();
        let diff = diff_conflict(&repo, "a.txt").unwrap();

        assert!(diff.is_conflicted);
        assert!(!diff.is_binary);
        // 競合の目印と両側の内容がそのまま見える。
        assert!(diff.lines.iter().any(|l| l.content.starts_with("<<<<<<<")));
        assert!(diff.lines.iter().any(|l| l.content.starts_with("=======")));
        assert!(diff.lines.iter().any(|l| l.content.starts_with(">>>>>>>")));
        assert!(diff.lines.iter().any(|l| l.content == "other side"));
        assert!(diff.lines.iter().any(|l| l.content == "main side"));
        // すべて文脈行として返す。
        assert!(diff.lines.iter().all(|l| l.kind == DiffLineKind::Context));
    }

    #[test]
    fn diff_conflict_rejects_path_traversal() {
        let fx = TestRepo::new();
        let repo = fx.open();
        assert!(diff_conflict(&repo, "../secret.txt").is_err());
        assert!(diff_conflict(&repo, "/etc/passwd").is_err());
    }
}
