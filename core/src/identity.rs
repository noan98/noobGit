//! コミット作者の identity（`user.name` / `user.email`）の確認と設定。
//!
//! Git 初学者が最初に詰まりやすいのが、identity 未設定でコミットできない問題。
//! ここでは `git2::Config` 越しに現在値の取得と保存を行い、ターミナルに戻らなくても
//! noobGit 内で初回セットアップを完結できるようにする。設定自体は事故性のない
//! 安全操作なので、ガードフロー（確認ダイアログ）には載せない。

use git2::{Config, ConfigLevel, Repository};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// identity の保存先スコープ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityScope {
    /// このリポジトリだけ（`.git/config`）。
    Local,
    /// この PC 全体（`~/.gitconfig`）。明示的に選んだときだけ書き込む。
    Global,
}

/// 現在の `user.name` / `user.email`。未設定（空文字含む）の項目は `None`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub name: Option<String>,
    pub email: Option<String>,
}

impl Identity {
    /// 名前・メールがどちらも設定済みか（＝コミットできる状態か）。
    pub fn is_complete(&self) -> bool {
        nonblank(self.name.as_deref()) && nonblank(self.email.as_deref())
    }
}

fn nonblank(v: Option<&str>) -> bool {
    v.map(|s| !s.trim().is_empty()).unwrap_or(false)
}

/// 現在有効な identity を返す（ローカル設定がグローバルより優先される）。
pub fn get_identity(repo: &Repository) -> Result<Identity> {
    let cfg = repo.config()?;
    Ok(Identity {
        name: read_nonblank(&cfg, "user.name"),
        email: read_nonblank(&cfg, "user.email"),
    })
}

fn read_nonblank(cfg: &Config, key: &str) -> Option<String> {
    match cfg.get_string(key) {
        Ok(v) if !v.trim().is_empty() => Some(v),
        _ => None,
    }
}

/// identity を保存する。スコープでローカル/グローバルの保存先を選ぶ。
///
/// 名前は空にできず、メールは最低限 `x@y` の形を満たす必要がある。厳密な検証は
/// せず、初学者が迷わない平易なエラー文言で弾く。
pub fn set_identity(
    repo: &Repository,
    name: &str,
    email: &str,
    scope: IdentityScope,
) -> Result<()> {
    let name = name.trim();
    let email = email.trim();
    if name.is_empty() {
        return Err(CoreError::InvalidInput(
            "名前を入力してください。".to_string(),
        ));
    }
    if !is_plausible_email(email) {
        return Err(CoreError::InvalidInput(
            "メールアドレスを正しく入力してください（例: you@example.com）。".to_string(),
        ));
    }

    let mut target = match scope {
        IdentityScope::Local => repo.config()?.open_level(ConfigLevel::Local)?,
        IdentityScope::Global => open_global_config()?,
    };
    target.set_str("user.name", name)?;
    target.set_str("user.email", email)?;
    Ok(())
}

/// 書き込み可能なグローバル設定（`~/.gitconfig`）を開く。無ければ新規作成する。
fn open_global_config() -> Result<Config> {
    if let Ok(path) = git2::Config::find_global() {
        return Config::open(&path).map_err(Into::into);
    }
    // グローバル設定ファイルがまだ無い環境。ホームフォルダ直下に作る。
    let base = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| {
            CoreError::Git(
                "グローバル設定の保存先（ホームフォルダ）を見つけられませんでした。".to_string(),
            )
        })?;
    let path = std::path::Path::new(&base).join(".gitconfig");
    Config::open(&path).map_err(Into::into)
}

/// 最低限のメール形式チェック（`local@domain` の形だけ確認する）。
fn is_plausible_email(s: &str) -> bool {
    let mut parts = s.splitn(2, '@');
    match (parts.next(), parts.next()) {
        (Some(local), Some(domain)) => {
            !local.is_empty() && !domain.is_empty() && !domain.contains('@')
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestRepo;

    #[test]
    fn is_complete_truth_table() {
        let complete = Identity {
            name: Some("山田 太郎".into()),
            email: Some("taro@example.com".into()),
        };
        assert!(complete.is_complete());

        let no_email = Identity {
            name: Some("山田 太郎".into()),
            email: None,
        };
        assert!(!no_email.is_complete());

        let blank_name = Identity {
            name: Some("   ".into()),
            email: Some("taro@example.com".into()),
        };
        assert!(!blank_name.is_complete());
    }

    #[test]
    fn email_validation() {
        assert!(is_plausible_email("you@example.com"));
        assert!(!is_plausible_email("no-at-sign"));
        assert!(!is_plausible_email("@example.com"));
        assert!(!is_plausible_email("you@"));
        assert!(!is_plausible_email("a@b@c"));
    }

    #[test]
    fn set_local_then_get_roundtrip() {
        let fx = TestRepo::new_without_identity();
        {
            let repo = fx.open();
            set_identity(&repo, "山田 太郎", "taro@example.com", IdentityScope::Local).unwrap();
        }
        // ローカル設定はグローバルより優先されるので、環境に依存せず設定値が読める。
        let id = get_identity(&fx.open()).unwrap();
        assert_eq!(id.name.as_deref(), Some("山田 太郎"));
        assert_eq!(id.email.as_deref(), Some("taro@example.com"));
        assert!(id.is_complete());
    }

    #[test]
    fn set_overwrites_existing_value() {
        let fx = TestRepo::new(); // 既定で Test User / test@example.com 済み。
        {
            let repo = fx.open();
            let before = get_identity(&repo).unwrap();
            assert_eq!(before.name.as_deref(), Some("Test User"));
            set_identity(&repo, "新 太郎", "shin@example.com", IdentityScope::Local).unwrap();
        }
        let after = get_identity(&fx.open()).unwrap();
        assert_eq!(after.name.as_deref(), Some("新 太郎"));
        assert_eq!(after.email.as_deref(), Some("shin@example.com"));
    }

    #[test]
    fn set_rejects_blank_name_and_bad_email() {
        let fx = TestRepo::new_without_identity();
        let repo = fx.open();
        assert!(matches!(
            set_identity(&repo, "   ", "taro@example.com", IdentityScope::Local).unwrap_err(),
            CoreError::InvalidInput(_)
        ));
        assert!(matches!(
            set_identity(&repo, "名前", "", IdentityScope::Local).unwrap_err(),
            CoreError::InvalidInput(_)
        ));
        assert!(matches!(
            set_identity(&repo, "名前", "no-at-sign", IdentityScope::Local).unwrap_err(),
            CoreError::InvalidInput(_)
        ));
    }

    #[test]
    fn commit_works_after_setting_identity() {
        let fx = TestRepo::new_without_identity();
        fx.write_file("a.txt", "hi");
        {
            let repo = fx.open();
            set_identity(&repo, "山田 太郎", "taro@example.com", IdentityScope::Local).unwrap();
        }
        // 設定を確実に反映させるため開き直してからステージ・コミットする。
        let repo = fx.open();
        crate::ops::stage_all(&repo).unwrap();
        let info = crate::ops::commit(&repo, "最初のコミット").unwrap();
        assert_eq!(info.author_name, "山田 太郎");
        assert_eq!(info.author_email, "taro@example.com");
    }
}
