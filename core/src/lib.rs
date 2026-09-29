//! noobGit コアロジック。
//!
//! 「ジュニアエンジニアが安心して使えるGitツール」のために、Git操作を
//! - 状態の可視化（[`repo`]）
//! - 安全な書き込み操作（[`ops`]）
//! - バグ混入コミットの二分探索（[`bisect`]）
//! - 初回セットアップ（identity の確認・設定）（[`identity`]）
//! - 操作のリスク判定（[`safety`]）
//! - 平易な日本語説明（[`explain`]）
//! - 取り消し / Undo（[`undo`]）
//!
//! という関心ごとに分けて提供する。GUI（Tauri）層はこのクレートを呼ぶだけにする。

pub mod bisect;
pub mod error;
pub mod explain;
pub mod identity;
pub mod model;
pub mod ops;
pub mod repo;
pub mod safety;
pub mod undo;

#[cfg(test)]
mod adversarial_tests;
#[cfg(test)]
mod test_support;

pub use bisect::{bisect_mark, bisect_reset, bisect_start, bisect_status};
pub use error::{classify_network_error, CoreError, ErrorKind, NetworkErrorKind, Result};
pub use explain::{explain, Explanation};
pub use identity::{get_identity, set_identity, Identity, IdentityScope};
pub use model::{
    BisectStatus, BranchInfo, ChangeKind, CloneOutcome, CommitInfo, DiffLine, DiffLineKind,
    FetchOutcome, FileChange, FileDiff, LfsCandidate, PullOutcome, ReflogEntry, RemoteInfo,
    RepoStatus, SensitiveWarning, StashInfo,
};
pub use safety::{assess, OperationKind, RiskAssessment, RiskLevel, SafetyContext};
pub use undo::{
    can_undo, peek, undo_last, undo_last_confirmed, UndoAction, UndoApplicability, UndoEntry,
};
