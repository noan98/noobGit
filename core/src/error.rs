use serde::{Deserialize, Serialize};
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

/// ネットワーク操作（fetch / pull / push）のエラー種別。
///
/// フロントエンドが種別ごとに日本語の解決手順ダイアログを表示するために使う。
/// git2 / libgit2 が返す英語エラーメッセージを [`classify_network_error`] で分類する。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkErrorKind {
    /// 認証失敗（401 / 403 / パスワード不正など）。
    AuthFailed,
    /// リモートリポジトリが見つからない（URL 誤り・削除済みなど）。
    RemoteNotFound,
    /// SSH 鍵が見つからないか読み込めない。
    SshKeyNotFound,
    /// non-fast-forward 拒否（ローカルよりリモートが進んでいる、または push 拒否）。
    NonFastForward,
    /// タイムアウト（ネットワークが遅い・サーバが応答しないなど）。
    Timeout,
    /// 上記のどれにも当てはまらないその他のエラー。
    Other,
}

/// git2 / libgit2 のエラーメッセージ（英語）を [`NetworkErrorKind`] に分類する。
///
/// 部分文字列の一致で判定する（大文字小文字を無視）。複数にマッチする場合は
/// より具体的な種別を優先するよう、判定の順序を上から精度の高い順にしている。
pub fn classify_network_error(raw: &str) -> NetworkErrorKind {
    let lower = raw.to_lowercase();

    // SSH 鍵が見つからない / 読み込めない（認証よりも先に判定する）。
    if lower.contains("no such file")
        || lower.contains("ssh key")
        || lower.contains("could not read username")
        || lower.contains("error loading key")
        || lower.contains("agent admitted failure")
        || (lower.contains("ssh") && lower.contains("key"))
    {
        return NetworkErrorKind::SshKeyNotFound;
    }

    // 認証失敗。
    if lower.contains("authentication")
        || lower.contains("401")
        || lower.contains("403")
        || lower.contains("invalid credentials")
        || lower.contains("bad credentials")
        || lower.contains("username")
        || lower.contains("password")
        || lower.contains("auth")
    {
        return NetworkErrorKind::AuthFailed;
    }

    // リモートが存在しない / 接続できない。
    if lower.contains("not found")
        || lower.contains("unable to connect")
        || lower.contains("could not resolve")
        || lower.contains("repository")
            && (lower.contains("not found") || lower.contains("does not exist"))
        || lower.contains("no such host")
        || lower.contains("failed to connect")
    {
        return NetworkErrorKind::RemoteNotFound;
    }

    // non-fast-forward / push 拒否。
    if lower.contains("non-fast-forward")
        || lower.contains("non fast forward")
        || lower.contains("fast-forward")
        || lower.contains("rejected")
    {
        return NetworkErrorKind::NonFastForward;
    }

    // タイムアウト。
    if lower.contains("timed out") || lower.contains("timeout") || lower.contains("time out") {
        return NetworkErrorKind::Timeout;
    }

    NetworkErrorKind::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_auth_failed() {
        assert_eq!(
            classify_network_error("Authentication failed for 'https://github.com/user/repo.git'"),
            NetworkErrorKind::AuthFailed
        );
        assert_eq!(
            classify_network_error("remote: HTTP 401 Unauthorized"),
            NetworkErrorKind::AuthFailed
        );
        assert_eq!(
            classify_network_error("invalid credentials"),
            NetworkErrorKind::AuthFailed
        );
    }

    #[test]
    fn test_classify_ssh_key_not_found() {
        assert_eq!(
            classify_network_error("Could not read Username for 'ssh://git@github.com'"),
            NetworkErrorKind::SshKeyNotFound
        );
        assert_eq!(
            classify_network_error("error loading key 'id_rsa': No such file or directory"),
            NetworkErrorKind::SshKeyNotFound
        );
        assert_eq!(
            classify_network_error("SSH key not found in the agent"),
            NetworkErrorKind::SshKeyNotFound
        );
    }

    #[test]
    fn test_classify_remote_not_found() {
        assert_eq!(
            classify_network_error("repository 'https://github.com/user/repo.git' not found"),
            NetworkErrorKind::RemoteNotFound
        );
        assert_eq!(
            classify_network_error("Could not resolve host: github.example.com"),
            NetworkErrorKind::RemoteNotFound
        );
        assert_eq!(
            classify_network_error("unable to connect to github.com"),
            NetworkErrorKind::RemoteNotFound
        );
    }

    #[test]
    fn test_classify_non_fast_forward() {
        assert_eq!(
            classify_network_error("[rejected] main -> main (non-fast-forward)"),
            NetworkErrorKind::NonFastForward
        );
        assert_eq!(
            classify_network_error("Updates were rejected because the remote contains work that you do not have locally"),
            // "rejected" が含まれる。
            NetworkErrorKind::NonFastForward
        );
        assert_eq!(
            classify_network_error("error: failed to push some refs (non fast forward)"),
            NetworkErrorKind::NonFastForward
        );
    }

    #[test]
    fn test_classify_timeout() {
        assert_eq!(
            classify_network_error("Connection timed out"),
            NetworkErrorKind::Timeout
        );
        assert_eq!(
            classify_network_error("Operation timeout: server did not respond"),
            NetworkErrorKind::Timeout
        );
    }

    #[test]
    fn test_classify_other() {
        assert_eq!(
            classify_network_error("unexpected error during pack transfer"),
            NetworkErrorKind::Other
        );
        assert_eq!(classify_network_error(""), NetworkErrorKind::Other);
    }
}
