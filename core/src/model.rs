use serde::{Deserialize, Serialize};

/// 作業ツリーの1ファイルの変更種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    TypeChange,
    Untracked,
    Conflicted,
}

impl ChangeKind {
    /// 初学者向けの日本語ラベル。
    pub fn label_ja(self) -> &'static str {
        match self {
            ChangeKind::Added => "追加",
            ChangeKind::Modified => "変更",
            ChangeKind::Deleted => "削除",
            ChangeKind::Renamed => "リネーム",
            ChangeKind::TypeChange => "種別変更",
            ChangeKind::Untracked => "未追跡",
            ChangeKind::Conflicted => "コンフリクト",
        }
    }
}

/// 1ファイルの変更。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
}

/// リポジトリの現在状態（status相当）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoStatus {
    /// 現在のブランチ名。detached HEAD等で取得できない場合は None。
    pub branch: Option<String>,
    /// ステージ済み（コミット予定）の変更。
    pub staged: Vec<FileChange>,
    /// 未ステージの変更（追跡中ファイルの変更）。
    pub unstaged: Vec<FileChange>,
    /// 未追跡ファイル。
    pub untracked: Vec<String>,
    /// コンフリクト中のファイル。
    pub conflicted: Vec<String>,
    /// 変更が何も無いか。
    pub is_clean: bool,
}

/// ブランチ1件の情報。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    /// 現在チェックアウト中のブランチか。
    pub is_head: bool,
    /// リモート追跡ブランチか。
    pub is_remote: bool,
    /// 上流ブランチ名（あれば）。
    pub upstream: Option<String>,
    /// 保護対象（main/master等）として扱われるか。
    pub is_protected: bool,
}

/// コミット1件の情報。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitInfo {
    pub id: String,
    pub short_id: String,
    pub summary: String,
    pub author_name: String,
    pub author_email: String,
    /// コミット日時（Unixエポック秒）。
    pub time: i64,
}
