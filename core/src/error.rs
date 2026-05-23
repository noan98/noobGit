use serde::Serialize;
use thiserror::Error;

/// noobGit コア全体で使うエラー型。
///
/// メッセージはすべて日本語で、初学者にも何が起きたか分かる文言にする。
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("Gitリポジトリを開けませんでした: {0}")]
    OpenRepo(String),

    #[error("Git操作に失敗しました: {0}")]
    Git(String),

    #[error("この操作は安全のためブロックされました: {0}")]
    Blocked(String),

    #[error("取り消せる操作がありません: {0}")]
    NothingToUndo(String),

    #[error("入力が正しくありません: {0}")]
    InvalidInput(String),
}

impl From<git2::Error> for CoreError {
    fn from(e: git2::Error) -> Self {
        CoreError::Git(e.message().to_string())
    }
}

/// フロントエンド(Tauri)へ返しやすいよう、`Result<T, String>` に変換するヘルパ。
impl From<CoreError> for String {
    fn from(e: CoreError) -> Self {
        e.to_string()
    }
}

/// シリアライズ可能なエラー表現。フロントでカテゴリ別に扱いたい場合に使う。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "message")]
pub enum ErrorKind {
    OpenRepo(String),
    Git(String),
    Blocked(String),
    NothingToUndo(String),
    InvalidInput(String),
}

impl From<&CoreError> for ErrorKind {
    fn from(e: &CoreError) -> Self {
        match e {
            CoreError::OpenRepo(m) => ErrorKind::OpenRepo(m.clone()),
            CoreError::Git(m) => ErrorKind::Git(m.clone()),
            CoreError::Blocked(m) => ErrorKind::Blocked(m.clone()),
            CoreError::NothingToUndo(m) => ErrorKind::NothingToUndo(m.clone()),
            CoreError::InvalidInput(m) => ErrorKind::InvalidInput(m.clone()),
        }
    }
}

pub type Result<T> = std::result::Result<T, CoreError>;
