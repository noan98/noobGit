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

/// git2 のエラーを `CoreError` に変換する（`?` で素通りする経路の共通の変換点）。
///
/// ネットワーク系（認証・接続・SSH など）は従来どおり生メッセージのまま返し、
/// [`classify_network_error`] とフロントの `NetworkErrorDialog` に任せる。
/// それ以外（ローカル操作）は [`describe_git2_error`] で日本語に包む（Issue #204）。
impl From<git2::Error> for CoreError {
    fn from(e: git2::Error) -> Self {
        if is_network_git2_error(&e) {
            CoreError::Git(e.message().to_string())
        } else {
            CoreError::Git(describe_git2_error(&e))
        }
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

/// ローカル操作（ステージ・コミット・チェックアウトなど）で起きやすいエラーの種別。
///
/// [`NetworkErrorKind`] のローカル版。libgit2 / OS が返す英語のエラーを
/// [`classify_local_error`] で分類し、フロントエンドが種別ごとに日本語の
/// 解決手順ダイアログを表示するために使う（文言は `explain::explain_local_error`）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalErrorKind {
    /// ロック競合（index.lock が残っている、他のツールがファイルを使用中など）。
    LockBusy,
    /// 権限がない・読み取り専用で書き込めない。
    PermissionDenied,
    /// リポジトリのデータ破損・オブジェクト欠損。
    RepoCorrupted,
    /// ディスクの空き容量が足りない。
    DiskFull,
    /// 上記のどれにも当てはまらないその他のエラー（生メッセージを添えて案内する）。
    Other,
}

impl LocalErrorKind {
    /// すべての種別（メッセージからの逆引きなどに使う）。
    pub const ALL: [LocalErrorKind; 5] = [
        LocalErrorKind::LockBusy,
        LocalErrorKind::PermissionDenied,
        LocalErrorKind::RepoCorrupted,
        LocalErrorKind::DiskFull,
        LocalErrorKind::Other,
    ];
}

/// git2 のエラーがネットワーク系（認証・接続・SSH・証明書）かどうか。
///
/// ネットワーク系は [`classify_network_error`] が生メッセージを見て分類するので、
/// ローカル用の日本語ラップをかけずそのまま通す。
pub fn is_network_git2_error(e: &git2::Error) -> bool {
    use git2::{ErrorClass, ErrorCode};
    matches!(
        e.class(),
        ErrorClass::Net | ErrorClass::Http | ErrorClass::Ssh | ErrorClass::Ssl
    ) || matches!(e.code(), ErrorCode::Auth | ErrorCode::Certificate)
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

/// libgit2 / OS のエラー（コード・分類・英語メッセージ）を [`LocalErrorKind`] に分類する。
///
/// 純粋関数。判定は「ディスク満杯 → ロック競合（`ErrorCode::Locked`）→ 権限 →
/// ロック競合（メッセージ）→ 破損 → その他」の順。書き込み失敗の根本原因
/// （空き容量・権限）を、二次的に出るロック関連メッセージより優先する。
pub fn classify_local_error(
    code: git2::ErrorCode,
    class: git2::ErrorClass,
    message: &str,
) -> LocalErrorKind {
    use git2::{ErrorClass, ErrorCode};
    let lower = message.to_lowercase();

    if contains_any(
        &lower,
        &[
            "no space left",
            "disk full",
            "not enough space",
            "not enough disk",
            "disk quota",
            "quota exceeded",
            "os error 28",
            "os error 112",
            "空き領域",
            "空き容量",
        ],
    ) {
        return LocalErrorKind::DiskFull;
    }

    if code == ErrorCode::Locked {
        return LocalErrorKind::LockBusy;
    }

    if contains_any(
        &lower,
        &[
            "permission denied",
            "access is denied",
            "access denied",
            "read-only file system",
            "read only file system",
            "operation not permitted",
            "アクセスが拒否",
        ],
    ) {
        return LocalErrorKind::PermissionDenied;
    }

    if (lower.contains(".lock") && contains_any(&lower, &["exist", "locked", "unable to create"]))
        || contains_any(
            &lower,
            &[
                "being used by another process",
                "sharing violation",
                "handle is invalid",
                "resource busy",
                "resource temporarily unavailable",
                "text file busy",
                "別のプロセスが使用中",
            ],
        )
    {
        return LocalErrorKind::LockBusy;
    }

    if contains_any(
        &lower,
        &[
            "object not found",
            "corrupt",
            "loose object",
            "packfile",
            "invalid pack",
            "failed to inflate",
            "zlib",
            "missing object",
            "checksum mismatch",
            "truncated",
            "unexpected end of",
            "malformed",
        ],
    ) || class == ErrorClass::Zlib
        || (matches!(
            class,
            ErrorClass::Odb | ErrorClass::Object | ErrorClass::Tree
        ) && code == ErrorCode::NotFound)
    {
        return LocalErrorKind::RepoCorrupted;
    }

    LocalErrorKind::Other
}

/// git2 のエラーを [`LocalErrorKind`] に分類する。
pub fn classify_git2_error(e: &git2::Error) -> LocalErrorKind {
    classify_local_error(e.code(), e.class(), e.message())
}

/// `std::io::Error`（ファイル操作の失敗）を [`LocalErrorKind`] に分類する。
///
/// まず OS が返す種別・エラー番号を見て、決められなければメッセージで判定する。
pub fn classify_io_error(e: &std::io::Error) -> LocalErrorKind {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => {
            return LocalErrorKind::PermissionDenied
        }
        ErrorKind::StorageFull | ErrorKind::QuotaExceeded => return LocalErrorKind::DiskFull,
        ErrorKind::ResourceBusy => return LocalErrorKind::LockBusy,
        _ => {}
    }
    // Windows の ERROR_SHARING_VIOLATION(32) / ERROR_LOCK_VIOLATION(33) は
    // 「他のプロセスが使用中」。番号は OS ごとに意味が違うので Windows でだけ見る。
    if cfg!(windows) && matches!(e.raw_os_error(), Some(32) | Some(33)) {
        return LocalErrorKind::LockBusy;
    }
    classify_local_error(
        git2::ErrorCode::GenericError,
        git2::ErrorClass::Os,
        &e.to_string(),
    )
}

/// 種別を示す見出しの目印（`【…】`）。メッセージ文字列からの逆引きに使う。
fn local_error_marker(kind: LocalErrorKind) -> String {
    format!("【{}】", crate::explain::explain_local_error(kind).title)
}

/// 分類結果と元のエラーメッセージから、ユーザーに見せる日本語メッセージを作る。
///
/// 形式: `【見出し】これは何か まず試すこと（元のエラー: 英語の生メッセージ）`。
/// 生メッセージは詳しい人・検索用に必ず残す。
pub fn format_local_error(kind: LocalErrorKind, raw: &str) -> String {
    let ex = crate::explain::explain_local_error(kind);
    let first_step = ex.steps.first().cloned().unwrap_or_default();
    format!(
        "{}{} まず試すこと: {}（元のエラー: {}）",
        local_error_marker(kind),
        ex.what,
        first_step,
        raw
    )
}

/// git2 のエラーを日本語のメッセージにする（分類不能なら汎用の文言で包む）。
pub fn describe_git2_error(e: &git2::Error) -> String {
    format_local_error(classify_git2_error(e), e.message())
}

/// [`describe_git2_error`] のネットワーク操作向け。分類不能（`Other`）なら
/// 生メッセージをそのまま返し、[`classify_network_error`] の分類を妨げない。
pub fn describe_git2_error_keep_unknown(e: &git2::Error) -> String {
    match classify_git2_error(e) {
        LocalErrorKind::Other => e.message().to_string(),
        kind => format_local_error(kind, e.message()),
    }
}

/// `std::io::Error` を日本語のメッセージにする（分類不能なら汎用の文言で包む）。
pub fn describe_io_error(e: &std::io::Error) -> String {
    format_local_error(classify_io_error(e), &e.to_string())
}

/// メッセージ文字列が [`format_local_error`] で作られたものなら、その種別を返す。
///
/// Tauri の境界ではエラーが文字列になるため、フロントは種別をここで逆引きする。
pub fn classify_local_message(message: &str) -> Option<LocalErrorKind> {
    LocalErrorKind::ALL
        .into_iter()
        .find(|k| message.contains(&local_error_marker(*k)))
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

    // ---- Issue #204: ローカル操作エラーの分類 ----

    use crate::test_support::TestRepo;
    use git2::{ErrorClass, ErrorCode};

    fn kind_of(code: ErrorCode, class: ErrorClass, msg: &str) -> LocalErrorKind {
        classify_local_error(code, class, msg)
    }

    #[test]
    fn local_classify_lock_busy() {
        assert_eq!(
            kind_of(
                ErrorCode::Locked,
                ErrorClass::Index,
                "failed to lock file '/r/.git/index.lock' for writing"
            ),
            LocalErrorKind::LockBusy
        );
        assert_eq!(
            kind_of(
                ErrorCode::GenericError,
                ErrorClass::Os,
                "failed to write index: The handle is invalid."
            ),
            LocalErrorKind::LockBusy
        );
        assert_eq!(
            kind_of(
                ErrorCode::GenericError,
                ErrorClass::Os,
                "The process cannot access the file because it is being used by another process."
            ),
            LocalErrorKind::LockBusy
        );
        assert_eq!(
            kind_of(
                ErrorCode::Exists,
                ErrorClass::Os,
                "unable to create '.git/HEAD.lock': File exists"
            ),
            LocalErrorKind::LockBusy
        );
    }

    #[test]
    fn local_classify_permission_denied() {
        assert_eq!(
            kind_of(
                ErrorCode::GenericError,
                ErrorClass::Os,
                "failed to create file '.git/index.lock': Permission denied"
            ),
            LocalErrorKind::PermissionDenied
        );
        assert_eq!(
            kind_of(ErrorCode::GenericError, ErrorClass::Os, "Access is denied."),
            LocalErrorKind::PermissionDenied
        );
        assert_eq!(
            kind_of(
                ErrorCode::GenericError,
                ErrorClass::Filesystem,
                "Read-only file system"
            ),
            LocalErrorKind::PermissionDenied
        );
    }

    #[test]
    fn local_classify_repo_corrupted() {
        assert_eq!(
            kind_of(
                ErrorCode::NotFound,
                ErrorClass::Odb,
                "object not found - no match for id (abc)"
            ),
            LocalErrorKind::RepoCorrupted
        );
        // メッセージが変わっても、分類とコードから破損と判断できる。
        assert_eq!(
            kind_of(ErrorCode::NotFound, ErrorClass::Odb, "no such thing"),
            LocalErrorKind::RepoCorrupted
        );
        assert_eq!(
            kind_of(ErrorCode::GenericError, ErrorClass::Zlib, "inflate failed"),
            LocalErrorKind::RepoCorrupted
        );
        assert_eq!(
            kind_of(
                ErrorCode::GenericError,
                ErrorClass::Odb,
                "loose object is corrupted"
            ),
            LocalErrorKind::RepoCorrupted
        );
    }

    #[test]
    fn local_classify_disk_full() {
        assert_eq!(
            kind_of(
                ErrorCode::GenericError,
                ErrorClass::Os,
                "failed to write file: No space left on device"
            ),
            LocalErrorKind::DiskFull
        );
        assert_eq!(
            kind_of(
                ErrorCode::GenericError,
                ErrorClass::Os,
                "There is not enough space on the disk."
            ),
            LocalErrorKind::DiskFull
        );
        // 空き容量不足は、二次的に出るロック関連メッセージより優先する。
        assert_eq!(
            kind_of(
                ErrorCode::Locked,
                ErrorClass::Index,
                "no space left on device while writing index.lock"
            ),
            LocalErrorKind::DiskFull
        );
    }

    #[test]
    fn local_classify_other() {
        assert_eq!(
            kind_of(ErrorCode::GenericError, ErrorClass::None, "something odd"),
            LocalErrorKind::Other
        );
        assert_eq!(
            kind_of(
                ErrorCode::NotFound,
                ErrorClass::Reference,
                "reference not found"
            ),
            LocalErrorKind::Other
        );
        assert_eq!(
            kind_of(ErrorCode::GenericError, ErrorClass::None, ""),
            LocalErrorKind::Other
        );
    }

    #[test]
    fn local_classify_io_errors() {
        use std::io::{Error, ErrorKind};
        assert_eq!(
            classify_io_error(&Error::from(ErrorKind::PermissionDenied)),
            LocalErrorKind::PermissionDenied
        );
        assert_eq!(
            classify_io_error(&Error::from(ErrorKind::StorageFull)),
            LocalErrorKind::DiskFull
        );
        assert_eq!(
            classify_io_error(&Error::from(ErrorKind::ResourceBusy)),
            LocalErrorKind::LockBusy
        );
        assert_eq!(
            classify_io_error(&Error::other("disk full while writing")),
            LocalErrorKind::DiskFull
        );
        assert_eq!(
            classify_io_error(&Error::other("boom")),
            LocalErrorKind::Other
        );
        #[cfg(unix)]
        assert_eq!(
            classify_io_error(&Error::from_raw_os_error(28)),
            LocalErrorKind::DiskFull
        );
    }

    #[test]
    fn local_message_roundtrip_for_every_kind() {
        for kind in LocalErrorKind::ALL {
            let msg = format_local_error(kind, "raw english message");
            assert!(msg.contains("raw english message"), "生メッセージを残す");
            assert_eq!(classify_local_message(&msg), Some(kind));
            // 日本語の文言が含まれる（英語のみのメッセージにならない）。
            assert!(msg.contains("【"));
            let ex = crate::explain::explain_local_error(kind);
            assert!(ex.steps.len() >= 3, "解決手順は複数ある: {kind:?}");
            assert!(!ex.what.is_empty() && !ex.why.is_empty());
        }
        assert_eq!(classify_local_message("Git操作に失敗しました: boom"), None);
        assert_eq!(classify_local_message(""), None);
    }

    #[test]
    fn local_messages_do_not_trip_network_classifier() {
        // 日本語ラップが classify_network_error の英語キーワードに反応しない
        // （ネットワーク操作の失敗を誤ってネットワーク種別にしない）。
        for kind in LocalErrorKind::ALL {
            let msg = format_local_error(kind, "x");
            assert_eq!(
                classify_network_error(&msg),
                NetworkErrorKind::Other,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn from_git2_wraps_unknown_errors_in_japanese() {
        let e = git2::Error::new(ErrorCode::GenericError, ErrorClass::None, "weird failure");
        let CoreError::Git(m) = CoreError::from(e) else {
            panic!("Git バリアントのはず");
        };
        assert_eq!(classify_local_message(&m), Some(LocalErrorKind::Other));
        assert!(m.contains("weird failure"));
    }

    #[test]
    fn from_git2_keeps_network_errors_raw() {
        // ネットワーク系は従来どおり生メッセージのまま（NetworkErrorKind の分類を壊さない）。
        let e = git2::Error::new(
            ErrorCode::Auth,
            ErrorClass::Http,
            "authentication required but no callback set",
        );
        let CoreError::Git(m) = CoreError::from(e) else {
            panic!("Git バリアントのはず");
        };
        assert_eq!(m, "authentication required but no callback set");
        assert_eq!(classify_network_error(&m), NetworkErrorKind::AuthFailed);
        assert_eq!(classify_local_message(&m), None);

        let e = git2::Error::new(
            ErrorCode::GenericError,
            ErrorClass::Net,
            "failed to connect",
        );
        assert!(is_network_git2_error(&e));
    }

    #[test]
    fn keep_unknown_leaves_unclassified_raw() {
        let e = git2::Error::new(ErrorCode::GenericError, ErrorClass::None, "odd");
        assert_eq!(describe_git2_error_keep_unknown(&e), "odd");
        let e = git2::Error::new(ErrorCode::Locked, ErrorClass::Index, "locked");
        assert_eq!(
            classify_local_message(&describe_git2_error_keep_unknown(&e)),
            Some(LocalErrorKind::LockBusy)
        );
    }

    // ---- 実リポジトリでの再現テスト ----

    #[test]
    fn repro_index_lock_is_lock_busy() {
        let t = TestRepo::new();
        t.write_file("a.txt", "hello");
        // 他のツールが操作中を模して index.lock を置く。
        std::fs::write(t.path().join(".git").join("index.lock"), "").unwrap();

        let repo = t.open();
        let err = crate::ops::stage_all(&repo).expect_err("index.lock があれば失敗する");
        let CoreError::Git(m) = err else {
            panic!("Git バリアントのはず");
        };
        assert_eq!(
            classify_local_message(&m),
            Some(LocalErrorKind::LockBusy),
            "{m}"
        );
        assert!(m.contains("他のツール"), "{m}");
    }

    #[test]
    fn repro_missing_object_is_repo_corrupted() {
        let t = TestRepo::new();
        t.write_file("a.txt", "hello");
        t.stage_all();
        let oid = t.commit("first");
        // HEAD コミットのオブジェクトファイルを消してリポジトリ破損を模す。
        let hex = oid.to_string();
        let obj = t
            .path()
            .join(".git")
            .join("objects")
            .join(&hex[..2])
            .join(&hex[2..]);
        // 読み取り専用属性が付いていることがあるので、権限を緩めてから消す。
        let mut perm = std::fs::metadata(&obj).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        std::fs::set_permissions(&obj, perm).unwrap();
        std::fs::remove_file(&obj).unwrap();

        let repo = t.open();
        let err = crate::repo::log(&repo, 10).expect_err("コミットが無ければ失敗する");
        let CoreError::Git(m) = err else {
            panic!("Git バリアントのはず: {err:?}");
        };
        assert_eq!(
            classify_local_message(&m),
            Some(LocalErrorKind::RepoCorrupted),
            "{m}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn repro_disk_full_io_error_is_wrapped() {
        let e = std::io::Error::from_raw_os_error(28);
        let m = describe_io_error(&e);
        assert_eq!(classify_local_message(&m), Some(LocalErrorKind::DiskFull));
    }
}
