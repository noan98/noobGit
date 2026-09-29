//! 操作アクティビティログ（Issue #208）。
//!
//! 「今日このリポジトリに自分が何をしたか」を時系列で振り返れる、読み物としての
//! 操作履歴。取り消し用の [`crate::undo`] ジャーナルとは別物で、こちらは
//! 戻せない操作（push・破棄など）や失敗した操作、取り消し（undo）そのものも含めた
//! **全記録**を残す。トラブルのときに「何をしたの？」に答えられるようにするのが目的。
//!
//! - 保存先は `.git/noobgit_activity.json`。undo ジャーナルと同じく一時ファイルへ書いてから
//!   rename する原子的な書き込みで、[`MAX_ENTRIES`] 件を超えたら古いものから捨てる。
//! - 記録は**ベストエフォート**。[`record`] は書き込みに失敗しても何も返さず、Git 操作を
//!   失敗させない（操作はすでに終わっている）。
//! - 壊れたファイルは「履歴なし」として扱い、パニックしない。
//! - 記録するのは操作メタデータ（操作名・パス名・ブランチ名など）だけで、ファイルの
//!   内容や差分は含めない。

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use git2::Repository;
use serde::{Deserialize, Serialize};

use crate::error::{describe_io_error, CoreError, Result};
use crate::explain::explain;
use crate::safety::OperationKind;

/// 保持する最大件数。超えた分は古いものから捨てる（ローテーション）。
pub const MAX_ENTRIES: usize = 500;

/// 書き込み時のファイル形式バージョン。`{ "version": 1, "entries": [...] }`。
const CURRENT_VERSION: u64 = 1;

/// 操作の結果。TypeScript で扱いやすいよう `{ "status": "failed", "message": "..." }`
/// のタグ付き形式でシリアライズする。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "message", rename_all = "snake_case")]
pub enum ActivityOutcome {
    /// 成功した。
    Success,
    /// 失敗した。中身は日本語のエラーメッセージ。
    Failed(String),
    /// 取り消し（undo）で元に戻した。
    Undone,
}

/// アクティビティログの1エントリ。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityEntry {
    /// 記録した時刻（UNIX 秒）。
    pub timestamp: i64,
    /// どの操作か。取り消し（`Undone`）のときは「取り消した操作」の種別。
    pub op: OperationKind,
    /// 人が読む1行の説明（例: 「ステージ（コミット準備）: src/a.rs」）。
    pub summary: String,
    pub outcome: ActivityOutcome,
}

fn log_path(repo: &Repository) -> PathBuf {
    repo.path().join("noobgit_activity.json")
}

/// 操作名（`explain.rs` の見出し）に、対象の詳細（パス名・ブランチ名など）を添えた
/// 1 行の説明を作る。`detail` が空・空白のみなら操作名だけ。
pub fn summarize(op: OperationKind, detail: Option<&str>) -> String {
    let title = explain(op).title;
    match detail.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => format!("{title}: {d}"),
        None => title,
    }
}

/// 取り消し（undo）を記録するときの説明。`description` は undo エントリの説明文。
pub fn summarize_undo(op: OperationKind, description: &str) -> String {
    format!("取り消し（{}）: {}", explain(op).title, description.trim())
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// ファイルの中身を寛容にパースする。JSON 全体が壊れていれば空、個々のエントリが
/// 読めなければそのエントリだけスキップする。
fn parse(bytes: &[u8]) -> Vec<ActivityEntry> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        eprintln!("noobgit: 操作ログのファイルが壊れているため、記録なしとして扱います");
        return Vec::new();
    };
    let raw = match value {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(mut o) => match o.remove("entries") {
            Some(serde_json::Value::Array(a)) => a,
            _ => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    raw.into_iter()
        .filter_map(|v| serde_json::from_value::<ActivityEntry>(v).ok())
        .collect()
}

fn load(repo: &Repository) -> Result<Vec<ActivityEntry>> {
    match fs::read(log_path(repo)) {
        Ok(bytes) => Ok(parse(&bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(CoreError::Git(format!(
            "操作ログの読み取りに失敗しました: {}",
            describe_io_error(&e)
        ))),
    }
}

#[derive(Serialize)]
struct LogFile<'a> {
    version: u64,
    entries: &'a [ActivityEntry],
}

fn save(repo: &Repository, entries: &[ActivityEntry]) -> Result<()> {
    let path = log_path(repo);
    let bytes = serde_json::to_vec_pretty(&LogFile {
        version: CURRENT_VERSION,
        entries,
    })
    .map_err(|e| CoreError::Git(format!("操作ログの保存に失敗しました: {e}")))?;
    let fail = |e: std::io::Error| {
        CoreError::Git(format!(
            "操作ログの保存に失敗しました: {}",
            describe_io_error(&e)
        ))
    };
    // 一時ファイルへ書いてから rename し、書き込み途中の中断でログが壊れるのを防ぐ。
    let tmp = path.with_file_name("noobgit_activity.json.tmp");
    fs::write(&tmp, bytes).map_err(fail)?;
    fs::rename(&tmp, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        fail(e)
    })
}

/// エントリを追加して保存する（失敗は `Err` で返す。テストと内部用）。
/// 上限 `max` を超えた分は古いものから捨てる。
fn try_append(repo: &Repository, entry: ActivityEntry, max: usize) -> Result<()> {
    let mut entries = load(repo)?;
    entries.push(entry);
    if entries.len() > max {
        let excess = entries.len() - max;
        entries.drain(..excess);
    }
    save(repo, &entries)
}

/// 操作の結果を記録する（ベストエフォート。書き込みに失敗しても何も起きない）。
///
/// Git 操作そのものはすでに終わっているので、ログの失敗でその結果を覆してはならない。
pub fn record(repo: &Repository, op: OperationKind, summary: String, outcome: ActivityOutcome) {
    let entry = ActivityEntry {
        timestamp: now_secs(),
        op,
        summary,
        outcome,
    };
    let _ = try_append(repo, entry, MAX_ENTRIES);
}

/// 全エントリを古い順で返す。
pub fn list(repo: &Repository) -> Result<Vec<ActivityEntry>> {
    load(repo)
}

/// ログを空にする。
pub fn clear(repo: &Repository) -> Result<()> {
    match fs::remove_file(log_path(repo)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(CoreError::Git(format!(
            "操作ログの削除に失敗しました: {}",
            describe_io_error(&e)
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestRepo;

    fn entry(n: i64) -> ActivityEntry {
        ActivityEntry {
            timestamp: n,
            op: OperationKind::Stage,
            summary: format!("操作{n}"),
            outcome: ActivityOutcome::Success,
        }
    }

    #[test]
    fn record_and_list_in_chronological_order() {
        let fx = TestRepo::new();
        let repo = fx.open();
        record(
            &repo,
            OperationKind::Stage,
            summarize(OperationKind::Stage, Some("a.txt")),
            ActivityOutcome::Success,
        );
        record(
            &repo,
            OperationKind::Push,
            summarize(OperationKind::Push, None),
            ActivityOutcome::Failed("接続できません".into()),
        );
        record(
            &repo,
            OperationKind::Commit,
            summarize_undo(OperationKind::Commit, "コミットを取り消す"),
            ActivityOutcome::Undone,
        );
        let entries = list(&repo).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].op, OperationKind::Stage);
        assert!(entries[0].summary.contains("a.txt"));
        assert_eq!(
            entries[1].outcome,
            ActivityOutcome::Failed("接続できません".into())
        );
        assert_eq!(entries[2].outcome, ActivityOutcome::Undone);
        assert!(entries[2].summary.starts_with("取り消し（"));
        assert!(entries[0].timestamp > 0);
    }

    #[test]
    fn summarize_skips_blank_detail() {
        let plain = summarize(OperationKind::Commit, None);
        assert_eq!(summarize(OperationKind::Commit, Some("  ")), plain);
        assert!(summarize(OperationKind::Commit, Some("x")).ends_with(": x"));
    }

    #[test]
    fn rotation_keeps_only_newest_entries() {
        let fx = TestRepo::new();
        let repo = fx.open();
        for n in 1..=7 {
            try_append(&repo, entry(n), 5).unwrap();
        }
        let entries = list(&repo).unwrap();
        assert_eq!(entries.len(), 5);
        assert_eq!(entries.first().unwrap().timestamp, 3);
        assert_eq!(entries.last().unwrap().timestamp, 7);
    }

    #[test]
    fn record_never_exceeds_max_entries() {
        let fx = TestRepo::new();
        let repo = fx.open();
        let seeded: Vec<ActivityEntry> = (0..MAX_ENTRIES as i64).map(entry).collect();
        save(&repo, &seeded).unwrap();
        record(
            &repo,
            OperationKind::Commit,
            "最新".into(),
            ActivityOutcome::Success,
        );
        let entries = list(&repo).unwrap();
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(entries.last().unwrap().summary, "最新");
        assert_eq!(entries.first().unwrap().timestamp, 1);
    }

    #[test]
    fn write_failure_does_not_panic_or_propagate() {
        let fx = TestRepo::new();
        let repo = fx.open();
        // ログのパスをディレクトリにして、書き込み（rename）を不能にする。
        fs::create_dir(repo.path().join("noobgit_activity.json")).unwrap();
        // 保存自体は失敗する…
        assert!(try_append(&repo, entry(1), 5).is_err());
        // …が、record は何も返さず・パニックもしない（Git 操作を失敗させない）。
        record(
            &repo,
            OperationKind::Stage,
            "x".into(),
            ActivityOutcome::Success,
        );
    }

    #[test]
    fn broken_json_is_treated_as_empty() {
        let fx = TestRepo::new();
        let repo = fx.open();
        fs::write(repo.path().join("noobgit_activity.json"), b"{ not json").unwrap();
        assert!(list(&repo).unwrap().is_empty());
        // 壊れていても、次の記録で正常なファイルに置き換わる。
        record(
            &repo,
            OperationKind::Stage,
            "y".into(),
            ActivityOutcome::Success,
        );
        assert_eq!(list(&repo).unwrap().len(), 1);
    }

    #[test]
    fn unreadable_entries_are_skipped() {
        let fx = TestRepo::new();
        let repo = fx.open();
        let json = r#"{"version":1,"entries":[
            {"timestamp":1,"op":"stage","summary":"ok","outcome":{"status":"success"}},
            {"timestamp":2,"op":"unknown_op","summary":"bad","outcome":{"status":"success"}},
            {"timestamp":3,"op":"commit","summary":"ok2","outcome":{"status":"failed","message":"m"}}
        ]}"#;
        fs::write(repo.path().join("noobgit_activity.json"), json).unwrap();
        let entries = list(&repo).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].outcome, ActivityOutcome::Failed("m".into()));
    }

    #[test]
    fn outcome_serializes_as_tagged_object() {
        let s = serde_json::to_string(&ActivityOutcome::Failed("e".into())).unwrap();
        assert_eq!(s, r#"{"status":"failed","message":"e"}"#);
        assert_eq!(
            serde_json::to_string(&ActivityOutcome::Success).unwrap(),
            r#"{"status":"success"}"#
        );
        assert_eq!(
            serde_json::to_string(&ActivityOutcome::Undone).unwrap(),
            r#"{"status":"undone"}"#
        );
    }

    #[test]
    fn clear_removes_entries_and_is_idempotent() {
        let fx = TestRepo::new();
        let repo = fx.open();
        record(
            &repo,
            OperationKind::Stage,
            "a".into(),
            ActivityOutcome::Success,
        );
        clear(&repo).unwrap();
        assert!(list(&repo).unwrap().is_empty());
        clear(&repo).unwrap();
    }
}
