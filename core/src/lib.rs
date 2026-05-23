//! noobGit コアロジック。
//!
//! 「ジュニアエンジニアが安心して使えるGitツール」のために、Git操作を
//! - 状態の可視化（[`repo`]）
//! - 安全な書き込み操作（[`ops`]）
//! - 操作のリスク判定（[`safety`]）
//! - 平易な日本語説明（[`explain`]）
//! - 取り消し / Undo（[`undo`]）
//!
//! という関心ごとに分けて提供する。GUI（Tauri）層はこのクレートを呼ぶだけにする。

pub mod error;
pub mod explain;
pub mod model;
pub mod ops;
pub mod repo;
pub mod safety;
pub mod undo;

#[cfg(test)]
mod test_support;

pub use error::{CoreError, ErrorKind, Result};
pub use explain::{explain, Explanation};
pub use model::{BranchInfo, ChangeKind, CommitInfo, FileChange, RepoStatus};
pub use safety::{assess, OperationKind, RiskAssessment, RiskLevel, SafetyContext};
pub use undo::{can_undo, peek, undo_last, UndoAction, UndoEntry};
