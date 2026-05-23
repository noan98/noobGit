use serde::{Deserialize, Serialize};

use crate::safety::OperationKind;

/// 1操作についての初学者向け説明。確認ダイアログや結果表示に添える。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Explanation {
    /// 操作の短い名前（日本語）。
    pub title: String,
    /// これは何をするか。
    pub what: String,
    /// なぜ安全/危険か、注意点。
    pub why: String,
    /// 失敗・想定外時に何が起きるか、どう対処するか。
    pub on_trouble: String,
}

/// 操作種別に対応する日本語説明を返す。
///
/// 文言はここに一元管理する。将来i18n化する場合もこの関数を起点に差し替えられる。
pub fn explain(op: OperationKind) -> Explanation {
    match op {
        OperationKind::Stage => Explanation {
            title: "ステージ（コミット準備）".into(),
            what: "変更したファイルを「次のコミットに含める」印を付けます。".into(),
            why: "ファイルの中身は変わりません。間違えても unstage で簡単に外せます。".into(),
            on_trouble: "対象を間違えたら、コミット前ならいつでも外せます。".into(),
        },
        OperationKind::Unstage => Explanation {
            title: "ステージ解除".into(),
            what: "コミットに含める印を外します。変更そのものは残ります。".into(),
            why: "中身は消えないので安全です。".into(),
            on_trouble: "もう一度ステージし直せば元に戻ります。".into(),
        },
        OperationKind::Commit => Explanation {
            title: "コミット（変更の記録）".into(),
            what: "ステージした変更に名前（メッセージ）を付けて履歴に1つ記録します。".into(),
            why: "記録はあとから参照・取り消しができます。こまめに行うほど安全です。".into(),
            on_trouble: "直後なら Undo で取り消せます。変更内容は失われません。".into(),
        },
        OperationKind::CreateBranch => Explanation {
            title: "ブランチ作成".into(),
            what: "今の状態を起点にした新しい作業の枝を作ります。".into(),
            why: "既存のブランチには影響しません。試したいことを安全に分けられます。".into(),
            on_trouble: "不要なら削除できます。".into(),
        },
        OperationKind::SwitchBranch => Explanation {
            title: "ブランチ切り替え".into(),
            what: "作業するブランチを変えます。ファイルがそのブランチの状態になります。".into(),
            why: "未コミットの変更があると切り替えに失敗することがあります。".into(),
            on_trouble: "失敗した場合は、変更をコミットか stash してから再度切り替えてください。".into(),
        },
        OperationKind::DeleteBranch => Explanation {
            title: "ブランチ削除".into(),
            what: "指定したブランチの「枝」を消します。".into(),
            why: "マージ前のコミットは見つけにくくなります。保護ブランチの削除は通常しません。".into(),
            on_trouble: "直後なら Undo で復元できます。".into(),
        },
        OperationKind::ResetHard => Explanation {
            title: "ハードリセット（強制巻き戻し）".into(),
            what: "指定地点まで履歴と作業ファイルを強制的に戻します。".into(),
            why: "未コミットの変更は完全に消え、元に戻せません。とても強力で危険な操作です。".into(),
            on_trouble: "巻き戻し先の移動自体は reflog から戻せますが、未コミット変更は復元できません。".into(),
        },
        OperationKind::Pull => Explanation {
            title: "プル（取り込み）".into(),
            what: "リモートの最新の変更を自分の手元に取り込みます。".into(),
            why: "他の人の変更と重なるとコンフリクト（衝突）が起きることがあります。".into(),
            on_trouble: "コンフリクトが出たら、対象ファイルを直して解決します。慌てなくて大丈夫です。".into(),
        },
        OperationKind::Push => Explanation {
            title: "プッシュ（送信）".into(),
            what: "自分のコミットをリモート（共有の場所）へ送ります。".into(),
            why: "通常は安全です。ただし共有ブランチへ直接送る前にチームの運用を確認しましょう。".into(),
            on_trouble: "拒否された場合は、先にpullして取り込んでから再度pushします。".into(),
        },
        OperationKind::ForcePush => Explanation {
            title: "強制プッシュ（force push）".into(),
            what: "リモートの履歴を自分のもので上書きします。".into(),
            why: "他人のコミットを消す恐れがある、非常に危険な操作です。基本的に使いません。".into(),
            on_trouble: "消えた履歴は元に戻すのが困難です。実行前に必ずチームへ確認してください。".into(),
        },
        OperationKind::Merge => Explanation {
            title: "マージ（統合）".into(),
            what: "別のブランチの変更を今のブランチに取り込みます。".into(),
            why: "コンフリクトが起きることがありますが、落ち着いて解決できます。".into(),
            on_trouble: "解決中に迷ったら、マージを中止して元の状態に戻すこともできます。".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_operation_has_nonempty_explanation() {
        for op in [
            OperationKind::Stage,
            OperationKind::Unstage,
            OperationKind::Commit,
            OperationKind::CreateBranch,
            OperationKind::SwitchBranch,
            OperationKind::DeleteBranch,
            OperationKind::ResetHard,
            OperationKind::Pull,
            OperationKind::Push,
            OperationKind::ForcePush,
            OperationKind::Merge,
        ] {
            let e = explain(op);
            assert!(!e.title.is_empty());
            assert!(!e.what.is_empty());
            assert!(!e.why.is_empty());
            assert!(!e.on_trouble.is_empty());
        }
    }
}
