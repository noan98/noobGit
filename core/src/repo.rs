use git2::{BranchType, DiffOptions, Repository, Status, StatusOptions};

use crate::error::{CoreError, Result};
use crate::model::{
    BlameHunk, BranchGraph, BranchInfo, BranchRelation, ChangeKind, CommitInfo, ConflictFile,
    DiffLine, DiffLineKind, FileChange, FileDiff, LikelyBase, RemoteInfo, RepoStatus, TagInfo,
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

/// タグの一覧を返す（名前順）。
///
/// 軽量タグ・注釈付きタグの両方を扱う。注釈付きタグは `repo.find_tag` で解決でき、
/// メッセージと指す対象（多くはコミット）を持つ。軽量タグは参照が直接コミットを指す。
/// `target_id` には最終的に指すコミット等の oid を入れ、`target_short_id` はその先頭7文字。
pub fn list_tags(repo: &Repository) -> Result<Vec<TagInfo>> {
    let mut out = Vec::new();

    let names = repo.tag_names(None)?;
    for name in names.iter().flatten() {
        // タグ参照（refs/tags/<name>）を解決する。
        let refname = format!("refs/tags/{name}");
        let reference = match repo.find_reference(&refname) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let oid = match reference.target() {
            Some(o) => o,
            None => continue,
        };

        // 注釈付きタグなら、その oid から Tag オブジェクトを取得できる。
        let (target_oid, message) = match repo.find_tag(oid) {
            Ok(tag) => {
                let msg = tag.message().map(|m| m.trim_end().to_string());
                (tag.target_id(), msg)
            }
            // 軽量タグ: 参照が直接対象（コミット等）を指す。
            Err(_) => (oid, None),
        };

        let id = target_oid.to_string();
        out.push(TagInfo {
            target_short_id: id.chars().take(7).collect(),
            target_id: id,
            name: name.to_string(),
            message,
        });
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// リモート一覧を返す（名前順）。
///
/// 各リモートの fetch URL と push URL を取得する。push URL が fetch URL と同じか
/// 設定されていない場合は `push_url` を `None` にして返す（UI での表示を簡潔にするため）。
pub fn list_remotes(repo: &Repository) -> Result<Vec<RemoteInfo>> {
    let names = repo.remotes().map_err(|e| {
        CoreError::Git(format!("リモート一覧の取得に失敗しました: {}", e.message()))
    })?;

    let mut out = Vec::new();
    for name in names.iter().flatten() {
        let remote = match repo.find_remote(name) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let fetch_url = remote.url().unwrap_or("").to_string();
        let push_raw = remote.pushurl().unwrap_or("");
        // push URL が空か fetch URL と同じなら None とする（UI を簡潔に保つ）。
        let push_url = if push_raw.is_empty() || push_raw == fetch_url {
            None
        } else {
            Some(push_raw.to_string())
        };
        out.push(RemoteInfo {
            name: name.to_string(),
            fetch_url,
            push_url,
        });
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
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

/// コミット履歴の絞り込み条件。すべて任意で、`None` の項目は条件として使わない。
///
/// `message` はコミットのメッセージ（件名・本文）への部分一致（大文字小文字無視）、
/// `author` は author の名前またはメールアドレスへの部分一致（大文字小文字無視）、
/// `since` / `until` はコミット時刻（Unix エポック秒）の下限・上限（両端を含む）。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LogFilter {
    /// メッセージに含まれていてほしい文字列（部分一致・大文字小文字無視）。
    pub message: Option<String>,
    /// author 名またはメールに含まれていてほしい文字列（部分一致・大文字小文字無視）。
    pub author: Option<String>,
    /// この時刻（Unix エポック秒）以降のコミットだけを残す（その時刻を含む）。
    pub since: Option<i64>,
    /// この時刻（Unix エポック秒）以前のコミットだけを残す（その時刻を含む）。
    pub until: Option<i64>,
}

impl LogFilter {
    /// 条件が一つも設定されていない（＝全件素通し）かどうか。
    fn is_empty(&self) -> bool {
        self.message.is_none()
            && self.author.is_none()
            && self.since.is_none()
            && self.until.is_none()
    }

    /// 与えられたコミットがこの条件をすべて満たすか判定する。
    fn matches(&self, commit: &git2::Commit) -> bool {
        if let Some(needle) = &self.message {
            let needle = needle.to_lowercase();
            // 件名だけでなく本文も対象にする（message() は件名＋本文を含む）。
            let haystack = commit.message().unwrap_or("").to_lowercase();
            if !haystack.contains(&needle) {
                return false;
            }
        }
        if let Some(needle) = &self.author {
            let needle = needle.to_lowercase();
            let author = commit.author();
            let name = author.name().unwrap_or("").to_lowercase();
            let email = author.email().unwrap_or("").to_lowercase();
            if !name.contains(&needle) && !email.contains(&needle) {
                return false;
            }
        }
        let time = commit.time().seconds();
        if let Some(since) = self.since {
            if time < since {
                return false;
            }
        }
        if let Some(until) = self.until {
            if time > until {
                return false;
            }
        }
        true
    }
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
    log_filtered(repo, skip, max, &LogFilter::default())
}

/// 条件で絞り込んだコミット履歴を、新しい順に `skip` 件飛ばして `max` 件返す。
///
/// `Revwalk` で履歴を新しい順にたどり、`filter` を通過したコミットだけを対象に
/// `skip` / `max`（ページング）を適用する（＝クライアント側フィルタ）。`filter` に
/// 条件が一つも無いときは [`log_paged`] と同じ結果になり、後方互換を保つ。
/// `skip` が条件通過後の総数を超える場合は空のベクタを返す。
pub fn log_filtered(
    repo: &Repository,
    skip: usize,
    max: usize,
    filter: &LogFilter,
) -> Result<Vec<CommitInfo>> {
    if repo.head().is_err() {
        // コミットが1件も無いリポジトリ。
        return Ok(Vec::new());
    }

    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;
    revwalk.set_sorting(git2::Sort::TIME)?;

    let no_filter = filter.is_empty();
    let mut out = Vec::new();
    let mut skipped = 0usize;
    for oid in revwalk {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        // 条件を満たさないコミットは skip にも max にも数えない。
        if !no_filter && !filter.matches(&commit) {
            continue;
        }
        // 条件通過分のうち、先頭 `skip` 件を読み飛ばす。
        if skipped < skip {
            skipped += 1;
            continue;
        }
        let author = commit.author();
        out.push(CommitInfo {
            id: oid.to_string(),
            short_id: oid.to_string().chars().take(7).collect(),
            summary: commit.summary().unwrap_or("").to_string(),
            author_name: author.name().unwrap_or("").to_string(),
            author_email: author.email().unwrap_or("").to_string(),
            time: commit.time().seconds(),
            parent_ids: commit.parent_ids().map(|p| p.to_string()).collect(),
        });
        if out.len() >= max {
            break;
        }
    }

    Ok(out)
}

/// 指定ファイルを変更したコミットだけを新しい順に最大 `max` 件返す（ファイル別履歴）。
///
/// 全コミットを時刻順に走査し、各コミットについて「そのコミットのツリー」と「第1親の
/// ツリー」を `path` に絞って差分（`diff_tree_to_tree`）し、変更（delta）が1件以上ある
/// コミットだけを集める。親が無い最初のコミットは、そのツリーに `path` が含まれていれば
/// 対象にする。HEAD が無い（コミット0件）リポジトリは空の `Vec` を返す。
pub fn file_log(repo: &Repository, path: &str, max: usize) -> Result<Vec<CommitInfo>> {
    if repo.head().is_err() {
        // コミットが1件も無いリポジトリ。
        return Ok(Vec::new());
    }

    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;
    revwalk.set_sorting(git2::Sort::TIME)?;

    let mut out = Vec::new();
    for oid in revwalk {
        if out.len() >= max {
            break;
        }
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        let tree = commit.tree()?;

        // このコミットで `path` が変更されたか判定する。
        let touched = if commit.parent_count() == 0 {
            // 最初のコミットには親が無いので、ツリーに `path` があれば「追加された」とみなす。
            tree.get_path(std::path::Path::new(path)).is_ok()
        } else {
            let parent_tree = commit.parent(0)?.tree()?;
            let mut opts = DiffOptions::new();
            opts.pathspec(path);
            let diff = repo.diff_tree_to_tree(Some(&parent_tree), Some(&tree), Some(&mut opts))?;
            diff.deltas().len() > 0
        };

        if !touched {
            continue;
        }

        let author = commit.author();
        out.push(CommitInfo {
            id: oid.to_string(),
            short_id: oid.to_string().chars().take(7).collect(),
            summary: commit.summary().unwrap_or("").to_string(),
            author_name: author.name().unwrap_or("").to_string(),
            author_email: author.email().unwrap_or("").to_string(),
            time: commit.time().seconds(),
            parent_ids: commit.parent_ids().map(|p| p.to_string()).collect(),
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

/// 2 つのコミット間（または親コミット↔指定コミット）の全変更ファイルの差分を返す。
///
/// `to_oid` は比較対象（新しい側）のコミット。`from_oid` を渡すとその間の差分、
/// `None` のときは `to` コミットの第1親との比較になる（親が無い最初のコミットなら
/// 空ツリーとの比較＝すべて追加として表示）。
///
/// oid のパースに失敗した場合や、そのコミットが見つからない場合は
/// [`CoreError::InvalidInput`] を返す。ファイルごとに [`FileDiff`] を組み立てて
/// ベクタで返し、各ファイルの行差分は [`build_file_diff`] と同じ流儀で作る。
pub fn diff_commits(
    repo: &Repository,
    from_oid: Option<&str>,
    to_oid: &str,
) -> Result<Vec<FileDiff>> {
    let to_commit = parse_commit(repo, to_oid)?;
    let to_tree = to_commit.tree()?;

    // from を決める。明示指定があればそれを、無ければ to の第1親を使う。
    // 親が無い最初のコミットでは from_tree を None（空ツリー扱い）にする。
    let from_tree = match from_oid {
        Some(oid) => Some(parse_commit(repo, oid)?.tree()?),
        None => match to_commit.parent(0) {
            Ok(parent) => Some(parent.tree()?),
            Err(_) => None,
        },
    };

    let mut opts = DiffOptions::new();
    opts.context_lines(3);
    let diff = repo.diff_tree_to_tree(from_tree.as_ref(), Some(&to_tree), Some(&mut opts))?;

    build_file_diffs(&diff)
}

/// 文字列の oid をパースし、対応するコミットを取り出す。
///
/// oid が不正な形式、またはそのコミットが見つからない場合は
/// [`CoreError::InvalidInput`] を日本語で返す。
fn parse_commit<'r>(repo: &'r Repository, oid: &str) -> Result<git2::Commit<'r>> {
    let parsed = git2::Oid::from_str(oid).map_err(|_| {
        CoreError::InvalidInput(format!("コミットIDの形式が正しくありません: {oid}"))
    })?;
    repo.find_commit(parsed)
        .map_err(|_| CoreError::InvalidInput(format!("コミットが見つかりません: {oid}")))
}

/// ツリー間の `git2::Diff` を走査し、ファイルごとに [`FileDiff`] へ組み立てる。
///
/// [`build_file_diff`] が単一パス向けなのに対し、こちらは差分に含まれる全ファイルを
/// それぞれ独立した [`FileDiff`] に分けて返す。バイナリ判定・[`MAX_DIFF_LINES`] での
/// 打ち切りは各ファイルごとに行う。
fn build_file_diffs(diff: &git2::Diff) -> Result<Vec<FileDiff>> {
    use std::cell::RefCell;
    use std::collections::HashMap;

    // パスごとの組み立て状態と、初出順を保つためのパス並び。
    #[derive(Default)]
    struct State {
        order: Vec<String>,
        builds: HashMap<String, DiffBuild>,
    }

    impl State {
        // 初出のパスは順序リストに登録しつつ、その組み立て状態への可変参照を返す。
        fn entry(&mut self, path: String) -> &mut DiffBuild {
            if !self.builds.contains_key(&path) {
                self.order.push(path.clone());
                self.builds.insert(path.clone(), DiffBuild::default());
            }
            self.builds.get_mut(&path).unwrap()
        }
    }

    // デルタから表示用のパスを取る（new_file 優先、無ければ old_file）。
    fn delta_path(delta: &git2::DiffDelta) -> String {
        delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    let state = RefCell::new(State::default());

    let mut file_cb = |delta: git2::DiffDelta, _progress: f32| -> bool {
        let path = delta_path(&delta);
        state.borrow_mut().entry(path);
        true
    };

    let mut binary_cb = |delta: git2::DiffDelta, _binary: git2::DiffBinary| -> bool {
        let path = delta_path(&delta);
        state.borrow_mut().entry(path).is_binary = true;
        true
    };

    let mut hunk_cb = |delta: git2::DiffDelta, hunk: git2::DiffHunk| -> bool {
        let path = delta_path(&delta);
        let mut s = state.borrow_mut();
        let build = s.entry(path);
        if build.lines.len() >= MAX_DIFF_LINES {
            build.truncated = true;
            return true;
        }
        let header = String::from_utf8_lossy(hunk.header());
        build.lines.push(DiffLine {
            kind: DiffLineKind::Hunk,
            old_lineno: None,
            new_lineno: None,
            content: header.trim_end().to_string(),
        });
        true
    };

    let mut line_cb = |delta: git2::DiffDelta,
                       _hunk: Option<git2::DiffHunk>,
                       line: git2::DiffLine|
     -> bool {
        let path = delta_path(&delta);
        let mut s = state.borrow_mut();
        let build = s.entry(path);
        if build.lines.len() >= MAX_DIFF_LINES {
            build.truncated = true;
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
        build.lines.push(DiffLine {
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

    // バイナリ判定の取りこぼし対策: 走査後にデルタのフラグでも確認する。
    {
        let mut s = state.borrow_mut();
        for delta in diff.deltas() {
            if delta.flags().contains(git2::DiffFlags::BINARY) {
                let path = delta_path(&delta);
                s.entry(path).is_binary = true;
            }
        }
    }

    let state = state.into_inner();
    let mut builds = state.builds;
    let mut out = Vec::with_capacity(state.order.len());
    for path in state.order {
        let build = builds.remove(&path).unwrap_or_default();
        out.push(FileDiff {
            path,
            is_binary: build.is_binary,
            truncated: build.truncated,
            is_conflicted: false,
            lines: if build.is_binary {
                Vec::new()
            } else {
                build.lines
            },
        });
    }

    Ok(out)
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

/// 指定ファイルの blame（各行を最後に変更したコミット）を行のかたまり単位で返す。
///
/// `git blame` 相当。連続する行が同じコミットで最後に変更された場合はまとめて1つの
/// hunk になる。`path` は作業ツリー内の相対パス。ファイルが存在しない・コミットが
/// 1件も無い・バイナリ等の場合は日本語エラーを返す。
pub fn blame_file(repo: &Repository, path: &str) -> Result<Vec<BlameHunk>> {
    // 作業ツリー外を指す相対パスは扱わない（安全のため）。
    let rel = std::path::Path::new(path);
    if rel.is_absolute()
        || rel
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(CoreError::InvalidInput(format!("不正なパスです: {path}")));
    }

    let blame = repo.blame_file(rel, None).map_err(|e| {
        CoreError::Git(format!(
            "ファイル「{path}」の変更履歴（blame）を取得できませんでした: {}",
            e.message()
        ))
    })?;

    let mut out = Vec::with_capacity(blame.len());
    for hunk in blame.iter() {
        let oid = hunk.final_commit_id();
        let commit = repo.find_commit(oid)?;
        let author = commit.author();
        out.push(BlameHunk {
            lines_start: hunk.final_start_line(),
            lines_count: hunk.lines_in_hunk(),
            commit_id: oid.to_string(),
            short_id: oid.to_string().chars().take(7).collect(),
            message_short: commit.summary().unwrap_or("").to_string(),
            author_name: author.name().unwrap_or("").to_string(),
            time: commit.time().seconds(),
        });
    }

    Ok(out)
}

/// コンフリクト中のファイル一覧を返す。
///
/// インデックスの conflict エントリ（stage 1=共通祖先 / 2=our / 3=their）を走査し、
/// ファイルごとに 1 件へまとめる。パスは `our`→`their`→`ancestor` の順で取れたものを採用し、
/// `String::from_utf8_lossy` で文字列化する。同じパスが重複しないようにする。
/// コンフリクトが無ければ空のベクタを返す。
pub fn get_conflicts(repo: &Repository) -> Result<Vec<ConflictFile>> {
    let index = repo.index()?;
    // conflicts() はコンフリクトが無いリポジトリでも空イテレータを返す。
    let conflicts = match index.conflicts() {
        Ok(c) => c,
        // index にコンフリクトの仕組みが無い等の場合は「競合なし」として扱う。
        Err(_) => return Ok(Vec::new()),
    };

    let mut out: Vec<ConflictFile> = Vec::new();
    for item in conflicts {
        let c = item?;
        // パスは our → their → ancestor の順に、最初に取れたものを使う。
        let path = c
            .our
            .as_ref()
            .or(c.their.as_ref())
            .or(c.ancestor.as_ref())
            .map(|e| String::from_utf8_lossy(&e.path).into_owned());
        let path = match path {
            Some(p) if !p.is_empty() => p,
            _ => continue,
        };
        // 同じパスが複数回出ても 1 件にまとめる。
        if out.iter().any(|f| f.path == path) {
            continue;
        }
        out.push(ConflictFile {
            has_ancestor: c.ancestor.is_some(),
            path,
        });
    }

    Ok(out)
}

/// リポジトリ直下の `.gitignore` の内容を返す。ファイルが無ければ `None`。
///
/// `.gitignore` は「Git に無視させたいファイルのパターン」を 1 行ずつ書くテキスト
/// ファイル。GUI で現在の内容を確認できるようにするための読み取り専用関数。
/// `.gitignore` が無い（`None`）のと、空ファイル（`Some("")`）は区別して返す。
pub fn read_gitignore(repo: &Repository) -> Result<Option<String>> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| CoreError::Git("作業ツリーがありません。".to_string()))?;
    let path = workdir.join(".gitignore");
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(Some(content)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CoreError::Git(format!(
            ".gitignore を読み込めませんでした: {e}"
        ))),
    }
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
    fn file_log_returns_only_commits_that_touched_the_path() {
        let fx = TestRepo::new();

        // c1: a.txt を作成。
        fx.write_file("a.txt", "a1");
        fx.stage_all();
        fx.commit("c1: add a");

        // c2: b.txt だけを変更（a.txt は触らない）。
        fx.write_file("b.txt", "b1");
        fx.stage_all();
        fx.commit("c2: add b");

        // c3: a.txt を変更。
        fx.write_file("a.txt", "a2");
        fx.stage_all();
        fx.commit("c3: change a");

        // c4: b.txt を変更。
        fx.write_file("b.txt", "b2");
        fx.stage_all();
        fx.commit("c4: change b");

        let repo = fx.open();
        // a.txt を触ったのは c1（作成）と c3（変更）だけ。b 系は含まれない。
        // テストではコミット時刻が同秒になりうるため、特定の並び順は仮定せず
        // 「対象コミットだけが含まれる」という集合の性質で検証する（log_paged テストと同じ方針）。
        let a_log = file_log(&repo, "a.txt", 100).unwrap();
        let mut a_summaries: Vec<&str> = a_log.iter().map(|c| c.summary.as_str()).collect();
        a_summaries.sort_unstable();
        assert_eq!(a_summaries, vec!["c1: add a", "c3: change a"]);

        let b_log = file_log(&repo, "b.txt", 100).unwrap();
        let mut b_summaries: Vec<&str> = b_log.iter().map(|c| c.summary.as_str()).collect();
        b_summaries.sort_unstable();
        assert_eq!(b_summaries, vec!["c2: add b", "c4: change b"]);
    }

    #[test]
    fn file_log_respects_max_limit() {
        let fx = TestRepo::new();
        for i in 0..5 {
            fx.write_file("a.txt", &format!("v{i}"));
            fx.stage_all();
            fx.commit(&format!("c{i}"));
        }

        let repo = fx.open();
        // 5 コミットすべてが a.txt を変更しているが、max=2 で 2 件に制限される。
        let log = file_log(&repo, "a.txt", 2).unwrap();
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn file_log_empty_repo_returns_empty() {
        let fx = TestRepo::new();
        let repo = fx.open();
        assert!(file_log(&repo, "a.txt", 10).unwrap().is_empty());
    }

    #[test]
    fn log_filtered_empty_matches_log_paged() {
        // 条件なしのフィルタは log_paged と完全に同じ結果（後方互換）。
        let fx = TestRepo::new();
        for i in 0..4 {
            fx.write_file("a.txt", &format!("v{i}"));
            fx.stage_all();
            fx.commit(&format!("c{i}"));
        }

        let repo = fx.open();
        let paged: Vec<String> = log_paged(&repo, 0, 100)
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect();
        let filtered: Vec<String> = log_filtered(&repo, 0, 100, &LogFilter::default())
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(paged, filtered);

        // ページング（skip/max）も条件なしなら一致する。
        let paged_page: Vec<String> = log_paged(&repo, 1, 2)
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect();
        let filtered_page: Vec<String> = log_filtered(&repo, 1, 2, &LogFilter::default())
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(paged_page, filtered_page);
    }

    #[test]
    fn log_filtered_by_message_substring() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("ログイン画面を追加");
        fx.write_file("a.txt", "2");
        fx.stage_all();
        fx.commit("バグを修正");
        fx.write_file("a.txt", "3");
        fx.stage_all();
        fx.commit("ログアウト処理を追加");

        let repo = fx.open();
        // 「ログ」を含む 2 件だけが残る。
        let filter = LogFilter {
            message: Some("ログ".to_string()),
            ..Default::default()
        };
        let got = log_filtered(&repo, 0, 100, &filter).unwrap();
        assert_eq!(got.len(), 2);
        assert!(got.iter().all(|c| c.summary.contains("ログ")));

        // 大文字小文字を無視して英字も部分一致する。
        let filter = LogFilter {
            message: Some("BUG".to_string()),
            ..Default::default()
        };
        // 日本語の件名には "BUG" は無いので 0 件。
        assert!(log_filtered(&repo, 0, 100, &filter).unwrap().is_empty());
    }

    #[test]
    fn log_filtered_by_author() {
        use crate::ops::{commit, stage_all};

        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1"); // 既定の author（TestRepo の identity）

        // author を切り替えて 2 件目を積む。
        let repo = fx.open();
        {
            let mut cfg = repo.config().unwrap();
            cfg.set_str("user.name", "Alice Example").unwrap();
            cfg.set_str("user.email", "alice@example.com").unwrap();
        }
        std::fs::write(fx.path().join("a.txt"), "2").unwrap();
        stage_all(&repo).unwrap();
        commit(&repo, "c2").unwrap();

        let repo = fx.open();
        // 名前の一部「alice」で 1 件に絞れる（大文字小文字無視）。
        let filter = LogFilter {
            author: Some("ALICE".to_string()),
            ..Default::default()
        };
        let got = log_filtered(&repo, 0, 100, &filter).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].author_name, "Alice Example");

        // メールアドレスの一部でも絞れる。
        let filter = LogFilter {
            author: Some("alice@example".to_string()),
            ..Default::default()
        };
        let got = log_filtered(&repo, 0, 100, &filter).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].author_email, "alice@example.com");
    }

    /// upstream を用意してローカルにクローンし、(一時ディレクトリ, クローン先パス) を返す。
    /// クローン直後は HEAD == origin/main。identity を設定してローカルコミットを作れるようにする。
    fn clone_with_upstream(upstream: &TestRepo) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let dest = dir.path().join("clone");
        let cloned = git2::Repository::clone(upstream.path().to_str().unwrap(), &dest).unwrap();
        let mut cfg = cloned.config().unwrap();
        cfg.set_str("user.name", "Clone User").unwrap();
        cfg.set_str("user.email", "clone@example.com").unwrap();
        (dir, dest)
    }

    #[test]
    fn head_is_published_no_upstream_returns_false() {
        // ケース A: 上流が設定されていないローカルブランチは判断できないので false（未公開扱い）。
        // ここで誤って true を返すと amend に Destructive が付かず、警告なしで危険操作を許してしまう。
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        assert!(!head_is_published(&fx.open()).unwrap());
    }

    #[test]
    fn head_is_published_at_upstream_returns_true() {
        // 上流と同一（先行も後退もしていない）→ 公開済みとみなす。
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "1");
        upstream.stage_all();
        upstream.commit("c1");

        let (_keep, dest) = clone_with_upstream(&upstream);
        let repo = git2::Repository::open(&dest).unwrap();
        assert!(head_is_published(&repo).unwrap());
    }

    #[test]
    fn head_is_published_ahead_of_upstream_returns_false() {
        use crate::ops::{commit, stage_all};

        // クローン後にローカルだけコミットを積むと上流より先行する → まだ未公開（amend は安全）。
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "1");
        upstream.stage_all();
        upstream.commit("c1");

        let (_keep, dest) = clone_with_upstream(&upstream);
        let repo = git2::Repository::open(&dest).unwrap();
        std::fs::write(dest.join("a.txt"), "2").unwrap();
        stage_all(&repo).unwrap();
        commit(&repo, "local-c2").unwrap();

        let repo = git2::Repository::open(&dest).unwrap();
        assert!(!head_is_published(&repo).unwrap());
    }

    #[test]
    fn head_is_published_behind_upstream_returns_true() {
        use crate::ops::fetch;

        // ケース C: 上流が先に進み、ローカルが後退している（ahead=0, behind>0）状態。
        // ローカルの全コミットは上流に含まれる＝公開済みなので true が正しい（安全側）。
        let upstream = TestRepo::new();
        upstream.write_file("a.txt", "1");
        upstream.stage_all();
        upstream.commit("c1");

        let (_keep, dest) = clone_with_upstream(&upstream);

        // 上流を進めてから fetch する。origin/main だけが前進し、ローカル main は据え置きになる。
        upstream.write_file("a.txt", "2");
        upstream.stage_all();
        upstream.commit("c2");

        let repo = git2::Repository::open(&dest).unwrap();
        fetch(&repo, "origin").unwrap();

        let repo = git2::Repository::open(&dest).unwrap();
        assert!(head_is_published(&repo).unwrap());
    }

    #[test]
    fn head_is_published_detached_head_returns_false() {
        // detached HEAD（ブランチを指さない状態）はブランチ名が取れないので false。
        // amend を安全側（未公開扱い）とするための保守的な判断。
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        let oid = fx.commit("c1");

        let repo = fx.open();
        // HEAD を直接コミット OID に向けて detached 状態にする。
        repo.set_head_detached(oid).unwrap();

        let repo = fx.open();
        assert!(!head_is_published(&repo).unwrap());
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
    fn list_tags_returns_lightweight_and_annotated() {
        use crate::ops::create_tag;

        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        let head = fx.head_oid().to_string();

        let repo = fx.open();
        // 軽量タグと注釈付きタグを1つずつ作る。
        create_tag(&repo, "v1.0.0", None, None).unwrap();
        create_tag(&repo, "v1.1.0", None, Some("リリース 1.1.0")).unwrap();

        let tags = list_tags(&repo).unwrap();
        assert_eq!(tags.len(), 2);
        // 名前順に並ぶ。
        assert_eq!(tags[0].name, "v1.0.0");
        assert_eq!(tags[1].name, "v1.1.0");

        // 軽量タグはメッセージ無し・HEAD を指す。
        assert!(tags[0].message.is_none());
        assert_eq!(tags[0].target_id, head);
        assert_eq!(tags[0].target_short_id, &head[..7]);

        // 注釈付きタグはメッセージを持ち、対象は HEAD。
        assert_eq!(tags[1].message.as_deref(), Some("リリース 1.1.0"));
        assert_eq!(tags[1].target_id, head);
    }

    #[test]
    fn list_tags_empty_when_no_tags() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");
        let repo = fx.open();
        assert!(list_tags(&repo).unwrap().is_empty());
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

    #[test]
    fn diff_commits_shows_added_and_removed_lines() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "line1\nline2\nline3\n");
        fx.stage_all();
        fx.commit("c1");
        let from = fx.head_oid().to_string();

        fx.write_file("a.txt", "line1\nCHANGED\nline3\n");
        fx.stage_all();
        fx.commit("c2");
        let to = fx.head_oid().to_string();

        let repo = fx.open();
        let diffs = diff_commits(&repo, Some(&from), &to).unwrap();

        assert_eq!(diffs.len(), 1);
        let d = &diffs[0];
        assert_eq!(d.path, "a.txt");
        assert!(!d.is_binary);
        assert!(d
            .lines
            .iter()
            .any(|l| l.kind == DiffLineKind::Deletion && l.content == "line2"));
        assert!(d
            .lines
            .iter()
            .any(|l| l.kind == DiffLineKind::Addition && l.content == "CHANGED"));
    }

    #[test]
    fn diff_commits_none_from_uses_parent() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "v1\n");
        fx.stage_all();
        fx.commit("c1");

        fx.write_file("a.txt", "v2\n");
        fx.write_file("b.txt", "new file\n");
        fx.stage_all();
        fx.commit("c2");
        let to = fx.head_oid().to_string();

        let repo = fx.open();
        // from_oid=None なら c2 の第1親（c1）との比較になる。
        let diffs = diff_commits(&repo, None, &to).unwrap();

        // a.txt の変更と b.txt の新規追加の 2 ファイルが出る。
        assert_eq!(diffs.len(), 2);
        let a = diffs.iter().find(|d| d.path == "a.txt").unwrap();
        assert!(a
            .lines
            .iter()
            .any(|l| l.kind == DiffLineKind::Addition && l.content == "v2"));
        let b = diffs.iter().find(|d| d.path == "b.txt").unwrap();
        assert!(b
            .lines
            .iter()
            .any(|l| l.kind == DiffLineKind::Addition && l.content == "new file"));
    }

    #[test]
    fn diff_commits_first_commit_against_empty_tree() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "hello\nworld\n");
        fx.stage_all();
        fx.commit("c1");
        let to = fx.head_oid().to_string();

        let repo = fx.open();
        // 親が無い最初のコミットは空ツリーとの比較＝全行が追加になる。
        let diffs = diff_commits(&repo, None, &to).unwrap();
        assert_eq!(diffs.len(), 1);
        let adds: Vec<_> = diffs[0]
            .lines
            .iter()
            .filter(|l| l.kind == DiffLineKind::Addition)
            .collect();
        assert_eq!(adds.len(), 2);
    }

    #[test]
    fn diff_commits_invalid_oid_is_input_error() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "x\n");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        // 不正な形式の oid。
        let err = diff_commits(&repo, None, "not-a-valid-oid").unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput(_)));

        // 形式は正しいが存在しない oid。
        let missing = "0".repeat(40);
        let err = diff_commits(&repo, None, &missing).unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput(_)));

        // from 側が不正でもエラーになる。
        let to = fx.head_oid().to_string();
        let err = diff_commits(&repo, Some("zzzz"), &to).unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput(_)));
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

    #[test]
    fn blame_file_returns_multiple_hunks_for_two_commits() {
        let fx = TestRepo::new();
        // 1回目: 3行のファイルを作る。
        fx.write_file("a.txt", "line1\nline2\nline3\n");
        fx.stage_all();
        let c1 = fx.commit("c1");

        // 2回目: 真ん中の行だけを変更する。先頭・末尾は c1 のまま残る。
        fx.write_file("a.txt", "line1\nCHANGED\nline3\n");
        fx.stage_all();
        let c2 = fx.commit("c2");

        let repo = fx.open();
        let hunks = blame_file(&repo, "a.txt").unwrap();

        // 変更されていない行と変更された行で、別々の hunk に分かれる。
        assert!(
            hunks.len() >= 2,
            "複数の hunk が返るはず: {} 件",
            hunks.len()
        );

        // 全行ぶんがちょうど覆われている（行番号は1始まりで連続）。
        let total: usize = hunks.iter().map(|h| h.lines_count).sum();
        assert_eq!(total, 3);
        let mut next = 1usize;
        for h in &hunks {
            assert_eq!(h.lines_start, next);
            next += h.lines_count;
            // commit_id は妥当（c1 か c2 のいずれか）で、short_id はその先頭7桁。
            assert!(h.commit_id == c1.to_string() || h.commit_id == c2.to_string());
            assert_eq!(h.short_id, &h.commit_id[..7]);
            assert!(!h.message_short.is_empty());
            assert_eq!(h.author_name, "Test User");
        }

        // 真ん中の行（2行目）を覆う hunk は c2、それ以外の行は c1 が担当する。
        let owner = |lineno: usize| -> &str {
            &hunks
                .iter()
                .find(|h| lineno >= h.lines_start && lineno < h.lines_start + h.lines_count)
                .unwrap()
                .commit_id
        };
        assert_eq!(owner(1), c1.to_string());
        assert_eq!(owner(2), c2.to_string());
        assert_eq!(owner(3), c1.to_string());
    }

    #[test]
    fn blame_file_missing_path_errors() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "x\n");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        assert!(blame_file(&repo, "does-not-exist.txt").is_err());
    }

    #[test]
    fn blame_file_rejects_path_traversal() {
        let fx = TestRepo::new();
        let repo = fx.open();
        assert!(blame_file(&repo, "../secret.txt").is_err());
        assert!(blame_file(&repo, "/etc/passwd").is_err());
    }

    #[test]
    fn get_conflicts_lists_conflicted_path_with_ancestor() {
        let fx = repo_with_conflict();
        let repo = fx.open();
        let conflicts = get_conflicts(&repo).unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].path, "a.txt");
        // base からの 3-way マージなので共通祖先側のエントリがある。
        assert!(conflicts[0].has_ancestor);
    }

    #[test]
    fn get_conflicts_empty_when_no_conflict() {
        let fx = TestRepo::new();
        fx.write_file("a.txt", "1");
        fx.stage_all();
        fx.commit("c1");

        let repo = fx.open();
        // コンフリクトを作っていないリポジトリでは空のベクタが返る。
        assert!(get_conflicts(&repo).unwrap().is_empty());
    }

    #[test]
    fn read_gitignore_none_when_missing() {
        let fx = TestRepo::new();
        let repo = fx.open();
        // .gitignore が無いときは None（空ファイルとは区別する）。
        assert!(read_gitignore(&repo).unwrap().is_none());
    }

    #[test]
    fn read_gitignore_returns_contents() {
        let fx = TestRepo::new();
        fx.write_file(".gitignore", "target/\n*.log\n");
        let repo = fx.open();
        assert_eq!(
            read_gitignore(&repo).unwrap().as_deref(),
            Some("target/\n*.log\n")
        );
    }
}
