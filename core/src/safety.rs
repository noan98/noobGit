use serde::{Deserialize, Serialize};

/// noobGit が扱うGit操作の種別。説明・リスク判定の共通キーになる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Stage,
    Unstage,
    Commit,
    CreateBranch,
    SwitchBranch,
    DeleteBranch,
    ResetHard,
    Pull,
    Push,
    ForcePush,
    Merge,
}

/// 操作の危険度。フロントの表示色・確認の強さに対応させる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// 安全。確認なしで実行してよい。
    Safe,
    /// 注意。実行前に内容を一度確認させる。
    Caution,
    /// 破壊的。強い確認ダイアログを出す。
    Destructive,
}

/// 既定の保護ブランチ名。
pub const DEFAULT_PROTECTED_BRANCHES: &[&str] = &["main", "master"];

/// あるブランチ名が保護対象かを判定する。
pub fn is_protected(branch: &str, protected: &[String]) -> bool {
    if protected.is_empty() {
        return DEFAULT_PROTECTED_BRANCHES.contains(&branch);
    }
    protected.iter().any(|p| p == branch)
}

/// リスク判定に必要な文脈情報。
#[derive(Debug, Clone, Default)]
pub struct SafetyContext {
    /// 操作対象のブランチ名（switch先・delete対象・push先など）。
    pub target_branch: Option<String>,
    /// 未コミットの変更が存在するか。
    pub working_dir_dirty: bool,
    /// 保護ブランチ一覧。空なら既定値を使う。
    pub protected_branches: Vec<String>,
}

/// 操作のリスク評価結果。確認ダイアログの内容に使う。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskAssessment {
    pub level: RiskLevel,
    /// なぜ危険か（または安全か）の日本語理由。
    pub reasons: Vec<String>,
    /// HEAD/ブランチの移動を後から取り消せるか（reflogベースのUndo可否）。
    pub reversible: bool,
    /// 未コミットの変更など、復元不能な損失が起こりうるか。
    pub permanent_data_loss: bool,
    /// より安全な代替案（あれば）。
    pub recommended_alternative: Option<String>,
}

impl RiskAssessment {
    fn safe(reason: &str) -> Self {
        RiskAssessment {
            level: RiskLevel::Safe,
            reasons: vec![reason.to_string()],
            reversible: true,
            permanent_data_loss: false,
            recommended_alternative: None,
        }
    }
}

/// 操作と文脈からリスクを評価する。
pub fn assess(op: OperationKind, ctx: &SafetyContext) -> RiskAssessment {
    let protected = ctx
        .target_branch
        .as_deref()
        .map(|b| is_protected(b, &ctx.protected_branches))
        .unwrap_or(false);

    match op {
        OperationKind::Stage => RiskAssessment::safe("変更をコミット対象に加えるだけで、ファイルの中身は変わりません。"),
        OperationKind::Unstage => RiskAssessment::safe("コミット対象から外すだけで、ファイルの中身は変わりません。"),
        OperationKind::Commit => RiskAssessment::safe("変更の記録を1つ作るだけで、あとから取り消せます。"),
        OperationKind::CreateBranch => RiskAssessment::safe("新しいブランチを作るだけで、既存の内容は変わりません。"),

        OperationKind::SwitchBranch => {
            if ctx.working_dir_dirty {
                RiskAssessment {
                    level: RiskLevel::Caution,
                    reasons: vec![
                        "未コミットの変更があります。ブランチを切り替えると、変更が邪魔をして切り替えに失敗することがあります。".to_string(),
                    ],
                    reversible: true,
                    permanent_data_loss: false,
                    recommended_alternative: Some(
                        "先に変更をコミットするか、一時退避(stash)してから切り替えると安全です。".to_string(),
                    ),
                }
            } else {
                RiskAssessment::safe("作業中の変更が無いので、安全に切り替えられます。")
            }
        }

        OperationKind::DeleteBranch => RiskAssessment {
            level: if protected { RiskLevel::Destructive } else { RiskLevel::Caution },
            reasons: {
                let mut r = vec!["ブランチを削除します。".to_string()];
                if protected {
                    r.push("これは保護ブランチ（main/master等）です。削除は通常行いません。".to_string());
                }
                r.push("マージされていないコミットがある場合、それらが見つけにくくなります。".to_string());
                r
            },
            reversible: true,
            permanent_data_loss: false,
            recommended_alternative: Some(
                "本当に不要か確認してください。削除しても直後ならUndoで復元できます。".to_string(),
            ),
        },

        OperationKind::ResetHard => RiskAssessment {
            level: RiskLevel::Destructive,
            reasons: vec![
                "指定地点まで強制的に巻き戻します。".to_string(),
                "未コミットの変更はすべて消え、元に戻せません。".to_string(),
            ],
            // コミット位置自体はreflogで戻せるが、未コミット変更は失われる。
            reversible: true,
            permanent_data_loss: ctx.working_dir_dirty,
            recommended_alternative: Some(
                "残したい変更があるなら、先にコミットか stash をしてください。".to_string(),
            ),
        },

        OperationKind::Pull => RiskAssessment {
            level: RiskLevel::Caution,
            reasons: vec![
                "リモートの最新を取り込みます。コンフリクトが起きることがあります。".to_string(),
            ],
            reversible: true,
            permanent_data_loss: false,
            recommended_alternative: None,
        },

        OperationKind::Push => {
            if protected {
                RiskAssessment {
                    level: RiskLevel::Caution,
                    reasons: vec![
                        "保護ブランチ（main/master等）へ直接pushしようとしています。".to_string(),
                        "チーム開発では、別ブランチを作ってプルリクエスト経由が安全です。".to_string(),
                    ],
                    reversible: false,
                    permanent_data_loss: false,
                    recommended_alternative: Some(
                        "作業用ブランチを作ってそちらにpushし、レビューを受けることを検討してください。".to_string(),
                    ),
                }
            } else {
                RiskAssessment {
                    level: RiskLevel::Safe,
                    reasons: vec!["自分のコミットをリモートへ送ります。通常は安全です。".to_string()],
                    reversible: false,
                    permanent_data_loss: false,
                    recommended_alternative: None,
                }
            }
        }

        OperationKind::ForcePush => RiskAssessment {
            level: RiskLevel::Destructive,
            reasons: {
                let mut r = vec![
                    "強制push（force push）はリモートの履歴を上書きします。".to_string(),
                    "他の人が持っているコミットを消してしまう恐れがあります。".to_string(),
                ];
                if protected {
                    r.push("対象は保護ブランチです。極めて危険です。".to_string());
                }
                r
            },
            reversible: false,
            permanent_data_loss: true,
            recommended_alternative: Some(
                "本当に必要か、チームに確認してください。多くの場合 force push は不要です。".to_string(),
            ),
        },

        OperationKind::Merge => RiskAssessment {
            level: RiskLevel::Caution,
            reasons: vec![
                "別のブランチの内容を取り込みます。コンフリクトが起きることがあります。".to_string(),
            ],
            reversible: true,
            permanent_data_loss: false,
            recommended_alternative: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_defaults() {
        assert!(is_protected("main", &[]));
        assert!(is_protected("master", &[]));
        assert!(!is_protected("feature/x", &[]));
    }

    #[test]
    fn protected_custom_overrides_default() {
        let custom = vec!["release".to_string()];
        assert!(is_protected("release", &custom));
        // カスタム指定時は既定の main は保護対象に含まれない。
        assert!(!is_protected("main", &custom));
    }

    #[test]
    fn safe_operations_are_safe() {
        let ctx = SafetyContext::default();
        for op in [
            OperationKind::Stage,
            OperationKind::Unstage,
            OperationKind::Commit,
            OperationKind::CreateBranch,
        ] {
            assert_eq!(assess(op, &ctx).level, RiskLevel::Safe);
        }
    }

    #[test]
    fn reset_hard_is_destructive_and_flags_data_loss_when_dirty() {
        let clean = SafetyContext::default();
        let dirty = SafetyContext { working_dir_dirty: true, ..Default::default() };
        assert_eq!(assess(OperationKind::ResetHard, &clean).level, RiskLevel::Destructive);
        assert!(!assess(OperationKind::ResetHard, &clean).permanent_data_loss);
        assert!(assess(OperationKind::ResetHard, &dirty).permanent_data_loss);
    }

    #[test]
    fn force_push_is_destructive() {
        let ctx = SafetyContext::default();
        let a = assess(OperationKind::ForcePush, &ctx);
        assert_eq!(a.level, RiskLevel::Destructive);
        assert!(a.permanent_data_loss);
    }

    #[test]
    fn push_to_protected_is_caution() {
        let ctx = SafetyContext { target_branch: Some("main".to_string()), ..Default::default() };
        assert_eq!(assess(OperationKind::Push, &ctx).level, RiskLevel::Caution);
        let ctx2 = SafetyContext { target_branch: Some("feature/x".to_string()), ..Default::default() };
        assert_eq!(assess(OperationKind::Push, &ctx2).level, RiskLevel::Safe);
    }

    #[test]
    fn switch_with_dirty_tree_is_caution() {
        let dirty = SafetyContext { working_dir_dirty: true, ..Default::default() };
        assert_eq!(assess(OperationKind::SwitchBranch, &dirty).level, RiskLevel::Caution);
    }

    #[test]
    fn delete_protected_branch_is_destructive() {
        let ctx = SafetyContext { target_branch: Some("main".to_string()), ..Default::default() };
        assert_eq!(assess(OperationKind::DeleteBranch, &ctx).level, RiskLevel::Destructive);
    }
}
