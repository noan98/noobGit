//! ファイルシステム監視（#199）。
//!
//! エディタでの保存や、別ツール（VS Code の Git、ターミナルの git）による変更を
//! 検知して、フロントエンドへ Tauri イベント `repo-changed` を送る。古い status を
//! 見たまま discard / commit してしまう事故を防ぐための安全機能。
//!
//! ここは「監視のインフラ」であり Git のロジックは持たない。どのパスが画面に影響
//! するかの判定のうち、Git に依存しない部分（除外ディレクトリなど）だけを純粋関数
//! として置き、`.gitignore` 済みかどうかの判定は `noobgit-core` の
//! `repo::is_path_ignored` に任せる。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use noobgit_core::repo;

/// フロントへ送るイベント名。
pub const REPO_CHANGED_EVENT: &str = "repo-changed";

/// 変更をまとめる時間。保存 1 回で複数のイベントが連続して届くため、静かになって
/// からこの時間後に 1 回だけ通知する。
const DEBOUNCE: Duration = Duration::from_millis(300);

/// noobGit 自身の書き込みが終わってから、この時間内に届いた通知は自己イベントと
/// みなして捨てる。デバウンス（300ms、内部の間隔でさらに最大約 100ms 遅れる）と
/// OS の通知遅延より長く取る。
const SELF_EVENT_WINDOW: Duration = Duration::from_millis(1000);

/// 1 回の通知に含まれるパスがこの数を超えたら、`.gitignore` の判定を省いて
/// 「変更あり」とみなす（ブランチ切替などの大量変更で判定コストをかけない）。
const IGNORE_CHECK_LIMIT: usize = 200;

/// `.git` の中で、画面（状態・ブランチ・履歴）に影響するトップレベルの名前。
/// `objects/` `logs/` `noobgit_*.json`（undo ジャーナル・bisect）などの高頻度・
/// 無関係なものは含めない。`refs/` 配下は別途すべて対象にする。
const GIT_DIR_WATCHED_NAMES: &[&str] = &[
    "HEAD",
    "index",
    "packed-refs",
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REBASE_HEAD",
    "ORIG_HEAD",
];

// --- 自己イベントの抑制 ---------------------------------------------------

/// 実行中の書き込みコマンド数。
static ACTIVE_WRITES: AtomicUsize = AtomicUsize::new(0);
/// 直近の書き込み完了時刻（プロセス起動からのミリ秒。+1 して 0 を「まだ無い」に使う）。
static LAST_WRITE_END_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

/// 書き込みコマンドの開始を記録する。
pub fn write_started() {
    ACTIVE_WRITES.fetch_add(1, Ordering::SeqCst);
}

/// 書き込みコマンドの終了を記録する。
pub fn write_finished() {
    LAST_WRITE_END_MS.store(now_ms(), Ordering::SeqCst);
    ACTIVE_WRITES.fetch_sub(1, Ordering::SeqCst);
}

/// 通知を捨てるべきか（noobGit 自身の操作による変更か）を判定する純粋関数。
///
/// 書き込み中、または直近の書き込み完了から `window_ms` 以内なら自己イベント。
/// この間に起きた外部の変更も一緒に捨てられるが、書き込み直後に画面は必ず
/// 再読み込みされるので、その時点までの外部変更は反映される。
pub fn is_self_event(active_writes: usize, now_ms: u64, last_end_ms: u64, window_ms: u64) -> bool {
    active_writes > 0 || (last_end_ms != 0 && now_ms.saturating_sub(last_end_ms) < window_ms)
}

fn suppress_now() -> bool {
    is_self_event(
        ACTIVE_WRITES.load(Ordering::SeqCst),
        now_ms(),
        LAST_WRITE_END_MS.load(Ordering::SeqCst),
        SELF_EVENT_WINDOW.as_millis() as u64,
    )
}

// --- パスの判定（純粋関数） -----------------------------------------------

/// 変更されたパスが、画面の再読み込みに値するか（`.gitignore` 判定より前の段階）。
///
/// - `git_dir` 配下は許可リスト方式: `HEAD` / `index` / `packed-refs` / マージ系の
///   目印 / `refs/**` だけ。`*.lock` と `*.tmp`（書き込み途中の一時ファイル）、
///   `noobgit_*`（noobGit 自身のジャーナル）は除く。
/// - 作業ツリーでは、直下の `target/` と、どこにあっても `node_modules/` の中を
///   除く（高頻度で、追跡対象になることがほぼ無い）。
/// - どちらの配下でもないパスは無関係。
pub fn is_relevant_path(root: &Path, git_dir: &Path, path: &Path) -> bool {
    if let Ok(rel) = path.strip_prefix(git_dir) {
        return is_relevant_git_dir_entry(rel);
    }
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    let mut comps = rel.components().map(|c| c.as_os_str());
    let Some(first) = comps.next() else {
        // リポジトリルート自体の変更（メタデータ更新など）は無視する。
        return false;
    };
    if first == ".git" || first == "target" {
        return false;
    }
    if rel.components().any(|c| c.as_os_str() == "node_modules") {
        return false;
    }
    true
}

fn is_relevant_git_dir_entry(rel: &Path) -> bool {
    let mut comps = rel.components();
    let Some(first) = comps.next() else {
        return false;
    };
    let first = first.as_os_str().to_string_lossy();
    let last = rel
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if last.ends_with(".lock") || last.ends_with(".tmp") || last.starts_with("noobgit_") {
        return false;
    }
    if first == "refs" {
        // `refs` ディレクトリ自体ではなく、その配下だけを対象にする。
        return rel.components().count() > 1;
    }
    rel.components().count() == 1 && GIT_DIR_WATCHED_NAMES.contains(&first.as_ref())
}

// --- 監視の管理 -----------------------------------------------------------

type RepoDebouncer = Debouncer<RecommendedWatcher>;

/// タブ（リポジトリパス）ごとの監視ハンドル。ハンドルを drop すると監視が止まる。
#[derive(Default)]
pub struct WatcherRegistry {
    watchers: Mutex<HashMap<String, RepoDebouncer>>,
}

#[derive(Clone, Serialize)]
struct RepoChangedPayload {
    repo_path: String,
}

impl WatcherRegistry {
    /// `repo_path` の監視を開始する。すでに監視中なら作り直す。
    pub fn watch(&self, app: AppHandle, repo_path: String) -> Result<(), String> {
        let r = repo::open(&repo_path).map_err(|e| e.to_string())?;
        let root = r
            .workdir()
            .ok_or_else(|| "ベアリポジトリは監視できません。".to_string())?
            .canonicalize()
            .map_err(|e| format!("リポジトリの場所を確認できませんでした: {e}"))?;
        let git_dir = r
            .path()
            .canonicalize()
            .map_err(|e| format!("リポジトリの場所を確認できませんでした: {e}"))?;
        drop(r);

        let key = repo_path.clone();
        let (cb_root, cb_git_dir) = (root.clone(), git_dir.clone());
        let mut debouncer = new_debouncer(DEBOUNCE, move |res: DebounceEventResult| {
            let Ok(events) = res else { return };
            if suppress_now() {
                return;
            }
            let paths: Vec<PathBuf> = events.into_iter().map(|e| e.path).collect();
            if batch_is_relevant(&repo_path, &cb_root, &cb_git_dir, &paths) {
                let _ = app.emit(
                    REPO_CHANGED_EVENT,
                    RepoChangedPayload {
                        repo_path: repo_path.clone(),
                    },
                );
            }
        })
        .map_err(|e| format!("ファイル監視を開始できませんでした: {e}"))?;

        // 作業ツリー全体を再帰監視する（.git も含まれる。不要なパスは通知側で除外）。
        // Windows / macOS の再帰監視は OS 側の仕組みでコストが小さいため、除外は
        // 通知の段階で行う。
        debouncer
            .watcher()
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|e| format!("ファイル監視を開始できませんでした: {e}"))?;
        // .git が作業ツリーの外にある場合（worktree など）は別に監視する。
        if !git_dir.starts_with(&root) {
            debouncer
                .watcher()
                .watch(&git_dir, RecursiveMode::Recursive)
                .map_err(|e| format!("ファイル監視を開始できませんでした: {e}"))?;
        }

        self.watchers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, debouncer);
        Ok(())
    }

    /// `repo_path` の監視を止める。監視していなければ何もしない。
    pub fn unwatch(&self, repo_path: &str) {
        self.watchers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(repo_path);
    }
}

/// 1 回分の通知に、画面に影響するパスが 1 つでも含まれるか。
///
/// `.gitignore` 済みのファイル（ビルド成果物など）の変更は status に現れないので、
/// 通知しない。ただし `.git` 配下のパスは無視判定の対象外。
fn batch_is_relevant(repo_path: &str, root: &Path, git_dir: &Path, paths: &[PathBuf]) -> bool {
    let candidates: Vec<&PathBuf> = paths
        .iter()
        .filter(|p| is_relevant_path(root, git_dir, p))
        .collect();
    if candidates.is_empty() {
        return false;
    }
    if candidates.len() > IGNORE_CHECK_LIMIT {
        return true;
    }
    // .git 配下が 1 つでもあれば無視判定なしで通知する。
    let Some(worktree_paths) = candidates
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .ok()
                .filter(|_| !p.starts_with(git_dir))
        })
        .collect::<Option<Vec<&Path>>>()
    else {
        return true;
    };
    // 判定のためにリポジトリを開けなければ、安全側（通知する）に倒す。
    let Ok(r) = repo::open(repo_path) else {
        return true;
    };
    worktree_paths
        .iter()
        .any(|rel| !repo::is_path_ignored(&r, rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from("/work/proj")
    }
    fn git() -> PathBuf {
        PathBuf::from("/work/proj/.git")
    }
    fn rel(p: &str) -> bool {
        is_relevant_path(&root(), &git(), &Path::new("/work/proj").join(p))
    }

    #[test]
    fn worktree_files_are_relevant() {
        assert!(rel("src/main.rs"));
        assert!(rel("README.md"));
        assert!(rel("target_notes.txt"));
        assert!(rel("src/target/x.rs"));
    }

    #[test]
    fn excluded_directories_are_ignored() {
        assert!(!rel("target"));
        assert!(!rel("target/debug/app"));
        assert!(!rel("node_modules/react/index.js"));
        assert!(!rel("web/node_modules/react/index.js"));
    }

    #[test]
    fn git_dir_uses_allow_list() {
        assert!(rel(".git/HEAD"));
        assert!(rel(".git/index"));
        assert!(rel(".git/packed-refs"));
        assert!(rel(".git/MERGE_HEAD"));
        assert!(rel(".git/refs/heads/main"));
        assert!(rel(".git/refs/stash"));
        assert!(!rel(".git/refs"));
        assert!(!rel(".git"));
        assert!(!rel(".git/objects/ab/cdef"));
        assert!(!rel(".git/logs/HEAD"));
        assert!(!rel(".git/config"));
        assert!(!rel(".git/index.lock"));
        assert!(!rel(".git/HEAD.lock"));
        assert!(!rel(".git/refs/heads/main.lock"));
        assert!(!rel(".git/noobgit_undo.json"));
        assert!(!rel(".git/noobgit_undo.json.tmp"));
        assert!(!rel(".git/noobgit_bisect.json"));
        assert!(!rel(".git/sub/HEAD"));
    }

    #[test]
    fn unrelated_and_root_paths_are_ignored() {
        assert!(!is_relevant_path(
            &root(),
            &git(),
            Path::new("/elsewhere/a.txt")
        ));
        assert!(!is_relevant_path(&root(), &git(), &root()));
    }

    #[test]
    fn external_git_dir_is_supported() {
        let git = PathBuf::from("/repos/main/.git/worktrees/w1");
        let root = PathBuf::from("/work/w1");
        assert!(is_relevant_path(&root, &git, &git.join("HEAD")));
        assert!(!is_relevant_path(&root, &git, &git.join("logs/HEAD")));
        assert!(is_relevant_path(&root, &git, &root.join("a.txt")));
    }

    #[test]
    fn self_event_decision() {
        // 書き込み中は常に自己イベント。
        assert!(is_self_event(1, 5000, 0, 1000));
        // 一度も書き込んでいなければ自己イベントではない。
        assert!(!is_self_event(0, 5000, 0, 1000));
        // 完了直後は自己イベント、十分たてば外部の変更。
        assert!(is_self_event(0, 5500, 5000, 1000));
        assert!(!is_self_event(0, 6000, 5000, 1000));
        assert!(!is_self_event(0, 9000, 5000, 1000));
    }

    #[test]
    fn batch_skips_gitignored_files() {
        // core の TestRepo は core 内のテスト専用なので、一時ディレクトリに直接作る。
        let dir = std::env::temp_dir().join(format!(
            "noobgit_watcher_test_{}_{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        git2::Repository::init(&dir).unwrap();
        std::fs::write(dir.join(".gitignore"), "*.log\n").unwrap();
        std::fs::write(dir.join("a.txt"), "1").unwrap();
        std::fs::write(dir.join("debug.log"), "x").unwrap();
        let p = dir.to_str().unwrap().to_string();
        let root = dir.canonicalize().unwrap();
        let git = root.join(".git");
        assert!(!batch_is_relevant(
            &p,
            &root,
            &git,
            &[root.join("debug.log")]
        ));
        assert!(batch_is_relevant(
            &p,
            &root,
            &git,
            &[root.join("debug.log"), root.join("a.txt")]
        ));
        assert!(batch_is_relevant(&p, &root, &git, &[git.join("HEAD")]));
        assert!(!batch_is_relevant(
            &p,
            &root,
            &git,
            &[git.join("objects/x")]
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
